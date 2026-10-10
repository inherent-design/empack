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
pub(crate) fn prepare(root: &ProjectReadRoot) -> VerifiedFileChange {
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
            let directory = publisher.project_state(&root).unwrap();
            let abandoned: serde_json::Value =
                serde_json::from_slice(&directory.read("retention.json").unwrap()).unwrap();
            let abandoned = abandoned["operation"].as_str().unwrap();
            assert!(directory.exists(abandoned));
            publisher
                .publish(&root, prepare(&root), &Cancellation::default())
                .unwrap();
            assert!(
                !directory.exists(abandoned),
                "pre-intent copies leaked after restart"
            );
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

#[test]
fn separate_source_and_artifact_budgets_survive_interrupted_publication() {
    use crate::engine::verification::observed_artifacts_for;
    for artifact_limit in [16, 128] {
        let project = tempfile::tempdir().unwrap();
        fs::write(project.path().join("input"), b"input").unwrap();
        fs::create_dir_all(project.path().join("pack/ignored")).unwrap();
        fs::write(project.path().join("pack/.packwizignore"), b"ignored/\n").unwrap();
        fs::write(project.path().join("pack/ignored/huge"), vec![0; 1024]).unwrap();
        let filter =
            crate::engine::source::CaptureFilter::author(&["ignored/".into()], &[]).unwrap();
        let state = tempfile::tempdir().unwrap();
        let publisher = Publisher::open(&state.path().join("private")).unwrap();
        let root = ProjectReadRoot::open(project.path()).unwrap();
        let cancel = Cancellation::default();
        let source_limits = SnapshotLimits {
            file_bytes: 32,
            total_bytes: 32,
            ..SnapshotLimits::default()
        };
        let artifact_limits = SnapshotLimits {
            file_bytes: artifact_limit,
            total_bytes: artifact_limit,
            ..SnapshotLimits::default()
        };
        let base = root
            .capture_filtered(
                &[path("input"), path("pack")],
                source_limits,
                Some(&filter),
                &cancel,
            )
            .unwrap()
            .merge(
                root.capture(&[path("dist/result.zip")], artifact_limits, &cancel)
                    .unwrap(),
            )
            .unwrap();
        let target = ManagedPath::Artifact(path("result.zip"));
        let bytes = vec![0xab; 64];
        let desired = BTreeMap::from([(target.clone(), expected(&bytes))]);
        let plan = plan_files(
            &observed_artifacts_for(&base, [target]).unwrap(),
            &desired,
            &BTreeSet::new(),
        )
        .unwrap();
        let mut stage = MutableStage::empty().unwrap();
        stage
            .write(&path("dist/result.zip"), &mut bytes.as_slice(), 64, &cancel)
            .unwrap();
        let proof = VerifiedFileChange::verify_artifacts(
            base,
            plan,
            stage.freeze(SnapshotLimits::default(), &cancel).unwrap(),
        );
        if artifact_limit == 16 {
            assert!(
                proof.is_err(),
                "source budget must not widen artifact limits"
            );
            assert!(!project.path().join("dist").exists());
            continue;
        }
        assert!(
            publisher
                .publish_with_hook(&root, proof.unwrap(), &cancel, &mut |point| {
                    if point == PublicationPoint::TargetChanged {
                        anyhow::bail!("simulate interruption");
                    }
                    Ok(())
                })
                .is_err()
        );
        assert!(publisher.recovery_required(&root).unwrap());
        fs::write(project.path().join("pack/ignored/huge"), vec![1; 2048]).unwrap();
        fs::write(project.path().join("input"), vec![0; 33]).unwrap();
        assert!(
            publisher.recover(&root).is_err(),
            "artifact budget must not widen source limits"
        );
        fs::write(project.path().join("input"), b"input").unwrap();
        publisher.recover(&root).unwrap();
        assert_eq!(
            fs::read(project.path().join("dist/result.zip")).unwrap(),
            bytes
        );
    }
}

#[test]
fn recovery_plans_are_read_only_and_bind_journal_and_visible_effects() {
    for action in [RecoveryAction::Finish, RecoveryAction::Restore] {
        for point in [
            PublicationPoint::IntentDurable,
            PublicationPoint::TargetChanged,
        ] {
            let project = tempfile::tempdir().unwrap();
            fixture(project.path());
            let state = tempfile::tempdir().unwrap();
            let publisher = Publisher::open(&state.path().join("private")).unwrap();
            let root = ProjectReadRoot::open(project.path()).unwrap();
            assert!(publisher.inspect_recovery(&root).unwrap().is_none());
            assert!(!publisher.host.exists(root_key(&root).unwrap()));
            assert!(
                publisher
                    .publish_with_hook(
                        &root,
                        prepare(&root),
                        &Cancellation::default(),
                        &mut |actual| {
                            if actual == point {
                                anyhow::bail!("interrupted fixture");
                            }
                            Ok(())
                        }
                    )
                    .is_err()
            );
            let before = fs::read(project.path().join("empack.yml")).unwrap();
            let journal_dir = publisher
                .host
                .open_dir_nofollow(root_key(&root).unwrap())
                .unwrap();
            let old_journal = serde_json::to_vec(&load_journal(&journal_dir).unwrap()).unwrap();
            let status = publisher.inspect_recovery(&root).unwrap().unwrap();
            let plan = publisher.prepare_recovery(&root, action).unwrap().unwrap();
            assert_eq!(plan.status(), &status);
            assert_eq!(fs::read(project.path().join("empack.yml")).unwrap(), before);
            assert_eq!(
                serde_json::to_vec(&load_journal(&journal_dir).unwrap()).unwrap(),
                old_journal
            );
            let receipt = publisher.recover_prepared(&root, plan).unwrap();
            assert_eq!(
                receipt.disposition,
                if action == RecoveryAction::Finish {
                    PublicationDisposition::Published
                } else {
                    PublicationDisposition::Restored
                }
            );
            assert_eq!(
                fs::read(project.path().join("empack.yml")).unwrap(),
                if action == RecoveryAction::Finish {
                    b"new intent"
                } else {
                    b"old intent"
                }
            );
            assert!(publisher.inspect_recovery(&root).unwrap().is_none());
        }
    }
}
#[test]
fn approved_recovery_rejects_late_edits_and_journal_progress() {
    for changed_journal in [false, true] {
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
                            anyhow::bail!("interrupted fixture");
                        }
                        Ok(())
                    }
                )
                .is_err()
        );
        let prepared = publisher
            .prepare_recovery(&root, RecoveryAction::Finish)
            .unwrap()
            .unwrap();
        if changed_journal {
            publisher.recover(&root).unwrap();
        } else {
            fs::write(project.path().join("empack.yml"), b"user edit!").unwrap();
        }
        assert!(publisher.recover_prepared(&root, prepared).is_err());
        assert_eq!(
            fs::read(project.path().join("empack.yml")).unwrap(),
            if changed_journal {
                b"new intent"
            } else {
                b"user edit!"
            }
        );
    }
}

