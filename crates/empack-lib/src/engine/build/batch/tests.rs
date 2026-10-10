use super::*;
use crate::engine::{
    artifacts::{ArchiveLimits, verify_archive},
    content::{InitialObservation, verify_stream},
    documents::DocumentCodec,
    mrpack::{AcquiredBuildFile, LockedFileKey, tests::project},
    project::ProjectReader,
    publication::{Publisher, RecoveryReader},
    snapshot::SnapshotLimits,
    templates::TemplateOptions,
};
use empack_core::{
    files::FilePermissions,
    inventory::OptionalPolicy,
    model::{DistributionArchive, ExpectedContent},
    path::PathSyntax,
};
use std::{fs, path::Path};
fn path(value: &str) -> PortableRelPath {
    PortableRelPath::parse(value, PathSyntax::ProjectContent).unwrap()
}
fn fixture(root: &Path) -> BuildAcquisitions {
    let initial = project(false, false);
    let mut intent = initial.intent().clone();
    intent.distribution.native = Some(empack_core::model::NativeDistributionIntent {
        pack_id: "test.pack".into(),
        java_major: 21,
        policies: std::collections::BTreeMap::new(),
    });
    let project = crate::engine::mrpack::tests::explicitly_placed(intent, initial.lock().clone());
    fs::write(
        root.join("empack.yml"),
        DocumentCodec.encode_intent(project.intent()).unwrap(),
    )
    .unwrap();
    fs::write(
        root.join("empack.lock"),
        DocumentCodec.encode_lock(&project).unwrap(),
    )
    .unwrap();
    let mut acquired = BuildAcquisitions::default();
    for (key, dependency) in &project.lock().dependencies {
        for file in dependency.files.as_slice() {
            let content = verify_stream(
                &mut b"payload".as_slice(),
                &ExpectedContent {
                    digests: None,
                    size: None,
                    accepted_observation: None,
                },
                100,
                SourceEvidencePolicy::Compatibility,
                InitialObservation::Accepted,
                &Cancellation::default(),
            )
            .unwrap();
            acquired.locked.insert(
                LockedFileKey {
                    dependency: key.clone(),
                    slot: file.slot.clone(),
                },
                AcquiredBuildFile {
                    content,
                    permissions: FilePermissions {
                        readonly: false,
                        executable: false,
                    },
                },
            );
        }
    }
    fs::create_dir_all(root.join("dist")).unwrap();
    fs::write(root.join("dist/pack.mrpack"), b"old mrpack").unwrap();
    fs::write(root.join("dist/client.zip"), b"old client").unwrap();
    acquired
}
fn capture(root: &Path, host: &Path) -> WorkspaceSnapshot {
    ProjectReader::new(RecoveryReader::new(host.join("private")))
        .capture_build(
            root,
            &[path("pack.mrpack"), path("client.zip")],
            SnapshotLimits::default(),
            &Cancellation::default(),
        )
        .unwrap()
}
fn requests() -> NonEmpty<DistributionRequest> {
    NonEmpty::new(vec![
        DistributionRequest::Mrpack {
            recipe: Recipe::MODRINTH,
            artifact: path("pack.mrpack"),
            optional: OptionalConversion::RejectMetadataLoss,
            evidence: SourceEvidencePolicy::Compatibility,
        },
        DistributionRequest::Prism {
            recipe: Recipe::PRISM_BUNDLED,
            artifact: path("client.zip"),
            options: ClientOptions {
                archive: DistributionArchive::Zip,
                optional: OptionalPolicy::Preserve,
                templates: TemplateOptions::default(),
                evidence: SourceEvidencePolicy::Compatibility,
                limits: ArchiveLimits::default(),
            },
        },
    ])
    .unwrap()
}
#[test]
fn requested_artifacts_prepare_and_publish_under_one_receipt() {
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    let external = fixture(root.path());
    let cancel = Cancellation::default();
    let batch = prepare_build_batch(
        capture(root.path(), host.path()),
        requests(),
        &external,
        &cancel,
    )
    .unwrap();
    assert_eq!(batch.artifacts().len(), 2);
    let expected: Vec<_> = batch
        .artifacts()
        .iter()
        .map(|output| (output.artifact.clone(), output.members.clone()))
        .collect();
    assert_eq!(
        fs::read(root.path().join("dist/pack.mrpack")).unwrap(),
        b"old mrpack"
    );
    assert_eq!(
        fs::read(root.path().join("dist/client.zip")).unwrap(),
        b"old client"
    );
    batch
        .publish(
            &Publisher::open(&host.path().join("private")).unwrap(),
            &cancel,
        )
        .unwrap();
    for (artifact, members) in expected {
        let mut file = fs::File::open(root.path().join("dist").join(artifact.as_str())).unwrap();
        verify_archive(
            &mut file,
            DistributionArchive::Zip,
            &members,
            ArchiveLimits::default(),
            &cancel,
        )
        .unwrap();
    }
}
#[test]
fn later_requested_failure_or_stale_input_retains_every_prior_artifact() {
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    let external = fixture(root.path());
    let cancel = Cancellation::default();
    // Reference mrpack can complete first, but full client still requires exact bytes.
    assert!(
        prepare_build_batch(
            capture(root.path(), host.path()),
            requests(),
            &BuildAcquisitions::default(),
            &cancel
        )
        .is_err()
    );
    assert!(!host.path().join("private").exists());
    assert_eq!(
        fs::read(root.path().join("dist/pack.mrpack")).unwrap(),
        b"old mrpack"
    );
    assert_eq!(
        fs::read(root.path().join("dist/client.zip")).unwrap(),
        b"old client"
    );
    let prepared = prepare_build_batch(
        capture(root.path(), host.path()),
        requests(),
        &external,
        &cancel,
    )
    .unwrap();
    fs::create_dir_all(root.path().join("pack/config")).unwrap();
    fs::write(root.path().join("pack/config/late"), b"late").unwrap();
    assert!(
        prepared
            .publish(
                &Publisher::open(&host.path().join("private")).unwrap(),
                &cancel
            )
            .is_err()
    );
    assert_eq!(
        fs::read(root.path().join("dist/pack.mrpack")).unwrap(),
        b"old mrpack"
    );
    assert_eq!(
        fs::read(root.path().join("dist/client.zip")).unwrap(),
        b"old client"
    );
}
#[test]
fn duplicate_output_ownership_is_rejected_before_preparation() {
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    let external = fixture(root.path());
    let request = || DistributionRequest::Mrpack {
        recipe: Recipe::MODRINTH,
        artifact: path("pack.mrpack"),
        optional: OptionalConversion::RejectMetadataLoss,
        evidence: SourceEvidencePolicy::Compatibility,
    };
    assert!(
        prepare_build_batch(
            capture(root.path(), host.path()),
            NonEmpty::new(vec![request(), request()]).unwrap(),
            &external,
            &Cancellation::default()
        )
        .is_err()
    );
    assert_eq!(
        fs::read(root.path().join("dist/pack.mrpack")).unwrap(),
        b"old mrpack"
    );
    assert!(!host.path().join("private").exists());
}

