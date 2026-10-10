use super::*;
use crate::engine::release::trust::{ChannelDocument, ChannelRelease, EnvelopeKind, sign};
use ed25519_dalek::SigningKey;
use std::{fs, path::Path};
fn enroll(key: &SigningKey) -> SubscriptionRequest {
    SubscriptionRequest::Enroll {
        pack: "fixture".into(),
        channel: "stable".into(),
        url: "https://publisher.test/stable.json".into(),
        keys: vec![key.verifying_key()],
    }
}
fn observe(key: &SigningKey, sequence: u64, release: &str) -> SubscriptionRequest {
    let document = ChannelDocument {
        schema: 1,
        pack: "fixture".into(),
        channel: "stable".into(),
        sequence,
        expires: 2_000_000_060,
        minimum_engine: ">=0.6.0-beta".into(),
        release: ChannelRelease {
            id: release.into(),
            url: "https://publisher.test/release.json".into(),
            maximum_bytes: 1024,
        },
    };
    SubscriptionRequest::Observe {
        envelope: sign(EnvelopeKind::Channel, &document.encode().unwrap(), &[key]).unwrap(),
        now: 2_000_000_000,
        engine: semver::Version::parse("0.6.0-beta").unwrap(),
    }
}
async fn apply(engine: &Engine, root: &Path, request: SubscriptionRequest) -> SubscriptionRecord {
    let Preparation::Ready(prepared) = engine.prepare(root.to_owned(), request).await.unwrap()
    else {
        panic!()
    };
    let grant = ExecutionGrant {
        plan: prepared.view().plan(),
        network: NetworkPermission::Offline,
        run_installer: false,
        replacement: prepared.view().replacement(),
    };
    let mut operation = engine.start(prepared.authorize(grant).unwrap()).unwrap();
    let result = operation.wait().await;
    let OperationOutcome::Completed(ExecutionOutcome::Completed(ExecutionReceipt::Subscription(
        receipt,
    ))) = &*result
    else {
        panic!("subscription did not publish")
    };
    let record = receipt.record.clone();
    drop(result);
    engine.release_completed(operation.id());
    record
}
#[tokio::test]
async fn enrollment_rotation_revocation_and_restart_retain_sequence_authority() {
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    let key = SigningKey::from_bytes(&[11; 32]);
    let next = SigningKey::from_bytes(&[12; 32]);
    let (engine, governor) = super::super::tests::engine(host.path().join("state"));
    let Preparation::Ready(preview) = engine
        .prepare(root.path().to_owned(), enroll(&key))
        .await
        .unwrap()
    else {
        panic!()
    };
    drop(preview);
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
    apply(&engine, root.path(), enroll(&key)).await;
    assert!(
        engine
            .prepare(root.path().to_owned(), enroll(&next))
            .await
            .is_err()
    );
    apply(&engine, root.path(), observe(&key, 7, &"a".repeat(64))).await;
    engine.shutdown().await;
    assert_eq!(governor.status().reserved, ResourceRequest::default());
    let (engine, governor) = super::super::tests::engine(host.path().join("state"));
    assert!(
        engine
            .prepare(root.path().to_owned(), observe(&key, 6, &"a".repeat(64)))
            .await
            .is_err()
    );
    assert!(
        engine
            .prepare(root.path().to_owned(), observe(&key, 7, &"b".repeat(64)))
            .await
            .is_err()
    );
    apply(&engine, root.path(), observe(&key, 7, &"a".repeat(64))).await;
    let rotated = apply(
        &engine,
        root.path(),
        SubscriptionRequest::ReplaceKeys {
            keys: vec![next.verifying_key()],
        },
    )
    .await;
    assert!(
        rotated
            .authenticated_channel(
                2_000_000_000,
                &semver::Version::parse("0.6.0-beta").unwrap()
            )
            .is_err(),
        "Retained metadata must be reverified after key replacement"
    );
    assert_eq!(rotated.floor.unwrap().sequence, 7);
    assert!(
        engine
            .prepare(root.path().to_owned(), observe(&key, 8, &"b".repeat(64)))
            .await
            .is_err()
    );
    apply(&engine, root.path(), observe(&next, 8, &"b".repeat(64))).await;
    let disabled = apply(
        &engine,
        root.path(),
        SubscriptionRequest::ReplaceKeys { keys: vec![] },
    )
    .await;
    assert!(
        disabled
            .authenticated_channel(
                2_000_000_000,
                &semver::Version::parse("0.6.0-beta").unwrap()
            )
            .is_err()
    );
    assert_eq!(disabled.floor.unwrap().sequence, 8);
    assert!(
        engine
            .prepare(root.path().to_owned(), observe(&next, 9, &"b".repeat(64)))
            .await
            .is_err()
    );
    assert!(!root.path().join("game").exists());
    assert!(!root.path().join(".empack/instance.json").exists());
    engine.shutdown().await;
    assert_eq!(governor.status().reserved, ResourceRequest::default());
}
#[tokio::test]
async fn subscription_rejects_copied_roots_and_late_trust_edits() {
    let root = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    let key = SigningKey::from_bytes(&[11; 32]);
    let (engine, _) = super::super::tests::engine(host.path().join("state"));
    apply(&engine, root.path(), enroll(&key)).await;
    let path = root.path().join(".empack/subscription.json");
    fs::create_dir_all(other.path().join(".empack")).unwrap();
    fs::copy(&path, other.path().join(".empack/subscription.json")).unwrap();
    assert!(
        engine
            .prepare(other.path().to_owned(), observe(&key, 1, &"a".repeat(64)))
            .await
            .is_err()
    );
    let Preparation::Ready(prepared) = engine
        .prepare(root.path().to_owned(), observe(&key, 1, &"a".repeat(64)))
        .await
        .unwrap()
    else {
        panic!()
    };
    let grant = ExecutionGrant {
        plan: prepared.view().plan(),
        network: NetworkPermission::Offline,
        run_installer: false,
        replacement: prepared.view().replacement(),
    };
    fs::write(&path, b"changed locally").unwrap();
    let mut operation = engine.start(prepared.authorize(grant).unwrap()).unwrap();
    assert!(matches!(
        &*operation.wait().await,
        OperationOutcome::Completed(ExecutionOutcome::FailedBeforePublication(_))
    ));
    assert_eq!(fs::read(&path).unwrap(), b"changed locally");
    engine.shutdown().await;
}
