use super::*;
use crate::engine::{
    content::tests::{acquire, expected},
    resources::ResourceGovernor,
    runtime::{OperationOutcome, OperationRuntime},
};
use empack_core::digest::{DigestSet, IntegrityEvidence};
use std::fs;

#[tokio::test]
async fn source_hints_reverify_original_assertions_and_cleanup_preserves_retained_bytes() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("content");
    let store = FileContentStore::open(&path, ContentStoreLimits::default()).unwrap();
    fs::write(path.join("unowned.hint"), b"keep").unwrap();
    let content = acquire(b"payload", &expected()).unwrap();
    let governor = ResourceGovernor::new(ResourceRequest {
        jobs: 1,
        memory_bytes: 1 << 20,
        scratch_bytes: 64,
        open_files: 16,
    });
    let runtime = OperationRuntime::new(governor.clone(), 1);
    let mut handle = runtime.start(move |mut scope| async move {
        let lookup = store.lookup();
        assert!(lookup.retain_expected(&mut scope, expected(), 16, SourceEvidencePolicy::Compatibility, InitialObservation::RequireEvidence).await.unwrap().is_none());
        store.publish_expected(&mut scope, content.clone(), expected()).await.unwrap();
        let before = super::super::tests::footprint(&path);
        let retained = lookup.retain_expected(&mut scope, expected(), 16, SourceEvidencePolicy::Compatibility, InitialObservation::RequireEvidence).await.unwrap().unwrap();
        assert!(matches!(retained.evidence(), IntegrityEvidence::MatchedExpected { expected, .. } if expected.strongest() == empack_core::digest::DigestAlgorithm::Md5));
        assert_eq!(super::super::tests::footprint(&path), before);
        assert!(lookup.retain_expected(&mut scope, expected(), 16, SourceEvidencePolicy::StrongSourceRequired, InitialObservation::RequireEvidence).await.is_err());
        let mut wrong = expected();
        wrong.size = Some(8);
        assert!(lookup.retain_expected(&mut scope, wrong.clone(), 16, SourceEvidencePolicy::Compatibility, InitialObservation::RequireEvidence).await.is_err());
        assert!(store.publish_expected(&mut scope, content.clone(), wrong).await.is_err());
        assert_eq!(super::super::tests::footprint(&path), before);
        let complete = ExpectedContent { digests: Some(content.observed_digests().clone()), ..expected() };
        store.publish_expected(&mut scope, content.clone(), complete).await.unwrap();
        for digest in content.observed_digests().values() {
            let expectation = ExpectedContent { digests: Some(DigestSet::new(vec![digest.clone()]).unwrap()), ..expected() };
            assert!(lookup.retain_expected(&mut scope, expectation, 16, SourceEvidencePolicy::Compatibility, InitialObservation::RequireEvidence).await.unwrap().is_some());
        }
        let hint = hint_name(&expected().digests.unwrap().values()[0]);
        fs::write(path.join(&hint), [0; 32]).unwrap();
        assert!(lookup.retain_expected(&mut scope, expected(), 16, SourceEvidencePolicy::Compatibility, InitialObservation::RequireEvidence).await.unwrap().is_none());
        let impostor = verify_stream(&mut &b"forgery"[..], &ExpectedContent {
            digests: None, size: None, accepted_observation: None,
        }, 16, SourceEvidencePolicy::Compatibility, InitialObservation::Accepted, &Cancellation::default()).unwrap();
        store.publish_verified(&mut scope, impostor.clone()).await.unwrap();
        fs::write(path.join(&hint), impostor.lease().id().bytes()).unwrap();
        assert!(lookup.retain_expected(&mut scope, expected(), 16, SourceEvidencePolicy::Compatibility, InitialObservation::RequireEvidence).await.is_err());
        fs::write(path.join(&hint), b"short").unwrap();
        assert!(lookup.retain_expected(&mut scope, expected(), 16, SourceEvidencePolicy::Compatibility, InitialObservation::RequireEvidence).await.is_err());
        let plan = lookup.plan_cleanup(&mut scope).await.unwrap();
        assert_eq!(plan.objects().count(), 5);
        let receipt = store.evict(&mut scope, plan).await.unwrap();
        assert!(receipt.failure.is_none());
        assert_eq!(retained.lease().id(), content.lease().id());
        assert_eq!(fs::read(path.join("unowned.hint")).unwrap(), b"keep");
        Ok(())
    }).unwrap();
    let outcome = handle.wait().await;
    assert!(matches!(&*outcome, OperationOutcome::Completed(())));
    runtime.shutdown().await;
    assert_eq!(governor.status().reserved, ResourceRequest::default());
}

#[test]
fn only_canonical_bounded_hint_names_are_owned() {
    assert!(is_hint("md5-321c3cf486ed509164edec1e1981fec8.hint"));
    for name in [
        "../md5-321c3cf486ed509164edec1e1981fec8.hint",
        "sha1-ab.hint",
        "md5-321C3CF486ED509164EDEC1E1981FEC8.hint",
        "unknown.hint",
        "sha256-0000000000000000000000000000000000000000000000000000000000000000.hint",
    ] {
        assert!(!is_hint(name));
    }
}

#[test]
fn source_hint_publication_counts_entries_and_bytes_under_store_coordination() {
    for limits in [
        ContentStoreLimits {
            file_bytes: 16,
            total_bytes: 100,
            entries: 1,
        },
        ContentStoreLimits {
            file_bytes: 16,
            total_bytes: 38,
            entries: 10,
        },
    ] {
        let root = tempfile::tempdir().unwrap();
        let store = FileContentStore::open(&root.path().join("content"), limits).unwrap();
        let content = acquire(b"payload", &expected()).unwrap();
        store.0.publish(&content, &Cancellation::default()).unwrap();
        let name = hint_name(&expected().digests.unwrap().values()[0]);
        assert!(
            store
                .0
                .publish_hints(
                    std::slice::from_ref(&name),
                    &content.lease().id(),
                    &Cancellation::default()
                )
                .is_err()
        );
        assert!(!root.path().join("content").join(name).exists());
        assert_eq!(store.0.usage(&Cancellation::default()).unwrap(), (1, 7));
    }
}