#[test]
fn native_client_joins_requested_publication_without_installer_tools() {
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    let external = fixture(root.path());
    let cancel = Cancellation::default();
    let requests = NonEmpty::new(vec![
        DistributionRequest::Mrpack {
            recipe: Recipe::MODRINTH,
            artifact: path("pack.mrpack"),
            optional: OptionalConversion::RejectMetadataLoss,
            evidence: SourceEvidencePolicy::Compatibility,
        },
        DistributionRequest::Prism {
            recipe: Recipe::PRISM_REFERENCES,
            artifact: path("client.zip"),
            options: ClientOptions {
                archive: DistributionArchive::Zip,
                optional: OptionalPolicy::Preserve,
                templates: TemplateOptions::default(),
                evidence: SourceEvidencePolicy::Compatibility,
                limits: ArchiveLimits::default(),
            },
        },
    ])
    .unwrap();
    let plan = prepare_build_batch(
        capture(root.path(), host.path()),
        requests,
        &external,
        &cancel,
    )
    .unwrap();
    assert_eq!(plan.artifacts()[1].target, Recipe::PRISM_REFERENCES);
    plan.publish(
        &Publisher::open(&host.path().join("private")).unwrap(),
        &cancel,
    )
    .unwrap();
    assert_ne!(
        fs::read(root.path().join("dist/client.zip")).unwrap(),
        b"old client"
    );
    assert_ne!(
        fs::read(root.path().join("dist/pack.mrpack")).unwrap(),
        b"old mrpack"
    );
}