pub(in crate::engine) fn interrupted_fixture(project: &Path, state: &Path) -> RecoveryStatus {
    fixture(project);
    let publisher = Publisher::open(state).unwrap();
    let root = ProjectReadRoot::open(project).unwrap();
    assert!(
        publisher
            .publish_with_hook(
                &root,
                prepare(&root),
                &Cancellation::default(),
                &mut |point| {
                    if point == PublicationPoint::TargetChanged {
                        anyhow::bail!("interrupted fixture");
                    }
                    Ok(())
                }
            )
            .is_err()
    );
    publisher.inspect_recovery(&root).unwrap().unwrap()
}

#[test]
fn recovery_preview_preserves_journal_siblings_until_approved_execution() {
    for action in [RecoveryAction::Finish, RecoveryAction::Restore] {
        for occurrence in [1, 2] {
            let project = tempfile::tempdir().unwrap();
            fixture(project.path());
            let state = tempfile::tempdir().unwrap();
            let publisher = Publisher::open(&state.path().join("private")).unwrap();
            let root = ProjectReadRoot::open(project.path()).unwrap();
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
                .env("EMPACK_PUBLICATION_CRASH_POINT", "SiblingWritten")
                .env(
                    "EMPACK_PUBLICATION_CRASH_OCCURRENCE",
                    occurrence.to_string(),
                )
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::inherit())
                .status()
                .unwrap();
            assert_eq!(status.code(), Some(86));
            let directory = publisher
                .host
                .open_dir_nofollow(root_key(&root).unwrap())
                .unwrap();
            let journal = load_journal(&directory).unwrap().unwrap();
            let siblings: Vec<_> = journal
                .changes
                .iter()
                .filter_map(|change| {
                    let path = project
                        .path()
                        .join(&change.target)
                        .parent()
                        .unwrap()
                        .join(change.sibling.as_ref()?);
                    path.is_file().then(|| {
                        let bytes = fs::read(&path).unwrap();
                        let modified = fs::metadata(&path).unwrap().modified().unwrap();
                        (path, bytes, modified)
                    })
                })
                .collect();
            assert_eq!(siblings.len(), 1);
            // Only exact journal scratch is excluded; unrelated additions still conflict.
            let unrelated = project.path().join("pack/unrelated.txt");
            fs::create_dir_all(unrelated.parent().unwrap()).unwrap();
            fs::write(&unrelated, b"user content").unwrap();
            assert!(publisher.prepare_recovery(&root, action).is_err());
            assert!(siblings[0].0.exists());
            fs::remove_file(unrelated).unwrap();
            let planned = publisher.prepare_recovery(&root, action).unwrap().unwrap();
            for (path, bytes, modified) in &siblings {
                assert!(path.exists(), "preview removed journal sibling");
                assert_eq!(&fs::read(path).unwrap(), bytes);
                assert_eq!(&fs::metadata(path).unwrap().modified().unwrap(), modified);
            }
            publisher.recover_prepared(&root, planned).unwrap();
            for (path, _, _) in siblings {
                assert!(!path.exists());
            }
        }
    }
}

