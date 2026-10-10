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
        minimum_engine: ">=0.6.0-beta".into(),
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
    fs::create_dir(root.join("project")).unwrap();
    let release = release("A");
    fs::write(root.join("initial.json"), release.bytes()).unwrap();
    super::super::dispatch(
        &session(root, true, false),
        InstanceCommand::Install {
            conflicts: Default::default(),
            release: "initial.json".into(),
            sha256: release.id().into(),
            side: "client".into(),
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
fn channel(release: &DecodedRelease, key: &SigningKey, maximum: u64) -> Vec<u8> {
    sign(
        EnvelopeKind::Channel,
        &ChannelDocument {
            schema: 1,
            pack: "prelaunch".into(),
            channel: "stable".into(),
            sequence: 1,
            expires: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64
                + 3600,
            minimum_engine: ">=0.6.0-beta".into(),
            release: ChannelRelease {
                id: release.id().into(),
                url: "https://publisher.test/release.json".into(),
                maximum_bytes: maximum,
            },
        }
        .encode()
        .unwrap(),
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
