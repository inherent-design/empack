use super::*;
use crate::engine::{
    staging::MutableStage,
    verification::{VerifiedFileChange, observed_files_for, plan_files},
};
use empack_core::{
    digest::ContentId,
    files::{FilePermissions, ManagedPath},
    model::ContentLayer,
};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, fs};
fn path(input: &str) -> PortableRelPath {
    PortableRelPath::parse(input, PathSyntax::ProjectContent).unwrap()
}
fn expected(bytes: &[u8]) -> FileContent {
    FileContent {
        content: ContentId::from_sha256(Sha256::digest(bytes).into()),
        bytes: bytes.len() as u64,
        permissions: FilePermissions {
            readonly: false,
            executable: false,
        },
    }
}
fn prepare(root: &ProjectReadRoot) -> VerifiedFileChange {
    let cancel = Cancellation::default();
    let snapshot = root
        .capture(
            &[path("empack.yml"), path("pack")],
            SnapshotLimits::default(),
            &cancel,
        )
        .unwrap();
    let added = ManagedPath::Content {
        layer: ContentLayer::Common,
        path: path("config/new.txt"),
    };
    let desired = BTreeMap::from([
        (ManagedPath::IntentDocument, expected(b"new intent")),
        (added, expected(b"new content")),
    ]);
    let observed = observed_files_for(&snapshot, desired.keys().cloned()).unwrap();
    let plan = plan_files(&observed, &desired, &BTreeSet::new()).unwrap();
    let mut stage = MutableStage::from_snapshot(root, &snapshot, &cancel).unwrap();
    stage
        .write(&path("empack.yml"), &mut &b"new intent"[..], 10, &cancel)
        .unwrap();
    stage
        .write(
            &path("pack/config/new.txt"),
            &mut &b"new content"[..],
            11,
            &cancel,
        )
        .unwrap();
    let frozen = stage.freeze(SnapshotLimits::default(), &cancel).unwrap();
    VerifiedFileChange::verify(snapshot, plan, frozen).unwrap()
}
fn fixture(project: &Path) {
    fs::create_dir_all(project).unwrap();
    fs::write(project.join("empack.yml"), b"old intent").unwrap();
}

#[test]
fn publishes_new_ancestors_and_replacements_then_verifies_and_retains_recovery_data() {
    let project = tempfile::tempdir().unwrap();
    fixture(project.path());
    let state = tempfile::tempdir().unwrap();
    let publisher = Publisher::open(&state.path().join("private")).unwrap();
    let root = ProjectReadRoot::open(project.path()).unwrap();
    assert!(!publisher.recovery_required(&root).unwrap());
    assert_eq!(
        fs::read_dir(state.path().join("private")).unwrap().count(),
        0,
        "read gate must not create state"
    );
    let receipt = publisher
        .publish(&root, prepare(&root), &Cancellation::default())
        .unwrap();
    assert_eq!(receipt.changed_files, 2);
    assert_eq!(
        fs::read(project.path().join("empack.yml")).unwrap(),
        b"new intent"
    );
    assert_eq!(
        fs::read(project.path().join("pack/config/new.txt")).unwrap(),
        b"new content"
    );
    assert!(!publisher.recovery_required(&root).unwrap());
    assert_eq!(publisher.recover(&root).unwrap(), receipt);
    let state = publisher.project_state(&root).unwrap();
    let retained = state.open_dir_nofollow(receipt.operation).unwrap();
    assert_eq!(retained.read("before-0").unwrap(), b"old intent");
}

#[test]
fn stale_base_and_missing_candidate_never_publish() {
    let project = tempfile::tempdir().unwrap();
    fixture(project.path());
    let state = tempfile::tempdir().unwrap();
    let publisher = Publisher::open(&state.path().join("private")).unwrap();
    let root = ProjectReadRoot::open(project.path()).unwrap();
    let prepared = prepare(&root);
    fs::write(project.path().join("empack.yml"), b"external edit").unwrap();
    assert!(
        publisher
            .publish(&root, prepared, &Cancellation::default())
            .is_err()
    );
    assert_eq!(
        fs::read(project.path().join("empack.yml")).unwrap(),
        b"external edit"
    );
    assert!(!project.path().join("pack").exists());
    assert!(!publisher.recovery_required(&root).unwrap());
}

