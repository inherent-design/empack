use super::*;
use crate::engine::release::*;
use std::{fs, path::Path};
fn source(root: &Path, assertions: Vec<SourceDigest>) -> DecodedRelease {
    fs::create_dir_all(root.join("assets")).unwrap();
    fs::write(root.join("assets/config"), b"settings").unwrap();
    let release = DecodedRelease::encode(ReleaseDocument {
        require_subscription: false,
        server_launch: None,
        schema: 1,
        pack: "published".into(),
        version: "1".into(),
        minimum_engine: ">=0.6.0-beta".into(),
        runtime: ReleaseRuntime {
            minecraft: "1.21.1".into(),
            loader: ReleaseLoader::Vanilla,
            java_major: 21,
        },
        choices: vec![],
        files: vec![ReleaseFile {
            key: "config".into(),
            destination: "config/example.toml".into(),
            layer: ReleaseLayer::Common,
            policy: FilePolicy::Seed,
            client: Participation::Required,
            server: Participation::Required,
            sha256: release::hash(b"settings"),
            bytes: 8,
            readonly: false,
            executable: false,
            assertions,
            asset: None,
            source: ReleaseSource::Asset {
                path: "assets/config".into(),
            },
        }],
    })
    .unwrap();
    fs::write(root.join("release.json"), release.bytes()).unwrap();
    release
}
fn request(source: &Path) -> StageReleaseRequest {
    StageReleaseRequest {
        source: source.to_owned(),
        keys: vec![SigningKey::from_bytes(&[27; 32])],
    }
}
async fn apply(engine: &Engine, prepared: PreparedOperation) -> bool {
    let grant = ExecutionGrant {
        plan: prepared.view().plan(),
        network: NetworkPermission::Offline,
        run_installer: false,
        run_runtime: false,
        replacement: prepared.view().replacement(),
    };
    let mut operation = engine.start(prepared.authorize(grant).unwrap()).unwrap();
    let result = operation.wait().await;
    let complete = matches!(
        &*result,
        OperationOutcome::Completed(ExecutionOutcome::Completed(
            ExecutionReceipt::ReleasePublication(_)
        ))
    );
    drop(result);
    engine.release_completed(operation.id());
    complete
}
#[tokio::test]
async fn immutable_release_staging_verifies_assets_signatures_and_republication() {
    let root = tempfile::tempdir().unwrap();
    let input = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let release = source(input.path(), vec![]);
    let (engine, governor) = super::super::tests::engine(state.path().join("state"));
    for _ in 0..2 {
        let Preparation::Ready(prepared) = engine
            .prepare(root.path().to_owned(), request(input.path()))
            .await
            .unwrap()
        else {
            panic!()
        };
        let OperationPreview::ReleasePublication(view) = prepared.view() else {
            panic!()
        };
        assert_eq!(view.release, release.id());
        assert_eq!(view.keys.len(), 1);
        if !root.path().join("dist").exists() {
            assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
        }
        assert!(apply(&engine, prepared).await);
    }
    let output = root.path().join(format!("dist/releases/{}", release.id()));
    assert_eq!(fs::read(output.join("assets/config")).unwrap(), b"settings");
    let encoded = fs::read(output.join("release.json")).unwrap();
    let trust = trust::PublisherTrust::enroll(
        "published".into(),
        "https://publisher.test",
        vec![SigningKey::from_bytes(&[27; 32]).verifying_key()],
    )
    .unwrap();
    assert_eq!(
        trust
            .release(
                &encoded,
                release.id(),
                &semver::Version::parse("0.6.0-beta").unwrap()
            )
            .unwrap()
            .release()
            .id(),
        release.id()
    );
    assert!(!root.path().join("dist/channels").exists());
    fs::write(output.join("assets/config"), b"tampered").unwrap();
    assert!(
        engine
            .prepare(root.path().to_owned(), request(input.path()))
            .await
            .is_err()
    );
    assert_eq!(fs::read(output.join("assets/config")).unwrap(), b"tampered");
    engine.shutdown().await;
    assert_eq!(governor.status().reserved, ResourceRequest::default());
}
#[tokio::test]
async fn publication_refuses_false_source_assertions_and_late_source_changes() {
    let root = tempfile::tempdir().unwrap();
    let input = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    source(
        input.path(),
        vec![SourceDigest {
            algorithm: "sha512".into(),
            value: "a".repeat(128),
        }],
    );
    let (engine, _) = super::super::tests::engine(state.path().join("state"));
    assert!(
        engine
            .prepare(root.path().to_owned(), request(input.path()))
            .await
            .is_err()
    );
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
    source(input.path(), vec![]);
    let Preparation::Ready(prepared) = engine
        .prepare(root.path().to_owned(), request(input.path()))
        .await
        .unwrap()
    else {
        panic!()
    };
    fs::write(input.path().join("assets/config"), b"modified").unwrap();
    assert!(!apply(&engine, prepared).await);
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
    engine.shutdown().await;
}
#[test]
fn signing_key_selection_rejects_project_sources_and_wrong_kinds() {
    let root = tempfile::tempdir().unwrap();
    let input = tempfile::tempdir().unwrap();
    let secrets = tempfile::tempdir().unwrap();
    let key = secrets.path().join("key");
    fs::write(&key, "1b".repeat(32)).unwrap();
    assert_eq!(
        release::signing::read_keys(root.path(), input.path(), &[key], &Default::default())
            .unwrap()
            .len(),
        1
    );
    let key = input.path().join("key");
    fs::write(&key, "1b".repeat(32)).unwrap();
    assert!(
        release::signing::read_keys(root.path(), input.path(), &[key], &Default::default())
            .is_err()
    );
    assert!(
        release::signing::read_keys(
            root.path(),
            input.path(),
            &[secrets.path().to_owned()],
            &Default::default()
        )
        .is_err()
    );
}

