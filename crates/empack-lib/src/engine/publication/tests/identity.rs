use super::*;

#[test]
fn recovery_journals_bind_persistent_objects_and_preserve_strict_legacy_records() {
    for legacy in [false, true] {
        for action in [RecoveryAction::Finish, RecoveryAction::Restore] {
            let project = tempfile::tempdir().unwrap();
            fixture(project.path());
            let state = tempfile::tempdir().unwrap();
            let publisher = Publisher::open(&state.path().join("private")).unwrap();
            let root = ProjectReadRoot::open(project.path()).unwrap();
            let original =
                native::identity(&fs::File::open(project.path().join("empack.yml")).unwrap())
                    .unwrap();
            publisher
                .publish_with_hook(
                    &root,
                    prepare(&root),
                    &Cancellation::default(),
                    &mut |point| {
                        if point == PublicationPoint::TargetChanged {
                            anyhow::bail!("stop after first replacement");
                        }
                        Ok(())
                    },
                )
                .unwrap_err();
            let directory = publisher.project_state(&root).unwrap();
            let mut wire: serde_json::Value =
                serde_json::from_slice(&directory.read("journal.json").unwrap()).unwrap();
            assert_eq!(
                wire["schema"], 5,
                "new journals must use durable object bindings"
            );
            assert_eq!(wire["root"].as_object().unwrap().len(), 1);
            assert_eq!(
                wire["root"]["persistent"],
                serde_json::json!(root.binding.persistent.unwrap())
            );
            for change in wire["changes"].as_array_mut().unwrap() {
                if !change["before_object"].is_null() {
                    assert_eq!(change["target"], "empack.yml");
                    assert_eq!(
                        change["before_object"],
                        serde_json::json!({"persistent": original.persistent.unwrap()})
                    );
                    if legacy {
                        change["before_object"] =
                            serde_json::to_value(Binding::from(original)).unwrap();
                    }
                }
            }
            if legacy {
                wire["schema"] = 4.into();
                wire["root"] = serde_json::to_value(Binding::from(root.binding)).unwrap();
            }
            let bytes = serde_json::to_vec(&wire).unwrap();
            directory.write("journal.json", &bytes).unwrap();
            let prepared = publisher.prepare_recovery(&root, action).unwrap().unwrap();
            assert_eq!(
                directory.read("journal.json").unwrap(),
                bytes,
                "inspection cannot migrate or rewrite a record"
            );

            // A journal change cannot inherit the approval captured before it changed.
            let mut changed = wire.clone();
            if legacy {
                changed["root"]["volume"] = (root.binding.volume ^ 1).into();
            } else {
                changed["root"]["persistent"][0] = (root.binding.persistent.unwrap()[0] ^ 1).into();
            }
            let changed = serde_json::to_vec(&changed).unwrap();
            directory.write("journal.json", &changed).unwrap();
            assert!(publisher.prepare_recovery(&root, action).is_err());
            assert!(publisher.recover_prepared(&root, prepared).is_err());
            assert_eq!(directory.read("journal.json").unwrap(), changed);
            assert_eq!(
                fs::read(project.path().join("empack.yml")).unwrap(),
                b"new intent"
            );
            assert!(!project.path().join("pack").exists());

            directory.write("journal.json", &bytes).unwrap();
            let prepared = publisher.prepare_recovery(&root, action).unwrap().unwrap();
            publisher.recover_prepared(&root, prepared).unwrap();
            match action {
                RecoveryAction::Finish => {
                    assert_eq!(
                        fs::read(project.path().join("empack.yml")).unwrap(),
                        b"new intent"
                    );
                    assert_eq!(
                        fs::read(project.path().join("pack/config/new.txt")).unwrap(),
                        b"new content"
                    );
                }
                RecoveryAction::Restore => {
                    assert_eq!(
                        fs::read(project.path().join("empack.yml")).unwrap(),
                        b"old intent"
                    );
                    assert!(!project.path().join("pack/config/new.txt").exists());
                }
            }
            assert!(!publisher.recovery_required(&root).unwrap());
            let final_wire: serde_json::Value =
                serde_json::from_slice(&directory.read("journal.json").unwrap()).unwrap();
            assert_eq!(final_wire["schema"], if legacy { 4 } else { 5 });
        }
    }
}

