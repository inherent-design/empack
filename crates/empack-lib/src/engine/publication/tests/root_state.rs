use super::*;

// This fixture simulates a prior device assignment in host metadata. It does not
// change the current live observation or claim an actual operating-system remount.
fn move_state_to_prior_device(publisher: &Publisher, root: &ProjectReadRoot) -> String {
    let mut prior = Binding::from(root.binding);
    prior.volume ^= 1;
    let old_key = binding_key(&prior).unwrap();
    let current_key = root_key(root).unwrap();
    let state = publisher.host.open_dir_nofollow(&current_key).unwrap();
    let marker = serde_json::json!({
        "schema": 1, "persistent": root.binding.persistent.unwrap(), "original": prior,
    });
    state
        .write("root-identity.json", serde_json::to_vec(&marker).unwrap())
        .unwrap();
    // Windows directory capabilities prevent renaming their open directory.
    // Only fixture setup changes this name; production retains the physical slot.
    drop(state);
    publisher
        .host
        .rename(&current_key, &publisher.host, &old_key)
        .unwrap();
    old_key
}

#[test]
fn native_root_associations_recover_original_state_without_moving_locks() {
    let project = tempfile::tempdir().unwrap();
    fixture(project.path());
    let host = tempfile::tempdir().unwrap();
    let host_path = host.path().join("private");
    let publisher = Publisher::open(&host_path).unwrap();
    let root = ProjectReadRoot::open(project.path()).unwrap();
    publisher
        .publish_with_hook(
            &root,
            prepare(&root),
            &Cancellation::default(),
            &mut |point| {
                if point == PublicationPoint::IntentDurable {
                    anyhow::bail!("interrupt after intent");
                }
                Ok(())
            },
        )
        .unwrap_err();
    let old_key = move_state_to_prior_device(&publisher, &root);
    let directory = publisher.host.open_dir_nofollow(&old_key).unwrap();
    let before = directory.read("journal.json").unwrap();
    let owned_lock = lock(&directory).unwrap();
    assert!(
        publisher.recovery_required(&root).unwrap(),
        "stable association must discover the original hot journal"
    );
    assert!(
        publisher
            .prepare_recovery(&root, RecoveryAction::Finish)
            .is_err(),
        "original publication lock still coordinates recovery"
    );
    assert!(!publisher.host.exists(root_key(&root).unwrap()));
    assert_eq!(directory.read("journal.json").unwrap(), before);
    drop(owned_lock);
    let prepared = publisher
        .prepare_recovery(&root, RecoveryAction::Finish)
        .unwrap()
        .unwrap();
    publisher.recover_prepared(&root, prepared).unwrap();
    assert_eq!(
        fs::read(project.path().join("empack.yml")).unwrap(),
        b"new intent"
    );
    assert_eq!(
        fs::read(project.path().join("pack/config/new.txt")).unwrap(),
        b"new content"
    );
    assert!(!publisher.host.exists(root_key(&root).unwrap()));
    assert!(!publisher.recovery_required(&root).unwrap());
}

#[test]
fn native_root_associations_preserve_runtime_evidence_and_refuse_unassociated_state() {
    let project = tempfile::tempdir().unwrap();
    fixture(project.path());
    let host = tempfile::tempdir().unwrap();
    let publisher = Publisher::open(&host.path().join("private")).unwrap();
    let root = ProjectReadRoot::open(project.path()).unwrap();
    let mut lease = publisher.lease_instance(&root).unwrap();
    lease.begin().unwrap();
    drop(lease); // Retirement was not established.
    let old_key = move_state_to_prior_device(&publisher, &root);
    let directory = publisher.host.open_dir_nofollow(&old_key).unwrap();
    let marker = directory.read("root-identity.json").unwrap();
    assert!(publisher.observe_runtime(&root).unwrap().is_some());
    assert!(publisher.lease_instance(&root).is_err());
    directory.remove_file("root-identity.json").unwrap();
    let error = publisher
        .observe_runtime(&root)
        .err()
        .expect("unassociated state must not disappear");
    assert!(format!("{error:#}").contains("lacks persistent identity evidence"));
    assert!(!publisher.host.exists(root_key(&root).unwrap()));
    directory.write("root-identity.json", &marker).unwrap();
    let observed = publisher.observe_runtime(&root).unwrap().unwrap();
    publisher.acknowledge_stopped(&root, observed).unwrap();
    assert!(publisher.observe_runtime(&root).unwrap().is_none());
    assert_eq!(directory.read("root-identity.json").unwrap(), marker);
    assert!(!publisher.host.exists(root_key(&root).unwrap()));
}

