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
    distribution::Recipe,
    model::{
        DependencyKey, DistributionArchive, DistributionIntent, ExpectedContent, GameVersion,
        LoaderKind, PackMetadata,
    },
    path::{InstallDestination, PathSyntax, PortableRelPath},
    requirements::{ChoiceKey, OptionalChoice},
};
use std::collections::BTreeMap;

pub async fn initialize(session: &dyn Session, args: &InitArgs) -> Result<()> {
    if args.from_source.is_none() && !args.continue_import {
        return super::super::initialize(session, args).await;
    }
    let source = args.from_source.as_deref().unwrap_or_default();
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
    let source = if args.continue_import {
        ImportSource::Saved
    } else if let Some(provider) = url_provider(source)? {
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
            alternatives: NonEmpty::new(vec![source.to_owned()])?,
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
    if let Some(folder) = &args.world_folder {
        layout.insert(
            ContentKind::World,
            PortableRelPath::parse(folder, PathSyntax::ProjectContent)?,
        );
    }
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
                    .with_context(|| {
                        let choice = match kind {
                            ContentKind::World => "use --world-folder PATH",
                            ContentKind::DataPack => "use --datapack-folder PATH",
                            _ => "this content kind has no supported import folder",
                        };
                        format!(
                            "Imported {kind:?} project {} needs a destination: {choice}",
                            record.project.slug.as_str()
                        )
                    })?;
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
                        "{folder}/{}",
                        if kind == ContentKind::World {
                            record.project.slug.as_str()
                        } else {
                            filename.as_str()
                        }
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
            native: None,
            recipes: NonEmpty::new(vec![
                Recipe::MODRINTH,
                Recipe::PRISM_BUNDLED,
                Recipe::SERVER_BUNDLED,
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
                targets: vec!["modrinth".into()],
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

    #[tokio::test]
    async fn provider_import_preserves_kinds_optional_choices_and_exported_files() {
        use crate::application::engine_host::{BuildDecisions, build};
        use crate::engine::{
            acquisition::HttpAcquisition, build::BuildAcquisitions, import::ImportLimits,
            mrpack::OptionalConversion, providers::ProviderCatalog,
        };
        use serde_json::json;
        use std::{
            fs,
            io::{Read, Write},
        };
        let root = tempfile::tempdir().unwrap();
        let session = super::super::tests::session(root.path(), false);
        let mut server = mockito::Server::new_async().await;
        let cases = [
            (6, ContentKind::Mod, "mods"),
            (12, ContentKind::ResourcePack, "resourcepacks"),
            (6552, ContentKind::ShaderPack, "shaderpacks"),
            (6945, ContentKind::DataPack, "datapacks"),
        ];
        let mut references = Vec::new();
        let mut files = Vec::new();
        fs::write(root.path().join("renamed.bin"), b"payload").unwrap();
        for (index, (class, _, _)) in cases.iter().enumerate() {
            let project = 100 + index;
            let selection = 200 + index;
            references.push(json!({"projectID":project,"fileID":selection,"required":false}));
            server.mock("GET", format!("/mods/{project}").as_str())
                .with_body(json!({"data":{"id":project,"gameId":432,"classId":class,"slug":format!("fixture-{project}"),"name":"Fixture"}}).to_string())
                .expect(1).create_async().await;
            server.mock("GET", format!("/mods/{project}/files/{selection}").as_str())
                .with_body(json!({"data":{"id":selection,"gameId":432,"modId":project,"fileName":"fixture.zip","fileLength":7,"hashes":[{"algo":2,"value":"321c3cf486ed509164edec1e1981fec8"}],"downloadUrl":format!("https://example.invalid/{project}/different.bin"),"gameVersions":["1.21.1"],"dependencies":[]}}).to_string())
                .expect(1).create_async().await;
            files.push(ImportLocalFile {
                selector: format!("provider:curseforge:{project}:{selection}:fixture.zip"),
                source: "renamed.bin".into(),
            });
        }
        let manifest = serde_json::to_vec(&json!({"manifestVersion":1,"manifestType":"minecraftModpack","name":"Imported","version":"1","files":references,"minecraft":{"version":"1.21.1","modLoaders":[]},"overrides":"overrides"})).unwrap();
        let path = root.path().join("input.zip");
        let mut archive = zip::ZipWriter::new(fs::File::create(&path).unwrap());
        archive
            .start_file("manifest.json", zip::write::SimpleFileOptions::default())
            .unwrap();
        archive.write_all(&manifest).unwrap();
        archive.finish().unwrap();
        let args = InitArgs {
            import_optional_default: Some(true),
            datapack_folder: Some("datapacks".into()),
            ..Default::default()
        };
        super::super::super::import::import_with_services(
            &session,
            ImportHostRequest {
                source: ImportSource::Local {
                    path,
                    expected: ExpectedContent {
                        digests: None,
                        size: None,
                        accepted_observation: None,
                    },
                },
                destination: None,
                replacement: ProjectReplacementPolicy::RejectExisting,
                evidence: SourceEvidencePolicy::Compatibility,
                supplied: BTreeMap::new(),
                local_files: files,
            },
            |content| decisions(&session, &args, content),
            ProviderCatalog::for_loopback_tests(&server.url(), Some("fixture".into())),
            HttpAcquisition::for_loopback_tests(),
            ImportLimits::default(),
        )
        .await
        .unwrap();
        let project = super::super::tests::project(root.path());
        let target = root.path().join("project");
        let original =
            ["empack.yml", "empack.lock"].map(|name| fs::read(target.join(name)).unwrap());
        for (index, (_, kind, folder)) in cases.iter().enumerate() {
            let key = DependencyKey::parse(&format!("curseforge:{}", 100 + index)).unwrap();
            let locked = &project.lock().dependencies[&key];
            assert_eq!(locked.kind, *kind);
            assert_eq!(
                locked.selected.as_ref().unwrap().project.to_string(),
                (100 + index).to_string()
            );
            let file = &locked.files.as_slice()[0];
            let placement = &file.placements.as_slice()[0];
            assert_eq!(
                placement.destination.relative().as_str(),
                format!("{folder}/fixture.zip")
            );
            for requirement in [
                &placement.requirements.client,
                &placement.requirements.server,
            ] {
                let Requirement::Optional(choice) = requirement else {
                    panic!("optional requirement lost")
                };
                assert!(choice.default_enabled);
            }
            assert_eq!(
                file.expected.digests.as_ref().unwrap().values()[0].algorithm(),
                empack_core::digest::DigestAlgorithm::Md5
            );
            assert_eq!(
                fs::read(target.join(format!("pack/{folder}/fixture.zip"))).unwrap(),
                b"payload"
            );
        }
        for _ in 0..2 {
            super::super::synchronize(&session, false).await.unwrap();
        }
        build(
            &session,
            &crate::application::BuildArgs {
                targets: vec!["modrinth".into()],
                ..Default::default()
            },
            BuildDecisions {
                mrpack_optional: OptionalConversion::AcknowledgedMetadataLoss,
                ..Default::default()
            },
            BuildAcquisitions::default(),
        )
        .await
        .unwrap();
        let mut export =
            zip::ZipArchive::new(fs::File::open(target.join("dist/Imported-1.mrpack")).unwrap())
                .unwrap();
        let mut bytes = Vec::new();
        export
            .by_name("modrinth.index.json")
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        let index: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let files = index["files"].as_array().unwrap();
        assert_eq!(files.len(), cases.len());
        for (_, _, folder) in cases {
            let file = files
                .iter()
                .find(|file| file["path"] == format!("{folder}/fixture.zip"))
                .unwrap();
            assert_eq!(file["env"]["client"], "optional");
            assert_eq!(file["env"]["server"], "optional");
            assert_eq!(file["fileSize"], 7);
            assert!(file["hashes"]["sha512"].as_str().is_some());
        }
        for (index, name) in ["empack.yml", "empack.lock"].into_iter().enumerate() {
            assert_eq!(fs::read(target.join(name)).unwrap(), original[index]);
        }
    }
    #[tokio::test]
    async fn provider_world_import_interprets_verified_members_before_publication() {
        use crate::application::engine_host::{BuildDecisions, build, tests::snapshot};
        use crate::engine::{
            acquisition::HttpAcquisition, build::BuildAcquisitions, import::ImportLimits,
            providers::ProviderCatalog,
        };
        use empack_core::model::{AcquisitionSpec, PlacementIntent};
        use serde_json::json;
        use sha2::Digest;
        use std::{
            fs,
            io::{Cursor, Read, Write},
        };
        for mode in [
            "preview",
            "publish",
            "ambiguous",
            "digest",
            "missing-folder",
        ] {
            let root = tempfile::tempdir().unwrap();
            let session = super::super::tests::session(root.path(), mode == "preview");
            let mut server = mockito::Server::new_async().await;
            let mut world = zip::ZipWriter::new(Cursor::new(Vec::new()));
            let mut members = vec![
                ("Original/level.dat", b"world".as_slice()),
                ("Original/region/r.0.0.mca", b"region".as_slice()),
            ];
            if mode == "ambiguous" {
                members.push(("Another/level.dat", b"second"));
            }
            for (path, bytes) in members {
                world
                    .start_file(path, zip::write::SimpleFileOptions::default())
                    .unwrap();
                world.write_all(bytes).unwrap();
            }
            let world = world.finish().unwrap().into_inner();
            let digest = md5::Md5::digest(&world)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>();
            fs::write(
                root.path().join("supplied.zip"),
                if mode == "digest" {
                    b"wrong".as_slice()
                } else {
                    world.as_slice()
                },
            )
            .unwrap();
            server.mock("GET", "/mods/42").with_body(json!({"data":{"id":42,"gameId":432,"classId":17,"slug":"adventure","name":"Adventure"}}).to_string()).create_async().await;
            server.mock("GET", "/mods/42/files/456").with_body(json!({"data":{"id":456,"gameId":432,"modId":42,"fileName":"original.zip","fileLength":world.len(),"hashes":[{"algo":2,"value":digest}],"downloadUrl":null,"gameVersions":["1.21.1"],"dependencies":[]}}).to_string()).create_async().await;
            let manifest = json!({"manifestVersion":1,"manifestType":"minecraftModpack","name":"World Pack","version":"1","files":[{"projectID":42,"fileID":456,"required":true}],"minecraft":{"version":"1.21.1","modLoaders":[]},"overrides":"overrides"});
            let path = root.path().join("pack.zip");
            let mut source = zip::ZipWriter::new(fs::File::create(&path).unwrap());
            source
                .start_file("manifest.json", zip::write::SimpleFileOptions::default())
                .unwrap();
            source
                .write_all(&serde_json::to_vec(&manifest).unwrap())
                .unwrap();
            source.finish().unwrap();
            let args = InitArgs {
                world_folder: (mode != "missing-folder").then(|| "saves".into()),
                ..Default::default()
            };
            let before = snapshot(root.path());
            let result = super::super::super::import::import_with_services(
                &session,
                ImportHostRequest {
                    source: ImportSource::Local {
                        path,
                        expected: ExpectedContent {
                            digests: None,
                            size: None,
                            accepted_observation: None,
                        },
                    },
                    destination: None,
                    replacement: ProjectReplacementPolicy::RejectExisting,
                    evidence: SourceEvidencePolicy::Compatibility,
                    supplied: BTreeMap::new(),
                    local_files: vec![ImportLocalFile {
                        selector: "provider:curseforge:42:456:original.zip".into(),
                        source: "supplied.zip".into(),
                    }],
                },
                |content| decisions(&session, &args, content),
                ProviderCatalog::for_loopback_tests(&server.url(), Some("fixture".into())),
                HttpAcquisition::for_loopback_tests(),
                ImportLimits::default(),
            )
            .await;
            if mode == "preview" {
                result.unwrap();
                assert_eq!(snapshot(root.path()), before);
                continue;
            }
            if mode != "publish" {
                assert!(result.is_err(), "{mode}");
                if mode == "missing-folder" {
                    assert!(format!("{:#}", result.unwrap_err()).contains("--world-folder"));
                }
                assert_eq!(snapshot(root.path()), before);
                continue;
            }
            result.unwrap();
            let project = super::super::tests::project(root.path());
            let key = DependencyKey::parse("curseforge:42").unwrap();
            let dependency = &project.lock().dependencies[&key];
            assert_eq!(dependency.files.as_slice().len(), 2);
            assert!(matches!(
                project.intent().roots[&key].placement,
                PlacementIntent::ArchiveRoot(_)
            ));
            for file in dependency.files.as_slice() {
                let AcquisitionSpec::ProviderArchiveMember { archive, .. } = &file.acquisition
                else {
                    panic!("lost archive ownership")
                };
                assert_eq!(archive.slot.as_str(), "original.zip");
                assert_eq!(
                    archive.expected.digests.as_ref().unwrap().values()[0].algorithm(),
                    empack_core::digest::DigestAlgorithm::Md5
                );
                assert!(file.expected.digests.is_none());
            }
            let target = root.path().join("project");
            assert_eq!(
                fs::read(target.join("pack/saves/adventure/level.dat")).unwrap(),
                b"world"
            );
            let published = snapshot(&target);
            for _ in 0..2 {
                super::super::synchronize(&session, false).await.unwrap();
            }
            assert_eq!(snapshot(&target), published);
            build(
                &session,
                &crate::application::BuildArgs {
                    targets: vec!["modrinth".into()],
                    ..Default::default()
                },
                BuildDecisions::default(),
                BuildAcquisitions::default(),
            )
            .await
            .unwrap();
            let mut archive = zip::ZipArchive::new(
                fs::File::open(target.join("dist/World Pack-1.mrpack")).unwrap(),
            )
            .unwrap();
            let mut bytes = Vec::new();
            archive
                .by_name("overrides/saves/adventure/region/r.0.0.mca")
                .unwrap()
                .read_to_end(&mut bytes)
                .unwrap();
            assert_eq!(bytes, b"region");
        }
    }
}
