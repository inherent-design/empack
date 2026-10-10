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
fn references(project: &empack_core::model::ResolvedProject) -> DependencyContents {
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
                    DependencyContent::Reference,
                )
            })
        })
        .collect()
}

#[tokio::test]
async fn reference_add_sync_export_and_materialization_keep_distinct_byte_obligations() {
    use crate::engine::documents::DocumentCodec;
    use empack_core::{files::ManagedPath, model::ResolvedProject};
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    fixture(root.path());
    let resolved = project(false, false);
    let mut intent = resolved.intent().clone();
    intent.roots.clear();
    let source = DocumentCodec
        .decode_intent(&DocumentCodec.encode_intent(&intent).unwrap(), "empty")
        .unwrap();
    let mut lock = resolved.lock().clone();
    lock.dependencies.clear();
    lock.coverage.clear();
    lock.required_edges.clear();
    lock.intent_revision = source.semantic_revision();
    let empty = ResolvedProject::validate(intent, lock, source.semantic_revision()).unwrap();
    fs::write(
        root.path().join("empack.yml"),
        DocumentCodec.encode_intent(empty.intent()).unwrap(),
    )
    .unwrap();
    fs::write(
        root.path().join("empack.lock"),
        DocumentCodec.encode_lock(&empty).unwrap(),
    )
    .unwrap();
    for name in ["a.zip", "b.zip", "copy.zip"] {
        fs::remove_file(root.path().join(format!("pack/resourcepacks/{name}"))).unwrap();
    }
    let (engine, governor) = engine(state.path().join("state"));
    let make_request = || AddRequest {
        source_revision: None,
        group: AdditionGroup::from_resolved(&resolved).unwrap(),
        content: references(&resolved),
        existing: ExistingDependencyPolicy::RejectExisting,
    };
    let prepared = ready(&engine, root.path(), make_request()).await;
    let preview = prepared.view();
    assert_eq!(preview.add().unwrap().references.len(), 2);
    assert!(
        preview
            .add()
            .unwrap()
            .files
            .expected()
            .keys()
            .all(|target| !matches!(target, ManagedPath::Content { .. }))
    );
    let permission = grant(&prepared);
    let mut handle = engine
        .start(prepared.authorize(permission).unwrap())
        .unwrap();
    let outcome = handle.wait().await;
    let OperationOutcome::Completed(ExecutionOutcome::Completed(ExecutionReceipt::Add(receipt))) =
        &*outcome
    else {
        panic!("reference addition failed")
    };
    assert_eq!(receipt.references.len(), 2);
    assert_eq!(receipt.project.lock().dependencies.len(), 1);
    engine.release_completed(handle.id());
    drop((outcome, handle));
    for _ in 0..2 {
        let preview = engine
            .preview(
                root.path().to_path_buf(),
                SyncRequest::Supplied {
                    resolution: None,
                    content: references(&resolved),
                },
            )
            .await
            .unwrap();
        assert_eq!(preview.sync().unwrap().references.len(), 2);
        assert!(preview.sync().unwrap().files.changes().is_empty());
    }
    assert!(!root.path().join("pack/resourcepacks/a.zip").exists());
    let mut build = super::tests::request();
    build.outputs = NonEmpty::new(vec![BuildOutput {
        target: Recipe::MODRINTH,
        artifact: empack_core::path::PortableRelPath::parse(
            "result.mrpack",
            empack_core::path::PathSyntax::ProjectContent,
        )
        .unwrap(),
    }])
    .unwrap();
    let prepared = ready(&engine, root.path(), build).await;
    assert!(!prepared.view().build().unwrap().needs_network);
    let permission = grant(&prepared);
    let mut handle = engine
        .start(prepared.authorize(permission).unwrap())
        .unwrap();
    let outcome = handle.wait().await;
    assert!(matches!(
        &*outcome,
        OperationOutcome::Completed(ExecutionOutcome::Completed(ExecutionReceipt::Build(_)))
    ));
    engine.release_completed(handle.id());
    drop((outcome, handle));
    let mut archive =
        zip::ZipArchive::new(fs::File::open(root.path().join("dist/result.mrpack")).unwrap())
            .unwrap();
    let index: serde_json::Value =
        serde_json::from_reader(archive.by_name("modrinth.index.json").unwrap()).unwrap();
    assert_eq!(index["files"].as_array().unwrap().len(), 3);
    assert_eq!(index["files"][0]["path"], "resourcepacks/a.zip");
    assert_eq!(index["files"][0]["env"]["server"], "unsupported");
    let view = engine
        .preview(root.path().to_path_buf(), super::tests::request())
        .await
        .unwrap();
    assert!(view.build().unwrap().needs_network);
    assert!(!view.build().unwrap().content.is_empty());
    let prepared = ready(
        &engine,
        root.path(),
        request(ExistingDependencyPolicy::UpdateSameIdentity),
    )
    .await;
    let permission = grant(&prepared);
    let mut handle = engine
        .start(prepared.authorize(permission).unwrap())
        .unwrap();
    let outcome = handle.wait().await;
    let OperationOutcome::Completed(ExecutionOutcome::Completed(ExecutionReceipt::Add(receipt))) =
        &*outcome
    else {
        panic!("materialization failed")
    };
    assert!(receipt.references.is_empty());
    engine.release_completed(handle.id());
    drop((outcome, handle));
    let path = root.path().join("pack/resourcepacks/a.zip");
    let modified = fs::metadata(&path).unwrap().modified().unwrap();
    let preview = engine
        .preview(
            root.path().to_path_buf(),
            SyncRequest::Supplied {
                resolution: None,
                content: references(&resolved),
            },
        )
        .await
        .unwrap();
    assert!(preview.sync().unwrap().files.changes().is_empty());
    assert_eq!(fs::read(&path).unwrap(), b"payload");
    assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), modified);
    assert_eq!(governor.status().reserved, ResourceRequest::default());
    engine.shutdown().await;
}