#[test]
fn recovery_refuses_mixed_identity_formats_and_same_bytes_replacements() {
    let project = tempfile::tempdir().unwrap();
    fixture(project.path());
    let state = tempfile::tempdir().unwrap();
    let publisher = Publisher::open(&state.path().join("private")).unwrap();
    let root = ProjectReadRoot::open(project.path()).unwrap();
    let file = fs::File::open(project.path().join("empack.yml")).unwrap();
    let original = native::identity(&file).unwrap();
    publisher
        .publish_with_hook(
            &root,
            prepare(&root),
            &Cancellation::default(),
            &mut |point| {
                if point == PublicationPoint::IntentDurable {
                    anyhow::bail!("retain untouched input");
                }
                Ok(())
            },
        )
        .unwrap_err();
    let directory = publisher.project_state(&root).unwrap();
    let bytes = directory.read("journal.json").unwrap();
    let wire: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    for malformed in 0..5 {
        let mut changed = wire.clone();
        match malformed {
            0 => changed["schema"] = 4.into(),
            1 => changed["root"] = serde_json::to_value(Binding::from(root.binding)).unwrap(),
            2 => changed["root"]["volume"] = root.binding.volume.into(),
            3 => {
                changed["changes"][0]["before_object"] =
                    serde_json::to_value(Binding::from(original)).unwrap()
            }
            4 => {
                changed["changes"][0]["before_object"]["persistent"][0] =
                    (original.persistent.unwrap()[0] ^ 1).into()
            }
            _ => unreachable!(),
        }
        let changed = serde_json::to_vec(&changed).unwrap();
        directory.write("journal.json", &changed).unwrap();
        for action in [RecoveryAction::Finish, RecoveryAction::Restore] {
            assert!(
                publisher.prepare_recovery(&root, action).is_err(),
                "accepted malformed case {malformed}"
            );
        }
        assert_eq!(directory.read("journal.json").unwrap(), changed);
        assert_eq!(
            fs::read(project.path().join("empack.yml")).unwrap(),
            b"old intent"
        );
        assert!(!project.path().join("pack").exists());
    }
    directory.write("journal.json", &bytes).unwrap();
    let approved = publisher
        .prepare_recovery(&root, RecoveryAction::Finish)
        .unwrap()
        .unwrap();
    // Keep the original inode alive, so a new file cannot reuse it accidentally.
    fs::remove_file(project.path().join("empack.yml")).unwrap();
    fs::write(project.path().join("empack.yml"), b"old intent").unwrap();
    assert!(publisher.recover_prepared(&root, approved).is_err());
    for action in [RecoveryAction::Finish, RecoveryAction::Restore] {
        assert!(publisher.prepare_recovery(&root, action).is_err());
    }
    assert_eq!(directory.read("journal.json").unwrap(), bytes);
    assert_eq!(
        fs::read(project.path().join("empack.yml")).unwrap(),
        b"old intent"
    );
    assert!(!project.path().join("pack").exists());
}

#[test]
fn journal_identity_separates_persistent_comparison_from_live_and_legacy_checks() {
    let project = tempfile::tempdir().unwrap();
    fixture(project.path());
    let mut root = ProjectReadRoot::open(project.path()).unwrap();
    let observed = root.binding;
    let persistent = JournalBinding::capture(5, observed).unwrap();
    let legacy = JournalBinding::capture(4, observed).unwrap();
    let mut reassigned = observed;
    reassigned.volume ^= 1;
    assert!(persistent.matches(reassigned));
    assert!(!legacy.matches(reassigned));
    assert_ne!(observed, reassigned, "live equality must remain strict");
    reassigned.persistent = None;
    assert!(!persistent.matches(reassigned));
    assert!(JournalBinding::capture(5, reassigned).is_err());

    let prepared = prepare(&root);
    root.binding.persistent = None; // Explicit unavailable-adapter observation.
    let state = tempfile::tempdir().unwrap();
    let publisher = Publisher::open(&state.path().join("private")).unwrap();
    let error = publisher
        .publish(&root, prepared, &Cancellation::default())
        .unwrap_err();
    assert!(format!("{error:#}").contains("Filesystem lacks persistent object identity"));
    assert_eq!(
        fs::read_dir(state.path().join("private")).unwrap().count(),
        0
    );
    assert_eq!(
        fs::read(project.path().join("empack.yml")).unwrap(),
        b"old intent"
    );
    assert!(!project.path().join("pack").exists());
}

#[test]
fn retained_preparation_identity_refuses_foreign_or_mixed_records_before_cleanup() {
    for legacy in [false, true] {
        let project = tempfile::tempdir().unwrap();
        fixture(project.path());
        let host = tempfile::tempdir().unwrap();
        let publisher = Publisher::open(&host.path().join("private")).unwrap();
        let root = ProjectReadRoot::open(project.path()).unwrap();
        let state = publisher.project_state(&root).unwrap();
        let _lock = lock(&state).unwrap();
        state.create_dir("op-fixture").unwrap();
        state.write("op-fixture/before-0", b"retained").unwrap();
        state.write("neighbor", b"unowned").unwrap();
        retention::record_preparation(
            &state,
            &root,
            "op-fixture",
            BTreeSet::from(["before-0".into()]),
        )
        .unwrap();
        let mut wire: serde_json::Value =
            serde_json::from_slice(&state.read("retention.json").unwrap()).unwrap();
        assert_eq!(wire["schema"], 2);
        assert_eq!(
            wire["root"],
            serde_json::json!({"persistent": root.binding.persistent.unwrap()})
        );
        if legacy {
            wire["schema"] = 1.into();
            wire["root"] = serde_json::to_value(Binding::from(root.binding)).unwrap();
        }
        for malformed in [false, true] {
            let mut invalid = wire.clone();
            if malformed {
                invalid["schema"] = if legacy { 2.into() } else { 1.into() };
            } else if legacy {
                invalid["root"]["volume"] = (root.binding.volume ^ 1).into();
            } else {
                invalid["root"]["persistent"][0] = (root.binding.persistent.unwrap()[0] ^ 1).into();
            }
            let bytes = serde_json::to_vec(&invalid).unwrap();
            state.write("retention.json", &bytes).unwrap();
            let error = retention::reclaim_preparation(&state, &root).unwrap_err();
            assert!(format!("{error:#}").contains("Retention root or schema mismatch"));
            assert_eq!(state.read("retention.json").unwrap(), bytes);
            assert_eq!(state.read("op-fixture/before-0").unwrap(), b"retained");
            assert_eq!(state.read("neighbor").unwrap(), b"unowned");
        }
        state
            .write("retention.json", serde_json::to_vec(&wire).unwrap())
            .unwrap();
        retention::reclaim_preparation(&state, &root).unwrap();
        assert!(!state.exists("op-fixture"));
        assert!(!state.exists("retention.json"));
        assert_eq!(state.read("neighbor").unwrap(), b"unowned");
        assert_eq!(
            fs::read(project.path().join("empack.yml")).unwrap(),
            b"old intent"
        );
        retention::reclaim_preparation(&state, &root).unwrap();
    }
}
