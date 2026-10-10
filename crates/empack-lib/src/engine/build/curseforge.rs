//! CurseForge client snapshots: exact references plus authored, selected override bytes.
use super::{
    ArchiveCandidate, BuildAcquisitions, batch::BuiltDistribution,
    materialized::prepare_bootstrap_game_content,
};
use crate::{
    application::process_runtime::Cancellation,
    engine::{
        artifacts::{ArchiveLimits, write_archive},
        content::{InitialObservation, SourceEvidencePolicy, verify_stream},
        layout::CollisionIndex,
        mrpack::OptionalConversion,
        project::WorkspaceSnapshot,
        snapshot::SnapshotLimits,
        staging::{MutableStage, PrivateFile},
    },
};
use anyhow::{Context, Result, bail, ensure};
use empack_core::{
    files::{FileContent, FilePermissions},
    identity::{PinSelector, ProviderProjectId},
    inventory::{ContentOwner, DownloadOrigins, OptionalPolicy, Representation},
    model::{ContentKind, DistributionArchive, ExpectedContent, LoaderKind},
    path::{PathSyntax, PortableRelPath},
    projection::BuildTarget,
    requirements::Requirement,
};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

pub struct CurseForgeOptions {
    pub optional: OptionalPolicy,
    pub conversion: OptionalConversion,
    pub evidence: SourceEvidencePolicy,
    pub limits: ArchiveLimits,
}

