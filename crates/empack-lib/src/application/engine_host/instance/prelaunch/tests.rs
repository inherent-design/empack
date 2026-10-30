use super::*;
use crate::{
    application::{
        AppConfig,
        session_mocks::{
            MockCommandSession, MockConfigProvider, MockInteractiveProvider, MockInvocationProvider,
        },
    },
    engine::{
        acquisition::HttpAcquisition,
        release::{
            trust::{ChannelDocument, ChannelRelease, EnvelopeKind, sign},
            *,
        },
    },
};
use ed25519_dalek::SigningKey;
use std::fs;
fn session(root: &Path, yes: bool, dry: bool) -> MockCommandSession {
    MockCommandSession::new()
        .with_invocation(MockInvocationProvider::new().with_current_dir(root.into()))
        .with_config(MockConfigProvider::new(AppConfig {
            workdir: Some("project".into()),
            state_dir: Some("state".into()),
            cache_dir: Some("cache".into()),
            yes,
            dry_run: dry,
            ..Default::default()
        }))
        .with_interactive(MockInteractiveProvider::new().with_confirm(yes))
}
fn release(version: &str) -> DecodedRelease {
    DecodedRelease::encode(ReleaseDocument {
        require_subscription: false,
        server_launch: None,
        schema: 1,
        pack: "prelaunch".into(),
        version: version.into(),
        minimum_engine: ">=0.8.0-beta".into(),
        runtime: ReleaseRuntime {
            minecraft: "1.21.1".into(),
            loader: ReleaseLoader::Vanilla,
            java_major: 21,
        },
        files: vec![],
        choices: vec![],
    })
    .unwrap()
}
async fn fixture(root: &Path) -> SigningKey {
    fixture_side(root, "client", None).await
}
async fn fixture_side(root: &Path, side: &str, layout: Option<&str>) -> SigningKey {
    fs::create_dir(root.join("project")).unwrap();
    let release = release("A");
    fs::write(root.join("initial.json"), release.bytes()).unwrap();
    super::super::dispatch(
        &session(root, true, false),
        InstanceCommand::Install {
            conflicts: Default::default(),
            release: "initial.json".into(),
            sha256: release.id().into(),
            side: Some(side.into()),
            layout: layout.map(str::to_owned),
            choices: vec![],
            files: vec![],
        },
    )
    .await
    .unwrap();
    let key = SigningKey::from_bytes(&[43; 32]);
    super::super::dispatch(
        &session(root, true, false),
        InstanceCommand::Subscribe {
            pack: "prelaunch".into(),
            channel: "stable".into(),
            url: "https://publisher.test/channel.json".into(),
            keys: vec![
                key.verifying_key()
                    .as_bytes()
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect(),
            ],
        },
    )
    .await
    .unwrap();
    key
}
fn transport(origin: &str) -> HttpAcquisition {
    HttpAcquisition::for_loopback_tests().with_test_origin("https://publisher.test", origin)
}
fn channel_document(release: &DecodedRelease, maximum: u64) -> ChannelDocument {
    ChannelDocument {
        schema: 1,
        pack: "prelaunch".into(),
        channel: "stable".into(),
        sequence: 1,
        expires: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
            + 3600,
        minimum_engine: ">=0.8.0-beta".into(),
        release: ChannelRelease {
            id: release.id().into(),
            url: "https://publisher.test/release.json".into(),
            maximum_bytes: maximum,
        },
    }
}
fn channel(release: &DecodedRelease, key: &SigningKey, maximum: u64) -> Vec<u8> {
    sign(
        EnvelopeKind::Channel,
        &channel_document(release, maximum).encode().unwrap(),
        &[key],
    )
    .unwrap()
}

