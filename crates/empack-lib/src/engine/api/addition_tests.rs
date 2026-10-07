use super::tests::{engine, fixture};
use super::*;
use crate::engine::{
    content::{InitialObservation, verify_stream},
    mrpack::{AcquiredBuildFile, LockedFileKey, tests::project},
};
use empack_core::{
    addition::AdditionGroup,
    files::FilePermissions,
    model::DependencyKey,
    removal::{RemovalEvidencePolicy, RemovalMode},
};
use std::{collections::BTreeMap, fs, path::Path};
fn request(policy: ExistingDependencyPolicy) -> AddRequest {
    let project = project(false, false);
    let mut content = BTreeMap::new();
    for (key, dependency) in &project.lock().dependencies {
        for file in dependency.files.as_slice() {
            content.insert(
                LockedFileKey {
                    dependency: key.clone(),
                    slot: file.slot.clone(),
                },
                AcquiredBuildFile {
                    content: verify_stream(
                        &mut b"payload".as_slice(),
                        &file.expected,
                        100,
                        SourceEvidencePolicy::Compatibility,
                        InitialObservation::RequireEvidence,
                        &crate::application::process_runtime::Cancellation::default(),
                    )
                    .unwrap(),
                    permissions: FilePermissions {
                        readonly: false,
                        executable: false,
                    },
                },
            );
        }
    }
    AddRequest {
        group: AdditionGroup::from_resolved(&project).unwrap(),
        content,
        existing: policy,
    }
}
fn grant(prepared: &PreparedOperation) -> ExecutionGrant {
    ExecutionGrant {
        plan: prepared.view().plan(),
        network: NetworkPermission::Offline,
        run_installer: false,
        replacement: prepared.view().replacement(),
    }
}
async fn ready(engine: &Engine, root: &Path, request: impl Into<Request>) -> PreparedOperation {
    match engine.prepare(root.to_path_buf(), request).await.unwrap() {
        Preparation::Ready(value) => value,
        _ => panic!("unexpected missing input"),
    }
}
#[tokio::test]
async fn addition_requires_existing_identity_policy_exact_grants_and_engine_ownership() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    fixture(root.path());
    let (engine, governor) = engine(state.path().join("state"));
    let before = fs::read(root.path().join("empack.yml")).unwrap();
    assert!(
        engine
            .prepare(
                root.path().to_path_buf(),
                request(ExistingDependencyPolicy::RejectExisting)
            )
            .await
            .is_err()
    );
    let preview = engine
        .preview(
            root.path().to_path_buf(),
            request(ExistingDependencyPolicy::UpdateSameIdentity),
        )
        .await
        .unwrap();
    assert_eq!(preview.add().unwrap().existing_roots.len(), 1);
    assert!(preview.add().unwrap().files.changes().is_empty());
    assert_eq!(governor.status().reserved, ResourceRequest::default());
    let prepared = ready(
        &engine,
        root.path(),
        request(ExistingDependencyPolicy::UpdateSameIdentity),
    )
    .await;
    let mut permission = grant(&prepared);
    permission.replacement = None;
    assert!(prepared.authorize(permission).is_err());
    let first = ready(
        &engine,
        root.path(),
        request(ExistingDependencyPolicy::UpdateSameIdentity),
    )
    .await;
    let second = ready(
        &engine,
        root.path(),
        request(ExistingDependencyPolicy::UpdateSameIdentity),
    )
    .await;
    assert!(first.authorize(grant(&second)).is_err());
    drop(second);
    let other = Engine::new(engine.config.clone(), governor.clone()).unwrap();
    let prepared = ready(
        &engine,
        root.path(),
        request(ExistingDependencyPolicy::UpdateSameIdentity),
    )
    .await;
    let permission = grant(&prepared);
    assert!(
        other
            .start(prepared.authorize(permission).unwrap())
            .is_err()
    );
    assert!(
        engine
            .prepare(
                ProjectTarget::New(root.path().join("new")),
                request(ExistingDependencyPolicy::UpdateSameIdentity)
            )
            .await
            .is_err()
    );
    assert_eq!(fs::read(root.path().join("empack.yml")).unwrap(), before);
    assert!(!state.path().join("state").exists());
    assert_eq!(governor.status().reserved, ResourceRequest::default());
    other.shutdown().await;
    engine.shutdown().await;
}
#[tokio::test]
async fn remove_add_readd_sync_twice_build_share_logical_and_byte_postconditions() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    fixture(root.path());
    let (engine, governor) = engine(state.path().join("state"));
    let remove = RemoveRequest {
        selections: NonEmpty::new(vec![RemovalSelector::Key(
            DependencyKey::parse("assets").unwrap(),
        )])
        .unwrap(),
        mode: RemovalMode::RemoveContent,
        evidence: RemovalEvidencePolicy::RequireComplete,
    };
    let prepared = ready(&engine, root.path(), remove).await;
    let permission = grant(&prepared);
    let mut handle = engine
        .start(prepared.authorize(permission).unwrap())
        .unwrap();
    let outcome = handle.wait().await;
    assert!(matches!(
        &*outcome,
        OperationOutcome::Completed(ExecutionOutcome::Completed(ExecutionReceipt::Remove(_)))
    ));
    engine.release_completed(handle.id());
    drop(outcome);
    drop(handle);
    for policy in [
        ExistingDependencyPolicy::RejectExisting,
        ExistingDependencyPolicy::UpdateSameIdentity,
    ] {
        let prepared = ready(&engine, root.path(), request(policy)).await;
        if policy == ExistingDependencyPolicy::UpdateSameIdentity {
            assert!(prepared.view().add().unwrap().files.changes().is_empty());
        }
        let permission = grant(&prepared);
        let mut handle = engine
            .start(prepared.authorize(permission).unwrap())
            .unwrap();
        let outcome = handle.wait().await;
        match &*outcome {
            OperationOutcome::Completed(ExecutionOutcome::Completed(ExecutionReceipt::Add(
                receipt,
            ))) => {
                assert_eq!(receipt.project.intent().roots.len(), 1);
                assert_eq!(receipt.bindings.len(), 1);
                assert_eq!(governor.status().reserved, engine.config.resources.receipt);
            }
            OperationOutcome::Completed(ExecutionOutcome::FailedBeforePublication(error)) => {
                panic!("{error:#}")
            }
            _ => panic!("addition failed"),
        }
        engine.release_completed(handle.id());
        drop(outcome);
        drop(handle);
        assert_eq!(governor.status().reserved, ResourceRequest::default());
    }
    for name in ["a.zip", "b.zip", "copy.zip"] {
        assert_eq!(
            fs::read(root.path().join(format!("pack/resourcepacks/{name}"))).unwrap(),
            b"payload"
        );
    }
    fs::write(
        root.path().join("pack/resourcepacks/a.zip"),
        b"edited after add",
    )
    .unwrap();
    for repeat in [false, true] {
        let sync = SyncRequest {
            content: request(ExistingDependencyPolicy::UpdateSameIdentity).content,
        };
        let prepared = ready(&engine, root.path(), sync).await;
        assert_eq!(prepared.view().sync().unwrap().selected.len(), 1);
        assert_eq!(
            prepared.view().sync().unwrap().files.changes().is_empty(),
            repeat
        );
        let permission = grant(&prepared);
        let mut handle = engine
            .start(prepared.authorize(permission).unwrap())
            .unwrap();
        let outcome = handle.wait().await;
        match &*outcome {
            OperationOutcome::Completed(ExecutionOutcome::Completed(ExecutionReceipt::Sync(
                receipt,
            ))) => assert_eq!(receipt.project.intent().roots.len(), 1),
            OperationOutcome::Completed(ExecutionOutcome::FailedBeforePublication(error)) => {
                panic!("{error:#}")
            }
            _ => panic!("synchronization failed"),
        }
        engine.release_completed(handle.id());
        drop(outcome);
        drop(handle);
        assert_eq!(governor.status().reserved, ResourceRequest::default());
        assert_eq!(
            fs::read(root.path().join("pack/resourcepacks/a.zip")).unwrap(),
            b"payload"
        );
    }
    let prepared = ready(&engine, root.path(), super::tests::request()).await;
    let permission = grant(&prepared);
    let mut handle = engine
        .start(prepared.authorize(permission).unwrap())
        .unwrap();
    let outcome = handle.wait().await;
    match &*outcome {
        OperationOutcome::Completed(ExecutionOutcome::Completed(ExecutionReceipt::Build(
            receipt,
        ))) => assert_eq!(receipt.artifacts.len(), 2),
        OperationOutcome::Completed(ExecutionOutcome::FailedBeforePublication(error)) => {
            panic!("{error:#}")
        }
        _ => panic!("build after addition failed"),
    }
    engine.release_completed(handle.id());
    drop(outcome);
    drop(handle);
    engine.shutdown().await;
    assert_eq!(governor.status().reserved, ResourceRequest::default());
}