#[tokio::test]
async fn reference_requests_cannot_omit_slots_or_accept_changed_payloads() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    fixture(root.path());
    let (engine, governor) = engine(state.path().join("state"));
    let mut content = references(&project(false, false));
    content.pop_first();
    assert!(
        engine
            .prepare(
                root.path().to_path_buf(),
                SyncRequest::Supplied {
                    resolution: None,
                    content
                }
            )
            .await
            .is_err()
    );
    let mut add = request(ExistingDependencyPolicy::UpdateSameIdentity);
    add.content.clear();
    assert!(
        engine
            .prepare(root.path().to_path_buf(), add)
            .await
            .is_err()
    );
    fs::write(
        root.path().join("pack/resourcepacks/a.zip"),
        b"user changes",
    )
    .unwrap();
    assert!(
        engine
            .prepare(
                root.path().to_path_buf(),
                SyncRequest::Supplied {
                    resolution: None,
                    content: references(&project(false, false))
                }
            )
            .await
            .is_err()
    );
    assert_eq!(
        fs::read(root.path().join("pack/resourcepacks/a.zip")).unwrap(),
        b"user changes"
    );
    assert!(!state.path().join("state").exists());
    assert_eq!(governor.status().reserved, ResourceRequest::default());
    engine.shutdown().await;
}
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
        source_revision: None,
        group: AdditionGroup::from_resolved(&project).unwrap(),
        content: crate::engine::dependency_content::materialized(content),
        existing: policy,
    }
}
fn grant(prepared: &PreparedOperation) -> ExecutionGrant {
    ExecutionGrant {
        plan: prepared.view().plan(),
        network: NetworkPermission::Offline,
        run_installer: false,
        run_runtime: false,
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
        let prepared = ready(&engine, root.path(), request(policy.clone())).await;
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
        let sync = SyncRequest::Supplied {
            resolution: None,
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
    let sync = || SyncRequest::Supplied {
        resolution: None,
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
        source_revision: None,
        group: AdditionGroup::from_resolved(&requested).unwrap(),
        content: crate::engine::dependency_content::materialized(acquired_project(&requested)),
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
                receipt.selected,
                std::collections::BTreeSet::from([DependencyKey::parse("existing").unwrap()])
            );
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
                SyncRequest::Supplied {
                    resolution: None,
                    content: crate::engine::dependency_content::materialized(acquired_project(
                        &updated,
                    )),
                },
            )
            .await
            .unwrap();
        assert!(view.sync().unwrap().files.changes().is_empty());
        assert!(view.sync().unwrap().files.expected().is_empty());
    }
    // An author changes a pin after the explicit update. Fresh resolution may satisfy this
    // change, but the ordinary recorded-only request must not pretend the old pin still fits.
    let mut next_intent = updated.intent().clone();
    let pin = empack_core::identity::PinSelector::ModrinthVersion(
        empack_core::identity::ModrinthVersionId::parse("Version3").unwrap(),
    );
    next_intent
        .roots
        .get_mut(&DependencyKey::parse("existing").unwrap())
        .unwrap()
        .version = empack_core::model::VersionIntent::Exact(pin.clone());
    let raw = DocumentCodec.encode_intent(&next_intent).unwrap();
    let source = DocumentCodec.decode_intent(&raw, "edited pin").unwrap();
    let mut lock = updated.lock().clone();
    lock.intent_revision = source.semantic_revision();
    lock.dependencies
        .get_mut(&DependencyKey::parse("existing").unwrap())
        .unwrap()
        .selected
        .as_mut()
        .unwrap()
        .selection = pin;
    let resolved = empack_core::model::ResolvedProject::validate(
        next_intent,
        lock,
        source.semantic_revision(),
    )
    .unwrap();
    fs::write(root.path().join("empack.yml"), &raw).unwrap();
    assert!(
        engine
            .preview(
                root.path().to_path_buf(),
                SyncRequest::Supplied {
                    resolution: None,
                    content: crate::engine::dependency_content::materialized(acquired_project(
                        &updated
                    ))
                }
            )
            .await
            .is_err()
    );
    let prepared = ready(
        &engine,
        root.path(),
        SyncRequest::Supplied {
            resolution: Some(resolved.clone()),
            content: crate::engine::dependency_content::materialized(acquired_project(&resolved)),
        },
    )
    .await;
    let permission = grant(&prepared);
    let mut handle = engine
        .start(prepared.authorize(permission).unwrap())
        .unwrap();
    let outcome = handle.wait().await;
    match &*outcome {
        OperationOutcome::Completed(ExecutionOutcome::Completed(ExecutionReceipt::Sync(
            receipt,
        ))) => assert_eq!(receipt.project.lock(), resolved.lock()),
        OperationOutcome::Completed(ExecutionOutcome::FailedBeforePublication(error)) => {
            panic!("{error:#}")
        }
        _ => panic!("changed-intent synchronization did not publish"),
    }
    engine.release_completed(handle.id());
    drop(outcome);
    drop(handle);
    assert_eq!(fs::read(root.path().join("empack.yml")).unwrap(), raw);
    let no_op = engine
        .preview(
            root.path().to_path_buf(),
            SyncRequest::Supplied {
                resolution: None,
                content: crate::engine::dependency_content::materialized(acquired_project(
                    &resolved,
                )),
            },
        )
        .await
        .unwrap();
    assert!(no_op.sync().unwrap().files.changes().is_empty());
    assert_eq!(
        fs::read(root.path().join("pack/unrelated.pw.toml")).unwrap(),
        b"invalid = ["
    );
    assert_eq!(governor.status().reserved, ResourceRequest::default());
    engine.shutdown().await;
}

