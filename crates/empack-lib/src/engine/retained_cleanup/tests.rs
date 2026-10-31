use super::*;
use crate::engine::{
    content::{InitialObservation, SourceEvidencePolicy, verify_stream},
    resources::ResourceGovernor,
    runtime::{OperationOutcome, OperationRuntime},
};
use empack_core::model::ExpectedContent;
use std::{collections::BTreeMap, fs, io::Read};
fn runtime() -> (OperationRuntime<Result<()>>, ResourceGovernor) {
    let governor = ResourceGovernor::new(ResourceRequest {
        jobs: 2,
        memory_bytes: 8 << 20,
        scratch_bytes: 1 << 20,
        open_files: 32,
    });
    (OperationRuntime::new(governor.clone(), 1), governor)
}
fn snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(root: &Path, at: &Path, result: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(at).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                visit(root, &entry.path(), result);
            } else {
                result.insert(
                    entry.path().strip_prefix(root).unwrap().into(),
                    fs::read(entry.path()).unwrap(),
                );
            }
        }
    }
    let mut result = BTreeMap::new();
    visit(root, root, &mut result);
    result
}
#[tokio::test]
async fn retained_cleanup_preserves_pending_categories_live_leases_and_unowned_state() {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("state");
    let (runtime, governor) = runtime();
    let mut handle = runtime
        .start(move |mut scope| async move {
            let result = async {
                assert!(prepare(&mut scope, state.clone()).await?.is_none());
                assert!(!state.exists());
                let content = verify_stream(
                    &mut b"payload".as_slice(),
                    &ExpectedContent {
                        digests: None,
                        size: None,
                        accepted_observation: None,
                    },
                    7,
                    SourceEvidencePolicy::Compatibility,
                    InitialObservation::Accepted,
                    &Cancellation::default(),
                )?;
                let mut retained = None;
                for (_, category) in CATEGORIES {
                    let store = FileContentStore::open(
                        &state.join(category),
                        ContentStoreLimits::default(),
                    )?;
                    store.publish_verified(&mut scope, content.clone()).await?;
                    if category == "pending-content" {
                        retained = store
                            .lookup()
                            .retain(
                                &mut scope,
                                crate::engine::content::store::CachedFileRequest {
                                    id: content.lease().id(),
                                    expected: ExpectedContent {
                                        digests: None,
                                        size: Some(7),
                                        accepted_observation: Some(content.lease().id()),
                                    },
                                    maximum: 7,
                                    evidence: SourceEvidencePolicy::Compatibility,
                                    initial: InitialObservation::Accepted,
                                },
                            )
                            .await?;
                    }
                    fs::write(state.join(category).join("neighbor"), b"keep")?;
                }
                open_private_directory(&state.join("pending-imports"), true)?;
                fs::write(
                    state.join("pending-imports/invalid.json"),
                    b"invalid but still pins category",
                )?;
                fs::create_dir_all(state.join("operations/recovery"))?;
                fs::write(state.join("operations/recovery/preimage"), b"original")?;
                let before = snapshot(&state);
                let plan = prepare(&mut scope, state.clone()).await?.unwrap();
                assert_eq!(plan.preserved(), ["pending-import-content"]);
                assert_eq!(plan.selected()?.len(), CATEGORIES.len() - 1);
                assert_eq!(snapshot(&state), before);
                let result = execute(&mut scope, plan).await?;
                assert_eq!(result.len(), CATEGORIES.len() - 1);
                for (_, category) in CATEGORIES {
                    assert_eq!(fs::read(state.join(category).join("neighbor"))?, b"keep");
                    let blobs = fs::read_dir(state.join(category))?
                        .filter(|entry| {
                            entry
                                .as_ref()
                                .unwrap()
                                .path()
                                .extension()
                                .is_some_and(|ext| ext == "blob")
                        })
                        .count();
                    assert_eq!(blobs, usize::from(category == "pending-import-content"));
                }
                let mut bytes = Vec::new();
                retained.unwrap().lease().open().read_to_end(&mut bytes)?;
                assert_eq!(bytes, b"payload");
                assert_eq!(
                    fs::read(state.join("operations/recovery/preimage"))?,
                    b"original"
                );
                fs::remove_file(state.join("pending-imports/invalid.json"))?;
                let plan = prepare(&mut scope, state.clone()).await?.unwrap();
                assert_eq!(execute(&mut scope, plan).await?.len(), 1);
                Ok(())
            }
            .await;
            Ok(result)
        })
        .unwrap();
    let outcome = handle.wait().await;
    let OperationOutcome::Completed(result) = &*outcome else {
        panic!("cleanup worker failed");
    };
    assert!(result.is_ok(), "{result:?}");
    runtime.release_completed(handle.id());
    drop((outcome, handle));
    runtime.shutdown().await;
    assert_eq!(governor.status().reserved, ResourceRequest::default());
}