#[test]
fn progress_flags_do_not_authorize_overwriting_an_external_edit() {
    let project = tempfile::tempdir().unwrap();
    fixture(project.path());
    let state = tempfile::tempdir().unwrap();
    let publisher = Publisher::open(&state.path().join("private")).unwrap();
    let root = ProjectReadRoot::open(project.path()).unwrap();
    let error = publisher.publish_with_hook(
        &root,
        prepare(&root),
        &Cancellation::default(),
        &mut |point| {
            if point == PublicationPoint::TargetChanged {
                anyhow::bail!("injected crash");
            }
            Ok(())
        },
    );
    assert!(error.is_err());
    assert!(publisher.recovery_required(&root).unwrap());
    fs::write(project.path().join("empack.yml"), b"user edit!").unwrap();
    assert!(publisher.recover(&root).is_err());
    assert_eq!(
        fs::read(project.path().join("empack.yml")).unwrap(),
        b"user edit!"
    );
    assert!(!project.path().join("pack").exists());
}

#[test]
fn actual_process_crashes_recover_at_every_durable_boundary() {
    for (point, occurrence) in [
        (PublicationPoint::RecoveryDataDurable, 1),
        (PublicationPoint::IntentDurable, 1),
        (PublicationPoint::SiblingWritten, 1),
        (PublicationPoint::SiblingSynced, 1),
        (PublicationPoint::TargetChanged, 1),
        (PublicationPoint::DirectorySynced, 1),
        (PublicationPoint::ProgressDurable, 1),
        (PublicationPoint::Verified, 1),
        (PublicationPoint::Committed, 1),
        (PublicationPoint::SiblingWritten, 2),
        (PublicationPoint::SiblingSynced, 2),
        (PublicationPoint::TargetChanged, 2),
        (PublicationPoint::ProgressDurable, 2),
    ] {
        let project = tempfile::tempdir().unwrap();
        fixture(project.path());
        let state = tempfile::tempdir().unwrap();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "engine::publication::tests::crash_worker",
                "--nocapture",
            ])
            .env("EMPACK_PUBLICATION_CRASH_PROJECT", project.path())
            .env(
                "EMPACK_PUBLICATION_CRASH_STATE",
                state.path().join("private"),
            )
            .env("EMPACK_PUBLICATION_CRASH_POINT", format!("{point:?}"))
            .env(
                "EMPACK_PUBLICATION_CRASH_OCCURRENCE",
                occurrence.to_string(),
            )
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::inherit())
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(86), "worker failed before {point:?}");
        let root = ProjectReadRoot::open(project.path()).unwrap();
        let publisher = Publisher::open(&state.path().join("private")).unwrap();
        if point == PublicationPoint::RecoveryDataDurable {
            assert!(!publisher.recovery_required(&root).unwrap());
            assert_eq!(
                fs::read(project.path().join("empack.yml")).unwrap(),
                b"old intent"
            );
            assert!(!project.path().join("pack").exists());
        } else {
            publisher.recover(&root).unwrap();
            assert!(!publisher.recovery_required(&root).unwrap());
            assert_eq!(
                fs::read(project.path().join("empack.yml")).unwrap(),
                b"new intent"
            );
            assert_eq!(
                fs::read(project.path().join("pack/config/new.txt")).unwrap(),
                b"new content"
            );
        }
    }
}
#[test]
fn crash_worker() {
    let Some(project) = std::env::var_os("EMPACK_PUBLICATION_CRASH_PROJECT") else {
        return;
    };
    let state = std::env::var_os("EMPACK_PUBLICATION_CRASH_STATE").unwrap();
    let point = std::env::var("EMPACK_PUBLICATION_CRASH_POINT").unwrap();
    let occurrence: u32 = std::env::var("EMPACK_PUBLICATION_CRASH_OCCURRENCE")
        .unwrap()
        .parse()
        .unwrap();
    let mut seen = 0;
    let publisher = Publisher::open(Path::new(&state)).unwrap();
    let root = ProjectReadRoot::open(Path::new(&project)).unwrap();
    let mut hook = |actual| {
        if format!("{actual:?}") == point {
            seen += 1;
            if seen == occurrence {
                std::process::exit(86);
            }
        }
        Ok(())
    };
    if std::env::var_os("EMPACK_PUBLICATION_RESTORE").is_some() {
        publisher.restore_with_hook(&root, &mut hook).unwrap();
    } else {
        publisher
            .publish_with_hook(&root, prepare(&root), &Cancellation::default(), &mut hook)
            .unwrap();
    }
    panic!("crash point not reached");
}