#[tokio::test]
async fn adoption_publishes_observed_intent_without_payload_writes_then_sync_retains_it() {
    use empack_core::{
        digest::{DigestSet, ExpectedDigest},
        files::ManagedPath,
        model::{NonEmpty, ResolvedProject},
    };
    use sha2::{Digest, Sha256};
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    fixture(root.path());
    let previous = project(false, false);
    let mut lock = previous.lock().clone();
    let digest = DigestSet::new(vec![ExpectedDigest::Sha256(
        Sha256::digest(b"adopted").into(),
    )])
    .unwrap();
    for dependency in lock.dependencies.values_mut() {
        dependency.files = NonEmpty::new(
            dependency
                .files
                .as_slice()
                .iter()
                .cloned()
                .map(|mut file| {
                    file.expected.digests = Some(digest.clone());
                    file.provenance.declared_digests = Some(digest.clone());
                    file
                })
                .collect(),
        )
        .unwrap();
    }
    let proposed = ResolvedProject::validate(
        previous.intent().clone(),
        lock,
        previous.lock().intent_revision,
    )
    .unwrap();
    for name in ["a.zip", "b.zip", "copy.zip"] {
        fs::write(
            root.path().join(format!("pack/resourcepacks/{name}")),
            b"adopted",
        )
        .unwrap();
    }
    let before = fs::metadata(root.path().join("pack/resourcepacks/a.zip"))
        .unwrap()
        .modified()
        .unwrap();
    let old_lock = fs::read(root.path().join("empack.lock")).unwrap();
    let (engine, governor) = engine(state.path().join("state"));
    let make_request = || AdoptObservedRequest {
        group: AdditionGroup::from_resolved(&proposed).unwrap(),
    };
    let view = engine
        .preview(root.path().to_path_buf(), make_request())
        .await
        .unwrap();
    let selections = &view.adoption().unwrap().selections;
    assert_eq!(selections.len(), proposed.lock().dependencies.len());
    for change in selections {
        assert_eq!(
            change.before.as_ref().unwrap().dependency,
            previous.lock().dependencies[&change.key]
        );
        assert_eq!(
            change.after.dependency,
            proposed.lock().dependencies[&change.key]
        );
        assert_ne!(
            change.before.as_ref().unwrap().dependency.files,
            change.after.dependency.files
        );
    }
    assert!(
        view.adoption()
            .unwrap()
            .files
            .changes()
            .iter()
            .all(|change| !matches!(change.target(), ManagedPath::Content { .. }))
    );
    assert_eq!(fs::read(root.path().join("empack.lock")).unwrap(), old_lock);
    assert!(!state.path().join("state").exists());
    let prepared = ready(&engine, root.path(), make_request()).await;
    let mut wrong = grant(&prepared);
    wrong.replacement = None;
    assert!(prepared.authorize(wrong).is_err());
    let prepared = ready(&engine, root.path(), make_request()).await;
    let permission = grant(&prepared);
    let mut handle = engine
        .start(prepared.authorize(permission).unwrap())
        .unwrap();
    let outcome = handle.wait().await;
    match &*outcome {
        OperationOutcome::Completed(ExecutionOutcome::Completed(
            ExecutionReceipt::AdoptObserved(receipt),
        )) => assert_eq!(receipt.project.lock(), proposed.lock()),
        OperationOutcome::Completed(ExecutionOutcome::FailedBeforePublication(error)) => {
            panic!("{error:#}")
        }
        _ => panic!("adoption did not publish"),
    }
    engine.release_completed(handle.id());
    drop(outcome);
    drop(handle);
    assert_eq!(
        fs::metadata(root.path().join("pack/resourcepacks/a.zip"))
            .unwrap()
            .modified()
            .unwrap(),
        before
    );
    let observed = verify_stream(
        &mut b"adopted".as_slice(),
        &proposed
            .lock()
            .dependencies
            .values()
            .next()
            .unwrap()
            .files
            .as_slice()[0]
            .expected,
        100,
        SourceEvidencePolicy::Compatibility,
        InitialObservation::RequireEvidence,
        &crate::application::process_runtime::Cancellation::default(),
    )
    .unwrap();
    let content = proposed
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
                        content: observed.clone(),
                        permissions: FilePermissions {
                            readonly: false,
                            executable: false,
                        },
                    },
                )
            })
        })
        .collect();
    let no_op = engine
        .preview(
            root.path().to_path_buf(),
            SyncRequest::Supplied {
                resolution: None,
                content: crate::engine::dependency_content::materialized(content),
            },
        )
        .await
        .unwrap();
    assert!(no_op.sync().unwrap().files.changes().is_empty());
    assert_eq!(governor.status().reserved, ResourceRequest::default());
    engine.shutdown().await;
}