#[tokio::test]
async fn channel_publication_verifies_hosted_assets_before_replacing_pointer() {
    let root = tempfile::tempdir().unwrap();
    let input = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let release = source(input.path(), vec![]);
    let mut document = release.document().clone();
    let mut shared = document.files[0].clone();
    shared.key = "shared-config".into();
    shared.destination = "config/another.toml".into();
    use sha2::Digest;
    shared.assertions.push(SourceDigest {
        algorithm: "sha512".into(),
        value: sha2::Sha512::digest(b"settings")
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
    });
    document.files.push(shared);
    let release = DecodedRelease::encode(document).unwrap();
    fs::write(input.path().join("release.json"), release.bytes()).unwrap();
    let (mut engine, _) = super::super::tests::engine(state.path().join("state"));
    let Preparation::Ready(prepared) = engine
        .prepare(root.path().to_owned(), request(input.path()))
        .await
        .unwrap()
    else {
        panic!()
    };
    assert!(apply(&engine, prepared).await);
    let mut server = mockito::Server::new_async().await;
    engine.transport = HttpAcquisition::for_loopback_tests()
        .with_test_origin("https://publisher.test", &server.url());
    let encoded = fs::read(
        root.path()
            .join(format!("dist/releases/{}/release.json", release.id())),
    )
    .unwrap();
    let envelope = server
        .mock(
            "GET",
            format!("/releases/{}/release.json", release.id()).as_str(),
        )
        .with_body(encoded)
        .expect(2)
        .create_async()
        .await;
    let asset_path = format!("/releases/{}/assets/config", release.id());
    let good = server
        .mock("GET", asset_path.as_str())
        .with_body("settings")
        .expect(1)
        .create_async()
        .await;
    let now: i64 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        .try_into()
        .unwrap();
    let channel = |sequence, expires| PublishChannelRequest {
        release: release.id().into(),
        channel: "stable".into(),
        base_url: "https://publisher.test/".into(),
        sequence,
        expires,
        keys: vec![SigningKey::from_bytes(&[27; 32])],
        previous_keys: vec![],
    };
    let Preparation::Ready(prepared) = engine
        .prepare(root.path().to_owned(), channel(1, now + 3600))
        .await
        .unwrap()
    else {
        panic!()
    };
    let rejected = ExecutionGrant {
        plan: prepared.view().plan(),
        network: NetworkPermission::Offline,
        run_installer: false,
        run_runtime: false,
        replacement: prepared.view().replacement(),
    };
    assert!(prepared.authorize(rejected).is_err());
    for (sequence, success) in [(1, true), (2, false)] {
        let bad = if !success {
            Some(
                server
                    .mock("GET", asset_path.as_str())
                    .with_body("wrongcfg")
                    .expect(1)
                    .create_async()
                    .await,
            )
        } else {
            None
        };
        let Preparation::Ready(prepared) = engine
            .prepare(root.path().to_owned(), channel(sequence, now + 3600))
            .await
            .unwrap()
        else {
            panic!()
        };
        let grant = ExecutionGrant {
            plan: prepared.view().plan(),
            network: NetworkPermission::Allow,
            run_installer: false,
            run_runtime: false,
            replacement: prepared.view().replacement(),
        };
        let mut operation = engine.start(prepared.authorize(grant).unwrap()).unwrap();
        let result = operation.wait().await;
        assert_eq!(
            matches!(
                &*result,
                OperationOutcome::Completed(ExecutionOutcome::Completed(_))
            ),
            success
        );
        if let Some(mock) = bad {
            mock.assert_async().await;
        }
        drop(result);
        engine.release_completed(operation.id());
        let pointer = fs::read(root.path().join("dist/channels/stable.json")).unwrap();
        let trust = trust::PublisherTrust::enroll(
            "published".into(),
            "https://publisher.test",
            vec![SigningKey::from_bytes(&[27; 32]).verifying_key()],
        )
        .unwrap();
        assert_eq!(
            trust
                .channel(
                    &pointer,
                    "stable",
                    now,
                    &semver::Version::parse("0.6.0-beta").unwrap(),
                    None
                )
                .unwrap()
                .document()
                .sequence,
            1
        );
    }
    assert!(
        engine
            .prepare(root.path().to_owned(), channel(1, now + 7200))
            .await
            .is_err()
    );
    assert!(
        engine
            .prepare(root.path().to_owned(), channel(0, now + 3600))
            .await
            .is_err()
    );
    assert!(
        engine
            .prepare(root.path().to_owned(), channel(2, now - 1))
            .await
            .is_err()
    );
    envelope.assert_async().await;
    good.assert_async().await;
    engine.shutdown().await;
}

#[test]
fn expired_publisher_pointer_retains_ordering_without_update_authority() {
    use crate::engine::release::trust::*;
    let key = SigningKey::from_bytes(&[5; 32]);
    let trust = PublisherTrust::enroll(
        "published".into(),
        "https://publisher.test",
        vec![key.verifying_key()],
    )
    .unwrap();
    let channel = ChannelDocument {
        schema: 1,
        pack: "published".into(),
        channel: "stable".into(),
        sequence: 4,
        expires: 1_700_000_000,
        minimum_engine: ">=0.6.0-beta".into(),
        release: ChannelRelease {
            id: "a".repeat(64),
            url: "https://publisher.test/release.json".into(),
            maximum_bytes: 1024,
        },
    };
    let bytes = sign(EnvelopeKind::Channel, &channel.encode().unwrap(), &[&key]).unwrap();
    assert_eq!(
        trust.previous_channel(&bytes, "stable").unwrap().sequence,
        4
    );
    assert!(
        trust
            .channel(
                &bytes,
                "stable",
                1_800_000_000,
                &semver::Version::parse("0.6.0-beta").unwrap(),
                None
            )
            .is_err()
    );
    assert!(trust.previous_channel(&bytes, "other").is_err());
}