pub(super) fn prepare_archive(
    workspace: &WorkspaceSnapshot,
    artifact: PortableRelPath,
    external: &BuildAcquisitions,
    options: &CurseForgeOptions,
    cancel: &Cancellation,
) -> Result<(ArchiveCandidate, BuiltDistribution)> {
    ensure!(
        artifact.as_str().ends_with(".zip"),
        "CurseForge output requires a .zip filename"
    );
    let game = prepare_bootstrap_game_content(
        workspace,
        external,
        BuildTarget::CurseForge,
        &options.optional,
        options.evidence,
        cancel,
    )?;
    ensure!(
        game.observed().is_empty(),
        "CurseForge export requires owned content; adopt observed files first"
    );
    let mut references = BTreeMap::new();
    let mut stage = MutableStage::empty()?;
    let mut members = BTreeMap::new();
    let mut collisions = CollisionIndex::default();
    let mut conversions = BTreeSet::new();
    for entry in game.inventory().entries() {
        cancel.check()?;
        let required = match &entry.requirements.client {
            Requirement::Required => true,
            Requirement::Unsupported => {
                bail!("Unsupported client content reached CurseForge export")
            }
            Requirement::Optional(choice) => {
                ensure!(
                    options.conversion == OptionalConversion::AcknowledgedMetadataLoss,
                    "CurseForge cannot preserve optional group/default metadata for {}; resolve choices or acknowledge optional metadata conversion",
                    entry.destination.relative().as_str()
                );
                conversions.insert(format!(
                    "CurseForge optional file loses group/default metadata: {}",
                    choice.key.as_str()
                ));
                false
            }
        };
        match &entry.representation {
            Representation::Download {
                allowed: DownloadOrigins::Provider { pin, slot },
                ..
            } => {
                let (ProviderProjectId::CurseForge(project), PinSelector::CurseForgeFile(file)) =
                    (&pin.project, &pin.selection)
                else {
                    bail!(
                        "CurseForge references require exact CurseForge project and file identities"
                    );
                };
                let ContentOwner::Dependency { key, .. } = &entry.owner else {
                    bail!(
                        "CurseForge export requires an owned dependency record; adopt observed content first"
                    );
                };
                ensure!(
                    game.files()
                        .get(entry.destination.relative())
                        .is_none_or(
                            |file| !file.permissions.readonly && !file.permissions.executable
                        ),
                    "CurseForge references cannot preserve custom file permissions"
                );
                let dependency = &game.project().lock().dependencies[key];
                let folder = match dependency.kind {
                    ContentKind::Mod => "mods",
                    ContentKind::ResourcePack => "resourcepacks",
                    ContentKind::ShaderPack => "shaderpacks",
                    _ => bail!(
                        "CurseForge cannot represent the placement of {}",
                        key.as_str()
                    ),
                };
                ensure!(
                    entry.destination.relative().as_str() == format!("{folder}/{}", slot.as_str()),
                    "CurseForge references cannot preserve a renamed or relocated provider file: {}",
                    entry.destination.relative().as_str()
                );
                ensure!(
                    references
                        .insert(project.get(), (file.get(), required))
                        .is_none(),
                    "CurseForge cannot represent multiple selected files from project {project}"
                );
            }
            Representation::Embedded {
                content,
                bytes,
                permissions,
            } => {
                if let ContentOwner::Dependency { key, .. } = &entry.owner {
                    ensure!(
                        matches!(
                            game.project().lock().dependencies[key].identity,
                            empack_core::model::ResolvedIdentity::Local(_)
                        ),
                        "CurseForge overrides require authored local content; acquired provider bytes retain provider ownership"
                    );
                }
                ensure!(
                    required,
                    "CurseForge overrides cannot preserve optional participation; resolve choices first"
                );
                ensure!(
                    !permissions.readonly && !permissions.executable,
                    "CurseForge overrides cannot guarantee portable file permissions"
                );
                let path = PortableRelPath::parse(
                    &format!("overrides/{}", entry.destination.relative().as_str()),
                    PathSyntax::ArchiveMember,
                )?;
                collisions.insert_file(&path)?;
                let file = game
                    .files()
                    .get(entry.destination.relative())
                    .context("Missing captured override bytes")?;
                stage.write(
                    &path,
                    &mut file.content.lease().open(),
                    options.limits.file_bytes,
                    cancel,
                )?;
                members.insert(
                    path,
                    FileContent {
                        content: content.clone(),
                        bytes: *bytes,
                        permissions: *permissions,
                    },
                );
            }
            _ => bail!(
                "CurseForge cannot represent {}; use exact CurseForge references or authored local overrides",
                entry.destination.relative().as_str()
            ),
        }
    }
    let runtime = &game.project().lock().runtime;
    let loaders = match runtime.loader {
        LoaderKind::Vanilla => Vec::new(),
        kind => {
            let prefix = match kind {
                LoaderKind::Fabric => "fabric",
                LoaderKind::Forge => "forge",
                LoaderKind::NeoForge => "neoforge",
                LoaderKind::Quilt => "quilt",
                LoaderKind::Vanilla => unreachable!(),
            };
            vec![
                json!({"id":format!("{prefix}-{}", runtime.loader_version.as_ref().context("Missing exact loader version")?.as_str()), "primary":true}),
            ]
        }
    };
    let metadata = &game.project().intent().metadata;
    let manifest = serde_json::to_vec_pretty(&json!({
        "manifestType":"minecraftModpack", "manifestVersion":1,
        "name":metadata.name, "version":metadata.version, "author":metadata.author.as_deref().unwrap_or(""),
        "minecraft":{"version":runtime.minecraft.as_str(), "modLoaders":loaders},
        "files":references.into_iter().map(|(project,(file,required))| json!({"projectID":project,"fileID":file,"required":required})).collect::<Vec<_>>(),
        "overrides":"overrides"
    }))?;
    let generated = verify_stream(
        &mut manifest.as_slice(),
        &ExpectedContent {
            digests: None,
            size: Some(manifest.len() as u64),
            accepted_observation: None,
        },
        16 << 20,
        SourceEvidencePolicy::Compatibility,
        InitialObservation::Accepted,
        cancel,
    )?;
    let path = PortableRelPath::parse("manifest.json", PathSyntax::ArchiveMember)?;
    stage.write(
        &path,
        &mut generated.lease().open(),
        generated.lease().len(),
        cancel,
    )?;
    members.insert(
        path,
        FileContent {
            content: generated.lease().id(),
            bytes: generated.lease().len(),
            permissions: FilePermissions {
                readonly: false,
                executable: false,
            },
        },
    );
    let mut frozen = stage.freeze(
        SnapshotLimits {
            file_bytes: options.limits.file_bytes,
            total_bytes: options.limits.total_bytes,
            entries: options.limits.entries,
            depth: options.limits.depth,
        },
        cancel,
    )?;
    let mut archive = PrivateFile::new()?;
    let verified = write_archive(
        &mut frozen,
        archive.file(),
        DistributionArchive::Zip,
        &members,
        options.limits,
        cancel,
    )?;
    let receipt = BuiltDistribution {
        target: BuildTarget::CurseForge,
        artifact: artifact.clone(),
        bytes: verified.len(),
        content: game.inventory().clone(),
        members,
        resolution: game.project().lock().clone(),
        observed: game.observed().to_vec(),
        backend_comparisons: game.backend_comparisons().to_vec(),
        conversions: conversions.into_iter().collect(),
        user_configuration: None,
        toolchain: Vec::new(),
        server_runtime: None,
    };
    Ok((
        ArchiveCandidate {
            artifact,
            archive,
            verified,
        },
        receipt,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{
        build::batch::{DistributionRequest, prepare_build_batch},
        documents::DocumentCodec,
        mrpack::tests::{explicitly_placed, project},
        project::ProjectReader,
        publication::{Publisher, RecoveryReader},
    };
    use empack_core::{
        identity::{CurseForgeFileId, CurseForgeProjectId},
        model::*,
        path::InstallDestination,
    };
    use std::{fs, io::Read, path::Path};

    fn path(value: &str) -> PortableRelPath {
        PortableRelPath::parse(value, PathSyntax::ProjectContent).unwrap()
    }
    fn fixture(optional: bool, renamed: bool) -> ResolvedProject {
        let base = project(false, optional);
        let mut intent = base.intent().clone();
        let mut lock = base.lock().clone();
        let key = DependencyKey::parse("assets").unwrap();
        let identity = ProviderProjectId::CurseForge(CurseForgeProjectId::parse("123").unwrap());
        let pin = ResolvedPin {
            project: identity.clone(),
            selection: PinSelector::CurseForgeFile(CurseForgeFileId::parse("456").unwrap()),
        };
        intent.roots.get_mut(&key).unwrap().source = SourceIntent::Provider(identity.clone());
        let dep = lock.dependencies.get_mut(&key).unwrap();
        dep.identity = ResolvedIdentity::Provider(identity);
        dep.selected = Some(pin.clone());
        let mut file = dep.files.as_slice()[0].clone();
        file.slot = FileSlot::parse("assets.zip").unwrap();
        file.acquisition = AcquisitionSpec::Provider {
            pin,
            slot: file.slot.clone(),
            alternatives: vec!["https://example.invalid/assets.zip".into()],
        };
        let mut placement = file.placements.as_slice()[0].clone();
        placement.destination = InstallDestination::parse(if renamed {
            "resourcepacks/renamed.zip"
        } else {
            "resourcepacks/assets.zip"
        })
        .unwrap();
        file.placements = NonEmpty::new(vec![placement]).unwrap();
        dep.files = NonEmpty::new(vec![file]).unwrap();
        explicitly_placed(intent, lock)
    }
    fn write(root: &Path, project: &ResolvedProject) {
        fs::write(
            root.join("empack.yml"),
            DocumentCodec.encode_intent(project.intent()).unwrap(),
        )
        .unwrap();
        fs::write(
            root.join("empack.lock"),
            DocumentCodec.encode_lock(project).unwrap(),
        )
        .unwrap();
        for (name, bytes) in [
            ("pack/config/options.txt", "common"),
            ("overrides/client/config/options.txt", "client"),
            ("overrides/server/config/options.txt", "server"),
            ("overrides/server/server-only.txt", "server only"),
        ] {
            let file = root.join(name);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, bytes).unwrap();
        }
    }
    fn capture(root: &Path, host: &Path) -> WorkspaceSnapshot {
        ProjectReader::new(RecoveryReader::new(host.join("private")))
            .capture_build(
                root,
                &[path("curseforge.zip"), path("first.mrpack")],
                SnapshotLimits::default(),
                &Cancellation::default(),
            )
            .unwrap()
    }
    fn request(conversion: OptionalConversion) -> DistributionRequest {
        DistributionRequest::CurseForge {
            artifact: path("curseforge.zip"),
            options: CurseForgeOptions {
                optional: OptionalPolicy::Preserve,
                conversion,
                evidence: SourceEvidencePolicy::Compatibility,
                limits: ArchiveLimits::default(),
            },
        }
    }
    #[test]
    fn exact_references_and_client_overrides_publish_without_downloads_or_installers() {
        let root = tempfile::tempdir().unwrap();
        let host = tempfile::tempdir().unwrap();
        write(root.path(), &fixture(false, false));
        let cancel = Cancellation::default();
        let snapshot = capture(root.path(), host.path());
        let external = BuildAcquisitions::default();
        let acquisition = super::super::acquisition::plan_target_build_acquisitions(
            &snapshot,
            &external,
            BuildTarget::CurseForge,
            &OptionalPolicy::Preserve,
            SourceEvidencePolicy::Compatibility,
            &cancel,
        )
        .unwrap();
        assert!(acquisition.needs().is_empty());
        let batch = prepare_build_batch(
            snapshot,
            NonEmpty::new(vec![request(OptionalConversion::RejectMetadataLoss)]).unwrap(),
            &external,
            &cancel,
        )
        .unwrap();
        assert!(batch.artifacts()[0].toolchain.is_empty());
        assert_eq!(batch.artifacts()[0].members.len(), 2);
        batch
            .publish(
                &Publisher::open(&host.path().join("private")).unwrap(),
                &cancel,
            )
            .unwrap();
        let mut zip =
            zip::ZipArchive::new(fs::File::open(root.path().join("dist/curseforge.zip")).unwrap())
                .unwrap();
        let manifest: serde_json::Value =
            serde_json::from_reader(zip.by_name("manifest.json").unwrap()).unwrap();
        assert_eq!(
            manifest["files"],
            json!([{"projectID":123,"fileID":456,"required":true}])
        );
        assert_eq!(
            manifest["minecraft"]["modLoaders"],
            json!([{"id":"fabric-0.16.0","primary":true}])
        );
        let mut bytes = String::new();
        zip.by_name("overrides/config/options.txt")
            .unwrap()
            .read_to_string(&mut bytes)
            .unwrap();
        assert_eq!(bytes, "client");
        assert!(zip.by_name("overrides/server-only.txt").is_err());
    }
    #[test]
    fn optional_reference_requires_explicit_metadata_conversion() {
        let root = tempfile::tempdir().unwrap();
        let host = tempfile::tempdir().unwrap();
        write(root.path(), &fixture(true, false));
        let cancel = Cancellation::default();
        assert!(
            prepare_build_batch(
                capture(root.path(), host.path()),
                NonEmpty::new(vec![request(OptionalConversion::RejectMetadataLoss)]).unwrap(),
                &BuildAcquisitions::default(),
                &cancel
            )
            .is_err()
        );
        let (mut archive, receipt) = prepare_archive(
            &capture(root.path(), host.path()),
            path("curseforge.zip"),
            &BuildAcquisitions::default(),
            &CurseForgeOptions {
                optional: OptionalPolicy::Preserve,
                conversion: OptionalConversion::AcknowledgedMetadataLoss,
                evidence: SourceEvidencePolicy::Compatibility,
                limits: ArchiveLimits::default(),
            },
            &cancel,
        )
        .unwrap();
        assert_eq!(receipt.conversions.len(), 1);
        use std::io::{Seek, SeekFrom};
        archive.archive.file().seek(SeekFrom::Start(0)).unwrap();
        let mut zip = zip::ZipArchive::new(archive.archive.file()).unwrap();
        let manifest: serde_json::Value =
            serde_json::from_reader(zip.by_name("manifest.json").unwrap()).unwrap();
        assert_eq!(manifest["files"][0]["required"], false);
    }
    #[test]
    fn late_incompatible_export_preserves_every_previous_artifact() {
        let root = tempfile::tempdir().unwrap();
        let host = tempfile::tempdir().unwrap();
        // A URL-backed pack is representable by mrpack, but has no CurseForge identity.
        write(root.path(), &project(false, false));
        fs::create_dir_all(root.path().join("dist")).unwrap();
        fs::write(root.path().join("dist/first.mrpack"), b"previous mrpack").unwrap();
        fs::write(
            root.path().join("dist/curseforge.zip"),
            b"previous curseforge",
        )
        .unwrap();
        let result = prepare_build_batch(
            capture(root.path(), host.path()),
            NonEmpty::new(vec![
                DistributionRequest::Mrpack {
                    artifact: path("first.mrpack"),
                    optional: OptionalConversion::RejectMetadataLoss,
                    evidence: SourceEvidencePolicy::Compatibility,
                },
                request(OptionalConversion::RejectMetadataLoss),
            ])
            .unwrap(),
            &BuildAcquisitions::default(),
            &Cancellation::default(),
        );
        let error = result.err().unwrap();
        assert!(
            format!("{error:#}").contains("CurseForge cannot represent"),
            "{error:#}"
        );
        assert_eq!(
            fs::read(root.path().join("dist/first.mrpack")).unwrap(),
            b"previous mrpack"
        );
        assert_eq!(
            fs::read(root.path().join("dist/curseforge.zip")).unwrap(),
            b"previous curseforge"
        );
    }
    #[test]
    fn renamed_provider_file_is_not_silently_restored_under_original_name() {
        let root = tempfile::tempdir().unwrap();
        let host = tempfile::tempdir().unwrap();
        write(root.path(), &fixture(false, true));
        let error = prepare_build_batch(
            capture(root.path(), host.path()),
            NonEmpty::new(vec![request(OptionalConversion::RejectMetadataLoss)]).unwrap(),
            &BuildAcquisitions::default(),
            &Cancellation::default(),
        )
        .err()
        .unwrap();
        assert!(format!("{error:#}").contains("renamed or relocated"));
    }
    #[test]
    fn acquired_provider_content_does_not_become_an_authored_override() {
        let root = tempfile::tempdir().unwrap();
        let host = tempfile::tempdir().unwrap();
        let project = fixture(false, false);
        let mut lock = project.lock().clone();
        let key = DependencyKey::parse("assets").unwrap();
        let dependency = lock.dependencies.get_mut(&key).unwrap();
        let mut file = dependency.files.as_slice()[0].clone();
        file.acquisition = AcquisitionSpec::Manual {
            pin: None,
            instructions: "Supply the exact restricted provider file".into(),
        };
        dependency.files = NonEmpty::new(vec![file.clone()]).unwrap();
        let project = explicitly_placed(project.intent().clone(), lock);
        write(root.path(), &project);
        let cancel = Cancellation::default();
        let content = verify_stream(
            &mut b"payload".as_slice(),
            &file.expected,
            100,
            SourceEvidencePolicy::Compatibility,
            InitialObservation::Accepted,
            &cancel,
        )
        .unwrap();
        let mut external = BuildAcquisitions::default();
        external.locked.insert(
            crate::engine::mrpack::LockedFileKey {
                dependency: key,
                slot: file.slot,
            },
            crate::engine::mrpack::AcquiredBuildFile {
                content,
                permissions: FilePermissions {
                    readonly: false,
                    executable: false,
                },
            },
        );
        let result = prepare_build_batch(
            capture(root.path(), host.path()),
            NonEmpty::new(vec![request(OptionalConversion::RejectMetadataLoss)]).unwrap(),
            &external,
            &cancel,
        );
        let error = result
            .err()
            .expect("Provider ownership must survive acquisition");
        assert!(format!("{error:#}").contains("authored local"), "{error:#}");
    }
    #[test]
    fn restricted_file_with_exact_pin_stays_a_reference_after_acquisition() {
        let root = tempfile::tempdir().unwrap();
        let host = tempfile::tempdir().unwrap();
        let project = fixture(false, false);
        let mut lock = project.lock().clone();
        let key = DependencyKey::parse("assets").unwrap();
        let dependency = lock.dependencies.get_mut(&key).unwrap();
        let mut file = dependency.files.as_slice()[0].clone();
        file.acquisition = AcquisitionSpec::Manual {
            pin: dependency.selected.clone(),
            instructions: "Supply the exact restricted provider file".into(),
        };
        dependency.files = NonEmpty::new(vec![file.clone()]).unwrap();
        let project = explicitly_placed(project.intent().clone(), lock);
        write(root.path(), &project);
        let cancel = Cancellation::default();
        let content = verify_stream(
            &mut b"payload".as_slice(),
            &file.expected,
            100,
            SourceEvidencePolicy::Compatibility,
            InitialObservation::Accepted,
            &cancel,
        )
        .unwrap();
        let mut external = BuildAcquisitions::default();
        external.locked.insert(
            crate::engine::mrpack::LockedFileKey {
                dependency: key,
                slot: file.slot,
            },
            crate::engine::mrpack::AcquiredBuildFile {
                content,
                permissions: FilePermissions {
                    readonly: false,
                    executable: false,
                },
            },
        );
        let result = prepare_build_batch(
            capture(root.path(), host.path()),
            NonEmpty::new(vec![request(OptionalConversion::RejectMetadataLoss)]).unwrap(),
            &external,
            &cancel,
        );
        let batch = result.unwrap();
        assert!(
            batch.artifacts()[0]
                .content
                .entries()
                .iter()
                .any(|entry| matches!(
                    entry.representation,
                    Representation::Download {
                        allowed: DownloadOrigins::Provider { .. },
                        ..
                    }
                ))
        );
        assert!(
            !batch.artifacts()[0]
                .members
                .keys()
                .any(|path| path.as_str().starts_with("overrides/resourcepacks/"))
        );
    }
}
