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
    let project = project(false, false);
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
            artifact: path("pack.mrpack"),
            optional: OptionalConversion::RejectMetadataLoss,
            evidence: SourceEvidencePolicy::Compatibility,
        },
        DistributionRequest::ClientFull {
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
fn bootstrap_client_joins_requested_publication_with_tool_evidence() {
    use crate::engine::{bootstrap_tools::InstallerAssets, packwiz::InstallerInteraction};
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    let external = fixture(root.path());
    let cancel = Cancellation::default();
    let requests = NonEmpty::new(vec![
        DistributionRequest::Mrpack {
            artifact: path("pack.mrpack"),
            optional: OptionalConversion::RejectMetadataLoss,
            evidence: SourceEvidencePolicy::Compatibility,
        },
        DistributionRequest::Client {
            artifact: path("client.zip"),
            options: ClientOptions {
                archive: DistributionArchive::Zip,
                optional: OptionalPolicy::Preserve,
                templates: TemplateOptions::default(),
                evidence: SourceEvidencePolicy::Compatibility,
                limits: ArchiveLimits::default(),
            },
            bootstrap: ClientBootstrap {
                assets: InstallerAssets::fixture(),
                interaction: InstallerInteraction::Headless,
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
    assert_eq!(plan.artifacts()[1].toolchain.len(), 2);
    assert_eq!(plan.artifacts()[1].target, BuildTarget::Client);
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
            DistributionRequest::ClientFull {
                artifact: path("client.zip"),
                options: ClientOptions {
                    archive: DistributionArchive::Zip,
                    optional: OptionalPolicy::Preserve,
                    templates: TemplateOptions::default(),
                    evidence: SourceEvidencePolicy::Compatibility,
                    limits: ArchiveLimits::default(),
                },
            },
            DistributionRequest::ServerFull {
                artifact: path("server.zip"),
                options: server::ServerOptions {
                    archive: DistributionArchive::Zip,
                    optional: OptionalPolicy::Preserve,
                    templates: TemplateOptions::default(),
                    evidence: SourceEvidencePolicy::Compatibility,
                    limits: ArchiveLimits::default(),
                },
                runtime: prepared_fixture(),
            },
        ])
        .unwrap()
    };
    fs::create_dir_all(root.path().join("templates/server")).unwrap();
    fs::write(
        root.path().join("templates/server/server.jar"),
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
    fs::remove_file(root.path().join("templates/server/server.jar")).unwrap();
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
