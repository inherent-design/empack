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
    fixture_side(root, "client").await
}
async fn fixture_side(root: &Path, side: &str) -> SigningKey {
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
            layout: None,
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
    let key = fixture_side(root.path(), "server").await;
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