#[test]
fn corrupted_candidates_and_unknown_journals_block_recovery_without_overwrite() {
    let project = tempfile::tempdir().unwrap();
    fixture(project.path());
    let state = tempfile::tempdir().unwrap();
    let publisher = Publisher::open(&state.path().join("private")).unwrap();
    let root = ProjectReadRoot::open(project.path()).unwrap();
    assert!(
        publisher
            .publish_with_hook(
                &root,
                prepare(&root),
                &Cancellation::default(),
                &mut |point| {
                    if point == PublicationPoint::IntentDurable {
                        anyhow::bail!("stop before live effects");
                    }
                    Ok(())
                }
            )
            .is_err()
    );
    let state = publisher.project_state(&root).unwrap();
    let journal = load_journal(&state).unwrap().unwrap();
    let retained = state.open_dir_nofollow(&journal.operation).unwrap();
    retained.write("after-1", b"wrong data!").unwrap();
    assert!(publisher.recover(&root).is_err());
    assert_eq!(
        fs::read(project.path().join("empack.yml")).unwrap(),
        b"old intent"
    );
    assert!(!project.path().join("pack").exists());
    let mut corrupt = journal;
    corrupt.schema = 500;
    write_journal(&state, &corrupt).unwrap();
    assert!(publisher.recovery_required(&root).is_err());
    assert!(publisher.recover(&root).is_err());
    state.write("journal.json", b"{ truncated").unwrap();
    assert!(publisher.recover(&root).is_err());
}

#[test]
fn explicit_removal_is_one_file_and_a_busy_project_cannot_publish() {
    let project = tempfile::tempdir().unwrap();
    fixture(project.path());
    fs::create_dir(project.path().join("pack")).unwrap();
    fs::write(project.path().join("pack/selected"), b"remove").unwrap();
    fs::write(project.path().join("pack/retained"), b"keep").unwrap();
    let state = tempfile::tempdir().unwrap();
    let publisher = Publisher::open(&state.path().join("private")).unwrap();
    let root = ProjectReadRoot::open(project.path()).unwrap();
    let cancel = Cancellation::default();
    let snapshot = root
        .capture(&[path("pack")], SnapshotLimits::default(), &cancel)
        .unwrap();
    let selected = ProjectLayout::classify(&path("pack/selected")).unwrap();
    let observed = observed_files_for(&snapshot, [selected.clone()]).unwrap();
    let plan = plan_files(&observed, &BTreeMap::new(), &BTreeSet::from([selected])).unwrap();
    let mut stage = MutableStage::from_snapshot(&root, &snapshot, &cancel).unwrap();
    stage.remove(&path("pack/selected")).unwrap();
    let proof = VerifiedFileChange::verify(
        snapshot,
        plan,
        stage.freeze(SnapshotLimits::default(), &cancel).unwrap(),
    )
    .unwrap();
    let state = publisher.project_state(&root).unwrap();
    let lease = lock(&state).unwrap();
    assert!(lock(&state).is_err());
    drop(lease);
    publisher.publish(&root, proof, &cancel).unwrap();
    assert!(!project.path().join("pack/selected").exists());
    assert_eq!(
        fs::read(project.path().join("pack/retained")).unwrap(),
        b"keep"
    );
    assert_eq!(
        fs::read(project.path().join("empack.yml")).unwrap(),
        b"old intent"
    );
}

#[test]
fn missing_preview_state_is_not_created() {
    let directory = tempfile::tempdir().unwrap();
    let absent = directory.path().join("state");
    assert!(Publisher::open_existing(&absent).unwrap().is_none());
    assert!(!absent.exists());
}

#[test]
fn unchanged_input_conflicts_are_detected_before_resuming_any_replacement() {
    let project = tempfile::tempdir().unwrap();
    fixture(project.path());
    fs::create_dir(project.path().join("pack")).unwrap();
    fs::write(project.path().join("pack/retained"), b"original").unwrap();
    let state = tempfile::tempdir().unwrap();
    let publisher = Publisher::open(&state.path().join("private")).unwrap();
    let root = ProjectReadRoot::open(project.path()).unwrap();
    let error = publisher
        .publish_with_hook(
            &root,
            prepare(&root),
            &Cancellation::default(),
            &mut |point| {
                if point == PublicationPoint::IntentDurable {
                    anyhow::bail!("injected crash");
                }
                Ok(())
            },
        )
        .err()
        .unwrap();
    assert!(error.downcast_ref::<RecoveryRequired>().is_some());
    fs::write(project.path().join("pack/retained"), b"useredit").unwrap();
    assert!(publisher.recover(&root).is_err());
    assert_eq!(
        fs::read(project.path().join("empack.yml")).unwrap(),
        b"old intent"
    );
    assert_eq!(
        fs::read(project.path().join("pack/retained")).unwrap(),
        b"useredit"
    );
    assert!(!project.path().join("pack/config/new.txt").exists());
}