#[tokio::test]
async fn adoption_creates_first_lock_without_rewriting_observed_payloads() {
    use empack_core::files::ManagedPath;
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    fixture(root.path());
    fs::remove_file(root.path().join("empack.lock")).unwrap();
    let resolved = project(false, false);
    let intent = fs::read(root.path().join("empack.yml")).unwrap();
    let payload = root.path().join("pack/resourcepacks/a.zip");
    let before = fs::metadata(&payload).unwrap().modified().unwrap();
    let (engine, governor) = engine(state.path().join("state"));
    let request = || AdoptObservedRequest {
        group: AdditionGroup::from_resolved(&resolved).unwrap(),
    };
    let view = engine
        .preview(root.path().to_path_buf(), request())
        .await
        .unwrap();
    assert_eq!(
        view.adoption().unwrap().selections.len(),
        resolved.lock().dependencies.len()
    );
    assert!(
        view.adoption()
            .unwrap()
            .selections
            .iter()
            .all(|change| change.before.is_none())
    );
    assert!(
        view.adoption()
            .unwrap()
            .files
            .changes()
            .iter()
            .any(|change| *change.target() == ManagedPath::LockDocument)
    );
    assert!(
        view.adoption()
            .unwrap()
            .files
            .changes()
            .iter()
            .all(|change| !matches!(change.target(), ManagedPath::Content { .. }))
    );
    assert!(!root.path().join("empack.lock").exists());
    assert!(!state.path().join("state").exists());
    let prepared = ready(&engine, root.path(), request()).await;
    let permission = grant(&prepared);
    let mut handle = engine
        .start(prepared.authorize(permission).unwrap())
        .unwrap();
    let outcome = handle.wait().await;
    assert!(matches!(
        &*outcome,
        OperationOutcome::Completed(ExecutionOutcome::Completed(
            ExecutionReceipt::AdoptObserved(_)
        ))
    ));
    engine.release_completed(handle.id());
    drop((outcome, handle));
    assert_eq!(fs::read(root.path().join("empack.yml")).unwrap(), intent);
    assert_eq!(fs::metadata(&payload).unwrap().modified().unwrap(), before);
    assert!(
        engine
            .preview(root.path().to_path_buf(), request())
            .await
            .unwrap()
            .adoption()
            .unwrap()
            .selections
            .is_empty()
    );
    for _ in 0..2 {
        let view = engine
            .preview(
                root.path().to_path_buf(),
                SyncRequest::Supplied {
                    resolution: None,
                    content: crate::engine::dependency_content::materialized(acquired_project(
                        &resolved,
                    )),
                },
            )
            .await
            .unwrap();
        assert!(view.sync().unwrap().files.changes().is_empty());
    }
    assert_eq!(governor.status().reserved, ResourceRequest::default());
    engine.shutdown().await;
}

#[tokio::test]
async fn initial_adoption_refuses_missing_changed_or_late_conflicting_inputs() {
    for mode in ["missing", "mismatch", "late-payload", "late-lock"] {
        let root = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        fixture(root.path());
        fs::remove_file(root.path().join("empack.lock")).unwrap();
        let intent = fs::read(root.path().join("empack.yml")).unwrap();
        let payload = root.path().join("pack/resourcepacks/a.zip");
        let resolved = project(false, false);
        let (engine, governor) = engine(state.path().join("state"));
        let request = || AdoptObservedRequest {
            group: AdditionGroup::from_resolved(&resolved).unwrap(),
        };
        if mode == "missing" {
            fs::remove_file(&payload).unwrap();
        }
        if mode == "mismatch" {
            fs::write(&payload, b"changed").unwrap();
        }
        if mode.starts_with("late-") {
            let prepared = ready(&engine, root.path(), request()).await;
            let permission = grant(&prepared);
            if mode == "late-lock" {
                fs::write(root.path().join("empack.lock"), b"another writer").unwrap();
            } else {
                fs::write(&payload, b"changed").unwrap();
            }
            let mut handle = engine
                .start(prepared.authorize(permission).unwrap())
                .unwrap();
            let outcome = handle.wait().await;
            assert!(
                matches!(
                    &*outcome,
                    OperationOutcome::Completed(ExecutionOutcome::FailedBeforePublication(_))
                ),
                "{mode}"
            );
            engine.release_completed(handle.id());
            drop((outcome, handle));
        } else {
            assert!(
                engine
                    .preview(root.path().to_path_buf(), request())
                    .await
                    .is_err()
            );
        }
        assert_eq!(fs::read(root.path().join("empack.yml")).unwrap(), intent);
        if mode == "late-lock" {
            assert_eq!(
                fs::read(root.path().join("empack.lock")).unwrap(),
                b"another writer"
            );
        } else {
            assert!(!root.path().join("empack.lock").exists());
        }
        assert_eq!(governor.status().reserved, ResourceRequest::default());
        engine.shutdown().await;
    }
}