#[tokio::test]
async fn retained_cleanup_refuses_active_save_new_records_and_changed_objects() {
    for scenario in ["saving", "new-record", "changed-blob"] {
        let root = tempfile::tempdir().unwrap();
        let state = root.path().join("state");
        let (runtime, governor) = runtime();
        let mut handle = runtime
            .start(move |mut scope| async move {
                let result = async {
                    let content = state.join("pending-sync-content");
                    FileContentStore::open(&content, ContentStoreLimits::default())?;
                    let blob = content.join(format!("{}.blob", "ab".repeat(32)));
                    fs::write(&blob, b"unused payload")?;
                    let plan = prepare(&mut scope, state.clone()).await?.unwrap();
                    let saving = begin_save(&mut scope, state.clone()).await?;
                    if scenario == "new-record" {
                        open_private_directory(&state.join("pending-sync"), true)?;
                        fs::write(state.join("pending-sync/new.json"), b"record")?;
                    }
                    let saving = (scenario == "saving").then_some(saving);
                    if scenario == "changed-blob" {
                        fs::write(&blob, b"changed payload")?;
                    }
                    let before = snapshot(&state);
                    let error = execute(&mut scope, plan).await.unwrap_err();
                    let expected = match scenario {
                        "saving" => "Continuation storage is busy",
                        "new-record" => "A saved operation now retains pending-sync",
                        "changed-blob" => "Cache object changed after cleanup preparation",
                        _ => unreachable!(),
                    };
                    assert!(format!("{error:#}").contains(expected), "{error:#}");
                    assert_eq!(snapshot(&state), before);
                    drop(saving);
                    if scenario == "new-record" {
                        fs::remove_file(state.join("pending-sync/new.json"))?;
                    }
                    let plan = prepare(&mut scope, state.clone()).await?.unwrap();
                    execute(&mut scope, plan).await?;
                    assert!(!blob.exists());
                    Ok(())
                }
                .await;
                Ok(result)
            })
            .unwrap();
        let outcome = handle.wait().await;
        let OperationOutcome::Completed(result) = &*outcome else {
            panic!("cleanup worker failed");
        };
        assert!(result.is_ok(), "{scenario}: {result:?}");
        runtime.release_completed(handle.id());
        drop((outcome, handle));
        runtime.shutdown().await;
        assert_eq!(governor.status().reserved, ResourceRequest::default());
    }
}

#[tokio::test]
async fn retained_cleanup_reports_completed_scopes_and_reopens_surviving_content() {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("state");
    let (runtime, governor) = runtime();
    let mut handle = runtime.start(move |mut scope| async move {
        let result = async {
            let content = verify_stream(&mut b"payload".as_slice(), &ExpectedContent {
                digests: None, size: None, accepted_observation: None,
            }, 7, SourceEvidencePolicy::Compatibility, InitialObservation::Accepted,
                &Cancellation::default())?;
            let blob = "239f59ed55e737c77147cf55ad0c1b030b6d7ee748a7426952f9b852d5a935e5.blob";
            for (_, category) in CATEGORIES {
                let store = FileContentStore::open(&state.join(category), ContentStoreLimits::default())?;
                store.publish_verified(&mut scope, content.clone()).await?;
                assert_eq!(fs::read(state.join(category).join(blob))?, b"payload");
                fs::write(state.join(category).join("user-download.jar"), b"keep download")?;
            }
            fs::create_dir_all(state.join("operations/recovery"))?;
            fs::write(state.join("operations/recovery/preimage"), b"keep recovery")?;
            drop(begin_save(&mut scope, state.clone()).await?);
            let plan = prepare(&mut scope, state.clone()).await?.unwrap();
            assert_eq!(plan.selected()?, vec![
                ("pending-content", 1, 7), ("pending-import-content", 1, 7),
                ("pending-sync-content", 1, 7), ("pending-instance-content", 1, 7),
            ]);
            fs::write(state.join("pending-sync-content").join(blob), b"changed size")?;
            let mut expected = snapshot(&state);
            let error = execute(&mut scope, plan).await.unwrap_err();
            assert_eq!(format!("{error:#}"),
                "Retained cleanup failed; earlier completed scopes: [(\"pending-content\", 7), (\"pending-import-content\", 7)]: Cache object changed after cleanup preparation");
            for category in ["pending-content", "pending-import-content"] {
                expected.remove(&PathBuf::from(category).join(blob)).unwrap();
            }
            assert_eq!(snapshot(&state), expected);
            // A new capture can reclaim the remaining owned backing, including corrupt
            // disposable bytes, without deleting neighboring downloads or recovery data.
            let plan = prepare(&mut scope, state.clone()).await?.unwrap();
            assert_eq!(execute(&mut scope, plan).await?, vec![
                ("pending-sync-content", 12), ("pending-instance-content", 7),
            ]);
            for category in ["pending-sync-content", "pending-instance-content"] {
                expected.remove(&PathBuf::from(category).join(blob)).unwrap();
            }
            assert_eq!(snapshot(&state), expected);
            assert!(prepare(&mut scope, state.clone()).await?.unwrap().is_empty());
            for (_, category) in CATEGORIES {
                let store = FileContentStore::open(&state.join(category), ContentStoreLimits::default())?;
                store.publish_verified(&mut scope, content.clone()).await?;
                let retained = store.lookup().retain(&mut scope,
                    crate::engine::content::store::CachedFileRequest {
                        id: content.lease().id(), expected: ExpectedContent {
                            digests: None, size: Some(7), accepted_observation: Some(content.lease().id()),
                        }, maximum: 7, evidence: SourceEvidencePolicy::Compatibility,
                        initial: InitialObservation::Accepted,
                    }).await?.unwrap();
                let mut bytes = Vec::new();
                retained.lease().open().read_to_end(&mut bytes)?;
                assert_eq!(bytes, b"payload");
                expected.insert(PathBuf::from(category).join(blob), b"payload".to_vec());
            }
            assert_eq!(snapshot(&state), expected);
            Ok(())
        }.await;
        Ok(result)
    }).unwrap();
    let outcome = handle.wait().await;
    let OperationOutcome::Completed(result) = &*outcome else {
        panic!("cleanup worker failed")
    };
    assert!(result.is_ok(), "{result:?}");
    runtime.release_completed(handle.id());
    drop((outcome, handle));
    runtime.shutdown().await;
    assert_eq!(governor.status().reserved, ResourceRequest::default());
}
