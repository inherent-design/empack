use super::*;
use crate::{
    application::process_runtime::Cancellation,
    engine::{
        snapshot::{ProjectReadRoot, SnapshotLimits},
        staging::MutableStage,
    },
};
use empack_core::path::{PathSyntax, PortableRelPath};
use sha2::{Digest, Sha256};
use std::fs;
fn path(value: &str) -> PortableRelPath {
    PortableRelPath::parse(value, PathSyntax::ProjectContent).unwrap()
}
fn wanted(value: &[u8]) -> FileContent {
    FileContent {
        content: ContentId::from_sha256(Sha256::digest(value).into()),
        bytes: value.len() as u64,
        permissions: FilePermissions {
            readonly: false,
            executable: false,
        },
    }
}
#[test]
fn missing_extra_and_corrupt_candidate_files_cannot_produce_a_proof() {
    for mode in ["missing", "extra", "corrupt"] {
        let project = tempfile::tempdir().unwrap();
        fs::write(project.path().join("empack.yml"), b"before").unwrap();
        let root = ProjectReadRoot::open(project.path()).unwrap();
        let cancel = Cancellation::default();
        let base = root
            .capture(&[path("empack.yml")], SnapshotLimits::default(), &cancel)
            .unwrap();
        let plan = plan_files(
            &observed_files(&base).unwrap(),
            &BTreeMap::from([(ManagedPath::IntentDocument, wanted(b"after"))]),
            &BTreeSet::new(),
        )
        .unwrap();
        let mut stage = MutableStage::empty().unwrap();
        if mode != "missing" {
            stage
                .write(
                    &path("empack.yml"),
                    &mut if mode == "corrupt" {
                        &b"wrong"[..]
                    } else {
                        &b"after"[..]
                    },
                    5,
                    &cancel,
                )
                .unwrap();
        }
        if mode == "extra" {
            stage
                .write(&path("unplanned"), &mut &b"extra"[..], 5, &cancel)
                .unwrap();
        }
        assert!(
            VerifiedFileChange::verify(
                base,
                plan,
                stage.freeze(SnapshotLimits::default(), &cancel).unwrap()
            )
            .is_err(),
            "{mode}"
        );
        assert_eq!(
            fs::read(project.path().join("empack.yml")).unwrap(),
            b"before"
        );
    }
}
#[test]
fn a_plan_from_another_observation_set_is_rejected_even_if_its_stage_matches() {
    let project = tempfile::tempdir().unwrap();
    fs::write(project.path().join("empack.yml"), b"before").unwrap();
    let root = ProjectReadRoot::open(project.path()).unwrap();
    let cancel = Cancellation::default();
    let base = root
        .capture(&[path("empack.yml")], SnapshotLimits::default(), &cancel)
        .unwrap();
    let fake_observation = BTreeMap::from([(
        ManagedPath::IntentDocument,
        ObservedPath::File(wanted(b"fake")),
    )]);
    let plan = plan_files(&fake_observation, &BTreeMap::new(), &BTreeSet::new()).unwrap();
    assert!(plan.changes().is_empty());
    let mut stage = MutableStage::empty().unwrap();
    stage
        .write(&path("empack.yml"), &mut &b"fake"[..], 4, &cancel)
        .unwrap();
    assert!(
        VerifiedFileChange::verify(
            base,
            plan,
            stage.freeze(SnapshotLimits::default(), &cancel).unwrap()
        )
        .is_err()
    );
}

#[test]
fn candidate_cannot_exceed_the_read_budget_needed_for_postpublication_checks() {
    let project = tempfile::tempdir().unwrap();
    fs::write(project.path().join("empack.yml"), b"old").unwrap();
    let root = ProjectReadRoot::open(project.path()).unwrap();
    let cancel = Cancellation::default();
    let base = root
        .capture(
            &[path("empack.yml")],
            SnapshotLimits {
                file_bytes: 3,
                ..SnapshotLimits::default()
            },
            &cancel,
        )
        .unwrap();
    let plan = plan_files(
        &observed_files(&base).unwrap(),
        &BTreeMap::from([(ManagedPath::IntentDocument, wanted(b"larger"))]),
        &BTreeSet::new(),
    )
    .unwrap();
    let mut stage = MutableStage::empty().unwrap();
    stage
        .write(&path("empack.yml"), &mut &b"larger"[..], 6, &cancel)
        .unwrap();
    assert!(
        VerifiedFileChange::verify(
            base,
            plan,
            stage.freeze(SnapshotLimits::default(), &cancel).unwrap()
        )
        .is_err()
    );
    assert_eq!(fs::read(project.path().join("empack.yml")).unwrap(), b"old");
}
