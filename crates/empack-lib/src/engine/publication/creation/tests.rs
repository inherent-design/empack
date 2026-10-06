use super::*;
use crate::engine::{
    project::ProjectReader,
    staging::MutableStage,
    verification::{observed_files_for, plan_files},
};
use empack_core::{digest::ContentId, files::FilePermissions};
use std::fs;

fn prepare(selected: &Path, state: &Path) -> PreparedRootCreation {
    prepare_with_permissions(
        selected,
        state,
        FilePermissions {
            readonly: false,
            executable: false,
        },
    )
}
fn prepare_with_permissions(
    selected: &Path,
    state: &Path,
    payload_permissions: FilePermissions,
) -> PreparedRootCreation {
    let cancel = Cancellation::default();
    let target = ProjectReader::new(RecoveryReader::new(state.to_path_buf()))
        .capture_new(selected, &cancel)
        .unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let root = ProjectReadRoot::open(scratch.path()).unwrap();
    let values = [
        ("empack.yml", b"intent".as_slice()),
        ("empack.lock", b"lock".as_slice()),
        ("pack/config/value", b"payload".as_slice()),
    ];
    let paths = values
        .iter()
        .map(|(name, _)| PortableRelPath::parse(name, PathSyntax::ProjectContent).unwrap())
        .collect::<Vec<_>>();
    let base = root
        .capture(&paths, SnapshotLimits::default(), &cancel)
        .unwrap();
    let mut stage = MutableStage::empty().unwrap();
    let mut desired = BTreeMap::new();
    for ((_, bytes), path) in values.iter().zip(&paths) {
        let permissions = if path.as_str().starts_with("pack/") {
            payload_permissions
        } else {
            FilePermissions {
                readonly: false,
                executable: false,
            }
        };
        let mut reader: &[u8] = bytes;
        stage
            .write_attributed(path, &mut reader, bytes.len() as u64, permissions, &cancel)
            .unwrap();
        desired.insert(
            ProjectLayout::classify(path).unwrap(),
            FileContent {
                content: ContentId::from_sha256(Sha256::digest(bytes).into()),
                bytes: bytes.len() as u64,
                permissions,
            },
        );
    }
    let observed = observed_files_for(&base, desired.keys().cloned()).unwrap();
    let plan = plan_files(&observed, &desired, &BTreeSet::new()).unwrap();
    let change = VerifiedFileChange::verify(
        base,
        plan,
        stage.freeze(SnapshotLimits::default(), &cancel).unwrap(),
    )
    .unwrap();
    PreparedRootCreation::from_verified(target, change).unwrap()
}
fn check_contents(selected: &Path) {
    assert_eq!(fs::read(selected.join("empack.yml")).unwrap(), b"intent");
    assert_eq!(fs::read(selected.join("empack.lock")).unwrap(), b"lock");
    assert_eq!(
        fs::read(selected.join("pack/config/value")).unwrap(),
        b"payload"
    );
}

#[test]
fn creation_publishes_complete_bytes_and_keeps_committed_recovery_idempotent() {
    let temp = tempfile::tempdir().unwrap();
    let selected = temp.path().join("new");
    let host = tempfile::tempdir().unwrap();
    let state = host.path().join("private");
    let prepared = prepare(&selected, &state);
    assert!(!selected.exists() && !state.exists());
    let publisher = Publisher::open(&state).unwrap();
    let receipt = publisher
        .publish_new(prepared, &Cancellation::default())
        .unwrap();
    assert_eq!(receipt.changed_files, 3);
    check_contents(&selected);
    assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 1);
    let root = ProjectReadRoot::open(&selected).unwrap();
    RecoveryReader::new(state.clone()).enter(&root).unwrap();
    assert_eq!(publisher.recover_new(&selected).unwrap(), receipt);
    fs::write(selected.join("pack/config/value"), b"user edit").unwrap();
    assert_eq!(publisher.recover_new(&selected).unwrap(), receipt);
    assert_eq!(
        fs::read(selected.join("pack/config/value")).unwrap(),
        b"user edit"
    );
}