pub(crate) fn interrupt_publication(
    publisher: &Publisher,
    root: &ProjectReadRoot,
    verified: VerifiedFileChange,
    point: PublicationPoint,
) -> Result<PublicationReceipt> {
    publisher.publish_with_hook(root, verified, &Cancellation::default(), &mut |actual| {
        if actual == point {
            anyhow::bail!("injected publication interruption");
        }
        Ok(())
    })
}

#[test]
fn concurrent_private_directory_openers_share_one_verified_boundary() {
    let parent = tempfile::tempdir().unwrap();
    let path = parent.path().join("state");
    let start = std::sync::Barrier::new(8);
    let identities = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..8)
            .map(|_| {
                scope.spawn(|| {
                    start.wait();
                    let directory = open_private_directory(&path, true).unwrap();
                    native::directory_identity(&directory).unwrap()
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert!(identities.iter().all(|identity| *identity == identities[0]));
}

#[test]
fn repeated_publications_reclaim_previous_copies_and_preserve_unknown_neighbors() {
    let project = tempfile::tempdir().unwrap();
    fixture(project.path());
    let state = tempfile::tempdir().unwrap();
    let publisher = Publisher::open(&state.path().join("private")).unwrap();
    let root = ProjectReadRoot::open(project.path()).unwrap();
    let directory = publisher.project_state(&root).unwrap();
    directory.create_dir("op-unknown").unwrap();
    directory.write("op-unknown/keep", b"unowned").unwrap();
    let mut previous = None;
    for _ in 0..3 {
        fs::write(project.path().join("empack.yml"), b"old intent").unwrap();
        let receipt = publisher
            .publish(&root, prepare(&root), &Cancellation::default())
            .unwrap();
        if let Some(previous) = previous {
            assert!(
                !directory.exists(previous),
                "previous recovery copies leaked"
            );
        }
        previous = Some(receipt.operation);
    }
    publisher.reclaim_committed(&root).unwrap();
    assert!(!directory.exists(previous.unwrap()));
    assert_eq!(directory.read("op-unknown/keep").unwrap(), b"unowned");
}

#[test]
fn unexpected_retained_object_blocks_superseding_its_descriptor() {
    let project = tempfile::tempdir().unwrap();
    fixture(project.path());
    let state = tempfile::tempdir().unwrap();
    let publisher = Publisher::open(&state.path().join("private")).unwrap();
    let root = ProjectReadRoot::open(project.path()).unwrap();
    let receipt = publisher
        .publish(&root, prepare(&root), &Cancellation::default())
        .unwrap();
    let directory = publisher.project_state(&root).unwrap();
    let unknown = format!("{}/unowned", receipt.operation);
    directory.write(&unknown, b"keep").unwrap();
    fs::write(project.path().join("empack.yml"), b"old intent").unwrap();
    assert!(
        publisher
            .publish(&root, prepare(&root), &Cancellation::default())
            .is_err()
    );
    assert_eq!(directory.read(&unknown).unwrap(), b"keep");
    assert_eq!(
        load_journal(&directory).unwrap().unwrap().operation,
        receipt.operation
    );
    assert_eq!(
        fs::read(project.path().join("empack.yml")).unwrap(),
        b"old intent"
    );
    directory.remove_file(unknown).unwrap();
    publisher
        .publish(&root, prepare(&root), &Cancellation::default())
        .unwrap();
}
