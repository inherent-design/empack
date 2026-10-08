use super::*;
use crate::engine::{
    content::{
        InitialObservation, SourceEvidencePolicy,
        tests::{acquire, expected},
    },
    resources::ResourceGovernor,
    runtime::{OperationOutcome, OperationRuntime},
};
use std::fs;
fn store(path: &Path) -> FileContentStore {
    FileContentStore::open(
        path,
        ContentStoreLimits {
            entries: 100,
            ..Default::default()
        },
    )
    .unwrap()
}
fn fill(store: &FileContentStore, bytes: &[u8]) -> AcquiredContent {
    let content = verify_stream(
        &mut &bytes[..],
        &ExpectedContent {
            digests: None,
            size: None,
            accepted_observation: None,
        },
        100,
        SourceEvidencePolicy::Compatibility,
        InitialObservation::Accepted,
        &Cancellation::default(),
    )
    .unwrap();
    store.0.publish(&content, &Cancellation::default()).unwrap();
    content
}
#[tokio::test]
async fn cache_cleanup_preserves_unknown_files_and_retained_readers_without_a_project() {
    let host = tempfile::tempdir().unwrap();
    let path = host.path().join("content");
    let store = store(&path);
    let content = acquire(b"payload", &expected()).unwrap();
    store.0.publish(&content, &Cancellation::default()).unwrap();
    fs::write(path.join("unowned"), b"keep").unwrap();
    let governor = ResourceGovernor::new(ResourceRequest {
        jobs: 1,
        memory_bytes: 2 << 20,
        scratch_bytes: 16,
        open_files: 12,
    });
    let runtime = OperationRuntime::new(governor.clone(), 1);
    let mut handle = runtime
        .start(move |mut scope| async move {
            let lease = store
                .lookup()
                .retain(&mut scope, super::super::tests::request(&content))
                .await
                .unwrap()
                .unwrap();
            let plan = store.lookup().plan_cleanup(&mut scope).await.unwrap();
            assert_eq!(plan.objects().count(), 1);
            assert_eq!(plan.bytes().unwrap(), 7);
            let before = super::super::tests::footprint(&path);
            assert_eq!(
                fs::read(path.join(name(&content.lease().id()))).unwrap(),
                b"payload"
            );
            let newer = fill(&store, b"newer");
            let receipt = store.evict(&mut scope, plan).await.unwrap();
            assert!(receipt.failure.is_none());
            assert_eq!(receipt.removed.len(), 1);
            assert!(receipt.retained.is_empty());
            assert_eq!(
                fs::read(path.join("unowned")).unwrap(),
                before[std::ffi::OsStr::new("unowned")].0
            );
            assert!(path.join(name(&newer.lease().id())).exists());
            let mut bytes = Vec::new();
            std::io::Read::read_to_end(&mut lease.lease().open(), &mut bytes).unwrap();
            assert_eq!(bytes, b"payload");
            Ok(receipt)
        })
        .unwrap();
    let result = handle.wait().await;
    assert!(matches!(&*result, OperationOutcome::Completed(_)));
    runtime.release_completed(handle.id());
    drop((result, handle));
    assert_eq!(governor.status().reserved, ResourceRequest::default());
    runtime.shutdown().await;
}
#[test]
fn cache_cleanup_refuses_stale_selections_and_other_stores_before_deleting_anything() {
    for mode in ["changed", "other"] {
        let host = tempfile::tempdir().unwrap();
        let path = host.path().join("content");
        let source = store(&path);
        let first = fill(&source, b"first");
        let second = fill(&source, b"second");
        let plan = source.0.plan_cleanup(&Cancellation::default()).unwrap();
        if mode == "changed" {
            fs::write(path.join(name(&second.lease().id())), b"changed size").unwrap();
            assert!(source.0.evict(plan, &Cancellation::default()).is_err());
        } else {
            let other = store(&host.path().join("other"));
            assert!(other.0.evict(plan, &Cancellation::default()).is_err());
        }
        assert!(path.join(name(&first.lease().id())).exists());
        assert!(path.join(name(&second.lease().id())).exists());
    }
}
#[test]
fn eviction_reports_completed_effects_when_later_work_fails_or_is_cancelled() {
    for cancel_after_first in [false, true] {
        let host = tempfile::tempdir().unwrap();
        let path = host.path().join("content");
        let store = store(&path);
        fill(&store, b"first");
        fill(&store, b"second");
        let cancel = Cancellation::default();
        let plan = store.0.plan_cleanup(&cancel).unwrap();
        let receipt = store
            .0
            .evict_with_hook(plan, &cancel, &mut |_| {
                if cancel_after_first {
                    cancel.cancel();
                    Ok(())
                } else {
                    anyhow::bail!("injected eviction failure")
                }
            })
            .unwrap();
        assert!(receipt.failure.is_some());
        assert_eq!(receipt.removed.len(), 1);
        assert_eq!(receipt.retained.len(), 1);
        assert!(!path.join(name(&receipt.removed[0].id)).exists());
        assert!(path.join(name(&receipt.retained[0].id)).exists());
    }
}
#[cfg(unix)]
#[test]
fn eviction_never_follows_owned_name_links_and_can_remove_corrupt_regular_objects() {
    let host = tempfile::tempdir().unwrap();
    let path = host.path().join("content");
    let store = store(&path);
    let content = fill(&store, b"payload");
    let object = path.join(name(&content.lease().id()));
    fs::write(&object, b"corrupt cache data").unwrap();
    let plan = store.0.plan_cleanup(&Cancellation::default()).unwrap();
    let outside = host.path().join("outside");
    fs::write(&outside, b"outside").unwrap();
    fs::remove_file(&object).unwrap();
    std::os::unix::fs::symlink(&outside, &object).unwrap();
    assert!(store.0.evict(plan, &Cancellation::default()).is_err());
    assert!(store.0.plan_cleanup(&Cancellation::default()).is_err());
    assert_eq!(fs::read(&outside).unwrap(), b"outside");
    fs::remove_file(&object).unwrap();
    fs::write(&object, b"corrupt cache data").unwrap();
    let plan = store.0.plan_cleanup(&Cancellation::default()).unwrap();
    let receipt = store.0.evict(plan, &Cancellation::default()).unwrap();
    assert!(receipt.failure.is_none());
    assert!(!object.exists());
}