#[test]
fn server_and_client_candidates_share_publication_and_reject_late_collisions() {
    use crate::engine::{build::server, server_runtime::tests::prepared_fixture};
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    let cancel = Cancellation::default();
    let external = server::tests::fixture(root.path());
    fs::create_dir_all(root.path().join("dist")).unwrap();
    fs::write(root.path().join("dist/client.zip"), b"previous client").unwrap();
    fs::write(root.path().join("dist/server.zip"), b"previous server").unwrap();
    let capture = || {
        ProjectReader::new(RecoveryReader::new(host.path().join("private")))
            .capture_build(
                root.path(),
                &[path("client.zip"), path("server.zip")],
                SnapshotLimits::default(),
                &cancel,
            )
            .unwrap()
    };
    let requests = || {
        NonEmpty::new(vec![
            DistributionRequest::Prism {
                recipe: Recipe::PRISM_BUNDLED,
                artifact: path("client.zip"),
                options: ClientOptions {
                    archive: DistributionArchive::Zip,
                    optional: OptionalPolicy::Preserve,
                    templates: TemplateOptions::default(),
                    evidence: SourceEvidencePolicy::Compatibility,
                    limits: ArchiveLimits::default(),
                },
            },
            DistributionRequest::Server {
                recipe: Recipe::SERVER_BUNDLED,
                artifact: path("server.zip"),
                options: server::ServerOptions {
                    archive: DistributionArchive::Zip,
                    optional: OptionalPolicy::Preserve,
                    templates: TemplateOptions::default(),
                    evidence: SourceEvidencePolicy::Compatibility,
                    limits: ArchiveLimits::default(),
                },
                runtime: Box::new(prepared_fixture()),
            },
        ])
        .unwrap()
    };
    fs::create_dir_all(root.path().join("templates/server/game")).unwrap();
    fs::write(
        root.path().join("templates/server/game/server.jar"),
        b"bad runtime",
    )
    .unwrap();
    assert!(prepare_build_batch(capture(), requests(), &external, &cancel).is_err());
    assert_eq!(
        fs::read(root.path().join("dist/client.zip")).unwrap(),
        b"previous client"
    );
    assert_eq!(
        fs::read(root.path().join("dist/server.zip")).unwrap(),
        b"previous server"
    );
    fs::remove_file(root.path().join("templates/server/game/server.jar")).unwrap();
    let batch = prepare_build_batch(capture(), requests(), &external, &cancel).unwrap();
    assert!(batch.artifacts()[1].server_runtime.is_some());
    batch
        .publish(
            &Publisher::open(&host.path().join("private")).unwrap(),
            &cancel,
        )
        .unwrap();
    assert_ne!(
        fs::read(root.path().join("dist/client.zip")).unwrap(),
        b"previous client"
    );
    assert_ne!(
        fs::read(root.path().join("dist/server.zip")).unwrap(),
        b"previous server"
    );
}

