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

#[test]
fn artifact_only_publication_binds_read_only_sources_without_copying_or_replacing_them() {
    use crate::engine::publication::Publisher;
    for edit_source in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("project");
        fs::create_dir(&project).unwrap();
        fs::write(project.join("local-input.jar"), b"input").unwrap();
        fs::write(project.join("empack.yml"), b"intent").unwrap();
        let root = ProjectReadRoot::open(&project).unwrap();
        let cancel = Cancellation::default();
        let base = root
            .capture(
                &[path("local-input.jar"), path("empack.yml"), path("dist")],
                SnapshotLimits::default(),
                &cancel,
            )
            .unwrap();
        let target = ManagedPath::Artifact(path("pack.mrpack"));
        let observed = observed_artifacts_for(&base, [target.clone()]).unwrap();
        let plan = plan_files(
            &observed,
            &BTreeMap::from([(target, wanted(b"artifact"))]),
            &BTreeSet::new(),
        )
        .unwrap();
        let mut stage = MutableStage::empty().unwrap();
        stage
            .write(&path("dist/pack.mrpack"), &mut &b"artifact"[..], 8, &cancel)
            .unwrap();
        let verified = VerifiedFileChange::verify_artifacts(
            base,
            plan,
            stage.freeze(SnapshotLimits::default(), &cancel).unwrap(),
        )
        .unwrap();
        if edit_source {
            fs::write(project.join("local-input.jar"), b"changed").unwrap();
        }
        let publisher = Publisher::open(&temp.path().join("private-state")).unwrap();
        let result = publisher.publish(&root, verified, &cancel);
        assert_eq!(result.is_err(), edit_source, "{result:?}");
        assert_eq!(fs::read(project.join("empack.yml")).unwrap(), b"intent");
        if edit_source {
            assert!(!project.join("dist/pack.mrpack").exists());
        } else {
            assert_eq!(
                fs::read(project.join("dist/pack.mrpack")).unwrap(),
                b"artifact"
            );
            assert_eq!(fs::read(project.join("local-input.jar")).unwrap(), b"input");
        }
    }
}

#[test]
fn archive_preflight_honors_captured_budgets_without_widening_source_limits() {
    // Synthetic content sizes exercise large-file accounting without allocating the payload.
    let project = tempfile::tempdir().unwrap();
    fs::write(project.path().join("empack.yml"), b"old").unwrap();
    let root = ProjectReadRoot::open(project.path()).unwrap();
    let cancel = Cancellation::default();
    let huge = 65 * 1024 * 1024 * 1024;
    let sources = root
        .capture(
            &[path("empack.yml")],
            SnapshotLimits {
                file_bytes: 3,
                total_bytes: 3,
                ..SnapshotLimits::default()
            },
            &cancel,
        )
        .unwrap();
    let outputs = root
        .capture(
            &[path("dist")],
            SnapshotLimits {
                file_bytes: huge,
                total_bytes: huge + 7,
                ..SnapshotLimits::default()
            },
            &cancel,
        )
        .unwrap();
    let base = NativeSnapshot::merge(sources, outputs).unwrap();
    let first = ManagedPath::Artifact(path("client.zip"));
    let second = ManagedPath::Artifact(path("pack.mrpack"));
    let mut large = wanted(b"synthetic");
    large.bytes = huge;
    let mut desired =
        BTreeMap::from([(first.clone(), large), (second.clone(), wanted(b"archive"))]);
    let observed = observed_artifacts_for(&base, desired.keys().cloned()).unwrap();
    let prepare = |desired: &BTreeMap<ManagedPath, FileContent>| {
        candidate_stage_limits(
            &base,
            &plan_files(&observed, desired, &BTreeSet::new()).unwrap(),
        )
    };
    let allowed = prepare(&desired).unwrap();
    assert_eq!(allowed.file_bytes, huge);
    assert_eq!(allowed.total_bytes, huge + 7);
    assert_eq!(allowed.entries, 3); // dist and two leaves
    assert_eq!(allowed.depth, 2);
    desired.get_mut(&second).unwrap().bytes = 8;
    assert!(
        prepare(&desired)
            .unwrap_err()
            .to_string()
            .contains("read budget")
    );
    desired.get_mut(&second).unwrap().bytes = 0;
    desired.get_mut(&first).unwrap().bytes = huge + 1;
    assert!(
        prepare(&desired)
            .unwrap_err()
            .to_string()
            .contains("read budget")
    );
    let source_plan = plan_files(
        &observed_files(&base).unwrap(),
        &BTreeMap::from([(ManagedPath::IntentDocument, wanted(b"wider"))]),
        &BTreeSet::new(),
    )
    .unwrap();
    assert!(
        candidate_stage_limits(&base, &source_plan)
            .unwrap_err()
            .to_string()
            .contains("read budget")
    );
}

#[test]
fn filtered_membership_cannot_prove_an_excluded_file_is_absent() {
    let project = tempfile::tempdir().unwrap();
    fs::create_dir(project.path().join("pack")).unwrap();
    fs::write(
        project.path().join("pack/unselected.jar"),
        b"existing content",
    )
    .unwrap();
    let root = ProjectReadRoot::open(project.path()).unwrap();
    let filter =
        crate::engine::source::CaptureFilter::mutation(&[path("pack/selected.jar")]).unwrap();
    let captured = root
        .capture_filtered(
            &[path("pack")],
            SnapshotLimits::default(),
            Some(&filter),
            &Cancellation::default(),
        )
        .unwrap();
    let target = |name| ManagedPath::Content {
        layer: empack_core::model::ContentLayer::Common,
        path: path(name),
    };
    assert!(
        observed_mutation_for(&captured, [target("unselected.jar")]).is_err(),
        "excluded bytes are not an absence observation"
    );
    assert!(matches!(
        observed_mutation_for(&captured, [target("selected.jar")]).unwrap()
            [&target("selected.jar")],
        ObservedPath::Absent
    ));
}
