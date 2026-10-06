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
            options: ClientFullOptions {
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