#[tokio::test]
async fn runtime_transition_retains_observation_but_requires_supported_launch_integration() {
    use crate::application::session_mocks::MockResponse;
    use sha2::Digest;
    fn assert_observation(prior: &[u8], saved: &[u8], pointer: &[u8]) {
        let record = crate::engine::api::SubscriptionRecord::decode(saved).unwrap();
        assert_eq!(record.floor.as_ref().unwrap().sequence, 1);
        assert_eq!(
            record.observed.as_deref(),
            Some(std::str::from_utf8(pointer).unwrap())
        );
        let mut previous: serde_json::Value = serde_json::from_slice(prior).unwrap();
        let mut observed: serde_json::Value = serde_json::from_slice(saved).unwrap();
        for value in [&mut previous, &mut observed] {
            value.as_object_mut().unwrap().remove("floor");
            value.as_object_mut().unwrap().remove("observed");
        }
        assert_eq!(
            observed, previous,
            "only the approved observation may advance trust"
        );
    }
    for (name, side, layout, server_mode, entrypoint, accepted, diagnostic) in [
        (
            "Prism client",
            "client",
            Some("prism"),
            false,
            true,
            false,
            "stop Prism, run instance update, then relaunch",
        ),
        (
            "Prism cannot claim server mode",
            "client",
            Some("prism"),
            true,
            true,
            false,
            "stop Prism, run instance update, then relaunch",
        ),
        (
            "game client",
            "client",
            None,
            false,
            false,
            false,
            "prepare its launcher/runtime integration",
        ),
        (
            "server without server mode",
            "server",
            None,
            false,
            true,
            false,
            "prepare its launcher/runtime integration",
        ),
        (
            "server without entrypoint",
            "server",
            None,
            true,
            false,
            false,
            "prepare its launcher/runtime integration",
        ),
        ("managed server", "server", None, true, true, true, ""),
    ] {
        for allow_offline in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let key = fixture_side(root.path(), side, layout).await;
            let project = root.path().join("project");
            let game = project.join(if layout == Some("prism") {
                ".minecraft"
            } else {
                "game"
            });
            fs::create_dir_all(game.join("saves/played")).unwrap();
            fs::write(game.join("saves/played/level.dat"), b"played world").unwrap();
            fs::write(project.join("launcher-note.txt"), b"unrelated settings").unwrap();
            let mut document = release("B").document().clone();
            document.runtime.minecraft = "1.21.2".into();
            document.runtime.loader = ReleaseLoader::Fabric {
                version: "0.16.0".into(),
            };
            if entrypoint {
                document.server_launch = Some(ReleaseServerLaunch::Jar {
                    path: "server.jar".into(),
                });
                document.files.push(ReleaseFile {
                    world_root: None,
                    key: "runtime".into(),
                    destination: "server.jar".into(),
                    layer: ReleaseLayer::Common,
                    policy: FilePolicy::Managed,
                    client: Participation::Unsupported,
                    server: Participation::Required,
                    sha256: sha2::Sha256::digest(b"incoming runtime")
                        .iter()
                        .map(|b| format!("{b:02x}"))
                        .collect(),
                    bytes: b"incoming runtime".len() as u64,
                    readonly: false,
                    executable: false,
                    assertions: vec![],
                    asset: None,
                    source: ReleaseSource::Asset {
                        path: "assets/runtime".into(),
                    },
                });
            }
            let offered = DecodedRelease::encode(document).unwrap();
            let envelope = sign(EnvelopeKind::Release, offered.bytes(), &[&key]).unwrap();
            let pointer = channel(&offered, &key, envelope.len() as u64);
            let mut remote = mockito::Server::new_async().await;
            let channel_route = remote
                .mock("GET", "/channel.json")
                .with_body(pointer.clone())
                .expect(if accepted { 2 } else { 1 })
                .create_async()
                .await;
            let release_route = remote
                .mock("GET", "/release.json")
                .with_body(envelope)
                .expect(if accepted { 2 } else { 1 })
                .create_async()
                .await;
            let mut asset_route = remote
                .mock("GET", "/assets/runtime")
                .with_body("incoming runtime")
                .expect(0)
                .create_async()
                .await;
            let mut before = crate::application::engine_host::tests::snapshot(&project);
            let prior = before
                .remove(Path::new(".empack/subscription.json"))
                .unwrap();
            if accepted {
                let interactive = MockInteractiveProvider::new();
                interactive
                    .response_queue
                    .lock()
                    .unwrap()
                    .extend([MockResponse::Confirm(true), MockResponse::Confirm(false)]);
                let prompts = interactive.confirm_calls.clone();
                let host = session(root.path(), false, false).with_interactive(interactive);
                assert!(
                    update_with_transport(
                        &host,
                        allow_offline,
                        server_mode,
                        transport(&remote.url())
                    )
                    .await
                    .unwrap()
                    .is_none()
                );
                assert_eq!(prompts.lock().unwrap().len(), 2);
                let mut declined = crate::application::engine_host::tests::snapshot(&project);
                let saved = declined
                    .remove(Path::new(".empack/subscription.json"))
                    .unwrap();
                assert_observation(&prior, &saved, &pointer);
                assert_eq!(
                    declined, before,
                    "declining installation must retain the completed project"
                );
                asset_route.assert_async().await;
                asset_route.remove_async().await;
                asset_route = remote
                    .mock("GET", "/assets/runtime")
                    .with_body("incoming runtime")
                    .expect(1)
                    .create_async()
                    .await;
            }
            let interactive = MockInteractiveProvider::new().with_confirm(true);
            let prompts = interactive.confirm_calls.clone();
            let host = session(root.path(), false, false).with_interactive(interactive);
            let result =
                update_with_transport(&host, allow_offline, server_mode, transport(&remote.url()))
                    .await;
            let mut after = crate::application::engine_host::tests::snapshot(&project);
            let saved = after
                .remove(Path::new(".empack/subscription.json"))
                .unwrap();
            assert_observation(&prior, &saved, &pointer);
            if accepted {
                assert_eq!(result.unwrap(), Some(offered.id().to_owned()));
                assert_eq!(prompts.lock().unwrap().len(), 2);
                assert_eq!(
                    fs::read(game.join("server.jar")).unwrap(),
                    b"incoming runtime"
                );
                let installed = inspect(&host, project.clone(), None, root.path().join("state"))
                    .await
                    .unwrap();
                assert_eq!(installed.0.release, offered.id());
                assert_eq!(installed.1.document().runtime, offered.document().runtime);
            } else {
                let error =
                    result.expect_err("unsupported runtime transition became launch permission");
                assert!(
                    format!("{error:#}").contains(diagnostic),
                    "{name}: {error:#}"
                );
                assert_eq!(
                    prompts.lock().unwrap().len(),
                    1,
                    "{name}: installation must not be approved"
                );
                assert_eq!(
                    after, before,
                    "{name}: rejected runtime change mutated the installation"
                );
                let installed = inspect(&host, project.clone(), None, root.path().join("state"))
                    .await
                    .unwrap();
                assert_eq!(installed.0.release, release("A").id());
            }
            assert_eq!(
                fs::read(game.join("saves/played/level.dat")).unwrap(),
                b"played world"
            );
            assert_eq!(
                fs::read(project.join("launcher-note.txt")).unwrap(),
                b"unrelated settings"
            );
            channel_route.assert_async().await;
            release_route.assert_async().await;
            asset_route.assert_async().await;
        }
    }
}

