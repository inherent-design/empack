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
    observe_bounded(key, sequence, release, 1024)
}
fn observe_bounded(
    key: &SigningKey,
    sequence: u64,
    release: &str,
    maximum_bytes: u64,
) -> SubscriptionRequest {
    let document = ChannelDocument {
        schema: 1,
        pack: "fixture".into(),
        channel: "stable".into(),
        sequence,
        expires: 2_000_000_060,
        minimum_engine: ">=0.8.0-beta".into(),
        release: ChannelRelease {
            id: release.into(),
            url: "https://publisher.test/release.json".into(),
            maximum_bytes,
        },
    };
    SubscriptionRequest::Observe {
        envelope: sign(EnvelopeKind::Channel, &document.encode().unwrap(), &[key]).unwrap(),
        now: 2_000_000_000,
        engine: semver::Version::parse("0.8.0-beta").unwrap(),
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
        run_runtime: false,
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
                &semver::Version::parse("0.8.0-beta").unwrap()
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
                &semver::Version::parse("0.8.0-beta").unwrap()
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
        run_runtime: false,
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

fn native_release(key: &SigningKey, version: &str) -> (release::DecodedRelease, Vec<u8>) {
    let document = release::DecodedRelease::encode(release::ReleaseDocument {
        require_subscription: false,
        server_launch: None,
        schema: 1,
        pack: "fixture".into(),
        version: version.into(),
        minimum_engine: ">=0.8.0-beta".into(),
        runtime: release::ReleaseRuntime {
            minecraft: "1.21.1".into(),
            loader: release::ReleaseLoader::Vanilla,
            java_major: 21,
        },
        choices: vec![],
        files: vec![],
    })
    .unwrap();
    let envelope = sign(EnvelopeKind::Release, document.bytes(), &[key]).unwrap();
    (document, envelope)
}
fn install(
    selected: impl Into<std::sync::Arc<crate::engine::instance::subscription::SubscribedRelease>>,
) -> InstallInstanceRequest {
    InstallInstanceRequest {
        require_subscription: false,
        conflicts: Vec::new(),
        action: crate::engine::instance::InstanceAction::Apply,
        release: crate::engine::instance::SelectedRelease::Subscribed(selected.into()),
        side: crate::engine::instance::InstanceSide::Client,
        layout: None,
        choices: vec![],
        supplied: BTreeMap::new(),
        local_files: BTreeMap::new(),
        assets: None,
    }
}
#[tokio::test]
async fn subscribed_selection_binds_current_keys_floor_and_exact_signed_release() {
    use crate::engine::{instance::subscription::select_release, publication::RecoveryReader};
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    let state = host.path().join("state");
    let key = SigningKey::from_bytes(&[11; 32]);
    let other = SigningKey::from_bytes(&[12; 32]);
    let (a, signed_a) = native_release(&key, "a");
    let (b, signed_b) = native_release(&key, "b");
    let version = semver::Version::parse("0.8.0-beta").unwrap();
    let select = |bytes: &[u8]| {
        select_release(
            root.path(),
            bytes,
            2_000_000_000,
            &version,
            RecoveryReader::new(state.clone()),
            &Default::default(),
        )
    };
    let (engine, governor) = super::super::tests::engine(state.clone());
    apply(&engine, root.path(), enroll(&key)).await;
    assert!(
        select(&signed_a).is_err(),
        "a signature alone cannot replace durable channel observation"
    );
    apply(&engine, root.path(), observe(&key, 1, a.id())).await;
    assert!(
        select(&signed_b).is_err(),
        "channel requires its exact selected payload"
    );
    let prior = select(&signed_a).unwrap();
    apply(
        &engine,
        root.path(),
        SubscriptionRequest::ReplaceKeys {
            keys: vec![other.verifying_key()],
        },
    )
    .await;
    assert!(
        engine
            .prepare(root.path().to_owned(), install(prior))
            .await
            .is_err()
    );
    assert!(
        select(&signed_a).is_err(),
        "revoked keys cannot reuse a retained observation"
    );
    apply(
        &engine,
        root.path(),
        SubscriptionRequest::ReplaceKeys {
            keys: vec![key.verifying_key()],
        },
    )
    .await;
    let Preparation::Ready(prepared) = engine
        .prepare(root.path().to_owned(), install(select(&signed_a).unwrap()))
        .await
        .unwrap()
    else {
        panic!()
    };
    let grant = ExecutionGrant {
        plan: prepared.view().plan(),
        network: NetworkPermission::Offline,
        run_installer: false,
        run_runtime: false,
        replacement: prepared.view().replacement(),
    };
    apply(&engine, root.path(), observe(&key, 2, b.id())).await;
    let mut stale = engine.start(prepared.authorize(grant).unwrap()).unwrap();
    assert!(matches!(
        &*stale.wait().await,
        OperationOutcome::Completed(ExecutionOutcome::FailedBeforePublication(_))
    ));
    engine.release_completed(stale.id());
    drop(stale);
    assert!(!root.path().join(".empack/instance.json").exists());
    let Preparation::Ready(prepared) = engine
        .prepare(root.path().to_owned(), install(select(&signed_b).unwrap()))
        .await
        .unwrap()
    else {
        panic!()
    };
    let grant = ExecutionGrant {
        plan: prepared.view().plan(),
        network: NetworkPermission::Offline,
        run_installer: false,
        run_runtime: false,
        replacement: prepared.view().replacement(),
    };
    let mut operation = engine.start(prepared.authorize(grant).unwrap()).unwrap();
    let outcome = operation.wait().await;
    let OperationOutcome::Completed(ExecutionOutcome::Completed(ExecutionReceipt::Instance(
        receipt,
    ))) = &*outcome
    else {
        panic!("signed installation did not complete")
    };
    assert_eq!(receipt.record.release, b.id());
    drop(outcome);
    engine.release_completed(operation.id());
    drop(operation);
    engine.shutdown().await;
    assert_eq!(governor.status().reserved, ResourceRequest::default());
}

#[tokio::test]
async fn remote_release_uses_durable_channel_and_acquires_exact_relative_assets_after_approval() {
    use crate::engine::{instance::subscription, release::*, runtime::OperationRuntime};
    // Bundling does not replace provenance with an Asset source. Hosted bytes
    // must work for each retained source, including an extracted archive member.
    let sources = [
        ReleaseSource::Asset {
            path: "assets/a%2fb".into(),
        },
        ReleaseSource::Url {
            alternatives: vec!["https://unavailable.test/a".into()],
        },
        ReleaseSource::Provider {
            provider: ReleaseProvider::Modrinth,
            project: "AANobbMI".into(),
            selection: "abcdefgh".into(),
            slot: "primary".into(),
            alternatives: vec![],
        },
        ReleaseSource::Manual {
            instructions: "Download from the author".into(),
            selection: None,
        },
        ReleaseSource::ProviderArchiveMember {
            archive: ReleaseArchiveSource {
                selection: ReleaseSelection {
                    provider: ReleaseProvider::Modrinth,
                    project: "AANobbMI".into(),
                    selection: "abcdefgh".into(),
                    slot: "primary".into(),
                },
                alternatives: vec![],
                assertions: vec![SourceDigest {
                    algorithm: "sha256".into(),
                    value: hash(b"archive"),
                }],
                bytes: Some(100),
                sha256: Some(hash(b"archive")),
            },
            member: "world/level.dat".into(),
        },
    ];
    for (source, bad_asset) in sources
        .into_iter()
        .flat_map(|s| [(s.clone(), false), (s, true)])
    {
        let mut server = mockito::Server::new_async().await;
        let root = tempfile::tempdir().unwrap();
        let host = tempfile::tempdir().unwrap();
        let state = host.path().join("state");
        let key = SigningKey::from_bytes(&[21; 32]);
        let (mut engine, _) = super::super::tests::engine(state.clone());
        let transport = HttpAcquisition::for_loopback_tests()
            .with_test_origin("https://publisher.test", &server.url());
        engine.transport = transport.clone();
        apply(&engine, root.path(), enroll(&key)).await;
        let mut document = native_release(&key, "remote").0.document().clone();
        let world = matches!(&source, ReleaseSource::ProviderArchiveMember { .. });
        let destination = if world {
            "custom/world/level.dat"
        } else {
            "mods/a.jar"
        };
        let installed = root.path().join("game").join(destination);
        document.files.push(ReleaseFile {
            world_root: world.then(|| "custom/world".into()),
            key: "mod".into(),
            destination: destination.into(),
            layer: ReleaseLayer::Common,
            policy: if matches!(&source, ReleaseSource::ProviderArchiveMember { .. }) {
                FilePolicy::Seed
            } else {
                FilePolicy::Managed
            },
            client: Participation::Required,
            server: Participation::Required,
            sha256: hash(b"exact bytes"),
            bytes: 11,
            readonly: false,
            executable: false,
            assertions: vec![SourceDigest {
                algorithm: "sha256".into(),
                value: hash(b"exact bytes"),
            }],
            asset: (!matches!(&source, ReleaseSource::Asset { .. })).then(|| "assets/a%2fb".into()),
            source,
        });
        let release = DecodedRelease::encode(document).unwrap();
        let signed = sign(EnvelopeKind::Release, release.bytes(), &[&key]).unwrap();
        let response = server
            .mock("GET", "/release.json")
            .with_body(&signed)
            .expect(1)
            .create_async()
            .await;
        let asset = server
            .mock("GET", "/assets/a%252fb")
            .with_body(if bad_asset {
                "wrong bytes"
            } else {
                "exact bytes"
            })
            .expect(1)
            .create_async()
            .await;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let channel = ChannelDocument {
            schema: 1,
            pack: "fixture".into(),
            channel: "stable".into(),
            sequence: 1,
            expires: now + 600,
            minimum_engine: ">=0.8.0-beta".into(),
            release: ChannelRelease {
                id: release.id().into(),
                url: "https://publisher.test/release.json".into(),
                maximum_bytes: signed.len() as u64,
            },
        };
        let channel_bytes =
            sign(EnvelopeKind::Channel, &channel.encode().unwrap(), &[&key]).unwrap();
        apply(
            &engine,
            root.path(),
            SubscriptionRequest::Observe {
                envelope: channel_bytes,
                now,
                engine: semver::Version::parse("0.8.0-beta").unwrap(),
            },
        )
        .await;
        let governor = ResourceGovernor::new(ResourceRequest {
            jobs: 2,
            memory_bytes: 512 << 20,
            open_files: 64,
            ..Default::default()
        });
        let runtime = OperationRuntime::new(governor.clone(), 1);
        let selected = root.path().to_owned();
        let mut handle = runtime
            .start(move |mut scope| async move {
                Ok(subscription::fetch_release(
                    &mut scope,
                    selected,
                    state,
                    &transport,
                    semver::Version::parse("0.8.0-beta").unwrap(),
                )
                .await
                .map(|proof| proof.map(std::sync::Arc::new)))
            })
            .unwrap();
        let output = handle.wait().await;
        // Keep the output's resource owner alive while passing its proof into preparation.
        let OperationOutcome::Completed(Ok(proof)) = &*output else {
            panic!("remote selection failed")
        };
        response.assert_async().await;
        assert!(!asset.matched_async().await);
        let proof = std::sync::Arc::clone(proof);
        let Preparation::Ready(prepared) = engine
            .prepare(root.path().to_owned(), install(proof))
            .await
            .unwrap()
        else {
            panic!()
        };
        assert!(prepared.view().needs_network());
        assert!(!asset.matched_async().await);
        let grant = ExecutionGrant {
            plan: prepared.view().plan(),
            network: NetworkPermission::Allow,
            run_installer: false,
            run_runtime: false,
            replacement: prepared.view().replacement(),
        };
        let mut operation = engine.start(prepared.authorize(grant).unwrap()).unwrap();
        let result = operation.wait().await;
        if bad_asset {
            assert!(matches!(
                &*result,
                OperationOutcome::Completed(ExecutionOutcome::FailedBeforePublication(_))
            ));
            assert!(!root.path().join(".empack/instance.json").exists());
            assert!(!root.path().join("game").exists());
        } else {
            assert!(matches!(
                &*result,
                OperationOutcome::Completed(ExecutionOutcome::Completed(
                    ExecutionReceipt::Instance(_)
                ))
            ));
            assert_eq!(fs::read(&installed).unwrap(), b"exact bytes");
            assert_eq!(
                fs::read(
                    root.path()
                        .join(format!(".empack/releases/{}.json", release.id()))
                )
                .unwrap(),
                release.bytes()
            );
        }
        let record = subscription::inspect(
            root.path(),
            RecoveryReader::new(host.path().join("state")),
            &crate::application::process_runtime::Cancellation::default(),
        )
        .unwrap();
        assert_eq!(record.floor.unwrap().sequence, 1);
        asset.assert_async().await;
        drop(result);
        engine.release_completed(operation.id());
        if !bad_asset {
            fs::remove_file(&installed).unwrap();
            asset.remove_async().await;
            let repair_asset = server
                .mock("GET", "/assets/a%252fb")
                .with_body("exact bytes")
                .expect(if world { 0 } else { 1 })
                .create_async()
                .await;
            // Repair uses the completed locator after trust revocation, without selecting an update.
            apply(
                &engine,
                root.path(),
                SubscriptionRequest::ReplaceKeys { keys: vec![] },
            )
            .await;
            let snapshot = crate::engine::release::trust::SelectedSnapshot::select(
                release.bytes(),
                release.id(),
                &semver::Version::parse("0.8.0-beta").unwrap(),
            )
            .unwrap();
            let input = InstallInstanceRequest {
                require_subscription: false,
                conflicts: Vec::new(),
                action: crate::engine::instance::InstanceAction::Repair,
                release: crate::engine::instance::SelectedRelease::Snapshot(snapshot),
                side: crate::engine::instance::InstanceSide::Client,
                layout: None,
                choices: vec![],
                supplied: BTreeMap::new(),
                local_files: BTreeMap::new(),
                assets: None,
            };
            let Preparation::Ready(prepared) =
                engine.prepare(root.path().to_owned(), input).await.unwrap()
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
            let mut repair = engine.start(prepared.authorize(grant).unwrap()).unwrap();
            let repaired = repair.wait().await;
            assert!(matches!(
                &*repaired,
                OperationOutcome::Completed(ExecutionOutcome::Completed(
                    ExecutionReceipt::Instance(_)
                ))
            ));
            if world {
                assert!(
                    !installed.exists(),
                    "Repair must not refill an initialized world"
                );
            } else {
                assert_eq!(fs::read(&installed).unwrap(), b"exact bytes");
            }

            repair_asset.assert_async().await;
            drop(repaired);
            engine.release_completed(repair.id());
        }
        engine.shutdown().await;
        drop(output);
        runtime.release_completed(handle.id());
        drop(handle);
        runtime.shutdown().await;
        assert_eq!(governor.status().reserved, ResourceRequest::default());
    }
}

#[tokio::test]
async fn saved_subscribed_input_reverifies_enrollment_before_resume() {
    use crate::engine::{
        instance::subscription::select_release, publication::RecoveryReader, release::*,
    };
    use sha2::{Digest, Sha256};
    let now: i64 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        .try_into()
        .unwrap();
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    let state = host.path().join("state");
    let key = SigningKey::from_bytes(&[43; 32]);
    let next = SigningKey::from_bytes(&[44; 32]);
    let (empty, _) = native_release(&key, "manual");
    let mut document = empty.document().clone();
    document.files.push(ReleaseFile {
        world_root: None,
        key: "mod".into(),
        destination: "mods/a.jar".into(),
        layer: ReleaseLayer::Common,
        policy: FilePolicy::Managed,
        client: Participation::Required,
        server: Participation::Required,
        sha256: Sha256::digest(b"A")
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
        bytes: 1,
        readonly: false,
        executable: false,
        assertions: vec![],
        asset: None,
        source: ReleaseSource::Manual {
            instructions: "Supply A".into(),
            selection: None,
        },
    });
    let release = DecodedRelease::encode(document).unwrap();
    let envelope = sign(EnvelopeKind::Release, release.bytes(), &[&key]).unwrap();
    let (engine, _) = super::super::tests::engine(state.clone());
    apply(&engine, root.path(), enroll(&key)).await;
    let channel = ChannelDocument {
        schema: 1,
        pack: "fixture".into(),
        channel: "stable".into(),
        sequence: 1,
        expires: now + 3600,
        minimum_engine: ">=0.8.0-beta".into(),
        release: ChannelRelease {
            id: release.id().into(),
            url: "https://publisher.test/release.json".into(),
            maximum_bytes: 64 << 10,
        },
    };
    apply(
        &engine,
        root.path(),
        SubscriptionRequest::Observe {
            envelope: sign(EnvelopeKind::Channel, &channel.encode().unwrap(), &[&key]).unwrap(),
            now,
            engine: semver::Version::parse("0.8.0-beta").unwrap(),
        },
    )
    .await;
    let selected = select_release(
        root.path(),
        &envelope,
        now,
        &semver::Version::parse("0.8.0-beta").unwrap(),
        RecoveryReader::new(state.clone()),
        &Default::default(),
    )
    .unwrap();
    let Preparation::NeedsInput(pending) = engine
        .prepare(root.path().to_owned(), install(selected))
        .await
        .unwrap()
    else {
        panic!("manual input")
    };
    engine.suspend_instance(*pending, None).await.unwrap();
    engine.shutdown().await;
    let (engine, _) = super::super::tests::engine(state.clone());
    let resumed = engine
        .resume_saved_instance(root.path().to_owned(), BTreeMap::new(), None)
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(resumed.preparation, Preparation::NeedsInput(_)));
    drop(resumed);
    apply(
        &engine,
        root.path(),
        SubscriptionRequest::ReplaceKeys {
            keys: vec![next.verifying_key()],
        },
    )
    .await;
    assert!(
        engine
            .resume_saved_instance(root.path().to_owned(), BTreeMap::new(), None)
            .await
            .is_err()
    );
    assert!(!root.path().join("game/mods/a.jar").exists());
    assert!(
        engine
            .observe_pending_instance(root.path().to_owned())
            .await
            .unwrap()
            .is_some()
    );
    engine.shutdown().await;
}

#[tokio::test]
async fn prism_channel_updates_require_seed_icons_from_native_producer() {
    use crate::engine::{
        content::{InitialObservation, SourceEvidencePolicy, verify_stream},
        instance::{InstanceLayout, subscription::select_release},
        mrpack::{
            SourceFile,
            tests::{explicitly_placed, project},
        },
        publication::RecoveryReader,
        release::{
            FilePolicy, ReleaseSource,
            producer::{NativeReleaseOptions, NativeReleasePlan},
        },
    };
    use empack_core::{
        files::FilePermissions,
        model::{ContentLayer, ExpectedContent},
        path::InstallDestination,
        requirements::{Requirement, Requirements},
    };
    // Use the same consumer-neutral producer as native exports, including its
    // default policy calculation. Do not hand-author the incoming release policy.
    let produce = |version: &str, seed: bool| {
        let initial = project(false, false);
        let mut intent = initial.intent().clone();
        let mut lock = initial.lock().clone();
        intent.roots.clear();
        lock.dependencies.clear();
        lock.coverage.clear();
        intent.metadata.version = version.into();
        intent.installation_policies.clear();
        if seed {
            intent.installation_policies.insert(
                InstallDestination::parse("icon.png")
                    .unwrap()
                    .relative()
                    .clone(),
                empack_core::instance::FilePolicy::Seed,
            );
        }
        let project = explicitly_placed(intent, lock);
        let bytes = b"publisher icon";
        let content = verify_stream(
            &mut &bytes[..],
            &ExpectedContent {
                digests: None,
                size: None,
                accepted_observation: None,
            },
            bytes.len() as u64,
            SourceEvidencePolicy::Compatibility,
            InitialObservation::Accepted,
            &Default::default(),
        )
        .unwrap();
        let source = SourceFile {
            label: "pack/icon.png".into(),
            destination: InstallDestination::parse("icon.png").unwrap(),
            layer: ContentLayer::Common,
            requirements: Requirements {
                client: Requirement::Required,
                server: Requirement::Required,
            },
            permissions: FilePermissions {
                readonly: false,
                executable: false,
            },
            content,
        };
        NativeReleasePlan::prepare(
            &project,
            &BTreeMap::new(),
            vec![source],
            NativeReleaseOptions::from_project(&project).unwrap(),
        )
        .unwrap()
    };
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    let state = host.path().join("state");
    let (engine, _) = super::super::tests::engine(state.clone());
    let key = SigningKey::from_bytes(&[11; 32]);
    apply(&engine, root.path(), enroll(&key)).await;
    for (sequence, seed) in [(1, true), (2, false), (3, true)] {
        let plan = produce(&sequence.to_string(), seed);
        let document = plan.release();
        assert_eq!(
            document.document().files[0].policy,
            if seed {
                FilePolicy::Seed
            } else {
                FilePolicy::Managed
            }
        );
        let original = document.bytes().to_vec();
        let envelope = sign(EnvelopeKind::Release, &original, &[&key]).unwrap();
        apply(
            &engine,
            root.path(),
            observe_bounded(&key, sequence, document.id(), envelope.len() as u64),
        )
        .await;
        let selected = select_release(
            root.path(),
            &envelope,
            2_000_000_000,
            &semver::Version::parse("0.8.0-beta").unwrap(),
            RecoveryReader::new(state.clone()),
            &Default::default(),
        )
        .unwrap();
        let mut input = install(selected);
        if sequence == 1 {
            input.layout = Some(InstanceLayout::Prism);
        }
        for file in &document.document().files {
            let ReleaseSource::Asset { path } = &file.source else {
                panic!()
            };
            let asset = plan
                .assets()
                .iter()
                .find(|(name, _)| name.as_str() == path)
                .unwrap()
                .1;
            input
                .supplied
                .insert(file.key.clone(), asset.content.clone());
        }
        let result = engine.prepare(root.path().to_owned(), input).await;
        if !seed {
            let error = result
                .err()
                .expect("Prism accepted a managed icon from a signed native release");
            let message = format!("{error:#}");
            assert!(
                message.contains("Prism") && message.contains("icon.png: seed"),
                "{message}"
            );
            assert!(
                !message.contains("SeedOwnership"),
                "must fail before offering conflict decisions"
            );
            assert_eq!(
                fs::read(root.path().join(".minecraft/icon.png")).unwrap(),
                b"launcher icon"
            );
        } else {
            let Preparation::Ready(prepared) = result.unwrap() else {
                panic!()
            };
            let grant = ExecutionGrant {
                plan: prepared.view().plan(),
                replacement: prepared.view().replacement(),
                network: NetworkPermission::Offline,
                run_installer: false,
                run_runtime: false,
            };
            let mut operation = engine.start(prepared.authorize(grant).unwrap()).unwrap();
            assert!(matches!(
                &*operation.wait().await,
                OperationOutcome::Completed(ExecutionOutcome::Completed(
                    ExecutionReceipt::Instance(_)
                ))
            ));
            engine.release_completed(operation.id());
            if sequence == 1 {
                fs::write(root.path().join(".minecraft/icon.png"), b"launcher icon").unwrap();
            }
            assert_eq!(
                fs::read(root.path().join(".minecraft/icon.png")).unwrap(),
                b"launcher icon"
            );
            // Launch preparation uses the persisted Prism layout and the same
            // compatibility check; a launcher rewrite must not demand repair.
            let launch = engine
                .prepare(
                    root.path().to_owned(),
                    LaunchInstanceRequest {
                        server: false,
                        expected_release: Some(document.id().into()),
                        program: std::env::current_exe().unwrap(),
                        arguments: vec![
                            "--exact".into(),
                            "engine::api::launch::tests::runtime_fixture".into(),
                            "--nocapture".into(),
                        ],
                    },
                )
                .await
                .unwrap();
            let Preparation::Ready(launch) = launch else {
                panic!()
            };
            let grant = ExecutionGrant {
                plan: launch.view().plan(),
                replacement: launch.view().replacement(),
                network: NetworkPermission::Offline,
                run_installer: false,
                run_runtime: true,
            };
            let mut running = engine.start(launch.authorize(grant).unwrap()).unwrap();
            assert!(matches!(&*running.wait().await,
                OperationOutcome::Completed(ExecutionOutcome::Completed(ExecutionReceipt::Launch(receipt))) if receipt.status.success()));
            engine.release_completed(running.id());
            assert_eq!(
                fs::read(root.path().join(".minecraft/icon.png")).unwrap(),
                b"launcher icon"
            );
        }
        assert_eq!(
            document.bytes(),
            original,
            "authenticated policy must never be rewritten"
        );
    }
    engine.shutdown().await;
}
