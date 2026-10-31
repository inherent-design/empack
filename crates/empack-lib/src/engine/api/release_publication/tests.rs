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
        minimum_engine: ">=0.8.0-beta".into(),
        runtime: ReleaseRuntime {
            minecraft: "1.21.1".into(),
            loader: ReleaseLoader::Vanilla,
            java_major: 21,
        },
        choices: vec![],
        files: vec![ReleaseFile {
            world_root: None,
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
                &semver::Version::parse("0.8.0-beta").unwrap()
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
    use super::super::tests::inventory as snapshot;
    use crate::engine::{
        acquisition::{DownloadRequest, TransferError},
        content::cache::ContentCache,
        diagnostics::{Diagnostic, DiagnosticCode},
        runtime::OperationRuntime,
    };
    use sha2::Digest;
    use std::sync::Arc;

    async fn publish(
        engine: &Engine,
        prepared: PreparedOperation,
    ) -> Arc<OperationOutcome<ExecutionOutcome>> {
        let grant = ExecutionGrant {
            plan: prepared.view().plan(),
            network: NetworkPermission::Allow,
            run_installer: false,
            run_runtime: false,
            replacement: prepared.view().replacement(),
        };
        let mut operation = engine.start(prepared.authorize(grant).unwrap()).unwrap();
        let result = operation.wait().await;
        assert!(engine.release_completed(operation.id()));
        result
    }

    let root = tempfile::tempdir().unwrap();
    let input = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let release = source(input.path(), vec![]);
    let mut document = release.document().clone();
    let mut shared = document.files[0].clone();
    shared.key = "shared-config".into();
    shared.destination = "config/another.toml".into();
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
    fs::write(
        root.path().join("publisher-notes"),
        b"unrelated author work",
    )
    .unwrap();
    let (mut engine, governor) = super::super::tests::engine(state.path().join("state"));
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

    // A verified warm cache is useful acquisition evidence, never proof of hosting.
    let cache = ContentCache::new(state.path().join("cache"), Default::default()).unwrap();
    engine = engine.with_content_cache(cache.clone());
    let warm = server
        .mock("GET", "/cache-prime")
        .with_body("settings")
        .expect(1)
        .create_async()
        .await;
    let no_read = server
        .mock("GET", "/must-not-fetch")
        .expect(0)
        .create_async()
        .await;
    let runtime = OperationRuntime::new(governor.clone(), 1);
    let transport = engine.transport.clone();
    let expected = release
        .document()
        .files
        .iter()
        .find(|file| file.key == "shared-config")
        .unwrap()
        .expected()
        .unwrap();
    let limits = engine.config.transfer;
    let mut operation = runtime
        .start(move |mut scope| async move {
            let result: anyhow::Result<()> = async {
                let request = |path: &str| -> anyhow::Result<DownloadRequest> {
                    Ok(DownloadRequest {
                        alternatives: NonEmpty::new(vec![format!(
                            "https://publisher.test/{path}"
                        )])?,
                        expected: expected.clone(),
                        limits,
                        evidence: SourceEvidencePolicy::Compatibility,
                        initial: InitialObservation::RequireEvidence,
                    })
                };
                drop(
                    transport
                        .clone()
                        .with_execution_cache(cache.clone())
                        .acquire(&mut scope, request("cache-prime")?)
                        .await?,
                );
                let content = transport
                    .with_cache_lookup(cache)
                    .acquire(&mut scope, request("must-not-fetch")?)
                    .await?;
                assert_eq!(content.lease().len(), 8);
                Ok(())
            }
            .await;
            Ok(result)
        })
        .unwrap();
    let result = operation.wait().await;
    match &*result {
        OperationOutcome::Completed(Ok(())) => (),
        OperationOutcome::Completed(Err(error)) => panic!("cache setup: {error:#}"),
        _ => panic!("cache setup did not retire successfully"),
    }
    assert!(runtime.release_completed(operation.id()));
    drop((result, operation));
    runtime.shutdown().await;
    warm.assert_async().await;
    no_read.assert_async().await;

    let envelope_path = format!("/releases/{}/release.json", release.id());
    let asset_path = format!("/releases/{}/assets/config", release.id());
    let staged_asset = root
        .path()
        .join(format!("dist/releases/{}/assets/config", release.id()));
    let encoded = fs::read(
        root.path()
            .join(format!("dist/releases/{}/release.json", release.id())),
    )
    .unwrap();
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
    let original_inputs = snapshot(input.path());
    let initial = snapshot(root.path());
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
    assert_eq!(snapshot(root.path()), initial);
    assert!(!root.path().join("dist/channels").exists());

    let envelope = server
        .mock("GET", envelope_path.as_str())
        .with_body(encoded.clone())
        .expect(1)
        .create_async()
        .await;
    let asset = server
        .mock("GET", asset_path.as_str())
        .with_body("settings")
        .expect(1)
        .create_async()
        .await;
    let Preparation::Ready(prepared) = engine
        .prepare(root.path().to_owned(), channel(1, now + 3600))
        .await
        .unwrap()
    else {
        panic!()
    };
    let result = publish(&engine, prepared).await;
    match &*result {
        OperationOutcome::Completed(ExecutionOutcome::Completed(
            ExecutionReceipt::ReleasePublication(receipt),
        )) => {
            assert_eq!(receipt.release, release.id());
            assert_eq!(receipt.channel.as_deref(), Some("stable"));
            assert_eq!(receipt.publication.changed_files, 1);
        }
        OperationOutcome::Completed(ExecutionOutcome::FailedBeforePublication(error)) => {
            panic!("{error:#}")
        }
        _ => panic!("channel publication did not complete"),
    }
    drop(result);
    envelope.assert_async().await;
    asset.assert_async().await;
    envelope.remove_async().await;
    asset.remove_async().await;
    let pointer_path = root.path().join("dist/channels/stable.json");
    let pointer = fs::read(&pointer_path).unwrap();
    let trust = trust::PublisherTrust::enroll(
        "published".into(),
        "https://publisher.test",
        vec![SigningKey::from_bytes(&[27; 32]).verifying_key()],
    )
    .unwrap();
    let authenticated = trust
        .channel(
            &pointer,
            "stable",
            now,
            &semver::Version::parse("0.8.0-beta").unwrap(),
            None,
        )
        .unwrap();
    assert_eq!(authenticated.document().sequence, 1);
    assert_eq!(authenticated.document().release.id, release.id());
    assert_eq!(
        authenticated.document().release.maximum_bytes,
        encoded.len() as u64
    );
    let before = snapshot(root.path());
    let host = snapshot(state.path());

    for (case, envelope_status, body, asset_status, asset_body) in [
        (
            "wrong-envelope",
            200,
            b"wrong envelope".to_vec(),
            200,
            "settings",
        ),
        ("missing-envelope", 404, vec![], 200, "settings"),
        ("encoded-envelope", 200, encoded.clone(), 200, "settings"),
        ("wrong-asset", 200, encoded.clone(), 200, "wrongcfg"),
        ("missing-asset", 200, encoded.clone(), 404, ""),
        ("oversized-asset", 200, encoded.clone(), 200, "settings!"),
    ] {
        let needs_asset = matches!(case, "wrong-asset" | "missing-asset" | "oversized-asset");
        let mut envelope = server
            .mock("GET", envelope_path.as_str())
            .with_status(envelope_status)
            .with_body(body)
            .expect(1);
        if case == "encoded-envelope" {
            envelope = envelope.with_header("content-encoding", "gzip");
        }
        let envelope = envelope.create_async().await;
        let asset = server
            .mock("GET", asset_path.as_str())
            .with_status(asset_status)
            .with_body(asset_body)
            .expect(usize::from(needs_asset))
            .create_async()
            .await;
        let Preparation::Ready(prepared) = engine
            .prepare(root.path().to_owned(), channel(2, now + 3600))
            .await
            .unwrap()
        else {
            panic!()
        };
        let result = publish(&engine, prepared).await;
        let OperationOutcome::Completed(ExecutionOutcome::FailedBeforePublication(error)) =
            &*result
        else {
            panic!("{case} unexpectedly published or lost its failure")
        };
        match case {
            "wrong-envelope" => assert!(
                error
                    .to_string()
                    .contains("Hosted release envelope differs")
            ),
            "missing-envelope" | "missing-asset" => assert!(
                matches!(
                    error.downcast_ref::<TransferError>(),
                    Some(TransferError::NotFound)
                ),
                "{case}: {error:#}"
            ),
            "encoded-envelope" => assert!(
                matches!(
                    error.downcast_ref::<TransferError>(),
                    Some(TransferError::InvalidLocator)
                ),
                "{error:#}"
            ),
            "wrong-asset" => assert_eq!(
                error.downcast_ref::<Diagnostic>().unwrap().code,
                DiagnosticCode::DigestMismatch
            ),
            "oversized-asset" => assert!(
                matches!(
                    error.downcast_ref::<TransferError>(),
                    Some(TransferError::ByteLimit)
                ),
                "{error:#}"
            ),
            _ => unreachable!(),
        }
        drop(result);
        envelope.assert_async().await;
        asset.assert_async().await;
        envelope.remove_async().await;
        asset.remove_async().await;
        assert_eq!(snapshot(root.path()), before, "{case}");
        assert_eq!(snapshot(state.path()), host, "{case}");
        assert_eq!(snapshot(input.path()), original_inputs);
        assert_eq!(fs::read(&pointer_path).unwrap(), pointer);
    }

    // Successful remote checks cannot authorize publishing a stale local candidate.
    let envelope = server
        .mock("GET", envelope_path.as_str())
        .with_body(encoded)
        .expect(1)
        .create_async()
        .await;
    let asset = server
        .mock("GET", asset_path.as_str())
        .with_body("settings")
        .expect(1)
        .create_async()
        .await;
    let Preparation::Ready(prepared) = engine
        .prepare(root.path().to_owned(), channel(2, now + 3600))
        .await
        .unwrap()
    else {
        panic!()
    };
    fs::write(&staged_asset, b"lateedit").unwrap();
    let edited = snapshot(root.path());
    let result = publish(&engine, prepared).await;
    let OperationOutcome::Completed(ExecutionOutcome::FailedBeforePublication(error)) = &*result
    else {
        panic!("late source change was not refused")
    };
    assert_eq!(
        error.downcast_ref::<Diagnostic>().unwrap().code,
        DiagnosticCode::StaleSnapshot
    );
    drop(result);
    assert_eq!(snapshot(root.path()), edited);
    assert_eq!(snapshot(state.path()), host);
    assert_eq!(snapshot(input.path()), original_inputs);
    assert_eq!(fs::read(&pointer_path).unwrap(), pointer);
    envelope.assert_async().await;
    asset.assert_async().await;
    fs::write(&staged_asset, b"settings").unwrap();

    for (sequence, expiry, diagnostic) in [
        (1, now + 7200, "Channel sequence must advance"),
        (0, now + 3600, "Invalid channel sequence or expiry"),
        (2, now - 1, "Channel has expired"),
    ] {
        let error = engine
            .prepare(root.path().to_owned(), channel(sequence, expiry))
            .await
            .err()
            .expect("invalid channel metadata was accepted");
        assert!(format!("{error:#}").contains(diagnostic));
        assert_eq!(snapshot(root.path()), before);
        assert_eq!(snapshot(state.path()), host);
    }
    engine.shutdown().await;
    assert_eq!(governor.status().reserved, ResourceRequest::default());
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
        minimum_engine: ">=0.8.0-beta".into(),
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
                &semver::Version::parse("0.8.0-beta").unwrap(),
                None
            )
            .is_err()
    );
    assert!(trust.previous_channel(&bytes, "other").is_err());
}