#[tokio::test]
async fn synchronization_requires_grants_and_new_resolution_for_runtime_changes() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    fixture(root.path());
    let (engine, governor) = engine(state.path().join("state"));
    let sync = || SyncRequest {
        content: request(ExistingDependencyPolicy::UpdateSameIdentity).content,
    };
    let prepared = ready(&engine, root.path(), sync()).await;
    let mut permission = grant(&prepared);
    permission.replacement = None;
    assert!(prepared.authorize(permission).is_err());
    let before = fs::read(root.path().join("empack.yml")).unwrap();
    let codec = crate::engine::documents::DocumentCodec;
    let source = codec.decode_intent(&before, "fixture").unwrap();
    let mut changed = source.intent().clone();
    changed.runtime.minecraft = empack_core::model::GameVersion::parse("1.21.1").unwrap();
    let changed = codec.encode_intent(&changed).unwrap();
    fs::write(root.path().join("empack.yml"), &changed).unwrap();
    let result = engine.prepare(root.path().to_path_buf(), sync()).await;
    assert!(
        result
            .err()
            .unwrap()
            .downcast_ref::<crate::engine::synchronization::ResolutionRequired>()
            .is_some()
    );
    assert_eq!(fs::read(root.path().join("empack.yml")).unwrap(), changed);
    assert!(!state.path().join("state").exists());
    assert_eq!(governor.status().reserved, ResourceRequest::default());
    engine.shutdown().await;
}
