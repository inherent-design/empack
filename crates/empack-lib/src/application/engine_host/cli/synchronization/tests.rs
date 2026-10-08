use super::super::tests::{fixture, project, session};
use super::*;
use crate::application::engine_host::tests::snapshot;
use empack_core::{
    digest::{DigestSet, ExpectedDigest},
    identity::*,
    path::{PathSyntax, PortableRelPath},
};
use serde_json::json;
use sha2::{Digest, Sha256, Sha512};
use std::fs;

fn write_intent(root: &Path, intent: &ProjectIntent) {
    let mut bytes = b"# retain authored formatting\n".to_vec();
    bytes.extend(DocumentCodec.encode_intent(intent).unwrap());
    fs::write(root.join("project/empack.yml"), bytes).unwrap();
}
fn placed(name: &str) -> PlacementIntent {
    PlacementIntent::Explicit(
        NonEmpty::new(vec![Placement {
            destination: InstallDestination::parse(name).unwrap(),
            layer: ContentLayer::Common,
            requirements: required(),
        }])
        .unwrap(),
    )
}
fn services(base: &str) -> dependencies::AdditionServices {
    dependencies::AdditionServices {
        catalog: ProviderCatalog::for_loopback_tests(base, None),
        transport: HttpAcquisition::for_loopback_tests(),
        files: DirectFileLimits::default(),
    }
}
async fn sync(root: &Path, base: &str, dry: bool) -> Result<()> {
    synchronize_with_services(
        &session(root, dry),
        services(base),
        RuntimeCatalog::for_loopback_tests(base),
    )
    .await
}

#[tokio::test]
async fn missing_and_changed_intent_resolve_local_bytes_without_losing_authored_source() {
    for missing_lock in [false, true] {
        let root = tempfile::tempdir().unwrap();
        fixture(root.path()).await;
        let mut intent = project(root.path()).intent().clone();
        let key = DependencyKey::parse("settings").unwrap();
        intent.roots.insert(
            key.clone(),
            DependencyIntent {
                source: SourceIntent::Local(
                    PortableRelPath::parse("seed/settings.toml", PathSyntax::ProjectContent)
                        .unwrap(),
                ),
                kind: ContentKind::Config,
                version: VersionIntent::FollowCompatible,
                placement: placed("config/settings.toml"),
                requirements: required(),
            },
        );
        fs::create_dir(root.path().join("project/seed")).unwrap();
        fs::write(
            root.path().join("project/seed/settings.toml"),
            b"enabled = true",
        )
        .unwrap();
        write_intent(root.path(), &intent);
        if missing_lock {
            fs::remove_file(root.path().join("project/empack.lock")).unwrap();
        }
        let before = snapshot(root.path());
        sync(root.path(), "http://127.0.0.1:9", true).await.unwrap();
        assert_eq!(snapshot(root.path()), before);
        sync(root.path(), "http://127.0.0.1:9", false)
            .await
            .unwrap();
        assert_eq!(project(root.path()).intent(), &intent);
        assert_eq!(
            fs::read(root.path().join("project/pack/config/settings.toml")).unwrap(),
            b"enabled = true"
        );
        let committed = snapshot(&root.path().join("project"));
        for _ in 0..2 {
            sync(root.path(), "http://127.0.0.1:9", false)
                .await
                .unwrap();
        }
        assert_eq!(snapshot(&root.path().join("project")), committed);

        // Sync never accepts an unrequested byte change, even though the file can be read.
        fs::write(
            root.path().join("project/seed/settings.toml"),
            b"enabled = false",
        )
        .unwrap();
        let drifted = snapshot(root.path());
        assert!(
            sync(root.path(), "http://127.0.0.1:9", false)
                .await
                .is_err()
        );
        assert_eq!(snapshot(root.path()), drifted);
        intent.roots.get_mut(&key).unwrap().version = VersionIntent::ContentPinned(
            DigestSet::new(vec![ExpectedDigest::Sha256(
                Sha256::digest(b"enabled = false").into(),
            )])
            .unwrap(),
        );
        write_intent(root.path(), &intent);
        sync(root.path(), "http://127.0.0.1:9", false)
            .await
            .unwrap();
        assert_eq!(
            fs::read(root.path().join("project/pack/config/settings.toml")).unwrap(),
            b"enabled = false"
        );
        assert_eq!(project(root.path()).intent(), &intent);
    }
}