#[test]
fn failures_at_durable_creation_boundaries_gate_reads_and_recover_without_tools() {
    for point in [
        PublicationPoint::RecoveryDataDurable,
        PublicationPoint::CreationIndexDurable,
        PublicationPoint::IntentDurable,
        PublicationPoint::TargetChanged,
        PublicationPoint::DirectorySynced,
        PublicationPoint::Committed,
    ] {
        let temp = tempfile::tempdir().unwrap();
        let selected = temp.path().join("new");
        let host = tempfile::tempdir().unwrap();
        let state = host.path().join("private");
        let prepared = prepare(&selected, &state);
        let publisher = Publisher::open(&state).unwrap();
        let error = publisher
            .publish_new_with_hook(prepared, &Cancellation::default(), &mut |at| {
                if at == point {
                    anyhow::bail!("injected {point:?}");
                }
                Ok(())
            })
            .unwrap_err();
        if point == PublicationPoint::RecoveryDataDurable {
            assert!(error.downcast_ref::<RecoveryRequired>().is_none());
            assert!(!selected.exists());
            assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 0);
            continue;
        }
        assert!(error.downcast_ref::<RecoveryRequired>().is_some());
        let reader = RecoveryReader::new(state.clone());
        if matches!(
            point,
            PublicationPoint::CreationIndexDurable | PublicationPoint::IntentDurable
        ) {
            assert!(!selected.exists());
            assert!(
                ProjectReader::new(reader)
                    .capture_new(&selected, &Cancellation::default())
                    .is_err()
            );
        } else if point != PublicationPoint::Committed {
            let root = ProjectReadRoot::open(&selected).unwrap();
            assert!(reader.enter(&root).is_err());
        }
        let receipt = publisher.recover_new(&selected).unwrap();
        check_contents(&selected);
        assert_eq!(publisher.recover_new(&selected).unwrap(), receipt);
        let root = ProjectReadRoot::open(&selected).unwrap();
        RecoveryReader::new(state).enter(&root).unwrap();
    }
}

#[test]
fn competing_destination_and_changed_candidate_cannot_be_adopted_by_recovery() {
    for competing in [true, false] {
        let temp = tempfile::tempdir().unwrap();
        let selected = temp.path().join("new");
        let host = tempfile::tempdir().unwrap();
        let state = host.path().join("private");
        let prepared = prepare(&selected, &state);
        let publisher = Publisher::open(&state).unwrap();
        assert!(
            publisher
                .publish_new_with_hook(prepared, &Cancellation::default(), &mut |point| {
                    if point == PublicationPoint::IntentDurable {
                        anyhow::bail!("stop");
                    }
                    Ok(())
                })
                .is_err()
        );
        if competing {
            fs::create_dir(&selected).unwrap();
            fs::write(selected.join("sentinel"), b"unowned").unwrap();
        } else {
            let candidate = fs::read_dir(temp.path())
                .unwrap()
                .next()
                .unwrap()
                .unwrap()
                .path();
            fs::write(candidate.join("pack/config/value"), b"changed").unwrap();
        }
        assert!(publisher.recover_new(&selected).is_err());
        if competing {
            assert_eq!(fs::read(selected.join("sentinel")).unwrap(), b"unowned");
            assert!(!selected.join("empack.yml").exists());
        } else {
            assert!(!selected.exists());
        }
    }
}