#[test]
fn interrupted_restoration_itself_recovers_and_reclamation_preserves_receipt() {
    for point in [
        PublicationPoint::IntentDurable,
        PublicationPoint::SiblingWritten,
        PublicationPoint::SiblingSynced,
        PublicationPoint::TargetChanged,
        PublicationPoint::ProgressDurable,
        PublicationPoint::Verified,
        PublicationPoint::Committed,
    ] {
        let project = tempfile::tempdir().unwrap();
        fixture(project.path());
        let state = tempfile::tempdir().unwrap();
        let publisher = Publisher::open(&state.path().join("private")).unwrap();
        let root = ProjectReadRoot::open(project.path()).unwrap();
        let mut changes = 0;
        assert!(
            publisher
                .publish_with_hook(
                    &root,
                    prepare(&root),
                    &Cancellation::default(),
                    &mut |actual| {
                        if actual == PublicationPoint::TargetChanged {
                            changes += 1;
                            if changes == 2 {
                                anyhow::bail!("crash");
                            }
                        }
                        Ok(())
                    }
                )
                .is_err()
        );
        assert!(publisher.reclaim_committed(&root).is_err());
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "engine::publication::tests::crash_worker",
                "--nocapture",
            ])
            .env("EMPACK_PUBLICATION_CRASH_PROJECT", project.path())
            .env(
                "EMPACK_PUBLICATION_CRASH_STATE",
                state.path().join("private"),
            )
            .env("EMPACK_PUBLICATION_CRASH_POINT", format!("{point:?}"))
            .env("EMPACK_PUBLICATION_CRASH_OCCURRENCE", "1")
            .env("EMPACK_PUBLICATION_RESTORE", "1")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::inherit())
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(86), "{point:?}");
        let receipt = publisher.recover(&root).unwrap();
        assert_eq!(receipt.disposition, PublicationDisposition::Restored);
        assert_eq!(publisher.restore_before_images(&root).unwrap(), receipt);
        assert_eq!(
            fs::read(project.path().join("empack.yml")).unwrap(),
            b"old intent"
        );
        assert!(!project.path().join("pack/config/new.txt").exists());
        assert!(publisher.reclaim_committed(&root).unwrap() > 0);
        assert_eq!(publisher.reclaim_committed(&root).unwrap(), 0);
        assert_eq!(publisher.recover(&root).unwrap(), receipt);
    }
}

#[test]
fn restoration_can_recover_from_a_corrupt_pending_after_image() {
    let project = tempfile::tempdir().unwrap();
    fixture(project.path());
    let state = tempfile::tempdir().unwrap();
    let publisher = Publisher::open(&state.path().join("private")).unwrap();
    let root = ProjectReadRoot::open(project.path()).unwrap();
    assert!(
        publisher
            .publish_with_hook(
                &root,
                prepare(&root),
                &Cancellation::default(),
                &mut |point| {
                    if point == PublicationPoint::TargetChanged {
                        anyhow::bail!("crash");
                    }
                    Ok(())
                }
            )
            .is_err()
    );
    let state = publisher.project_state(&root).unwrap();
    let journal = load_journal(&state).unwrap().unwrap();
    state
        .open_dir_nofollow(&journal.operation)
        .unwrap()
        .write("after-1", b"bad pending")
        .unwrap();
    assert!(publisher.recover(&root).is_err());
    let receipt = publisher.restore_before_images(&root).unwrap();
    assert_eq!(receipt.disposition, PublicationDisposition::Restored);
    assert_eq!(
        fs::read(project.path().join("empack.yml")).unwrap(),
        b"old intent"
    );
    assert!(!project.path().join("pack/config/new.txt").exists());
}