#[test]
fn native_root_associations_reject_corrupt_foreign_and_duplicate_claims_without_effects() {
    for invalid in 0..5 {
        let project = tempfile::tempdir().unwrap();
        fixture(project.path());
        let host = tempfile::tempdir().unwrap();
        let host_path = host.path().join("private");
        let publisher = Publisher::open(&host_path).unwrap();
        let root = ProjectReadRoot::open(project.path()).unwrap();
        let state = publisher.project_state(&root).unwrap();
        state.write("unowned", b"keep").unwrap();
        let mut marker: serde_json::Value =
            serde_json::from_slice(&state.read("root-identity.json").unwrap()).unwrap();
        match invalid {
            0 => marker["schema"] = 2.into(),
            1 => marker["persistent"][0] = (root.binding.persistent.unwrap()[0] ^ 1).into(),
            2 => marker["original"]["volume"] = (root.binding.volume ^ 1).into(),
            3 => {
                let mut original = Binding::from(root.binding);
                original.volume ^= 1;
                let key = binding_key(&original).unwrap();
                publisher.host.create_dir(&key).unwrap();
                let duplicate = publisher.host.open_dir_nofollow(key).unwrap();
                let mut claim = marker.clone();
                claim["original"] = serde_json::to_value(original).unwrap();
                duplicate
                    .write("root-identity.json", serde_json::to_vec(&claim).unwrap())
                    .unwrap();
            }
            4 => {}
            _ => unreachable!(),
        }
        let bytes = if invalid == 4 {
            vec![b' '; 1025]
        } else {
            serde_json::to_vec(&marker).unwrap()
        };
        state.write("root-identity.json", &bytes).unwrap();
        assert!(
            RecoveryReader::new(host_path).enter(&root).is_err(),
            "case {invalid}"
        );
        assert!(
            publisher.recovery_required(&root).is_err(),
            "case {invalid}"
        );
        assert!(
            publisher
                .publish(&root, prepare(&root), &Cancellation::default())
                .is_err(),
            "case {invalid}"
        );
        assert_eq!(state.read("root-identity.json").unwrap(), bytes);
        assert_eq!(state.read("unowned").unwrap(), b"keep");
        assert_eq!(
            fs::read(project.path().join("empack.yml")).unwrap(),
            b"old intent"
        );
        assert!(!project.path().join("pack").exists());
        assert!(!state.exists("journal.json"));
        assert!(!state.exists("retention.json"));
    }
}

#[test]
fn pending_records_use_associated_parent_and_target_without_rebinding_copies() {
    use crate::engine::continuation_store::{self as store, BoundRecord, Kind};
    use empack_core::digest::ExpectedDigest;
    #[derive(Serialize, Deserialize)]
    struct Record {
        schema: u32,
        binding: store::Binding,
    }
    impl BoundRecord for Record {
        fn binding(&self) -> &store::Binding {
            &self.binding
        }
        fn schema(&self) -> u32 {
            self.schema
        }
    }
    for exists in [false, true] {
        let parent = tempfile::tempdir().unwrap();
        let target = parent.path().join("project");
        if exists {
            fixture(&target);
        }
        let host = tempfile::tempdir().unwrap();
        let state = host.path().join("private");
        let parent_root = ProjectReadRoot::open(parent.path()).unwrap();
        let target_root = exists.then(|| ProjectReadRoot::open(&target).unwrap());
        let cancel = Cancellation::default();
        assert!(
            store::observe(&state, Kind::Import, &target, &cancel)
                .unwrap()
                .is_none()
        );
        assert!(
            !state.exists(),
            "inspection cannot create identity evidence"
        );
        let mut record = Record {
            schema: 1,
            binding: store::bind(&state, &target, &cancel).unwrap().1,
        };
        let saved = store::save(&state, Kind::Import, &target, &record, None, &cancel).unwrap();
        let publisher = Publisher::open(&state).unwrap();
        // Model a record saved under the prior device label. Production never
        // edits saved bindings or moves the physical coordination directory.
        record.binding.parent = move_state_to_prior_device(&publisher, &parent_root);
        if let Some(root) = &target_root {
            record.binding.project = Some(move_state_to_prior_device(&publisher, root));
        }
        let mut digest = Sha256::new();
        digest.update(record.binding.parent.as_bytes());
        digest.update([0]);
        digest.update(b"project");
        let name = format!(
            "{}.json",
            ExpectedDigest::Sha256(digest.finalize().into()).hex()
        );
        let pending = state.join("pending-imports");
        fs::remove_file(pending.join(saved.name)).unwrap();
        let bytes = serde_json::to_vec(&record).unwrap();
        fs::write(pending.join(&name), &bytes).unwrap();
        let loaded =
            store::read::<Record>(&state, Kind::Import, &target, store::MAX_RECORD, &cancel)
                .unwrap()
                .expect("saved import must remain discoverable under its associated parent");
        assert_eq!(loaded.0.name, name);
        assert_eq!(loaded.1.binding.parent, record.binding.parent);
        assert_eq!(loaded.1.binding.project, record.binding.project);
        let observed = store::observe(&state, Kind::Import, &target, &cancel)
            .unwrap()
            .unwrap();
        assert_eq!(observed.identity(), loaded.0.identity());
        assert_eq!(fs::read(pending.join(&name)).unwrap(), bytes);
        assert!(!state.join(root_key(&parent_root).unwrap()).exists());
        if let Some(root) = &target_root {
            assert!(!state.join(root_key(root).unwrap()).exists());
        }
        if exists {
            // Release the captured directory before the replacement fixture's rename.
            drop(target_root);
            fs::rename(&target, parent.path().join("original")).unwrap();
            fixture(&target);
            assert!(
                store::read::<Record>(&state, Kind::Import, &target, store::MAX_RECORD, &cancel)
                    .is_err()
            );
            assert_eq!(fs::read(pending.join(&name)).unwrap(), bytes);
        }
    }
}