#[tokio::test]
async fn selected_identity_replacement_publishes_one_candidate_then_sync_converges() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    fixture(root.path());
    let previous = project(false, false);
    let replacement = crate::engine::addition::tests::fixture(
        &[("assets", "Project3", "Version3")],
        &["assets"],
        &[],
        true,
    );
    let (engine, governor) = engine(state.path().join("state"));
    let request = || AddRequest {
        source_revision: None,
        group: AdditionGroup::from_resolved(&replacement).unwrap(),
        content: crate::engine::dependency_content::materialized(acquired_project(&replacement)),
        existing: ExistingDependencyPolicy::ReplaceSelected(ReplacementSelection {
            keys: NonEmpty::new(vec![DependencyKey::parse("assets").unwrap()]).unwrap(),
            evidence: RemovalEvidencePolicy::RequireComplete,
        }),
    };
    let before = fs::read(root.path().join("empack.lock")).unwrap();
    let prepared = ready(&engine, root.path(), request()).await;
    assert_eq!(
        prepared.view().add().unwrap().replaced,
        previous.lock().dependencies
    );
    assert_eq!(fs::read(root.path().join("empack.lock")).unwrap(), before);
    let mut permission = grant(&prepared);
    permission.replacement = None;
    assert!(prepared.authorize(permission).is_err());
    let prepared = ready(&engine, root.path(), request()).await;
    let permission = grant(&prepared);
    let mut handle = engine
        .start(prepared.authorize(permission).unwrap())
        .unwrap();
    let outcome = handle.wait().await;
    match &*outcome {
        OperationOutcome::Completed(ExecutionOutcome::Completed(ExecutionReceipt::Add(
            receipt,
        ))) => {
            assert_eq!(receipt.replaced, previous.lock().dependencies);
            assert_eq!(
                receipt.project.lock().dependencies,
                replacement.lock().dependencies
            );
        }
        OperationOutcome::Completed(ExecutionOutcome::FailedBeforePublication(error)) => {
            panic!("{error:#}")
        }
        _ => panic!("replacement failed"),
    }
    engine.release_completed(handle.id());
    drop((outcome, handle));
    for file in ["a.zip", "copy.zip", "b.zip"] {
        assert!(
            !root
                .path()
                .join(format!("pack/resourcepacks/{file}"))
                .exists()
        );
        assert_eq!(
            fs::read(
                root.path()
                    .join(format!("pack/Project3/resourcepacks/{file}"))
            )
            .unwrap(),
            b"payload"
        );
    }
    assert_eq!(
        fs::read(root.path().join("pack/config/value")).unwrap(),
        b"current config"
    );
    for _ in 0..2 {
        let view = engine
            .preview(
                root.path().to_path_buf(),
                SyncRequest::Supplied {
                    resolution: None,
                    content: crate::engine::dependency_content::materialized(acquired_project(
                        &replacement,
                    )),
                },
            )
            .await
            .unwrap();
        assert!(view.sync().unwrap().files.changes().is_empty());
    }
    assert_eq!(governor.status().reserved, ResourceRequest::default());
    engine.shutdown().await;
}

#[tokio::test]
async fn selected_replacement_refuses_unverified_or_unowned_effects() {
    for mode in ["old-drift", "new-collision", "missing-slot", "late-drift"] {
        let root = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        fixture(root.path());
        let replacement = crate::engine::addition::tests::fixture(
            &[("assets", "Project3", "Version3")],
            &["assets"],
            &[],
            true,
        );
        let old = root.path().join("pack/resourcepacks/a.zip");
        if mode == "old-drift" {
            fs::write(&old, b"changed").unwrap();
        }
        if mode == "new-collision" {
            super::tests::put(root.path(), "pack/Project3/resourcepacks/a.zip", b"payload");
        }
        let (engine, governor) = engine(state.path().join("state"));
        let mut request = AddRequest {
            source_revision: None,
            group: AdditionGroup::from_resolved(&replacement).unwrap(),
            content: crate::engine::dependency_content::materialized(acquired_project(
                &replacement,
            )),
            existing: ExistingDependencyPolicy::ReplaceSelected(ReplacementSelection {
                keys: NonEmpty::new(vec![DependencyKey::parse("assets").unwrap()]).unwrap(),
                evidence: RemovalEvidencePolicy::RequireComplete,
            }),
        };
        if mode == "missing-slot" {
            request.content.pop_first();
        }
        let intent = fs::read(root.path().join("empack.yml")).unwrap();
        let lock = fs::read(root.path().join("empack.lock")).unwrap();
        if mode == "late-drift" {
            let prepared = ready(&engine, root.path(), request).await;
            let permission = grant(&prepared);
            fs::write(&old, b"changed").unwrap();
            let mut handle = engine
                .start(prepared.authorize(permission).unwrap())
                .unwrap();
            let outcome = handle.wait().await;
            assert!(matches!(
                &*outcome,
                OperationOutcome::Completed(ExecutionOutcome::FailedBeforePublication(_))
            ));
            engine.release_completed(handle.id());
            drop((outcome, handle));
        } else {
            assert!(
                engine
                    .prepare(root.path().to_path_buf(), request)
                    .await
                    .is_err(),
                "{mode}"
            );
        }
        assert_eq!(fs::read(root.path().join("empack.yml")).unwrap(), intent);
        assert_eq!(fs::read(root.path().join("empack.lock")).unwrap(), lock);
        assert_eq!(
            fs::read(root.path().join("pack/resourcepacks/b.zip")).unwrap(),
            b"payload"
        );
        if mode.ends_with("drift") {
            assert_eq!(fs::read(old).unwrap(), b"changed");
        }
        assert_eq!(governor.status().reserved, ResourceRequest::default());
        engine.shutdown().await;
    }
}

