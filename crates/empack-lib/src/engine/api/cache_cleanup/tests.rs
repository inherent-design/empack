use super::*;
use crate::engine::{api::tests, content::store::ContentStoreLimits};
use std::{fs, path::Path};

fn setup(path: &Path) -> (Engine, ResourceGovernor, PathBuf) {
    let cache = path.join("cache");
    let store = FileContentStore::open(
        &cache,
        ContentStoreLimits {
            entries: 10,
            ..Default::default()
        },
    )
    .unwrap();
    // Corrupt disposable bytes are still eligible for native-identity-bound eviction.
    fs::write(
        cache.join(format!("{}.blob", "ab".repeat(32))),
        b"old cached bytes",
    )
    .unwrap();
    fs::write(cache.join("neighbor"), b"not cache-owned").unwrap();
    let (engine, governor) = tests::engine(path.join("state"));
    (engine.with_content_store(store), governor, cache)
}
fn grant(prepared: &PreparedOperation) -> ExecutionGrant {
    ExecutionGrant {
        plan: prepared.view().plan(),
        replacement: prepared.view().replacement(),
        network: NetworkPermission::Offline,
        run_installer: false,
        run_runtime: false,
    }
}
#[tokio::test]
async fn maintenance_requires_explicit_store_and_no_project_or_recovery_state() {
    let host = tempfile::tempdir().unwrap();
    let (unconfigured, _) = tests::engine(host.path().join("unconfigured"));
    assert!(
        unconfigured
            .prepare_cache_cleanup(CacheCleanRequest::All)
            .await
            .is_err()
    );
    unconfigured.shutdown().await;
    let (engine, governor, cache) = setup(host.path());
    let before = fs::read_dir(&cache).unwrap().count();
    let preview = engine
        .preview_cache_cleanup(CacheCleanRequest::All)
        .await
        .unwrap();
    assert_eq!(preview.objects.len(), 1);
    assert_eq!(preview.bytes, 16);
    assert_eq!(fs::read_dir(&cache).unwrap().count(), before);
    assert!(!host.path().join("state").exists());
    assert_eq!(governor.status().reserved, ResourceRequest::default());
    let prepared = engine
        .prepare_cache_cleanup(CacheCleanRequest::All)
        .await
        .unwrap();
    let permission = grant(&prepared);
    // A newly inserted object is outside the approved footprint.
    let newer = cache.join(format!("{}.blob", "cd".repeat(32)));
    fs::write(&newer, b"new object").unwrap();
    let mut handle = engine
        .start(prepared.authorize(permission).unwrap())
        .unwrap();
    let outcome = handle.wait().await;
    match &*outcome {
        OperationOutcome::Completed(ExecutionOutcome::Completed(ExecutionReceipt::CacheClean(
            receipt,
        ))) => {
            assert_eq!(receipt.objects.removed.len(), 1);
            assert_eq!(receipt.objects.removed[0].bytes, 16);
            assert!(receipt.objects.retained.is_empty());
        }
        _ => panic!("expected cache cleanup completion"),
    }
    assert!(newer.exists());
    assert_eq!(
        fs::read(cache.join("neighbor")).unwrap(),
        b"not cache-owned"
    );
    assert!(!host.path().join("state").exists());
    engine.release_completed(handle.id());
    drop((outcome, handle));
    assert_eq!(governor.status().reserved, ResourceRequest::default());
    engine.shutdown().await;
}
#[tokio::test]
async fn cache_approval_binds_engine_plan_and_exact_selection() {
    for mode in ["engine", "plan", "footprint"] {
        let host = tempfile::tempdir().unwrap();
        let (engine, _, cache) = setup(host.path());
        let prepared = engine
            .prepare_cache_cleanup(CacheCleanRequest::All)
            .await
            .unwrap();
        let mut permission = grant(&prepared);
        match mode {
            "engine" => {
                let (other, _) = tests::engine(host.path().join("other"));
                assert!(
                    other
                        .start(prepared.authorize(permission).unwrap())
                        .is_err()
                );
                other.shutdown().await;
            }
            "plan" => {
                permission.plan = engine
                    .preview_cache_cleanup(CacheCleanRequest::All)
                    .await
                    .unwrap()
                    .plan;
                assert!(prepared.authorize(permission).is_err());
            }
            _ => {
                permission.replacement = None;
                assert!(prepared.authorize(permission).is_err());
            }
        }
        assert!(cache.join(format!("{}.blob", "ab".repeat(32))).exists());
        engine.shutdown().await;
    }
}
#[tokio::test]
async fn changed_cache_object_invalidates_execution_before_deletion() {
    let host = tempfile::tempdir().unwrap();
    let (engine, _, cache) = setup(host.path());
    let prepared = engine
        .prepare_cache_cleanup(CacheCleanRequest::All)
        .await
        .unwrap();
    let permission = grant(&prepared);
    let object = cache.join(format!("{}.blob", "ab".repeat(32)));
    fs::write(&object, b"different bytes").unwrap();
    let mut handle = engine
        .start(prepared.authorize(permission).unwrap())
        .unwrap();
    let outcome = handle.wait().await;
    assert!(matches!(
        &*outcome,
        OperationOutcome::Completed(ExecutionOutcome::FailedBeforePublication(_))
    ));
    assert_eq!(fs::read(object).unwrap(), b"different bytes");
    engine.shutdown().await;
}
