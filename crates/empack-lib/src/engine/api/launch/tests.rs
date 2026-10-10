use super::*;
use crate::engine::{
    content::{InitialObservation, SourceEvidencePolicy, verify_stream},
    instance::{InstanceAction, InstanceSide},
    release::*,
};
use std::{fs, path::Path, time::Duration};

#[test]
fn runtime_fixture() {
    let current = std::env::current_dir().unwrap();
    if !current.join("launch-fixture").is_file() {
        return;
    }
    if current.join("descendant-fixture").is_file() {
        let mut child = std::process::Command::new(std::env::current_exe().unwrap());
        child.args([
            "--exact",
            "engine::api::launch::tests::runtime_descendant",
            "--nocapture",
        ]);
        let descendant = child.spawn().unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(15);
        while !current.join("descendant-started").exists() {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        // The fixture deliberately exits without waiting, like a daemonizing game helper.
        drop(descendant);
        return;
    }
    fs::write(current.join("launch-started"), b"running").unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    while !current.join("launch-exit").exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "fixture retirement timed out"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn install_request(bytes: &[u8]) -> InstallInstanceRequest {
    let file = ReleaseFile {
        key: "mod".into(),
        destination: "mods/a.jar".into(),
        layer: ReleaseLayer::Common,
        policy: FilePolicy::Managed,
        client: Participation::Required,
        server: Participation::Required,
        sha256: hash(bytes),
        bytes: bytes.len() as u64,
        readonly: false,
        executable: false,
        assertions: vec![],
        asset: None,
        source: ReleaseSource::Asset {
            path: "assets/a".into(),
        },
    };
    let content = verify_stream(
        &mut &*bytes,
        &file.expected().unwrap(),
        file.bytes,
        SourceEvidencePolicy::Compatibility,
        InitialObservation::RequireEvidence,
        &crate::application::process_runtime::Cancellation::default(),
    )
    .unwrap();
    let payload = DecodedRelease::encode(ReleaseDocument {
        schema: 1,
        pack: "launch".into(),
        version: "1".into(),
        minimum_engine: ">=0.6.0-beta".into(),
        runtime: ReleaseRuntime {
            minecraft: "1.21.1".into(),
            loader: ReleaseLoader::Vanilla,
            java_major: 21,
        },
        choices: vec![],
        files: vec![file],
    })
    .unwrap();
    InstallInstanceRequest {
        require_subscription: false,
        conflicts: Vec::new(),
        action: InstanceAction::Apply,
        release: SelectedRelease::Snapshot(
            trust::SelectedSnapshot::select(
                payload.bytes(),
                payload.id(),
                &semver::Version::parse("0.6.0-beta").unwrap(),
            )
            .unwrap(),
        ),
        side: InstanceSide::Client,
        layout: None,
        choices: vec![],
        supplied: BTreeMap::from([("mod".into(), content)]),
        local_files: BTreeMap::new(),
        assets: None,
    }
}
fn grant(prepared: &PreparedOperation, runtime: bool) -> ExecutionGrant {
    ExecutionGrant {
        plan: prepared.view().plan(),
        replacement: prepared.view().replacement(),
        network: NetworkPermission::Offline,
        run_installer: false,
        run_runtime: runtime,
    }
}
fn request() -> LaunchInstanceRequest {
    LaunchInstanceRequest {
        expected_release: None,
        program: std::env::current_exe().unwrap(),
        arguments: vec![
            "--exact".into(),
            "engine::api::launch::tests::runtime_fixture".into(),
            "--nocapture".into(),
        ],
    }
}
async fn prepare(engine: &Engine, root: &Path, input: impl Into<Request>) -> PreparedOperation {
    let Preparation::Ready(prepared) = engine.prepare(root.to_owned(), input).await.unwrap() else {
        panic!()
    };
    prepared
}
async fn install(engine: &Engine, root: &Path) {
    let prepared = prepare(engine, root, install_request(b"original")).await;
    let grant = grant(&prepared, false);
    let mut operation = engine.start(prepared.authorize(grant).unwrap()).unwrap();
    assert!(matches!(
        &*operation.wait().await,
        OperationOutcome::Completed(ExecutionOutcome::Completed(_))
    ));
    engine.release_completed(operation.id());
}
#[tokio::test]
async fn runtime_lease_blocks_updates_and_duplicate_launch_until_owned_retirement() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let (engine, governor) = super::super::tests::engine(state.path().join("state"));
    install(&engine, root.path()).await;
    fs::write(root.path().join("game/launch-fixture"), b"wait").unwrap();
    let preview = prepare(&engine, root.path(), request()).await;
    let denied = grant(&preview, false);
    assert!(preview.authorize(denied).is_err());
    assert!(!root.path().join("game/launch-started").exists());
    let prepared = prepare(&engine, root.path(), request()).await;
    let granted = grant(&prepared, true);
    let mut running = engine.start(prepared.authorize(granted).unwrap()).unwrap();
    tokio::time::timeout(Duration::from_secs(15), async {
        while !root.path().join("game/launch-started").exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let recover = prepare(&engine, root.path(), AcknowledgeStoppedRuntime).await;
    let granted = grant(&recover, false);
    let mut recover = engine.start(recover.authorize(granted).unwrap()).unwrap();
    assert!(matches!(
        &*recover.wait().await,
        OperationOutcome::Completed(ExecutionOutcome::FailedBeforePublication(_))
    ));
    engine.release_completed(recover.id());
    let duplicate = prepare(&engine, root.path(), request()).await;
    let granted = grant(&duplicate, true);
    let mut duplicate = engine.start(duplicate.authorize(granted).unwrap()).unwrap();
    assert!(matches!(
        &*duplicate.wait().await,
        OperationOutcome::Completed(ExecutionOutcome::FailedBeforePublication(_))
    ));
    engine.release_completed(duplicate.id());
    let changed = prepare(&engine, root.path(), install_request(b"next")).await;
    let granted = grant(&changed, false);
    let mut changed = engine.start(changed.authorize(granted).unwrap()).unwrap();
    assert!(matches!(
        &*changed.wait().await,
        OperationOutcome::Completed(ExecutionOutcome::FailedBeforePublication(_))
    ));
    engine.release_completed(changed.id());
    assert_eq!(
        fs::read(root.path().join("game/mods/a.jar")).unwrap(),
        b"original"
    );
    running.cancel();
    tokio::time::timeout(Duration::from_secs(15), running.wait())
        .await
        .unwrap();
    engine.release_completed(running.id());
    assert!(
        engine
            .prepare(root.path().to_owned(), AcknowledgeStoppedRuntime)
            .await
            .is_err(),
        "confirmed cancellation clears runtime evidence"
    );
    // A fully retired process releases both locks; a later launch can finish normally.
    fs::write(root.path().join("game/launch-exit"), b"exit").unwrap();
    let prepared = prepare(&engine, root.path(), request()).await;
    let granted = grant(&prepared, true);
    let mut completed = engine.start(prepared.authorize(granted).unwrap()).unwrap();
    let outcome = completed.wait().await;
    let OperationOutcome::Completed(ExecutionOutcome::Completed(ExecutionReceipt::Launch(receipt))) =
        &*outcome
    else {
        panic!("runtime failed")
    };
    assert!(receipt.status.success());
    drop(outcome);
    engine.release_completed(completed.id());
    drop(completed);
    drop(running);
    drop(duplicate);
    drop(changed);
    engine.shutdown().await;
    assert_eq!(governor.status().reserved, ResourceRequest::default());
}
#[tokio::test]
async fn launch_rejects_missing_modified_and_late_changed_installed_content() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let (engine, _) = super::super::tests::engine(state.path().join("state"));
    assert!(
        engine
            .prepare(root.path().to_owned(), request())
            .await
            .is_err()
    );
    install(&engine, root.path()).await;
    let prepared = prepare(&engine, root.path(), request()).await;
    let granted = grant(&prepared, true);
    fs::write(root.path().join("game/mods/a.jar"), b"edited").unwrap();
    assert!(
        engine
            .prepare(root.path().to_owned(), request())
            .await
            .is_err()
    );
    let mut operation = engine.start(prepared.authorize(granted).unwrap()).unwrap();
    assert!(matches!(
        &*operation.wait().await,
        OperationOutcome::Completed(ExecutionOutcome::FailedBeforePublication(_))
    ));
    engine.release_completed(operation.id());
    fs::remove_file(root.path().join("game/mods/a.jar")).unwrap();
    assert!(
        engine
            .prepare(root.path().to_owned(), request())
            .await
            .is_err()
    );
    engine.shutdown().await;
}

#[tokio::test]
async fn empty_native_installation_has_a_launchable_owned_directory() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let (engine, _) = super::super::tests::engine(state.path().join("state"));
    let mut input = install_request(b"unused");
    let mut document = input.release.release().document().clone();
    document.files.clear();
    let release = DecodedRelease::encode(document).unwrap();
    input.release = SelectedRelease::Snapshot(
        trust::SelectedSnapshot::select(
            release.bytes(),
            release.id(),
            &semver::Version::parse("0.6.0-beta").unwrap(),
        )
        .unwrap(),
    );
    input.supplied.clear();
    let prepared = prepare(&engine, root.path(), input).await;
    let granted = grant(&prepared, false);
    let mut installed = engine.start(prepared.authorize(granted).unwrap()).unwrap();
    assert!(matches!(
        &*installed.wait().await,
        OperationOutcome::Completed(ExecutionOutcome::Completed(_))
    ));
    engine.release_completed(installed.id());
    assert!(root.path().join("game/.empack-layout").is_file());
    let prepared = prepare(&engine, root.path(), request()).await;
    let granted = grant(&prepared, true);
    let mut operation = engine.start(prepared.authorize(granted).unwrap()).unwrap();
    let result = operation.wait().await;
    assert!(
        matches!(&*result, OperationOutcome::Completed(ExecutionOutcome::Completed(ExecutionReceipt::Launch(receipt))) if receipt.status.success())
    );
    drop(result);
    engine.release_completed(operation.id());
    engine.shutdown().await;
}

#[tokio::test]
async fn failed_spawn_does_not_leave_runtime_recovery_evidence() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let (engine, _) = super::super::tests::engine(state.path().join("state"));
    install(&engine, root.path()).await;
    let mut input = request();
    input.program = root.path().join("missing-executable");
    let prepared = prepare(&engine, root.path(), input).await;
    let granted = grant(&prepared, true);
    let mut operation = engine.start(prepared.authorize(granted).unwrap()).unwrap();
    assert!(matches!(
        &*operation.wait().await,
        OperationOutcome::Completed(ExecutionOutcome::FailedBeforePublication(_))
    ));
    engine.release_completed(operation.id());
    #[cfg(unix)]
    assert!(
        engine
            .prepare(root.path().to_owned(), AcknowledgeStoppedRuntime)
            .await
            .is_err()
    );
    engine.shutdown().await;
}