#[tokio::test]
async fn fresh_provider_sync_keeps_followed_pin_for_placement_edits_then_applies_an_explicit_pin() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path()).await;
    let mut server = mockito::Server::new_async().await;
    let project_mock = server.mock("GET", "/project/Root0001")
        .with_body(json!({"id":"Root0001","slug":"renderer","title":"Renderer","project_type":"mod","loaders":["fabric"]}).to_string())
        .expect(8).create_async().await;
    let mut versions = Vec::new();
    for (version, expected) in [("RootVer1", 2), ("RootVer2", 1)] {
        versions.push(server.mock("GET", format!("/version/{version}").as_str()).with_body(json!({
            "id":version,"project_id":"Root0001","game_versions":["1.21.1"],"loaders":["fabric"],
            "files":[{"filename":"renderer.jar","primary":true,"size":7,"hashes":{"sha512":ExpectedDigest::Sha512(Sha512::digest(b"payload").into()).hex()},"url":"https://example.invalid/renderer.jar"}],
            "dependencies":[],"date_published":"2026-01-01T00:00:00Z","status":"listed","version_type":"release"
        }).to_string()).expect(expected).create_async().await);
    }
    let compatible = server
        .mock("GET", "/project/Root0001/version")
        .match_query(mockito::Matcher::Any)
        .expect(0)
        .create_async()
        .await;
    let mut intent = project(root.path()).intent().clone();
    let key = DependencyKey::parse("my-alias").unwrap();
    intent.roots.insert(
        key.clone(),
        DependencyIntent {
            source: SourceIntent::Provider(ProviderProjectId::Modrinth(
                ModrinthProjectId::parse("Root0001").unwrap(),
            )),
            kind: ContentKind::Mod,
            version: VersionIntent::Exact(PinSelector::ModrinthVersion(
                ModrinthVersionId::parse("RootVer1").unwrap(),
            )),
            placement: PlacementIntent::Automatic,
            requirements: required(),
        },
    );
    write_intent(root.path(), &intent);
    sync(root.path(), &server.url(), false).await.unwrap();
    let first = project(root.path()).lock().dependencies[&key]
        .selected
        .clone();
    intent.roots.get_mut(&key).unwrap().version = VersionIntent::FollowCompatible;
    intent.roots.get_mut(&key).unwrap().placement = placed("mods/custom-name.jar");
    write_intent(root.path(), &intent);
    sync(root.path(), &server.url(), false).await.unwrap();
    assert_eq!(
        project(root.path()).lock().dependencies[&key].selected,
        first
    );
    assert_eq!(project(root.path()).intent(), &intent);
    let original = versions.remove(0);
    original.assert_async().await;
    original.remove_async().await;
    let changed_assertions = server.mock("GET", "/version/RootVer1").with_body(json!({
        "id":"RootVer1","project_id":"Root0001","game_versions":["1.21.1"],"loaders":["fabric"],
        "files":[{"filename":"renderer.jar","primary":true,"size":7,"hashes":{"sha512":ExpectedDigest::Sha512(Sha512::digest(b"changed").into()).hex()},"url":"https://example.invalid/renderer.jar"}],
        "dependencies":[],"date_published":"2026-01-01T00:00:00Z","status":"listed","version_type":"release"
    }).to_string()).expect(1).create_async().await;
    intent.roots.get_mut(&key).unwrap().placement = placed("mods/another-name.jar");
    write_intent(root.path(), &intent);
    let before_failure = snapshot(root.path());
    let error = sync(root.path(), &server.url(), false).await.unwrap_err();
    assert!(format!("{error:#}").contains("assertions changed"));
    assert_eq!(snapshot(root.path()), before_failure);
    changed_assertions.assert_async().await;
    intent.roots.get_mut(&key).unwrap().version = VersionIntent::Exact(
        PinSelector::ModrinthVersion(ModrinthVersionId::parse("RootVer2").unwrap()),
    );
    write_intent(root.path(), &intent);
    sync(root.path(), &server.url(), false).await.unwrap();
    assert_eq!(
        project(root.path()).lock().dependencies[&key]
            .selected
            .as_ref()
            .unwrap()
            .selection,
        PinSelector::ModrinthVersion(ModrinthVersionId::parse("RootVer2").unwrap())
    );
    let committed = snapshot(&root.path().join("project"));
    for _ in 0..2 {
        sync(root.path(), &server.url(), false).await.unwrap();
    }
    assert_eq!(snapshot(&root.path().join("project")), committed);
    project_mock.assert_async().await;
    for version in versions {
        version.assert_async().await;
    }
    compatible.assert_async().await;
}

#[tokio::test]
async fn failed_fresh_batch_retains_documents_and_payloads() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path()).await;
    let mut intent = project(root.path()).intent().clone();
    for name in ["first", "missing"] {
        intent.roots.insert(
            DependencyKey::parse(name).unwrap(),
            DependencyIntent {
                source: SourceIntent::Local(
                    PortableRelPath::parse(&format!("{name}.toml"), PathSyntax::ProjectContent)
                        .unwrap(),
                ),
                kind: ContentKind::Config,
                version: VersionIntent::FollowCompatible,
                placement: placed(&format!("config/{name}.toml")),
                requirements: required(),
            },
        );
    }
    fs::write(root.path().join("project/first.toml"), b"valid").unwrap();
    write_intent(root.path(), &intent);
    let before = snapshot(root.path());
    assert!(
        sync(root.path(), "http://127.0.0.1:9", false)
            .await
            .is_err()
    );
    assert_eq!(snapshot(root.path()), before);
}

#[cfg(unix)]
#[tokio::test]
async fn authored_local_sources_cannot_traverse_symlinked_ancestors() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    fixture(root.path()).await;
    fs::write(outside.path().join("settings.toml"), b"outside").unwrap();
    std::os::unix::fs::symlink(outside.path(), root.path().join("project/linked")).unwrap();
    let mut intent = project(root.path()).intent().clone();
    intent.roots.insert(
        DependencyKey::parse("settings").unwrap(),
        DependencyIntent {
            source: SourceIntent::Local(
                PortableRelPath::parse("linked/settings.toml", PathSyntax::ProjectContent).unwrap(),
            ),
            kind: ContentKind::Config,
            version: VersionIntent::FollowCompatible,
            placement: placed("config/settings.toml"),
            requirements: required(),
        },
    );
    write_intent(root.path(), &intent);
    let before = fs::read(root.path().join("project/empack.lock")).unwrap();
    assert!(
        sync(root.path(), "http://127.0.0.1:9", false)
            .await
            .is_err()
    );
    assert_eq!(
        fs::read(root.path().join("project/empack.lock")).unwrap(),
        before
    );
    assert!(
        !root
            .path()
            .join("project/pack/config/settings.toml")
            .exists()
    );
    assert_eq!(
        fs::read(outside.path().join("settings.toml")).unwrap(),
        b"outside"
    );
}