#[test]
fn full_cache_preserves_unknown_neighbors_during_cleanup() {
    let host = tempfile::tempdir().unwrap();
    let path = host.path().join("content");
    let store = FileContentStore::open(
        &path,
        ContentStoreLimits {
            entries: 2,
            ..Default::default()
        },
    )
    .unwrap();
    fill(&store, b"one");
    fill(&store, b"two");
    fs::write(path.join("unowned"), b"preserve").unwrap();
    let plan = store.0.plan_cleanup(&Cancellation::default()).unwrap();
    assert_eq!(plan.objects().count(), 2);
    let result = store.0.evict(plan, &Cancellation::default()).unwrap();
    assert!(result.failure.is_none());
    assert_eq!(result.removed.len(), 2);
    assert_eq!(fs::read(path.join("unowned")).unwrap(), b"preserve");
}
#[tokio::test]
async fn empty_cache_inspection_fits_a_small_host_memory_allowance() {
    let host = tempfile::tempdir().unwrap();
    let store = FileContentStore::open(&host.path().join("content"), ContentStoreLimits::default())
        .unwrap();
    let governor = ResourceGovernor::new(ResourceRequest {
        jobs: 1,
        memory_bytes: 16 << 20,
        open_files: 8,
        ..Default::default()
    });
    let runtime = OperationRuntime::new(governor.clone(), 1);
    let mut handle = runtime
        .start(move |mut scope| async move {
            let plan = store.lookup().plan_cleanup(&mut scope).await.unwrap();
            assert_eq!(plan.objects().count(), 0);
            Ok(())
        })
        .unwrap();
    let result = handle.wait().await;
    assert!(matches!(&*result, OperationOutcome::Completed(())));
    assert_eq!(governor.status().reserved, ResourceRequest::default());
    runtime.shutdown().await;
}

#[test]
fn cleanup_capture_rejects_uncoordinated_membership_changes_after_counting() {
    for grow in [false, true] {
        let host = tempfile::tempdir().unwrap();
        let store = store(&host.path().join("content"));
        let original = fill(&store, b"original");
        let scan = store.0.scan_cleanup(&Cancellation::default()).unwrap();
        if grow {
            fs::write(
                host.path()
                    .join("content")
                    .join(format!("{}.blob", "ff".repeat(32))),
                b"new",
            )
            .unwrap();
        } else {
            fs::remove_file(
                host.path()
                    .join("content")
                    .join(name(&original.lease().id())),
            )
            .unwrap();
        }
        assert!(
            store
                .0
                .capture_cleanup(scan, &Cancellation::default())
                .is_err()
        );
    }
}
#[test]
fn unknown_neighbors_do_not_consume_object_insertion_capacity() {
    let host = tempfile::tempdir().unwrap();
    let path = host.path().join("content");
    let store = FileContentStore::open(
        &path,
        ContentStoreLimits {
            entries: 2,
            ..Default::default()
        },
    )
    .unwrap();
    fill(&store, b"one");
    for name in ["unknown-one", "unknown-two", "unknown-three"] {
        fs::write(path.join(name), b"keep").unwrap();
    }
    fill(&store, b"two");
    assert_eq!(
        store
            .0
            .plan_cleanup(&Cancellation::default())
            .unwrap()
            .objects()
            .count(),
        2
    );
}