#[test]
fn cancellation_before_intent_preserves_absence_and_after_intent_finishes_publication() {
    for before in [true, false] {
        let temp = tempfile::tempdir().unwrap();
        let selected = temp.path().join("new");
        let host = tempfile::tempdir().unwrap();
        let state = host.path().join("private");
        let prepared = prepare(&selected, &state);
        let publisher = Publisher::open(&state).unwrap();
        let cancel = Cancellation::default();
        if before {
            cancel.cancel();
        }
        let result = publisher.publish_new_with_hook(prepared, &cancel, &mut |point| {
            if point == PublicationPoint::IntentDurable {
                cancel.cancel();
            }
            Ok(())
        });
        if before {
            assert!(result.is_err());
            assert!(!selected.exists());
            assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 0);
        } else {
            result.unwrap();
            check_contents(&selected);
        }
    }
}

#[test]
fn new_root_crash_child() {
    let Some(selected) = std::env::var_os("EMPACK_TEST_CREATE_SELECTED") else {
        return;
    };
    let state = std::path::PathBuf::from(std::env::var_os("EMPACK_TEST_CREATE_STATE").unwrap());
    let selected = std::path::PathBuf::from(selected);
    let point = std::env::var("EMPACK_TEST_CREATE_POINT").unwrap();
    let prepared = prepare(&selected, &state);
    Publisher::open(&state)
        .unwrap()
        .publish_new_with_hook(prepared, &Cancellation::default(), &mut |at| {
            if format!("{at:?}") == point {
                std::process::exit(93);
            }
            Ok(())
        })
        .unwrap();
    panic!("crash boundary was not reached");
}

#[test]
fn actual_process_exit_before_and_after_root_rename_is_recoverable() {
    for point in [
        PublicationPoint::CreationIndexDurable,
        PublicationPoint::IntentDurable,
        PublicationPoint::TargetChanged,
        PublicationPoint::DirectorySynced,
        PublicationPoint::Committed,
    ] {
        let temp = tempfile::tempdir().unwrap();
        let host = tempfile::tempdir().unwrap();
        let selected = temp.path().join("new");
        let state = host.path().join("private");
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "engine::publication::creation::tests::new_root_crash_child",
                "--nocapture",
            ])
            .env("EMPACK_TEST_CREATE_SELECTED", &selected)
            .env("EMPACK_TEST_CREATE_STATE", &state)
            .env("EMPACK_TEST_CREATE_POINT", format!("{point:?}"))
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(93),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let publisher = Publisher::open(&state).unwrap();
        let receipt = publisher.recover_new(&selected).unwrap();
        check_contents(&selected);
        assert_eq!(publisher.recover_new(&selected).unwrap(), receipt);
        assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 1);
    }
}

#[test]
fn a_completed_moved_project_does_not_reserve_its_former_name_forever() {
    let temp = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    let selected = temp.path().join("new");
    let state = host.path().join("private");
    let first = prepare(&selected, &state);
    let publisher = Publisher::open(&state).unwrap();
    let first = publisher
        .publish_new(first, &Cancellation::default())
        .unwrap();
    let moved = temp.path().join("moved");
    fs::rename(&selected, &moved).unwrap();
    let second = prepare(&selected, &state);
    let second = publisher
        .publish_new(second, &Cancellation::default())
        .unwrap();
    assert_ne!(first.operation, second.operation);
    check_contents(&selected);
    check_contents(&moved);
    RecoveryReader::new(state.clone())
        .enter(&ProjectReadRoot::open(&moved).unwrap())
        .unwrap();
    assert_eq!(publisher.recover_new(&selected).unwrap(), second);
}

#[test]
fn created_payload_permissions_survive_and_private_failure_cleanup_is_bounded() {
    let temp = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    let selected = temp.path().join("new");
    let state = host.path().join("private");
    let permissions = FilePermissions {
        readonly: true,
        executable: true,
    };
    let prepared = prepare_with_permissions(&selected, &state, permissions);
    let publisher = Publisher::open(&state).unwrap();
    assert!(
        publisher
            .publish_new_with_hook(prepared, &Cancellation::default(), &mut |point| {
                if point == PublicationPoint::RecoveryDataDurable {
                    anyhow::bail!("stop");
                }
                Ok(())
            })
            .is_err()
    );
    assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 0);
    let prepared = prepare_with_permissions(&selected, &state, permissions);
    publisher
        .publish_new(prepared, &Cancellation::default())
        .unwrap();
    let attributes = fs::metadata(selected.join("pack/config/value"))
        .unwrap()
        .permissions();
    assert!(attributes.readonly());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_ne!(attributes.mode() & 0o100, 0);
    }
}