#[tokio::test]
async fn unavailable_channel_requires_explicit_offline_and_keeps_completed_release() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path()).await;
    let socket = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let origin = format!("http://{}", socket.local_addr().unwrap());
    drop(socket);
    let before = crate::application::engine_host::tests::snapshot(&root.path().join("project"));
    assert!(
        update_with_transport(
            &session(root.path(), true, false),
            false,
            false,
            transport(&origin)
        )
        .await
        .is_err()
    );
    assert_eq!(
        update_with_transport(
            &session(root.path(), true, false),
            true,
            false,
            transport(&origin)
        )
        .await
        .unwrap(),
        Some(release("A").id().into())
    );
    assert_eq!(
        before,
        crate::application::engine_host::tests::snapshot(&root.path().join("project"))
    );
    super::super::dispatch(
        &session(root.path(), true, false),
        InstanceCommand::Trust {
            keys: vec![],
            revoke_all: true,
        },
    )
    .await
    .unwrap();
    assert!(
        update_with_transport(
            &session(root.path(), true, false),
            true,
            false,
            transport(&origin)
        )
        .await
        .is_err()
    );
}
#[tokio::test]
async fn authenticated_prelaunch_stops_on_preview_decline_and_release_failure() {
    let root = tempfile::tempdir().unwrap();
    let key = fixture(root.path()).await;
    let release = release("B");
    let envelope = sign(EnvelopeKind::Release, release.bytes(), &[&key]).unwrap();
    let mut server = mockito::Server::new_async().await;
    let mut pointer = server
        .mock("GET", "/channel.json")
        .with_body(channel(&release, &key, envelope.len() as u64))
        .create_async()
        .await;
    let before = crate::application::engine_host::tests::snapshot(&root.path().join("project"));
    for (yes, dry) in [(true, true), (false, false)] {
        assert!(
            update_with_transport(
                &session(root.path(), yes, dry),
                true,
                false,
                transport(&server.url())
            )
            .await
            .unwrap()
            .is_none()
        );
        assert_eq!(
            before,
            crate::application::engine_host::tests::snapshot(&root.path().join("project"))
        );
    }
    // No release route: a failure after accepting the channel can never take offline fallback.
    assert!(
        update_with_transport(
            &session(root.path(), true, false),
            true,
            false,
            transport(&server.url())
        )
        .await
        .is_err()
    );
    let before_release_failure =
        crate::application::engine_host::tests::snapshot(&root.path().join("project"));
    for (bytes, diagnostic) in [
        (
            b"invalid release envelope".to_vec(),
            "Invalid signed envelope",
        ),
        (
            sign(
                EnvelopeKind::Release,
                release.bytes(),
                &[&SigningKey::from_bytes(&[44; 32])],
            )
            .unwrap(),
            "No enrolled publisher signed",
        ),
        (
            sign(EnvelopeKind::Release, self::release("C").bytes(), &[&key]).unwrap(),
            "Selected release payload identity mismatch",
        ),
    ] {
        let rejected = server
            .mock("GET", "/release.json")
            .with_body(bytes)
            .create_async()
            .await;
        let error = update_with_transport(
            &session(root.path(), true, false),
            true,
            false,
            transport(&server.url()),
        )
        .await
        .expect_err("a failed selected release cannot become offline permission");
        assert!(format!("{error:#}").contains(diagnostic), "{error:#}");
        assert_eq!(
            crate::application::engine_host::tests::snapshot(&root.path().join("project")),
            before_release_failure
        );
        let saved = fs::read(root.path().join("project/.empack/subscription.json")).unwrap();
        assert_eq!(
            crate::engine::api::SubscriptionRecord::decode(&saved)
                .unwrap()
                .floor
                .unwrap()
                .sequence,
            1
        );
        rejected.assert_async().await;
        rejected.remove_async().await;
    }
    let response = server
        .mock("GET", "/release.json")
        .with_body(envelope)
        .create_async()
        .await;
    assert_eq!(
        update_with_transport(
            &session(root.path(), true, false),
            false,
            false,
            transport(&server.url())
        )
        .await
        .unwrap(),
        Some(release.id().into())
    );
    response.assert_async().await;
    pointer.remove_async().await;
    pointer = server
        .mock("GET", "/channel.json")
        .with_body(b"invalid signature envelope")
        .create_async()
        .await;
    assert!(
        update_with_transport(
            &session(root.path(), true, false),
            true,
            false,
            transport(&server.url())
        )
        .await
        .is_err()
    );
    pointer.assert_async().await;
}

