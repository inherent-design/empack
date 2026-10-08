//! Interpret imported declarations without changing their destination or participation.
use super::*;
use crate::{
    application::InitArgs,
    engine::{
        import::{
            ImportCandidateOptions, ImportContentKey, ImportFileDecision, ImportLocalFile,
            ImportPersistence, ImportedAcquisition, ImportedRequirement, ImportedRequirements,
            ImportedRuntime, VerifiedImportContent,
        },
        project_change::ProjectReplacementPolicy,
        providers::ModpackSelector,
    },
};
use empack_core::{
    model::{
        DependencyKey, DistributionArchive, DistributionIntent, ExpectedContent, GameVersion,
        LoaderKind, PackMetadata,
    },
    path::{InstallDestination, PathSyntax, PortableRelPath},
    projection::BuildTarget,
    requirements::{ChoiceKey, OptionalChoice},
};
use std::collections::BTreeMap;

pub async fn initialize(session: &dyn Session, args: &InitArgs) -> Result<()> {
    let Some(source) = &args.from_source else {
        return super::super::initialize(session, args).await;
    };
    ensure!(
        args.import_files.len() <= 128,
        "At most 128 import-file associations are allowed"
    );
    let local_files = args
        .import_files
        .iter()
        .map(|value| {
            let (selector, path) = value
                .split_once('=')
                .context("Import file association must be SELECTOR=PATH")?;
            ensure!(
                !selector.is_empty() && !path.is_empty(),
                "Import file association needs a selector and source path"
            );
            Ok(ImportLocalFile {
                selector: selector.into(),
                source: path.into(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let expected = ExpectedContent {
        digests: None,
        size: None,
        accepted_observation: None,
    };
    let source = if let Some(provider) = url_provider(source)? {
        ImportSource::Provider {
            selector: ModpackSelector::parse(provider, source)?,
            releases: ReleasePolicy::PreferStable,
            supplied_archive: None,
        }
    } else if source.contains("://") {
        let url = reqwest::Url::parse(source)?;
        ensure!(
            url.scheme() == "https"
                && url.host_str().is_some()
                && url.username().is_empty()
                && url.password().is_none(),
            "Import downloads require HTTPS without URL credentials"
        );
        ImportSource::Download {
            alternatives: NonEmpty::new(vec![source.clone()])?,
            expected,
        }
    } else {
        ImportSource::Local {
            path: source.into(),
            expected,
        }
    };
    super::super::import(
        session,
        ImportHostRequest {
            source,
            destination: args.dir.as_ref().map(PathBuf::from),
            replacement: if args.force {
                ProjectReplacementPolicy::ReplaceManagedContent
            } else {
                ProjectReplacementPolicy::RejectExisting
            },
            evidence: SourceEvidencePolicy::Compatibility,
            supplied: BTreeMap::new(),
            local_files,
        },
        |content| decisions(session, args, content),
    )
    .await
}

fn choose(session: &dyn Session, prompt: &str, labels: &[String]) -> Result<usize> {
    ensure!(
        !session.config().app_config().yes && session.interactive().can_choose(),
        "{prompt} requires an explicit choice in headless mode"
    );
    let index = session
        .interactive()
        .fuzzy_select(prompt, labels)?
        .ok_or(crate::application::process_runtime::Interrupted)?;
    ensure!(index < labels.len(), "Import choice is out of range");
    Ok(index)
}
fn loader(
    session: &dyn Session,
    args: &InitArgs,
    runtime: &ImportedRuntime,
) -> Result<Option<usize>> {
    ensure!(
        args.mc_version
            .as_deref()
            .is_none_or(|value| value == runtime.minecraft.as_str()),
        "--mc-version differs from the imported runtime; change runtime intent after import"
    );
    let family = args
        .modloader
        .as_deref()
        .map(super::super::initialize::parse_loader)
        .transpose()?;
    if runtime.loaders.is_empty() {
        ensure!(
            family.is_none_or(|value| value == LoaderKind::Vanilla)
                && args.loader_version.is_none(),
            "Imported vanilla pack has no requested loader selection"
        );
        return Ok(None);
    }
    let candidates = runtime
        .loaders
        .iter()
        .enumerate()
        .filter(|(_, loader)| {
            family.is_none_or(|value| value == loader.kind)
                && args
                    .loader_version
                    .as_deref()
                    .is_none_or(|value| value == loader.version.as_str())
        })
        .collect::<Vec<_>>();
    ensure!(
        !candidates.is_empty(),
        "Requested loader is not declared by the imported pack"
    );
    if candidates.len() == 1 {
        return Ok(Some(candidates[0].0));
    }
    let primary = candidates
        .iter()
        .filter(|(_, loader)| loader.primary)
        .collect::<Vec<_>>();
    if primary.len() == 1 {
        return Ok(Some(primary[0].0));
    }
    let labels = candidates
        .iter()
        .map(|(_, value)| format!("{:?} {}", value.kind, value.version.as_str()))
        .collect::<Vec<_>>();
    Ok(Some(
        candidates[choose(
            session,
            "Select imported loader (or supply --modloader/--loader-version)",
            &labels,
        )?]
        .0,
    ))
}
fn requirements(
    source: &ImportedRequirements,
    key: &str,
    default: Option<bool>,
) -> Result<Requirements> {
    let convert = |value| -> Result<Requirement> {
        Ok(match value {
            ImportedRequirement::Required => Requirement::Required,
            ImportedRequirement::Unsupported => Requirement::Unsupported,
            ImportedRequirement::Optional => Requirement::Optional(OptionalChoice {
                key: ChoiceKey::parse(key)?,
                default_enabled: default
                    .context("Optional import requires --import-optional-default true/false")?,
                description: None,
            }),
        })
    };
    Ok(Requirements {
        client: convert(source.client)?,
        server: convert(source.server)?,
    })
}
fn file_kind(path: &str, layout: &BTreeMap<ContentKind, PortableRelPath>) -> ContentKind {
    for (kind, folder) in layout {
        if path.starts_with(&format!("{}/", folder.as_str())) {
            return *kind;
        }
    }
    if path.starts_with("mods/") {
        ContentKind::Mod
    } else if path.starts_with("resourcepacks/") {
        ContentKind::ResourcePack
    } else if path.starts_with("shaderpacks/") {
        ContentKind::ShaderPack
    } else if path.starts_with("config/") {
        ContentKind::Config
    } else {
        ContentKind::OtherFile
    }
}
fn decisions(
    session: &dyn Session,
    args: &InitArgs,
    content: &VerifiedImportContent,
) -> Result<ImportCandidateOptions> {
    let source = content.plan().imported();
    let loader = loader(session, args, &source.runtime)?;
    let mut acceptable_versions = args
        .game_versions
        .as_deref()
        .unwrap_or_default()
        .iter()
        .map(|value| GameVersion::parse(value))
        .collect::<Result<Vec<_>, _>>()?;
    let mut seen = BTreeSet::new();
    acceptable_versions
        .retain(|version| version != &source.runtime.minecraft && seen.insert(version.clone()));
    let mut layout = BTreeMap::new();
    if let Some(folder) = &args.datapack_folder {
        layout.insert(
            ContentKind::DataPack,
            PortableRelPath::parse(folder, PathSyntax::ProjectContent)?,
        );
    } else {
        let proposals = source
            .datapack_layout_proposals()
            .into_keys()
            .collect::<Vec<_>>();
        if proposals.len() == 1 {
            layout.insert(ContentKind::DataPack, proposals[0].clone());
        } else if !proposals.is_empty() {
            let labels = proposals
                .iter()
                .map(|path| path.as_str().to_owned())
                .collect::<Vec<_>>();
            let index = choose(
                session,
                "Select data pack folder (or supply --datapack-folder)",
                &labels,
            )?;
            layout.insert(ContentKind::DataPack, proposals[index].clone());
        }
    }
    for diagnostic in &source.diagnostics {
        session.display().status().warning(&diagnostic.message);
    }
    let exclude_auxiliary_members = if source.auxiliary_members.is_empty() || args.exclude_auxiliary
    {
        args.exclude_auxiliary
    } else {
        for member in &source.auxiliary_members {
            session
                .display()
                .status()
                .info(&format!("Auxiliary archive member: {}", member.as_str()));
        }
        ensure!(
            !session.config().app_config().yes && session.interactive().can_choose(),
            "Archive has auxiliary members; --exclude-auxiliary explicitly excludes them"
        );
        ensure!(
            session
                .interactive()
                .confirm("Exclude these auxiliary archive members?", false)?,
            "Import cancelled; nothing was published"
        );
        true
    };
    let optional = source
        .files
        .iter()
        .chain(&source.overrides)
        .map(|file| &file.requirements)
        .chain(
            source
                .providers
                .iter()
                .map(|reference| &reference.requirements),
        )
        .any(|req| {
            req.client == ImportedRequirement::Optional
                || req.server == ImportedRequirement::Optional
        });
    let default = if optional && args.import_optional_default.is_none() {
        ensure!(
            !session.config().app_config().yes && session.interactive().can_choose(),
            "Optional import requires --import-optional-default true/false; files remain optional"
        );
        Some(
            session
                .interactive()
                .confirm("Enable imported optional files by default?", true)?,
        )
    } else {
        args.import_optional_default
    };
    let mut files = BTreeMap::new();
    for key in content.content().keys() {
        let decision = match key {
            ImportContentKey::Declared(index) | ImportContentKey::Override(index) => {
                let file = if matches!(key, ImportContentKey::Declared(_)) {
                    &source.files[*index]
                } else {
                    &source.overrides[*index]
                };
                let label = format!("{:?}:{}", file.layer, file.destination.relative().as_str());
                let persistence = match &file.acquisition {
                    ImportedAcquisition::Embedded(_) => ImportPersistence::Local,
                    ImportedAcquisition::Downloads(urls) if !args.import_local_files => {
                        for url in urls {
                            crate::engine::documents::validate_download_url(url)
                            .context("Import locator cannot be persisted; use --import-local-files to keep verified bytes")?;
                        }
                        ImportPersistence::Url
                    }
                    _ => ImportPersistence::Local,
                };
                ImportFileDecision {
                    key: DependencyKey::parse(&label)?,
                    kind: file_kind(file.destination.relative().as_str(), &layout),
                    requirements: requirements(&file.requirements, &label, default)?,
                    persistence,
                    provider_destination: None,
                }
            }
            ImportContentKey::Provider { pin, filename } => {
                let record = content
                    .plan()
                    .providers()
                    .records()
                    .get(pin)
                    .context("Missing verified provider selection")?;
                let reference = source
                    .providers
                    .iter()
                    .find(|reference| reference.selection == *pin)
                    .context("Missing imported provider reference")?;
                let kinds = record.kinds.as_slice();
                let kind = if kinds.len() == 1 {
                    kinds[0]
                } else {
                    kinds[choose(
                        session,
                        "Select imported provider content kind",
                        &kinds
                            .iter()
                            .map(|kind| format!("{kind:?}"))
                            .collect::<Vec<_>>(),
                    )?]
                };
                let folder = layout
                    .get(&kind)
                    .map(|value| value.as_str())
                    .or(match kind {
                        ContentKind::Mod => Some("mods"),
                        ContentKind::ResourcePack => Some("resourcepacks"),
                        ContentKind::ShaderPack => Some("shaderpacks"),
                        _ => None,
                    })
                    .context("Imported provider content needs an explicit destination folder")?;
                let namespace = match pin.project {
                    empack_core::identity::ProviderProjectId::Modrinth(_) => "modrinth",
                    empack_core::identity::ProviderProjectId::CurseForge(_) => "curseforge",
                };
                let label = format!("{namespace}:{}", pin.project);
                ImportFileDecision {
                    key: DependencyKey::parse(&label)?,
                    kind,
                    requirements: requirements(&reference.requirements, &label, default)?,
                    persistence: ImportPersistence::Provider,
                    provider_destination: Some(InstallDestination::parse(&format!(
                        "{folder}/{filename}"
                    ))?),
                }
            }
        };
        files.insert(key.clone(), decision);
    }
    let metadata = PackMetadata {
        name: args
            .pack_name
            .clone()
            .or_else(|| source.metadata.name.clone())
            .unwrap_or_else(|| "Imported pack".into()),
        version: args
            .pack_version
            .clone()
            .or_else(|| source.metadata.version.clone())
            .unwrap_or_else(|| "1.0.0".into()),
        author: args
            .author
            .clone()
            .or_else(|| source.metadata.author.clone()),
        description: source.metadata.summary.clone(),
    };
    Ok(ImportCandidateOptions {
        metadata,
        loader,
        acceptable_versions,
        layout,
        distribution: DistributionIntent {
            targets: NonEmpty::new(vec![
                BuildTarget::Mrpack,
                BuildTarget::Client,
                BuildTarget::Server,
                BuildTarget::ClientFull,
                BuildTarget::ServerFull,
            ])?,
            archive: DistributionArchive::Zip,
        },
        files,
        exclude_auxiliary_members,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::engine_host::cli::tests::{project, session};
    use std::{
        fs,
        io::{Cursor, Write},
    };
    fn archive(root: &Path, auxiliary: bool) {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let manifest = serde_json::to_vec(&serde_json::json!({"formatVersion":1,"game":"minecraft","name":"Imported","versionId":"2","files":[],"dependencies":{"minecraft":"1.21.1"}})).unwrap();
        for (name, bytes) in [
            ("modrinth.index.json", manifest.as_slice()),
            ("overrides/config/example.toml", b"common"),
            ("client-overrides/config/example.toml", b"client"),
            ("server-overrides/config/example.toml", b"server"),
        ] {
            writer
                .start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            writer.write_all(bytes).unwrap();
        }
        if auxiliary {
            writer
                .start_file("readme.txt", zip::write::SimpleFileOptions::default())
                .unwrap();
            writer.write_all(b"auxiliary").unwrap();
        }
        fs::write(
            root.join("input.mrpack"),
            writer.finish().unwrap().into_inner(),
        )
        .unwrap();
    }
    fn args() -> InitArgs {
        InitArgs {
            from_source: Some("input.mrpack".into()),
            game_versions: Some(vec!["1.21.1".into(), "1.21".into(), "1.21".into()]),
            ..Default::default()
        }
    }
    #[tokio::test]
    async fn import_cli_preserves_side_layers_versions_and_preview_then_reexports() {
        use crate::application::BuildArgs;
        let root = tempfile::tempdir().unwrap();
        archive(root.path(), false);
        let before = crate::application::engine_host::tests::snapshot(root.path());
        initialize(&session(root.path(), true), &args())
            .await
            .unwrap();
        assert_eq!(
            crate::application::engine_host::tests::snapshot(root.path()),
            before
        );
        initialize(&session(root.path(), false), &args())
            .await
            .unwrap();
        let current = project(root.path());
        assert_eq!(current.intent().metadata.name, "Imported");
        assert_eq!(
            current.intent().runtime.acceptable_versions,
            vec![GameVersion::parse("1.21").unwrap()]
        );
        for (layer, expected) in [
            ("common", "common"),
            ("client", "client"),
            ("server", "server"),
        ] {
            assert_eq!(
                fs::read_to_string(
                    root.path()
                        .join(format!("project/overrides/{layer}/config/example.toml"))
                )
                .unwrap(),
                expected
            );
        }
        super::super::super::build(
            &session(root.path(), false),
            &BuildArgs {
                targets: vec!["mrpack".into()],
                ..Default::default()
            },
            BuildDecisions::default(),
            Default::default(),
        )
        .await
        .unwrap();
        let output = fs::read_dir(root.path().join("project/dist"))
            .unwrap()
            .find_map(|entry| {
                let path = entry.unwrap().path();
                (path.extension().is_some_and(|ext| ext == "mrpack")).then_some(path)
            })
            .unwrap();
        let mut output = zip::ZipArchive::new(fs::File::open(output).unwrap()).unwrap();
        // Export represents each effective environment; the common bytes remain authored
        // in the project but are shadowed in both distributions.
        assert!(output.by_name("overrides/config/example.toml").is_err());
        for (path, expected) in [
            ("client-overrides/config/example.toml", "client"),
            ("server-overrides/config/example.toml", "server"),
        ] {
            use std::io::Read;
            let mut bytes = String::new();
            output
                .by_name(path)
                .unwrap()
                .read_to_string(&mut bytes)
                .unwrap();
            assert_eq!(bytes, expected);
        }
    }
    #[tokio::test]
    async fn import_cli_requires_explicit_exclusion_and_never_changes_declared_runtime() {
        let root = tempfile::tempdir().unwrap();
        archive(root.path(), true);
        let before = crate::application::engine_host::tests::snapshot(root.path());
        assert!(
            initialize(&session(root.path(), false), &args())
                .await
                .unwrap_err()
                .to_string()
                .contains("auxiliary")
        );
        assert_eq!(
            crate::application::engine_host::tests::snapshot(root.path()),
            before
        );
        let mut selected = args();
        selected.exclude_auxiliary = true;
        selected.mc_version = Some("1.20".into());
        assert!(
            initialize(&session(root.path(), false), &selected)
                .await
                .unwrap_err()
                .to_string()
                .contains("differs")
        );
        assert_eq!(
            crate::application::engine_host::tests::snapshot(root.path()),
            before
        );
        selected.mc_version = None;
        initialize(&session(root.path(), false), &selected)
            .await
            .unwrap();
        assert!(!root.path().join("project/readme.txt").exists());
    }
    #[test]
    fn optional_defaults_preserve_environment_instead_of_becoming_required() {
        let source = ImportedRequirements {
            client: ImportedRequirement::Optional,
            server: ImportedRequirement::Unsupported,
        };
        assert!(requirements(&source, "theme", None).is_err());
        for default in [false, true] {
            let selected = requirements(&source, "theme", Some(default)).unwrap();
            let Requirement::Optional(choice) = selected.client else {
                panic!("optional was lost")
            };
            assert_eq!(choice.default_enabled, default);
            assert_eq!(selected.server, Requirement::Unsupported);
        }
    }
}