#[test]
fn restoration_preserves_conflicting_user_edits_and_corrupt_before_images() {
    for external_edit in [false, true] {
        let project = tempfile::tempdir().unwrap();
        fixture(project.path());
        let state = tempfile::tempdir().unwrap();
        let publisher = Publisher::open(&state.path().join("private")).unwrap();
        let root = ProjectReadRoot::open(project.path()).unwrap();
        assert!(
            publisher
                .publish_with_hook(
                    &root,
                    prepare(&root),
                    &Cancellation::default(),
                    &mut |point| {
                        if point == PublicationPoint::TargetChanged {
                            anyhow::bail!("crash");
                        }
                        Ok(())
                    }
                )
                .is_err()
        );
        if external_edit {
            fs::write(project.path().join("empack.yml"), b"useredit!!").unwrap();
        } else {
            let state = publisher.project_state(&root).unwrap();
            let journal = load_journal(&state).unwrap().unwrap();
            state
                .open_dir_nofollow(&journal.operation)
                .unwrap()
                .write("before-0", b"corrupted!")
                .unwrap();
        }
        assert!(publisher.restore_before_images(&root).is_err());
        assert_eq!(
            fs::read(project.path().join("empack.yml")).unwrap(),
            if external_edit {
                b"useredit!!"
            } else {
                b"new intent"
            }
        );
        assert!(publisher.recovery_required(&root).unwrap());
    }
}

#[cfg(windows)]
#[test]
fn executable_output_intent_publishes_and_converges_on_windows_without_claiming_native_mode_verification()
 {
    let project = tempfile::tempdir().unwrap();
    fixture(project.path());
    let state = tempfile::tempdir().unwrap();
    let publisher = Publisher::open(&state.path().join("private")).unwrap();
    let root = ProjectReadRoot::open(project.path()).unwrap();
    let cancel = Cancellation::default();
    let snapshot = root
        .capture(&[path("empack.yml")], SnapshotLimits::default(), &cancel)
        .unwrap();
    let mut output = expected(b"new script");
    output.permissions.executable = true;
    let desired = BTreeMap::from([(ManagedPath::IntentDocument, output)]);
    let observed = observed_files_for(&snapshot, desired.keys().cloned()).unwrap();
    let plan = plan_files(&observed, &desired, &BTreeSet::new()).unwrap();
    let mut stage = MutableStage::empty().unwrap();
    stage
        .write(&path("empack.yml"), &mut &b"new script"[..], 10, &cancel)
        .unwrap();
    let proof = VerifiedFileChange::verify(
        snapshot,
        plan,
        stage.freeze(SnapshotLimits::default(), &cancel).unwrap(),
    )
    .unwrap();
    let receipt = publisher.publish(&root, proof, &cancel).unwrap();
    assert!(!receipt.executable_bits_verified);
    let snapshot = root
        .capture(&[path("empack.yml")], SnapshotLimits::default(), &cancel)
        .unwrap();
    let observed = observed_files_for(&snapshot, desired.keys().cloned()).unwrap();
    let repeated = plan_files(&observed, &desired, &BTreeSet::new()).unwrap();
    assert!(repeated.changes().is_empty());
    assert!(
        repeated.expected()[&ManagedPath::IntentDocument]
            .permissions
            .executable
    );
}

#[test]
fn read_only_coordination_refuses_hot_journals_and_never_creates_state() {
    let project = tempfile::tempdir().unwrap();
    fixture(project.path());
    let host = tempfile::tempdir().unwrap();
    let path = host.path().join("private");
    let root = ProjectReadRoot::open(project.path()).unwrap();
    let reader = RecoveryReader::new(path.clone());
    drop(reader.enter(&root).unwrap());
    assert!(!path.exists());
    let publisher = Publisher::open(&path).unwrap();
    let result = publisher.publish_with_hook(
        &root,
        prepare(&root),
        &Cancellation::default(),
        &mut |point| {
            if point == PublicationPoint::IntentDurable {
                anyhow::bail!("interrupt fixture");
            }
            Ok(())
        },
    );
    assert!(result.is_err());
    assert!(reader.enter(&root).is_err());
    publisher.recover(&root).unwrap();
    let guard = reader.enter(&root).unwrap();
    let state = publisher.project_state(&root).unwrap();
    assert!(
        lock(&state).is_err(),
        "snapshot read coordination must exclude publication"
    );
    drop(guard);
    assert!(lock(&state).is_ok());
}
