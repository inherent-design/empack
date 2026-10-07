use super::*;
use crate::engine::{
    content::tests::{acquire, expected},
    resources::ResourceGovernor,
    runtime::{OperationOutcome, OperationRuntime},
};
use empack_core::digest::{DigestSet, IntegrityEvidence};
use std::{collections::BTreeMap, fs, io::Read};

fn request(content: &AcquiredContent) -> CachedFileRequest {
    CachedFileRequest {
        id: content.lease().id(),
        expected: expected(),
        maximum: 1 << 30,
        evidence: SourceEvidencePolicy::Compatibility,
        initial: InitialObservation::RequireEvidence,
    }
}
fn footprint(path: &Path) -> BTreeMap<std::ffi::OsString, (Vec<u8>, std::time::SystemTime)> {
    fs::read_dir(path)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            (
                entry.file_name(),
                (
                    fs::read(entry.path()).unwrap(),
                    entry.metadata().unwrap().modified().unwrap(),
                ),
            )
        })
        .collect()
}
#[tokio::test]
async fn durable_store_reopens_read_only_and_retains_request_evidence_and_owned_bytes() {
    let host = tempfile::tempdir().unwrap();
    let path = host.path().join("content");
    let limits = ContentStoreLimits::default();
    assert!(
        FileContentLookup::open_existing(&path, limits)
            .unwrap()
            .is_none()
    );
    assert!(!path.exists());
    let store = FileContentStore::open(&path, limits).unwrap();
    let content = acquire(b"payload", &expected()).unwrap();
    let governor = ResourceGovernor::new(ResourceRequest {
        jobs: 1,
        memory_bytes: 1 << 20,
        scratch_bytes: 16,
        open_files: 12,
    });
    let runtime = OperationRuntime::new(governor.clone(), 2);
    let mut handle = runtime
        .start(move |mut scope| async move {
            let stored = store
                .publish_verified(&mut scope, content.clone())
                .await
                .unwrap();
            assert!(!stored.already_present);
            assert!(
                store
                    .publish_verified(&mut scope, content.clone())
                    .await
                    .unwrap()
                    .already_present
            );
            Ok(request(&content))
        })
        .unwrap();
    let result = handle.wait().await;
    let OperationOutcome::Completed(input) = &*result else {
        panic!("publication failed")
    };
    let input = input.clone();
    runtime.release_completed(handle.id());
    drop((result, handle));
    assert_eq!(governor.status().reserved, ResourceRequest::default());
    runtime.shutdown().await;
    let runtime = OperationRuntime::new(governor.clone(), 1);
    let before = footprint(&path);
    let lookup = FileContentLookup::open_existing(&path, limits)
        .unwrap()
        .unwrap();
    let mut handle = runtime
        .start(move |mut scope| async move {
            Ok(lookup.retain(&mut scope, input).await.unwrap().unwrap())
        })
        .unwrap();
    let result = handle.wait().await;
    let OperationOutcome::Completed(acquired) = &*result else {
        panic!("lookup failed")
    };
    assert!(
        matches!(acquired.evidence(), IntegrityEvidence::MatchedExpected {expected, ..} if expected.strongest() == empack_core::digest::DigestAlgorithm::Md5)
    );
    assert_eq!(footprint(&path), before);
    let mut reader = acquired.lease().open();
    // Private copied bytes remain readable after cache eviction, on every platform.
    fs::remove_file(path.join(name(&acquired.lease().id()))).unwrap();
    runtime.release_completed(handle.id());
    drop((result, handle));
    assert_eq!(governor.status().reserved.scratch_bytes, 7);
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, b"payload");
    drop(reader);
    assert_eq!(governor.status().reserved, ResourceRequest::default());
    runtime.shutdown().await;
}
#[test]
fn lookup_never_upgrades_evidence_and_rejects_corrupt_or_changed_assertions() {
    let host = tempfile::tempdir().unwrap();
    let store = FileContentStore::open(&host.path().join("content"), ContentStoreLimits::default())
        .unwrap();
    let content = acquire(b"payload", &expected()).unwrap();
    store.0.publish(&content, &Cancellation::default()).unwrap();
    for mode in ["strong", "digest", "size", "address", "corrupt", "limit"] {
        let mut input = request(&content);
        let name = name(&input.id);
        match mode {
            "strong" => input.evidence = SourceEvidencePolicy::StrongSourceRequired,
            "digest" => {
                input.expected.digests =
                    Some(DigestSet::parse([("md5", "00000000000000000000000000000000")]).unwrap())
            }
            "size" => input.expected.size = Some(5),
            "address" => {
                input.id = ContentId::from_sha256([0; 32]);
                fs::write(
                    host.path().join("content").join(self::name(&input.id)),
                    b"payload",
                )
                .unwrap();
            }
            "corrupt" => fs::write(host.path().join("content").join(&name), b"corrupt").unwrap(),
            "limit" => input.maximum = 2,
            _ => unreachable!(),
        }
        let result = store
            .0
            .select(&input.id, input.maximum)
            .and_then(|selected| {
                verify_selected(selected.unwrap(), &input, &Cancellation::default())
            });
        assert!(result.is_err(), "{mode}");
    }
    assert!(store.0.publish(&content, &Cancellation::default()).is_err());
}
#[test]
fn store_limits_refuse_growth_and_publication_preserves_unowned_files() {
    for limits in [
        ContentStoreLimits {
            file_bytes: 6,
            total_bytes: 100,
            entries: 10,
        },
        ContentStoreLimits {
            file_bytes: 100,
            total_bytes: 6,
            entries: 10,
        },
        ContentStoreLimits {
            file_bytes: 100,
            total_bytes: 100,
            entries: 0,
        },
    ] {
        let host = tempfile::tempdir().unwrap();
        let store = FileContentStore::open(&host.path().join("content"), limits).unwrap();
        let before = footprint(&host.path().join("content"));
        assert!(
            store
                .0
                .publish(
                    &acquire(b"payload", &expected()).unwrap(),
                    &Cancellation::default()
                )
                .is_err()
        );
        assert_eq!(footprint(&host.path().join("content")), before);
    }
    let host = tempfile::tempdir().unwrap();
    let path = host.path().join("content");
    let store = FileContentStore::open(&path, ContentStoreLimits::default()).unwrap();
    fs::write(path.join("unowned"), b"keep").unwrap();
    let content = acquire(b"payload", &expected()).unwrap();
    let cancel = Cancellation::default();
    cancel.cancel();
    assert!(store.0.publish(&content, &cancel).is_err());
    assert_eq!(fs::read(path.join("unowned")).unwrap(), b"keep");
    assert!(!path.join(name(&content.lease().id())).exists());
    store.0.publish(&content, &Cancellation::default()).unwrap();
    assert_eq!(fs::read(path.join("unowned")).unwrap(), b"keep");
}
#[test]
fn shared_lookup_coordination_prevents_concurrent_publication_without_pin_writes() {
    let host = tempfile::tempdir().unwrap();
    let path = host.path().join("content");
    let store = FileContentStore::open(&path, ContentStoreLimits::default()).unwrap();
    let other = FileContentStore::open(&path, ContentStoreLimits::default()).unwrap();
    let content = acquire(b"payload", &expected()).unwrap();
    store.0.publish(&content, &Cancellation::default()).unwrap();
    let selected = store.0.select(&content.lease().id(), 100).unwrap().unwrap();
    let before = footprint(&path);
    assert!(other.0.publish(&content, &Cancellation::default()).is_err());
    assert_eq!(footprint(&path), before);
    drop(selected);
    assert!(
        other
            .0
            .publish(&content, &Cancellation::default())
            .unwrap()
            .already_present
    );
}
#[cfg(unix)]
#[test]
fn cache_refuses_links_special_files_and_public_roots() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let host = tempfile::tempdir().unwrap();
    let path = host.path().join("content");
    let store = FileContentStore::open(&path, ContentStoreLimits::default()).unwrap();
    let content = acquire(b"payload", &expected()).unwrap();
    let object = path.join(name(&content.lease().id()));
    let outside = host.path().join("outside");
    fs::write(&outside, b"payload").unwrap();
    symlink(&outside, &object).unwrap();
    assert!(store.0.select(&content.lease().id(), 100).is_err());
    assert!(store.0.publish(&content, &Cancellation::default()).is_err());
    assert_eq!(fs::read(&outside).unwrap(), b"payload");
    fs::remove_file(&object).unwrap();
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&object)
            .status()
            .unwrap()
            .success()
    );
    assert!(store.0.select(&content.lease().id(), 100).is_err());
    assert!(store.0.publish(&content, &Cancellation::default()).is_err());
    let alias = host.path().join("alias");
    symlink(&path, &alias).unwrap();
    assert!(FileContentLookup::open_existing(&alias, ContentStoreLimits::default()).is_err());
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(FileContentLookup::open_existing(&path, ContentStoreLimits::default()).is_err());
    assert!(FileContentStore::open(&path, ContentStoreLimits::default()).is_err());
}

