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