#[test]
fn runtime_descendant() {
    let root = std::env::current_dir().unwrap();
    if !root.join("descendant-fixture").is_file() {
        return;
    }
    // Publish readiness only after the observable work fixture exists.
    fs::write(root.join("descendant-heartbeat"), b"initial").unwrap();
    fs::write(root.join("descendant-started"), b"started").unwrap();
    let end = std::time::Instant::now() + Duration::from_secs(30);
    while std::time::Instant::now() < end {
        fs::write(
            root.join("descendant-heartbeat"),
            format!("{:?}", std::time::Instant::now()),
        )
        .unwrap();
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[tokio::test]
async fn launcher_exit_does_not_authorize_publication_until_descendants_retire() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let (engine, _) = super::super::tests::engine(state.path().join("state"));
    install(&engine, root.path()).await;
    fs::write(root.path().join("game/launch-fixture"), b"fixture").unwrap();
    fs::write(root.path().join("game/descendant-fixture"), b"fixture").unwrap();
    let prepared = prepare(&engine, root.path(), request()).await;
    let granted = grant(&prepared, true);
    let mut operation = engine.start(prepared.authorize(granted).unwrap()).unwrap();
    let result = tokio::time::timeout(Duration::from_secs(15), operation.wait())
        .await
        .unwrap();
    assert!(root.path().join("game/descendant-started").exists());
    let heartbeat = fs::read(root.path().join("game/descendant-heartbeat")).unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(
        fs::read(root.path().join("game/descendant-heartbeat")).unwrap(),
        heartbeat
    );
    match &*result {
        OperationOutcome::Completed(ExecutionOutcome::Completed(ExecutionReceipt::Launch(
            receipt,
        ))) => {
            assert!(receipt.status.success());
            assert!(
                engine
                    .prepare(root.path().to_owned(), AcknowledgeStoppedRuntime)
                    .await
                    .is_err()
            );
        }
        // Some hosts retain reparented zombies. Unconfirmed retirement must remain blocked.
        OperationOutcome::Completed(ExecutionOutcome::ExecutionUncertain(error)) => {
            assert!(error.is::<RuntimeRecoveryRequired>());
            let pending = prepare(&engine, root.path(), AcknowledgeStoppedRuntime).await;
            drop(pending);
            let mutation = prepare(&engine, root.path(), install_request(b"new")).await;
            let granted = grant(&mutation, false);
            let mut blocked = engine.start(mutation.authorize(granted).unwrap()).unwrap();
            assert!(matches!(
                &*blocked.wait().await,
                OperationOutcome::Completed(ExecutionOutcome::FailedBeforePublication(_))
            ));
            engine.release_completed(blocked.id());
            assert_eq!(
                fs::read(root.path().join("game/mods/a.jar")).unwrap(),
                b"original"
            );
        }
        _ => panic!("unexpected runtime outcome"),
    }
    drop(result);
    engine.release_completed(operation.id());
    engine.shutdown().await;
}

#[test]
fn runtime_recovery_failure_takes_precedence_over_cancellation() {
    let error = anyhow::Error::new(crate::application::process_runtime::Interrupted)
        .context(RuntimeRecoveryRequired);
    assert!(matches!(
        ExecutionOutcome::failed(error, true),
        ExecutionOutcome::ExecutionUncertain(_)
    ));
}

#[tokio::test]
async fn cancellation_cannot_discard_failed_runtime_evidence_cleanup() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let state_path = state.path().join("state");
    let (engine, _) = super::super::tests::engine(state_path.clone());
    install(&engine, root.path()).await;
    fs::write(root.path().join("game/launch-fixture"), b"fixture").unwrap();
    let prepared = prepare(&engine, root.path(), request()).await;
    let granted = grant(&prepared, true);
    let mut running = engine.start(prepared.authorize(granted).unwrap()).unwrap();
    tokio::time::timeout(Duration::from_secs(15), async {
        while !root.path().join("game/launch-started").exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let native_root = crate::engine::snapshot::ProjectReadRoot::open(root.path()).unwrap();
    let key = crate::engine::publication::root_key(&native_root).unwrap();
    let marker = state_path.join(key).join("runtime.json");
    assert!(marker.is_file());
    fs::write(&marker, b"changed recovery evidence").unwrap();
    running.cancel();
    let outcome = tokio::time::timeout(Duration::from_secs(15), running.wait())
        .await
        .unwrap();
    assert!(
        matches!(&*outcome, OperationOutcome::Completed(ExecutionOutcome::ExecutionUncertain(error)) if error.is::<RuntimeRecoveryRequired>())
    );
    assert_eq!(fs::read(marker).unwrap(), b"changed recovery evidence");
    drop(outcome);
    engine.release_completed(running.id());
    engine.shutdown().await;
}

#[tokio::test]
async fn launch_binds_the_release_selected_by_prelaunch() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let (engine, _) = super::super::tests::engine(state.path().join("state"));
    install(&engine, root.path()).await;
    let mut input = request();
    input.expected_release = Some("00".repeat(32));
    assert!(engine.prepare(root.path().to_owned(), input).await.is_err());
    assert!(!root.path().join("game/launch-started").exists());
    engine.shutdown().await;
}