#[test]
fn interrupted_clean_build_finishes_or_restores_the_same_publication() {
    use crate::engine::publication::{PublicationPoint, tests::interrupt_publication};
    for restore in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let host = tempfile::tempdir().unwrap();
        let external = fixture(root.path());
        fs::write(root.path().join("dist/aaa-old.zip"), b"obsolete artifact").unwrap();
        let cancel = Cancellation::default();
        let snapshot = ProjectReader::new(RecoveryReader::new(host.path().join("private")))
            .capture_build_selection(
                root.path(),
                &[path("pack.mrpack"), path("client.zip")],
                true,
                SnapshotLimits::default(),
                SnapshotLimits::default(),
                &cancel,
            )
            .unwrap();
        let removals = [ManagedPath::Artifact(path("aaa-old.zip"))]
            .into_iter()
            .collect();
        let batch =
            prepare_build_batch_with_cleanup(snapshot, requests(), &external, &removals, &cancel)
                .unwrap();
        let publisher = Publisher::open(&host.path().join("private")).unwrap();
        let prepared = batch.publication;
        assert!(
            interrupt_publication(
                &publisher,
                &prepared.root,
                prepared.change,
                PublicationPoint::TargetChanged
            )
            .is_err()
        );
        assert!(publisher.recovery_required(&prepared.root).unwrap());
        if restore {
            publisher.restore_before_images(&prepared.root).unwrap();
            assert_eq!(
                fs::read(root.path().join("dist/aaa-old.zip")).unwrap(),
                b"obsolete artifact"
            );
            assert_eq!(
                fs::read(root.path().join("dist/pack.mrpack")).unwrap(),
                b"old mrpack"
            );
            assert_eq!(
                fs::read(root.path().join("dist/client.zip")).unwrap(),
                b"old client"
            );
        } else {
            publisher.recover(&prepared.root).unwrap();
            assert!(!root.path().join("dist/aaa-old.zip").exists());
            for artifact in ["pack.mrpack", "client.zip"] {
                assert!(
                    zip::ZipArchive::new(
                        fs::File::open(root.path().join("dist").join(artifact)).unwrap()
                    )
                    .is_ok()
                );
            }
        }
        assert!(!publisher.recovery_required(&prepared.root).unwrap());
    }
}

#[test]
fn native_and_platform_exports_share_publication_and_preserve_prior_outputs_on_failure() {
    for authority in [UpdateAuthority::Snapshot, UpdateAuthority::Empack] {
        let recipe = Recipe::EMPACK_BUNDLED
            .with_update_authority(authority)
            .unwrap();
        let root = tempfile::tempdir().unwrap();
        let host = tempfile::tempdir().unwrap();
        let external = fixture(root.path());
        fs::write(root.path().join("dist/native.empack"), b"prior native").unwrap();
        let cancel = Cancellation::default();
        let capture = || {
            ProjectReader::new(RecoveryReader::new(host.path().join("state")))
                .capture_build(
                    root.path(),
                    &[path("pack.mrpack"), path("native.empack")],
                    SnapshotLimits::default(),
                    &cancel,
                )
                .unwrap()
        };
        let requests = || {
            NonEmpty::new(vec![
                DistributionRequest::Mrpack {
                    recipe: Recipe::MODRINTH,
                    artifact: path("pack.mrpack"),
                    optional: OptionalConversion::RejectMetadataLoss,
                    evidence: SourceEvidencePolicy::Compatibility,
                },
                DistributionRequest::Native {
                    artifact: path("native.empack"),
                    recipe,
                    archive: DistributionArchive::Zip,
                    evidence: SourceEvidencePolicy::Compatibility,
                    limits: ArchiveLimits::default(),
                },
            ])
            .unwrap()
        };
        assert!(
            prepare_build_batch(
                capture(),
                requests(),
                &BuildAcquisitions::default(),
                &cancel
            )
            .is_err()
        );
        assert_eq!(
            fs::read(root.path().join("dist/pack.mrpack")).unwrap(),
            b"old mrpack"
        );
        assert_eq!(
            fs::read(root.path().join("dist/native.empack")).unwrap(),
            b"prior native"
        );
        let batch = prepare_build_batch(capture(), requests(), &external, &cancel).unwrap();
        let release_id = batch.artifacts()[1].native_release.clone().unwrap();
        assert_eq!(batch.artifacts()[1].content.target(), recipe);
        assert_eq!(
            batch.artifacts()[1]
                .content
                .entries()
                .iter()
                .map(|file| file.destination.relative().as_str())
                .collect::<Vec<_>>(),
            [
                "resourcepacks/a.zip",
                "resourcepacks/b.zip",
                "resourcepacks/copy.zip"
            ]
        );
        assert_eq!(
            fs::read(root.path().join("dist/native.empack")).unwrap(),
            b"prior native"
        );
        batch
            .publish(
                &Publisher::open(&host.path().join("state")).unwrap(),
                &cancel,
            )
            .unwrap();
        let mut archive =
            zip::ZipArchive::new(fs::File::open(root.path().join("dist/native.empack")).unwrap())
                .unwrap();
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut archive.by_name("release.json").unwrap(), &mut bytes)
            .unwrap();
        let release = crate::engine::release::DecodedRelease::decode(&bytes).unwrap();
        assert_eq!(release.id(), release_id);
        assert_eq!(
            release.document().require_subscription,
            authority == UpdateAuthority::Empack
        );
        assert_eq!(release.document().files.len(), 3);
        assert!(
            release
                .document()
                .files
                .iter()
                .all(|file| file.asset.is_some())
        );
    }
}