#[tokio::test]
async fn channel_http_failures_do_not_become_offline_permission() {
    for (status, encoding, body, location, expected) in [
        (401, "identity", "", None, "Unauthorized"),
        (403, "identity", "", None, "Unauthorized"),
        (404, "identity", "", None, "NotFound"),
        (429, "identity", "", None, "RateLimited"),
        (500, "identity", "", None, "Server(500)"),
        (503, "identity", "", None, "Server(503)"),
        (400, "identity", "", None, "Status(400)"),
        (200, "gzip", "encoded", None, "InvalidLocator"),
        (
            302,
            "identity",
            "",
            Some("https://other.test/channel"),
            "InvalidLocator",
        ),
        (302, "identity", "", Some("/channel.json"), "RedirectLimit"),
        (
            200,
            "identity",
            &"x".repeat((48 << 10) + 1),
            None,
            "ByteLimit",
        ),
    ] {
        let root = tempfile::tempdir().unwrap();
        fixture(root.path()).await;
        let mut server = mockito::Server::new_async().await;
        let mut response = server
            .mock("GET", "/channel.json")
            .with_status(status)
            .with_header("content-encoding", encoding)
            .with_body(body);
        if let Some(location) = location {
            response = response.with_header("location", location);
        }
        let response = response
            .expect(if expected == "RedirectLimit" { 6 } else { 1 })
            .create_async()
            .await;
        let before = crate::application::engine_host::tests::snapshot(root.path());
        let error = update_with_transport(
            &session(root.path(), true, false),
            true,
            false,
            transport(&server.url()),
        )
        .await
        .expect_err("HTTP evidence failures must not allow offline launch");
        let category = error
            .downcast_ref::<crate::engine::acquisition::TransferError>()
            .unwrap_or_else(|| panic!("lost {expected} classification: {error:#}"));
        assert_eq!(format!("{category:?}"), expected);
        assert_eq!(
            crate::application::engine_host::tests::snapshot(root.path()),
            before
        );
        response.assert_async().await;
    }
}

