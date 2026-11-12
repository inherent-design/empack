use super::*;
use crate::{
    engine::{
        content::{InitialObservation, SourceEvidencePolicy, verify_stream},
        publication::{
            PublicationPoint,
            tests::{interrupt_publication, prepare},
        },
    },
    test_support::file_bytes,
};
use std::fs;
fn install(root: &Path, state: &Path, layout: InstanceLayout) {
    let release = super::super::publication_tests::release(false);
    let selected = super::super::super::release::trust::SelectedSnapshot::select(
        release.bytes(),
        release.id(),
        &semver::Version::parse(crate::engine::compatibility_version()).unwrap(),
    )
    .unwrap();
    let cancel = Cancellation::default();
    let plan = plan(
        root,
        InstanceSelection {
            require_subscription: false,
            conflicts: vec![],
            release: SelectedRelease::Snapshot(selected),
            side: InstanceSide::Client,
            layout: Some(layout),
            choices: vec![],
            action: InstanceAction::Apply,
        },
        RecoveryReader::new(state.into()),
        SnapshotLimits::default(),
        &cancel,
    )
    .unwrap();
    let supplied = release
        .document()
        .files
        .iter()
        .map(|file| {
            let mut bytes = super::super::publication_tests::input(&file.key, false);
            (
                file.key.clone(),
                verify_stream(
                    &mut bytes,
                    &file.expected().unwrap(),
                    file.bytes,
                    SourceEvidencePolicy::Compatibility,
                    InitialObservation::RequireEvidence,
                    &cancel,
                )
                .unwrap(),
            )
        })
        .collect();
    plan.stage(&supplied, &BTreeMap::new(), None, &cancel)
        .unwrap()
        .publish(&Publisher::open(state).unwrap(), &cancel)
        .unwrap();
}
fn checked(root: &Path, state: &Path) -> InstanceCheck {
    check(
        root,
        RecoveryReader::new(state.into()),
        SnapshotLimits::default(),
        &Cancellation::default(),
        &|_| {},
    )
}
#[test]
fn checking_preserves_player_content_and_detects_missing_or_modified_managed_files() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("instance");
    let state = tmp.path().join("state");
    fs::create_dir(&root).unwrap();
    install(&root, &state, InstanceLayout::Game);
    fs::write(root.join("game/config/settings.txt"), b"player choices").unwrap();
    fs::remove_file(root.join("game/saves/played/level.dat")).unwrap();
    let before = file_bytes(tmp.path());
    let report = checked(&root, &state);
    assert!(report.result.is_ok(), "{:?}", report.result);
    assert_eq!(report.report.verified_files, 1);
    assert_eq!(report.report.player_owned_entries, 2);
    assert_eq!(file_bytes(tmp.path()), before);
    for bytes in [Some(&b"modified"[..]), None] {
        if let Some(bytes) = bytes {
            fs::write(root.join("game/mods/a.jar"), bytes).unwrap();
        } else {
            fs::remove_file(root.join("game/mods/a.jar")).unwrap();
        }
        let before = file_bytes(tmp.path());
        let report = checked(&root, &state);
        assert!(report.result.is_err());
        assert!(!report.report.complete);
        assert!(
            report
                .report
                .findings
                .iter()
                .any(|f| f.subject == "mods/a.jar" && f.state == CheckState::Failed)
        );
        assert_eq!(file_bytes(tmp.path()), before);
    }
}
#[test]
fn accepted_overrides_are_checked_against_their_saved_identity() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("instance");
    let state = tmp.path().join("state");
    fs::create_dir(&root).unwrap();
    install(&root, &state, InstanceLayout::Game);
    fs::write(root.join("game/mods/a.jar"), b"local version").unwrap();
    let release = super::super::publication_tests::release(false);
    let selected = super::super::super::release::trust::SelectedSnapshot::select(
        release.bytes(),
        release.id(),
        &semver::Version::parse(crate::engine::compatibility_version()).unwrap(),
    )
    .unwrap();
    let cancel = Cancellation::default();
    let plan = plan(
        &root,
        InstanceSelection {
            require_subscription: false,
            conflicts: vec![ConflictResolution {
                destination: "mods/a.jar".into(),
                choice: ConflictChoice::Preserve,
            }],
            release: SelectedRelease::Snapshot(selected),
            side: InstanceSide::Client,
            layout: None,
            choices: vec![],
            action: InstanceAction::Repair,
        },
        RecoveryReader::new(state.clone()),
        SnapshotLimits::default(),
        &cancel,
    )
    .unwrap();
    plan.stage(&BTreeMap::new(), &BTreeMap::new(), None, &cancel)
        .unwrap()
        .publish(&Publisher::open(&state).unwrap(), &cancel)
        .unwrap();
    let before = file_bytes(tmp.path());
    let report = checked(&root, &state);
    assert!(report.result.is_ok(), "{:?}", report.result);
    assert_eq!(report.report.verified_overrides, 1);
    assert_eq!(report.report.verified_files, 0);
    assert_eq!(file_bytes(tmp.path()), before);
    fs::write(root.join("game/mods/a.jar"), b"changed again").unwrap();
    let before = file_bytes(tmp.path());
    assert!(checked(&root, &state).result.is_err());
    assert_eq!(file_bytes(tmp.path()), before);
}
#[test]
fn runtime_evidence_remains_owned_and_is_never_cleared_by_checking() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("instance");
    let state = tmp.path().join("state");
    fs::create_dir(&root).unwrap();
    install(&root, &state, InstanceLayout::Game);
    let publisher = Publisher::open(&state).unwrap();
    let read = ProjectReadRoot::open(&root).unwrap();
    let mut lease = publisher.lease_instance(&read).unwrap();
    lease.begin().unwrap();
    let before = file_bytes(tmp.path());
    let report = checked(&root, &state);
    assert!(report.result.is_err());
    assert_eq!(report.report.verified_files, 0);
    assert_eq!(file_bytes(tmp.path()), before);
    lease.complete().unwrap();
    assert!(checked(&root, &state).result.is_ok());
    let mut lease = publisher.lease_instance(&read).unwrap();
    lease.begin().unwrap();
    drop(lease);
    let before = file_bytes(tmp.path());
    for _ in 0..2 {
        assert!(checked(&root, &state).result.is_err());
        assert_eq!(file_bytes(tmp.path()), before);
    }
}
#[test]
fn interrupted_publication_and_damaged_records_are_reported_without_recovery() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("instance");
    let state = tmp.path().join("state");
    fs::create_dir(&root).unwrap();
    install(&root, &state, InstanceLayout::Game);
    let original = fs::read(root.join(".empack/instance.json")).unwrap();
    fs::write(root.join(".empack/instance.json"), b"{truncated").unwrap();
    let before = file_bytes(tmp.path());
    assert!(checked(&root, &state).result.is_err());
    assert_eq!(file_bytes(tmp.path()), before);
    fs::write(root.join(".empack/instance.json"), original).unwrap();
    let read = ProjectReadRoot::open(&root).unwrap();
    fs::write(root.join("empack.yml"), b"old intent").unwrap();
    let publisher = Publisher::open(&state).unwrap();
    assert!(
        interrupt_publication(
            &publisher,
            &read,
            prepare(&read),
            PublicationPoint::TargetChanged
        )
        .is_err()
    );
    let before = file_bytes(tmp.path());
    let result = checked(&root, &state);
    assert!(result.result.is_err());
    assert!(!result.report.complete);
    assert!(publisher.inspect_recovery(&read).unwrap().is_some());
    assert_eq!(file_bytes(tmp.path()), before);
}
#[test]
fn prism_checks_admit_launcher_cache_saves_but_refuse_runtime_changes() {
    for change_runtime in [false, true] {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("instance");
        let state = tmp.path().join("state");
        fs::create_dir(&root).unwrap();
        install(&root, &state, InstanceLayout::Prism);
        let path = root.join("mmc-pack.json");
        let report = check(
            &root,
            RecoveryReader::new(state.clone()),
            SnapshotLimits::default(),
            &Cancellation::default(),
            &|event| {
                if let WorkProgress::CheckingProjectFile { total, .. } = event
                    && total > 0
                {
                    let mut value: serde_json::Value =
                        serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
                    if change_runtime {
                        value["components"][0]["version"] = "1.21.3".into();
                    } else {
                        value["components"][0]["cachedName"] = "Launcher save".into();
                    }
                    fs::write(&path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
                }
            },
        );
        assert_eq!(
            report.result.is_err(),
            change_runtime,
            "{:?}",
            report.result
        );
        assert_eq!(fs::read(root.join(".minecraft/mods/a.jar")).unwrap(), b"A");
    }
}
#[test]
fn player_owned_changes_during_the_check_do_not_invalidate_managed_content() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("instance");
    let state = tmp.path().join("state");
    fs::create_dir(&root).unwrap();
    install(&root, &state, InstanceLayout::Game);
    let report = check(
        &root,
        RecoveryReader::new(state),
        SnapshotLimits::default(),
        &Cancellation::default(),
        &|event| {
            if let WorkProgress::CheckingProjectFile { total, .. } = event
                && total > 0
            {
                fs::write(
                    root.join("game/config/settings.txt"),
                    b"player saving settings",
                )
                .unwrap();
                fs::write(root.join("game/saves/played/level.dat"), b"player world").unwrap();
            }
        },
    );
    assert!(report.result.is_ok(), "{:?}", report.result);
}
