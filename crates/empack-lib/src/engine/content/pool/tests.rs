use super::*;
use crate::engine::{
    resources::ResourceGovernor,
    runtime::{OperationOutcome, OperationRuntime},
};

fn acquire(bytes: &[u8]) -> AcquiredContent {
    verify_stream(
        &mut &bytes[..],
        &ExpectedContent {
            digests: None,
            size: None,
            accepted_observation: None,
        },
        128,
        SourceEvidencePolicy::Compatibility,
        InitialObservation::Accepted,
        &Cancellation::default(),
    )
    .unwrap()
}
fn check(content: &AcquiredContent, expected: &[u8]) {
    let mut bytes = Vec::new();
    content
        .lease()
        .copy_verified(&mut bytes, &Cancellation::default())
        .unwrap();
    assert_eq!(bytes, expected);
}
#[test]
fn members_have_independent_bounded_readers_and_survive_pool_retirement() {
    let mut pool = ContentPool::new(32).unwrap();
    let first = pool
        .insert(acquire(b"first"), &Cancellation::default())
        .unwrap();
    let second = pool
        .insert(acquire(b"second"), &Cancellation::default())
        .unwrap();
    let empty = pool.insert(acquire(b""), &Cancellation::default()).unwrap();
    let mut reader = first.lease().open();
    let mut another = first.lease().open();
    drop(first);
    drop(pool);
    assert_eq!(reader.seek(SeekFrom::Start(u64::MAX)).unwrap(), u64::MAX);
    assert_eq!(reader.read(&mut [0; 8]).unwrap(), 0);
    reader.seek(SeekFrom::End(-2)).unwrap();
    let mut tail = Vec::new();
    reader.read_to_end(&mut tail).unwrap();
    assert_eq!(tail, b"st");
    let mut all = Vec::new();
    another.read_to_end(&mut all).unwrap();
    assert_eq!(all, b"first");
    check(&second, b"second");
    check(&empty, b"");
}
#[test]
fn deduplication_preserves_each_sources_assurance() {
    let mut pool = ContentPool::new(7).unwrap();
    let observed = pool
        .insert(acquire(b"payload"), &Cancellation::default())
        .unwrap();
    let weak = super::super::tests::expected();
    let weak = pool
        .insert(
            super::super::tests::acquire(b"payload", &weak).unwrap(),
            &Cancellation::default(),
        )
        .unwrap();
    let strong = ExpectedContent {
        digests: Some(
            DigestSet::new(vec![ExpectedDigest::Sha256(*observed.lease().id().bytes())]).unwrap(),
        ),
        size: Some(7),
        accepted_observation: None,
    };
    let strong = pool
        .insert(
            super::super::tests::acquire(b"payload", &strong).unwrap(),
            &Cancellation::default(),
        )
        .unwrap();
    assert!(matches!(
        observed.evidence(),
        IntegrityEvidence::ObservedOnly { .. }
    ));
    assert!(
        matches!(weak.evidence(), IntegrityEvidence::MatchedExpected { expected, .. } if expected.strongest() == DigestAlgorithm::Md5)
    );
    assert!(
        matches!(strong.evidence(), IntegrityEvidence::MatchedExpected { expected, .. } if expected.strongest() == DigestAlgorithm::Sha256)
    );
    assert_eq!(
        pool.storage
            .state
            .lock()
            .unwrap()
            .file
            .file()
            .metadata()
            .unwrap()
            .len(),
        7
    );
    assert!(
        pool.insert(acquire(b"other"), &Cancellation::default())
            .is_err()
    );
    check(&observed, b"payload");
    check(&weak, b"payload");
    check(&strong, b"payload");
}
#[test]
fn backing_corruption_is_detected_before_publication() {
    let mut pool = ContentPool::new(32).unwrap();
    let content = pool
        .insert(acquire(b"original"), &Cancellation::default())
        .unwrap();
    let mut state = pool.storage.state.lock().unwrap();
    state.file.file().seek(SeekFrom::Start(0)).unwrap();
    state.file.file().write_all(b"modified").unwrap();
    drop(state);
    assert!(
        content
            .lease()
            .copy_verified(&mut Vec::new(), &Cancellation::default())
            .is_err()
    );
}
#[tokio::test]
async fn owned_ranges_keep_charges_until_the_last_reader_and_preserve_source_clones() {
    let governor = ResourceGovernor::new(ResourceRequest {
        jobs: 1,
        memory_bytes: 1 << 20,
        scratch_bytes: 1024,
        open_files: 8,
    });
    let runtime = OperationRuntime::new(governor.clone(), 1);
    let ledger = governor.clone();
    let mut operation = runtime
        .start(move |mut scope| async move {
            let mut pool = ContentPool::owned(&mut scope, 128).await.unwrap();
            let worker = scope.spawn_blocking(
                ResourceRequest {
                    jobs: 1,
                    scratch_bytes: 32,
                    open_files: 4,
                    ..Default::default()
                },
                ResourceRequest {
                    scratch_bytes: 7,
                    open_files: 1,
                    ..Default::default()
                },
                |_| acquire(b"payload"),
            )?;
            let source =
                AcquiredContent::retain_resources(scope.accept(worker.wait().await?)?).unwrap();
            let clone = source.clone();
            let content = pool.insert_owned(&mut scope, source).await.unwrap();
            assert_eq!(ledger.status().reserved.open_files, 3);
            assert_eq!(ledger.status().reserved.scratch_bytes, 14);
            drop(clone);
            assert_eq!(ledger.status().reserved.open_files, 2);
            let duplicate = pool
                .insert_owned(&mut scope, acquire(b"payload"))
                .await
                .unwrap();
            assert_eq!(ledger.status().reserved.scratch_bytes, 7);
            let reader = content.lease().open();
            drop(content);
            drop(duplicate);
            drop(pool);
            assert_eq!(ledger.status().reserved.scratch_bytes, 7);
            Ok(std::sync::Mutex::new(Some(reader)))
        })
        .unwrap();
    let outcome = operation.wait().await;
    let OperationOutcome::Completed(reader) = &*outcome else {
        panic!("operation failed")
    };
    let mut reader = reader.lock().unwrap().take().unwrap();
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, b"payload");
    assert_eq!(governor.status().reserved.open_files, 2);
    drop(reader);
    assert_eq!(governor.status().reserved, ResourceRequest::default());
    runtime.shutdown().await;
}
#[tokio::test]
async fn failed_append_remains_charged_while_existing_ranges_are_readable() {
    let governor = ResourceGovernor::new(ResourceRequest {
        jobs: 1,
        memory_bytes: 1 << 20,
        scratch_bytes: 1024,
        open_files: 8,
    });
    let runtime = OperationRuntime::new(governor.clone(), 1);
    let ledger = governor.clone();
    let mut operation = runtime
        .start(move |mut scope| async move {
            let mut pool = ContentPool::owned(&mut scope, 128).await.unwrap();
            let prior = pool
                .insert_owned(&mut scope, acquire(b"prior"))
                .await
                .unwrap();
            // Deliberately corrupt a source range after verification. The append copies bytes,
            // then rejects the digest; the existing destination range must remain readable.
            let mut source = ContentPool::new(32).unwrap();
            let bad = source
                .insert(acquire(b"wrong"), &Cancellation::default())
                .unwrap();
            {
                let mut state = source.storage.state.lock().unwrap();
                state.file.file().seek(SeekFrom::Start(0)).unwrap();
                state.file.file().write_all(b"other").unwrap();
            }
            assert!(pool.insert_owned(&mut scope, bad).await.is_err());
            assert_eq!(ledger.status().reserved.scratch_bytes, 10);
            assert!(
                pool.insert_owned(&mut scope, acquire(b"next"))
                    .await
                    .is_err()
            );
            assert_eq!(ledger.status().reserved.scratch_bytes, 10);
            check(&prior, b"prior");
            drop(pool);
            assert_eq!(ledger.status().reserved.scratch_bytes, 10);
            drop(prior);
            assert_eq!(ledger.status().reserved, ResourceRequest::default());
            scope.cancellation().cancel();
            assert!(matches!(
                scope.reserve_storage(ResourceRequest::default()),
                Err(crate::engine::runtime::RuntimeError::Cancelled)
            ));
            Ok(())
        })
        .unwrap();
    assert!(matches!(
        &*operation.wait().await,
        OperationOutcome::Completed(())
    ));
    runtime.shutdown().await;
    assert_eq!(governor.status().reserved, ResourceRequest::default());
}