#[test]
fn recipe_identity_cannot_be_reinterpreted_by_another_adapter() {
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    let acquired = fixture(root.path());
    for recipe in [Recipe::MODRINTH, Recipe::SERVER_BUNDLED] {
        let request = DistributionRequest::Prism {
            recipe,
            artifact: path("client.zip"),
            options: ClientOptions {
                archive: DistributionArchive::Zip,
                optional: OptionalPolicy::Preserve,
                templates: TemplateOptions::default(),
                evidence: SourceEvidencePolicy::Compatibility,
                limits: ArchiveLimits::default(),
            },
        };
        assert!(
            prepare_build_batch(
                capture(root.path(), host.path()),
                NonEmpty::new(vec![request]).unwrap(),
                &acquired,
                &Cancellation::default()
            )
            .is_err()
        );
        assert_eq!(
            fs::read(root.path().join("dist/client.zip")).unwrap(),
            b"old client"
        );
    }
}

#[test]
fn subscribed_prism_recipes_preserve_delivery_and_require_local_enrollment() {
    use std::io::Read;
    for base in [Recipe::PRISM_REFERENCES, Recipe::PRISM_BUNDLED] {
        let root = tempfile::tempdir().unwrap();
        let host = tempfile::tempdir().unwrap();
        let external = fixture(root.path());
        let recipe = base.with_update_authority(UpdateAuthority::Empack).unwrap();
        let request = DistributionRequest::Prism {
            recipe,
            artifact: path("client.zip"),
            options: ClientOptions {
                archive: DistributionArchive::Zip,
                optional: OptionalPolicy::Preserve,
                templates: TemplateOptions::default(),
                evidence: SourceEvidencePolicy::Compatibility,
                limits: ArchiveLimits::default(),
            },
        };
        let batch = prepare_build_batch(
            capture(root.path(), host.path()),
            NonEmpty::new(vec![request]).unwrap(),
            &external,
            &Cancellation::default(),
        )
        .unwrap();
        assert_eq!(batch.artifacts()[0].target, recipe);
        assert_eq!(batch.artifacts()[0].content.target(), recipe);
        batch
            .publish(
                &Publisher::open(&host.path().join("private")).unwrap(),
                &Cancellation::default(),
            )
            .unwrap();
        let mut zip =
            zip::ZipArchive::new(fs::File::open(root.path().join("dist/client.zip")).unwrap())
                .unwrap();
        let mut ini = String::new();
        zip.by_name("instance.cfg")
            .unwrap()
            .read_to_string(&mut ini)
            .unwrap();
        assert!(ini.contains("--require-subscription"));
        assert!(ini.contains("instance launch --check-updates --"));
        let mut payload = Vec::new();
        zip.by_name(".minecraft/.empack-consumer/release.json")
            .unwrap()
            .read_to_end(&mut payload)
            .unwrap();
        let release = crate::engine::release::DecodedRelease::decode(&payload).unwrap();
        for file in &release.document().files {
            assert_eq!(
                file.asset_path().is_some(),
                base.delivery() == empack_core::distribution::Delivery::Bundled
            );
        }
        assert!(
            !zip.file_names()
                .any(|name| name.ends_with("subscription.json"))
        );
    }
}