#[cfg(windows)]
#[test]
fn creation_protects_its_candidate_without_changing_a_shared_selected_parent() {
    let temp = tempfile::tempdir().unwrap();
    let parent = Dir::open_ambient_dir(temp.path(), cap_std::ambient_authority()).unwrap();
    crate::engine::windows_privacy::share_for_test(&parent).unwrap();
    parent.write("unrelated", b"keep").unwrap();
    let host = tempfile::tempdir().unwrap();
    let state = host.path().join("private");
    let selected = temp.path().join("new");
    let prepared = prepare(&selected, &state);
    Publisher::open(&state)
        .unwrap()
        .publish_new(prepared, &Cancellation::default())
        .unwrap();
    assert!(crate::engine::windows_privacy::verify(&parent).is_err());
    crate::engine::windows_privacy::verify(&parent.open_dir_nofollow("new").unwrap()).unwrap();
    assert_eq!(parent.read("unrelated").unwrap(), b"keep");
    check_contents(&selected);
}

#[test]
fn interrupted_creation_recovers_after_the_published_root_moves() {
    let temp = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    let state = host.path().join("state");
    let publisher = Publisher::open(&state).unwrap();
    let selected = temp.path().join("new");
    let error = publisher
        .publish_new_with_hook(
            prepare(&selected, &state),
            &Cancellation::default(),
            &mut |point| {
                if point == PublicationPoint::TargetChanged {
                    anyhow::bail!("interrupted after rename");
                }
                Ok(())
            },
        )
        .unwrap_err();
    assert!(error.downcast_ref::<RecoveryRequired>().is_some());
    let other = tempfile::tempdir().unwrap();
    let moved = other.path().join("moved");
    fs::rename(&selected, &moved).unwrap();
    assert!(
        RecoveryReader::new(state.clone())
            .enter(&ProjectReadRoot::open(&moved).unwrap())
            .is_err()
    );
    fs::write(moved.join("pack/config/value"), b"tampered").unwrap();
    assert!(publisher.recover_new(&moved).is_err());
    fs::write(moved.join("pack/config/value"), b"payload").unwrap();
    publisher.recover_new(&moved).unwrap();
    check_contents(&moved);
    RecoveryReader::new(state.clone())
        .enter(&ProjectReadRoot::open(&moved).unwrap())
        .unwrap();
    assert!(publisher.recover_new(&selected).is_err());
    // The old name's completed selection can be reused for a distinct native root.
    publisher
        .publish_new(prepare(&selected, &state), &Cancellation::default())
        .unwrap();
    check_contents(&selected);
}

#[test]
fn failure_before_first_creation_index_publication_cleans_private_candidate() {
    let temp = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    let state = host.path().join("state");
    let publisher = Publisher::open(&state).unwrap();
    let selected = temp.path().join("new");
    let error = publisher
        .publish_new_with_hook(
            prepare(&selected, &state),
            &Cancellation::default(),
            &mut |point| {
                if point == PublicationPoint::RecoveryDataDurable {
                    let parent = ProjectReadRoot::open(temp.path()).unwrap();
                    let index = creation_state(&publisher, &parent, "new", false)?.unwrap();
                    // The first atomic index replacement must fail before publishing any intent.
                    index.create_dir("creation.json")?;
                }
                Ok(())
            },
        )
        .unwrap_err();
    assert!(error.downcast_ref::<RecoveryRequired>().is_none());
    assert!(!selected.exists());
    assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 0);
}