#[tokio::test]
async fn invalid_authenticated_channels_never_select_offline_launch_or_change_floor() {
    let root = tempfile::tempdir().unwrap();
    let key = fixture(root.path()).await;
    let offered = release("B");
    let envelope = sign(EnvelopeKind::Release, offered.bytes(), &[&key]).unwrap();
    let signed = |document: &ChannelDocument, key: &SigningKey| {
        sign(EnvelopeKind::Channel, &document.encode().unwrap(), &[key]).unwrap()
    };
    let mut observed = channel_document(&offered, envelope.len() as u64);
    observed.sequence = 2;
    fs::write(root.path().join("observed.json"), signed(&observed, &key)).unwrap();
    super::super::dispatch(
        &session(root.path(), true, false),
        InstanceCommand::ObserveChannel {
            envelope: Some("observed.json".into()),
        },
    )
    .await
    .unwrap();
    let baseline = crate::application::engine_host::tests::snapshot(root.path());
    let mut fresh = observed.clone();
    fresh.sequence = 3;
    let mut expired = fresh.clone();
    expired.expires -= 3601;
    let mut replay = fresh.clone();
    replay.sequence = 1;
    let mut corrupt: serde_json::Value = serde_json::from_slice(&signed(&fresh, &key)).unwrap();
    let signature = corrupt["signatures"][0]["signature"].as_str().unwrap();
    let mut changed = signature.to_owned();
    changed.replace_range(..1, if signature.starts_with('0') { "1" } else { "0" });
    corrupt["signatures"][0]["signature"] = serde_json::Value::String(changed);
    let mut server = mockito::Server::new_async().await;
    let release_route = server
        .mock("GET", "/release.json")
        .expect(0)
        .with_body(envelope)
        .create_async()
        .await;
    for (bytes, diagnostic) in [
        (
            b"invalid signature envelope".to_vec(),
            "Invalid signed envelope",
        ),
        (
            signed(&fresh, &SigningKey::from_bytes(&[44; 32])),
            "No enrolled publisher signed",
        ),
        (
            serde_json::to_vec(&corrupt).unwrap(),
            "Publisher signature verification failed",
        ),
        (signed(&expired, &key), "Channel has expired"),
        (signed(&replay, &key), "Channel sequence replay rejected"),
    ] {
        let pointer = server
            .mock("GET", "/channel.json")
            .with_body(bytes)
            .create_async()
            .await;
        let error = update_with_transport(
            &session(root.path(), true, false),
            true,
            false,
            transport(&server.url()),
        )
        .await
        .expect_err("failed authentication cannot select the completed release");
        assert!(format!("{error:#}").contains(diagnostic), "{error:#}");
        assert_eq!(
            crate::application::engine_host::tests::snapshot(root.path()),
            baseline
        );
        pointer.assert_async().await;
        pointer.remove_async().await;
    }
    release_route.assert_async().await;
}