#[tokio::test]
async fn verified_cache_hit_needs_no_second_scratch_allocation() {
    let host = tempfile::tempdir().unwrap();
    let path = host.path().join("content");
    let store = FileContentStore::open(&path, ContentStoreLimits::default()).unwrap();
    let content = acquire(b"payload", &expected()).unwrap();
    store.0.publish(&content, &Cancellation::default()).unwrap();
    let before = footprint(&path);
    let governor = ResourceGovernor::new(ResourceRequest {
        jobs: 1,
        memory_bytes: 1 << 20,
        scratch_bytes: 7,
        open_files: 12,
    });
    let runtime = OperationRuntime::new(governor.clone(), 1);
    let mut handle = runtime
        .start(move |mut scope| async move {
            let retained = store
                .lookup()
                .retain(&mut scope, request(&content))
                .await
                .unwrap()
                .unwrap();
            Ok(store.publish_verified(&mut scope, retained).await)
        })
        .unwrap();
    let result = handle.wait().await;
    match &*result {
        OperationOutcome::Completed(Ok(receipt)) => assert!(receipt.already_present),
        OperationOutcome::Completed(Err(error)) => {
            panic!("cached hit requested unnecessary scratch: {error:#}")
        }
        _ => panic!("cache hit failed"),
    }
    runtime.release_completed(handle.id());
    drop((result, handle));
    assert_eq!(governor.status().reserved, ResourceRequest::default());
    assert_eq!(footprint(&path), before);
    runtime.shutdown().await;
}