#[tokio::test]
async fn bound_addition_rejects_identical_documents_in_another_native_project() {
    use crate::application::process_runtime::Cancellation;
    let original = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    fixture(original.path());
    fixture(other.path());
    for document in ["empack.yml", "empack.lock"] {
        assert_eq!(
            fs::read(original.path().join(document)).unwrap(),
            fs::read(other.path().join(document)).unwrap()
        );
    }
    let state = host.path().join("state");
    let observed = ProjectReader::new(RecoveryReader::new(state.clone()))
        .capture(
            original.path(),
            &[],
            SnapshotLimits::default(),
            &Cancellation::default(),
        )
        .unwrap();
    let (engine, _) = engine(state.clone());
    let mut request = request(ExistingDependencyPolicy::UpdateSameIdentity);
    request.source_revision = Some(observed.revision());
    let result = engine.prepare(other.path().to_path_buf(), request).await;
    assert!(result.is_err());
    assert!(format!("{:#}", result.err().unwrap()).contains("changed project documents"));
    assert!(!state.exists());
    engine.shutdown().await;
}

#[tokio::test]
async fn approved_addition_caches_staged_bytes_and_unavailable_cache_does_not_block_publication() {
    use crate::engine::content::{cache::ContentCache, store::ContentStoreLimits};
    use sha2::Digest;
    for unavailable in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        fixture(root.path());
        for name in ["a.zip", "b.zip", "copy.zip"] {
            fs::remove_file(root.path().join(format!("pack/resourcepacks/{name}"))).unwrap();
        }
        let cache = state.path().join("content");
        if unavailable {
            fs::write(&cache, b"unowned").unwrap();
        }
        let (engine, governor) = engine(state.path().join("state"));
        let engine = engine.with_content_cache(
            ContentCache::new(cache.clone(), ContentStoreLimits::default()).unwrap(),
        );
        let preview = ready(
            &engine,
            root.path(),
            request(ExistingDependencyPolicy::UpdateSameIdentity),
        )
        .await;
        drop(preview);
        assert_eq!(
            cache.exists(),
            unavailable,
            "preparation cannot create disposable state"
        );
        let prepared = ready(
            &engine,
            root.path(),
            request(ExistingDependencyPolicy::UpdateSameIdentity),
        )
        .await;
        let permission = grant(&prepared);
        let mut handle = engine
            .start(prepared.authorize(permission).unwrap())
            .unwrap();
        let result = handle.wait().await;
        match &*result {
            OperationOutcome::Completed(ExecutionOutcome::Completed(ExecutionReceipt::Add(_))) => {}
            OperationOutcome::Completed(ExecutionOutcome::FailedBeforePublication(error)) => {
                panic!("{error:#}")
            }
            _ => panic!("addition did not complete"),
        }
        for name in ["a.zip", "b.zip", "copy.zip"] {
            assert_eq!(
                fs::read(root.path().join(format!("pack/resourcepacks/{name}"))).unwrap(),
                b"payload"
            );
        }
        if unavailable {
            assert_eq!(fs::read(&cache).unwrap(), b"unowned");
        } else {
            let id = empack_core::digest::ExpectedDigest::Sha256(
                sha2::Sha256::digest(b"payload").into(),
            );
            assert_eq!(
                fs::read(cache.join(format!("{}.blob", id.hex()))).unwrap(),
                b"payload"
            );
            assert_eq!(
                fs::read_dir(&cache)
                    .unwrap()
                    .filter(|entry| entry
                        .as_ref()
                        .unwrap()
                        .path()
                        .extension()
                        .is_some_and(|ext| ext == "blob"))
                    .count(),
                1
            );
        }
        engine.release_completed(handle.id());
        drop((result, handle));
        engine.shutdown().await;
        assert_eq!(governor.status().reserved, ResourceRequest::default());
    }
}