#[tokio::test]
async fn combined_update_checks_before_separate_apply_and_keeps_failed_floor() {
    use crate::application::session_mocks::MockResponse;
    let root = tempfile::tempdir().unwrap();
    let key = fixture_side(root.path(), "server", None).await;
    let offered = release("B");
    let envelope = sign(EnvelopeKind::Release, offered.bytes(), &[&key]).unwrap();
    let mut server = mockito::Server::new_async().await;
    let _pointer = server
        .mock("GET", "/channel.json")
        .with_body(channel(&offered, &key, envelope.len() as u64))
        .create_async()
        .await;
    let update = || InstanceCommand::Update {
        check: true,
        conflicts: Default::default(),
        release: None,
        side: None,
        layout: None,
        choices: vec![],
        files: vec![],
    };
    let before = crate::application::engine_host::tests::snapshot(&root.path().join("project"));
    for (yes, dry) in [(true, true), (false, false)] {
        super::super::install_with_transport(
            &session(root.path(), yes, dry),
            update(),
            transport(&server.url()),
        )
        .await
        .unwrap();
        assert_eq!(
            before,
            crate::application::engine_host::tests::snapshot(&root.path().join("project"))
        );
    }
    // A missing release fails after the approved observation, preserving its floor.
    assert!(
        super::super::install_with_transport(
            &session(root.path(), true, false),
            update(),
            transport(&server.url())
        )
        .await
        .is_err()
    );
    let saved = fs::read(root.path().join("project/.empack/subscription.json")).unwrap();
    assert_eq!(
        crate::engine::api::SubscriptionRecord::decode(&saved)
            .unwrap()
            .floor
            .unwrap()
            .sequence,
        1
    );
    let _response = server
        .mock("GET", "/release.json")
        .with_body(envelope)
        .create_async()
        .await;
    let interactive = MockInteractiveProvider::new();
    interactive
        .response_queue
        .lock()
        .unwrap()
        .extend([MockResponse::Confirm(true), MockResponse::Confirm(false)]);
    let prompts = interactive.confirm_calls.clone();
    let host = session(root.path(), false, false).with_interactive(interactive);
    super::super::install_with_transport(&host, update(), transport(&server.url()))
        .await
        .unwrap();
    assert_eq!(prompts.lock().unwrap().len(), 2);
    let installed = inspect(
        &host,
        root.path().join("project"),
        None,
        root.path().join("state"),
    )
    .await
    .unwrap();
    assert_eq!(installed.0.release, release("A").id());
    assert_eq!(
        saved,
        fs::read(root.path().join("project/.empack/subscription.json")).unwrap()
    );
    super::super::install_with_transport(
        &session(root.path(), true, false),
        update(),
        transport(&server.url()),
    )
    .await
    .unwrap();
    let installed = inspect(
        &host,
        root.path().join("project"),
        None,
        root.path().join("state"),
    )
    .await
    .unwrap();
    assert_eq!(installed.0.release, offered.id());
    assert_eq!(installed.0.side, InstanceSide::Server);
}
