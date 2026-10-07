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

fn acquired_project(
    project: &empack_core::model::ResolvedProject,
) -> BTreeMap<LockedFileKey, AcquiredBuildFile> {
    project
        .lock()
        .dependencies
        .iter()
        .flat_map(|(key, dependency)| {
            dependency.files.as_slice().iter().map(|file| {
                (
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
                )
            })
        })
        .collect()
}
#[tokio::test]
async fn explicit_update_preserves_alias_and_intent_then_sync_is_a_noop() {
    use crate::engine::{
        addition::tests::fixture as provider_fixture, documents::DocumentCodec,
        layout::ProjectLayout,
    };
    use empack_core::files::ManagedPath;
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let current = provider_fixture(
        &[
            ("existing", "Project1", "Version1"),
            ("unlisted", "Project2", "Version1"),
        ],
        &["existing"],
        &[],
        true,
    );
    let requested = provider_fixture(
        &[("selector", "Project1", "Version2")],
        &["selector"],
        &[],
        true,
    );
    let mut intent = b"# author comment\n".to_vec();
    intent.extend(DocumentCodec.encode_intent(current.intent()).unwrap());
    fs::write(root.path().join("empack.yml"), &intent).unwrap();
    fs::write(
        root.path().join("empack.lock"),
        DocumentCodec.encode_lock(&current).unwrap(),
    )
    .unwrap();
    for dependency in current.lock().dependencies.values() {
        for file in dependency.files.as_slice() {
            for placement in file.placements.as_slice() {
                let path = ProjectLayout::path(&ManagedPath::Content {
                    layer: placement.layer,
                    path: placement.destination.relative().clone(),
                })
                .unwrap();
                let path = root.path().join(path.as_str());
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(path, b"payload").unwrap();
            }
        }
    }
    fs::write(root.path().join("pack/unrelated.pw.toml"), b"invalid = [").unwrap();
    let (engine, governor) = engine(state.path().join("state"));
    let make_request = || UpdateRequest {
        group: AdditionGroup::from_resolved(&requested).unwrap(),
        content: acquired_project(&requested),
    };
    let preview = engine
        .preview(root.path().to_path_buf(), make_request())
        .await
        .unwrap();
    assert_eq!(
        preview.update().unwrap().selected,
        std::collections::BTreeSet::from([DependencyKey::parse("existing").unwrap()])
    );
    assert!(preview.add().is_none());
    assert_eq!(fs::read(root.path().join("empack.yml")).unwrap(), intent);
    assert!(!state.path().join("state").exists());
    let unapproved = ready(&engine, root.path(), make_request()).await;
    let mut wrong = grant(&unapproved);
    wrong.replacement = None;
    assert!(unapproved.authorize(wrong).is_err());
    let prepared = ready(&engine, root.path(), make_request()).await;
    let permission = grant(&prepared);
    let mut handle = engine
        .start(prepared.authorize(permission).unwrap())
        .unwrap();
    let outcome = handle.wait().await;
    let updated = match &*outcome {
        OperationOutcome::Completed(ExecutionOutcome::Completed(ExecutionReceipt::Update(
            receipt,
        ))) => {
            assert_eq!(receipt.project.intent(), current.intent());
            assert_eq!(
                receipt.project.lock().dependencies[&DependencyKey::parse("existing").unwrap()]
                    .selected,
                requested.lock().dependencies[&DependencyKey::parse("selector").unwrap()].selected
            );
            assert_eq!(
                receipt.project.lock().dependencies[&DependencyKey::parse("unlisted").unwrap()],
                current.lock().dependencies[&DependencyKey::parse("unlisted").unwrap()]
            );
            receipt.project.clone()
        }
        OperationOutcome::Completed(ExecutionOutcome::FailedBeforePublication(error)) => {
            panic!("{error:#}")
        }
        _ => panic!("update did not publish"),
    };
    engine.release_completed(handle.id());
    drop(outcome);
    drop(handle);
    assert_eq!(fs::read(root.path().join("empack.yml")).unwrap(), intent);
    for _ in 0..2 {
        let view = engine
            .preview(
                root.path().to_path_buf(),
                SyncRequest {
                    content: acquired_project(&updated),
                },
            )
            .await
            .unwrap();
        assert!(view.sync().unwrap().files.changes().is_empty());
        assert!(view.sync().unwrap().files.expected().is_empty());
    }
    assert_eq!(
        fs::read(root.path().join("pack/unrelated.pw.toml")).unwrap(),
        b"invalid = ["
    );
    assert_eq!(governor.status().reserved, ResourceRequest::default());
    engine.shutdown().await;
}