mod independent_batch {
    use super::*;
    use crate::engine::{documents::DocumentCodec, mrpack::tests::explicitly_placed};
    use empack_core::{model::*, path::InstallDestination};
    fn named(name: &str) -> ResolvedProject {
        let source = project(false, false);
        let mut intent = source.intent().clone();
        let mut lock = source.lock().clone();
        let old = DependencyKey::parse("assets").unwrap();
        let key = DependencyKey::parse(name).unwrap();
        let root = intent.roots.remove(&old).unwrap();
        intent.roots.insert(key.clone(), root);
        let mut dependency = lock.dependencies.remove(&old).unwrap();
        dependency.identity = ResolvedIdentity::Url(key.clone());
        dependency.files = NonEmpty::new(
            dependency
                .files
                .into_vec()
                .into_iter()
                .map(|mut file| {
                    file.placements = NonEmpty::new(
                        file.placements
                            .into_vec()
                            .into_iter()
                            .map(|mut place| {
                                place.destination = InstallDestination::parse(&format!(
                                    "resourcepacks/{name}-{}",
                                    place
                                        .destination
                                        .relative()
                                        .as_str()
                                        .rsplit('/')
                                        .next()
                                        .unwrap()
                                ))
                                .unwrap();
                                place
                            })
                            .collect(),
                    )
                    .unwrap();
                    file
                })
                .collect(),
        )
        .unwrap();
        lock.dependencies.insert(key.clone(), dependency);
        lock.coverage = [(key, Coverage::CompleteForSelection)].into();
        explicitly_placed(intent, lock)
    }
    fn item(project: &ResolvedProject) -> DependencyBatchItem {
        DependencyBatchItem {
            group: AdditionGroup::from_resolved(project).unwrap(),
            content: references(project),
        }
    }
    fn broken() -> DependencyBatchItem {
        let mut request = request(ExistingDependencyPolicy::UpdateSameIdentity);
        let first = request.content.values_mut().next().unwrap();
        *first = DependencyContent::Materialized(AcquiredBuildFile {
            content: verify_stream(
                &mut b"wrong bytes".as_slice(),
                &ExpectedContent {
                    digests: None,
                    size: None,
                    accepted_observation: None,
                },
                100,
                SourceEvidencePolicy::Compatibility,
                InitialObservation::Accepted,
                &crate::application::process_runtime::Cancellation::default(),
            )
            .unwrap(),
            permissions: FilePermissions {
                readonly: false,
                executable: false,
            },
        });
        DependencyBatchItem {
            group: request.group,
            content: request.content,
        }
    }
    fn snapshot(root: &Path) -> BTreeMap<std::path::PathBuf, Vec<u8>> {
        fn visit(root: &Path, dir: &Path, map: &mut BTreeMap<std::path::PathBuf, Vec<u8>>) {
            for entry in fs::read_dir(dir).unwrap() {
                let entry = entry.unwrap();
                if entry.file_type().unwrap().is_dir() {
                    visit(root, &entry.path(), map);
                } else {
                    map.insert(
                        entry.path().strip_prefix(root).unwrap().to_owned(),
                        fs::read(entry.path()).unwrap(),
                    );
                }
            }
        }
        let mut map = BTreeMap::new();
        visit(root, root, &mut map);
        map
    }
    #[tokio::test]
    async fn batch_default_preserves_all_and_partial_publishes_one_coherent_candidate() {
        for policy in [BatchPolicy::AllRequested, BatchPolicy::ContinueIndependent] {
            let root = tempfile::tempdir().unwrap();
            let state = tempfile::tempdir().unwrap();
            fixture(root.path());
            let before = snapshot(root.path());
            let (engine, governor) = engine(state.path().join("state"));
            let valid = named("new");
            let batch = || DependencyBatchRequest {
                source_revision: None,
                policy,
                change: DependencyBatchChange::Add(ExistingDependencyPolicy::UpdateSameIdentity),
                items: NonEmpty::new(vec![broken(), item(&valid)]).unwrap(),
            };
            let result = engine.prepare(root.path().to_path_buf(), batch()).await;
            assert_eq!(snapshot(root.path()), before);
            if policy == BatchPolicy::AllRequested {
                assert!(result.is_err());
            } else {
                let Preparation::Ready(prepared) = result.unwrap() else {
                    panic!("unexpected input")
                };
                let report = prepared.view().add().unwrap().batch.as_ref().unwrap();
                assert_eq!(report.successful, [vec![1]]);
                assert_eq!(report.blocked[0].requests, [0]);
                // Declining a fully prepared partial candidate remains read-only.
                drop(prepared);
                assert_eq!(snapshot(root.path()), before);
                let prepared = ready(&engine, root.path(), batch()).await;
                let permission = grant(&prepared);
                let mut handle = engine
                    .start(prepared.authorize(permission).unwrap())
                    .unwrap();
                let outcome = handle.wait().await;
                let OperationOutcome::Completed(ExecutionOutcome::PartiallyCompleted {
                    receipt: ExecutionReceipt::Add(receipt),
                    cause,
                }) = &*outcome
                else {
                    panic!("expected partial addition")
                };
                assert!(cause.downcast_ref::<DependencyBatchIncomplete>().is_some());
                assert_eq!(receipt.batch.as_ref().unwrap().successful, [vec![1]]);
                assert_eq!(receipt.project.lock().dependencies.len(), 2);
                let original = project(false, false);
                assert_eq!(
                    receipt.project.lock().dependencies[&DependencyKey::parse("assets").unwrap()],
                    original.lock().dependencies[&DependencyKey::parse("assets").unwrap()]
                );
                for (path, bytes) in &before {
                    if path != Path::new("empack.yml") && path != Path::new("empack.lock") {
                        assert_eq!(&fs::read(root.path().join(path)).unwrap(), bytes);
                    }
                }
                assert_eq!(
                    DocumentCodec
                        .decode_intent(
                            &fs::read(root.path().join("empack.yml")).unwrap(),
                            "published"
                        )
                        .unwrap()
                        .intent()
                        .roots
                        .len(),
                    2
                );
                for _ in 0..2 {
                    let sync = engine
                        .preview(
                            root.path().to_path_buf(),
                            SyncRequest::Recorded {
                                resolution: None,
                                evidence: SourceEvidencePolicy::Compatibility,
                            },
                        )
                        .await
                        .unwrap();
                    assert!(sync.sync().unwrap().files.changes().is_empty());
                }
                engine.release_completed(handle.id());
                drop((outcome, handle));
            }
            engine.shutdown().await;
            assert_eq!(governor.status().reserved, ResourceRequest::default());
        }
    }
    #[tokio::test]
    async fn failed_shared_component_blocks_all_its_requests_but_not_an_unrelated_group() {
        let root = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        fixture(root.path());
        let (engine, governor) = engine(state.path().join("state"));
        let original = project(false, false);
        let other = named("other");
        let partial = DependencyBatchRequest {
            source_revision: None,
            policy: BatchPolicy::ContinueIndependent,
            change: DependencyBatchChange::Add(ExistingDependencyPolicy::UpdateSameIdentity),
            items: NonEmpty::new(vec![broken(), item(&original), item(&other)]).unwrap(),
        };
        let prepared = ready(&engine, root.path(), partial).await;
        let report = prepared.view().add().unwrap().batch.as_ref().unwrap();
        assert_eq!(report.blocked[0].requests, [0, 1]);
        assert_eq!(report.successful, [vec![2]]);
        drop(prepared);
        let all_failed = DependencyBatchRequest {
            source_revision: None,
            policy: BatchPolicy::ContinueIndependent,
            change: DependencyBatchChange::Add(ExistingDependencyPolicy::UpdateSameIdentity),
            items: NonEmpty::new(vec![broken(), item(&original)]).unwrap(),
        };
        let before = snapshot(root.path());
        let error = engine
            .prepare(root.path().to_path_buf(), all_failed)
            .await
            .err()
            .unwrap();
        assert!(error.downcast_ref::<DependencyBatchIncomplete>().is_some());
        assert_eq!(snapshot(root.path()), before);
        engine.shutdown().await;
        assert_eq!(governor.status().reserved, ResourceRequest::default());
    }
    #[tokio::test]
    async fn partial_update_keeps_failed_intent_and_stale_approval_never_publishes() {
        for stale in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let state = tempfile::tempdir().unwrap();
            fixture(root.path());
            let (engine, governor) = engine(state.path().join("state"));
            let extra = named("extra");
            let prepared = ready(
                &engine,
                root.path(),
                AddRequest {
                    source_revision: None,
                    group: group_for(&extra),
                    content: references(&extra),
                    existing: ExistingDependencyPolicy::RejectExisting,
                },
            )
            .await;
            let permission = grant(&prepared);
            let mut handle = engine
                .start(prepared.authorize(permission).unwrap())
                .unwrap();
            let outcome = handle.wait().await;
            assert!(matches!(
                &*outcome,
                OperationOutcome::Completed(ExecutionOutcome::Completed(_))
            ));
            engine.release_completed(handle.id());
            drop((outcome, handle));
            let before = snapshot(root.path());
            let original = fs::read(root.path().join("empack.yml")).unwrap();
            let batch = DependencyBatchRequest {
                source_revision: None,
                policy: BatchPolicy::ContinueIndependent,
                change: DependencyBatchChange::Update,
                items: NonEmpty::new(vec![broken(), item(&extra)]).unwrap(),
            };
            let prepared = ready(&engine, root.path(), batch).await;
            assert_eq!(
                prepared
                    .view()
                    .update()
                    .unwrap()
                    .batch
                    .as_ref()
                    .unwrap()
                    .successful,
                [vec![1]]
            );
            if stale {
                fs::write(
                    root.path().join("empack.yml"),
                    [b"# later edit\n".as_slice(), &original].concat(),
                )
                .unwrap();
            }
            let expected = snapshot(root.path());
            let permission = grant(&prepared);
            let mut handle = engine
                .start(prepared.authorize(permission).unwrap())
                .unwrap();
            let outcome = handle.wait().await;
            if stale {
                assert!(!matches!(
                    &*outcome,
                    OperationOutcome::Completed(
                        ExecutionOutcome::Completed(_)
                            | ExecutionOutcome::PartiallyCompleted { .. }
                    )
                ));
                assert_eq!(snapshot(root.path()), expected);
            } else {
                let OperationOutcome::Completed(ExecutionOutcome::PartiallyCompleted {
                    receipt: ExecutionReceipt::Update(receipt),
                    ..
                }) = &*outcome
                else {
                    panic!("missing update partial receipt")
                };
                assert_eq!(receipt.batch.as_ref().unwrap().blocked[0].requests, [0]);
                assert_eq!(fs::read(root.path().join("empack.yml")).unwrap(), original);
                assert_eq!(snapshot(root.path()), before);
            }
            engine.release_completed(handle.id());
            drop((outcome, handle));
            engine.shutdown().await;
            assert_eq!(governor.status().reserved, ResourceRequest::default());
        }
    }
    fn group_for(project: &ResolvedProject) -> AdditionGroup {
        AdditionGroup::from_resolved(project).unwrap()
    }
}
