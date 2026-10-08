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
        false,
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

#[tokio::test]
async fn moved_or_removed_placement_cannot_delete_a_retained_acquisition_source() {
    use crate::engine::api::{RemovalSelector, RemoveRequest};
    use empack_core::removal::{RemovalEvidencePolicy, RemovalMode};
    for remove in [false, true] {
        let root = tempfile::tempdir().unwrap();
        fixture(root.path()).await;
        let mut intent = project(root.path()).intent().clone();
        let key = DependencyKey::parse("settings").unwrap();
        let selected = DependencyIntent {
            source: SourceIntent::Local(
                PortableRelPath::parse("seed.toml", PathSyntax::ProjectContent).unwrap(),
            ),
            kind: ContentKind::Config,
            version: VersionIntent::FollowCompatible,
            placement: placed("config/settings.toml"),
            requirements: required(),
        };
        intent.roots.insert(key.clone(), selected.clone());
        fs::write(root.path().join("project/seed.toml"), b"keep source").unwrap();
        write_intent(root.path(), &intent);
        sync(root.path(), "http://127.0.0.1:9", false)
            .await
            .unwrap();
        let mut moved = selected;
        moved.source = SourceIntent::Local(
            PortableRelPath::parse("pack/config/settings.toml", PathSyntax::ProjectContent)
                .unwrap(),
        );
        moved.placement = placed("config/moved.toml");
        if remove {
            intent
                .roots
                .insert(DependencyKey::parse("other").unwrap(), moved);
        } else {
            intent.roots.insert(key.clone(), moved);
        }
        write_intent(root.path(), &intent);
        sync(root.path(), "http://127.0.0.1:9", false)
            .await
            .unwrap();
        if remove {
            fs::write(root.path().join("replacement.toml"), b"different source").unwrap();
            let before = snapshot(root.path());
            assert!(
                dependencies::add(
                    &session(root.path(), false),
                    NonEmpty::new(vec![AddHostInput::File(DirectFileInput {
                        key: key.clone(),
                        title: "settings".into(),
                        source: DirectFileSource::Local(root.path().join("replacement.toml")),
                        evidence: FileEvidence::AcceptObserved,
                        kind: ContentKind::Config,
                        kind_policy: FileKindPolicy::AcceptUnrecognized,
                        requirements: required(),
                        placements: NonEmpty::new(vec![Placement {
                            destination: InstallDestination::parse("config/settings.toml").unwrap(),
                            layer: ContentLayer::Common,
                            requirements: required()
                        }])
                        .unwrap(),
                    })])
                    .unwrap(),
                    ReleasePolicy::PreferStable,
                    SourceEvidencePolicy::Compatibility,
                    ExistingDependencyPolicy::UpdateSameIdentity,
                )
                .await
                .is_err(),
                "Replacing one owner must not invalidate another source reference"
            );
            assert_eq!(snapshot(root.path()), before);
            dependencies::remove(
                &session(root.path(), false),
                RemoveRequest {
                    selections: NonEmpty::new(vec![RemovalSelector::Key(key)]).unwrap(),
                    mode: RemovalMode::RemoveContent,
                    evidence: RemovalEvidencePolicy::AcknowledgeUnknown,
                },
            )
            .await
            .unwrap();
        }
        assert!(
            root.path()
                .join("project/pack/config/settings.toml")
                .exists(),
            "A source still used by the published lock was deleted"
        );
        assert_eq!(
            fs::read(root.path().join("project/pack/config/moved.toml")).unwrap(),
            b"keep source"
        );
        let before = snapshot(&root.path().join("project"));
        for _ in 0..2 {
            sync(root.path(), "http://127.0.0.1:9", false)
                .await
                .unwrap();
        }
        assert_eq!(snapshot(&root.path().join("project")), before);
    }
}

#[tokio::test]
async fn materialization_verifies_the_whole_batch_without_updating_pins_or_preview_downloads() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path()).await;
    let mut server = mockito::Server::new_async().await;
    let mut intent = project(root.path()).intent().clone();
    let mut original_versions = Vec::new();
    for (id, version, key, filename) in [
        ("Root0001", "RootVer1", "first", "first.jar"),
        ("Root0002", "RootVer2", "second", "second.jar"),
    ] {
        server
            .mock("GET", format!("/project/{id}").as_str())
            .with_body(
                json!({"id":id,"slug":key,"title":key,"project_type":"mod","loaders":["fabric"]})
                    .to_string(),
            )
            .create_async()
            .await;
        original_versions.push(server.mock("GET", format!("/version/{version}").as_str()).with_body(json!({
            "id":version,"project_id":id,"game_versions":["1.21.1"],"loaders":["fabric"],
            "files":[{"filename":filename,"primary":true,"size":7,"hashes":{"sha512":ExpectedDigest::Sha512(Sha512::digest(b"payload").into()).hex()},"url":format!("https://example.invalid/{filename}")}],
            "dependencies":[],"date_published":"2026-01-01T00:00:00Z","status":"listed","version_type":"release"
        }).to_string()).create_async().await);
        intent.roots.insert(
            DependencyKey::parse(key).unwrap(),
            DependencyIntent {
                source: SourceIntent::Provider(ProviderProjectId::Modrinth(
                    ModrinthProjectId::parse(id).unwrap(),
                )),
                kind: ContentKind::Mod,
                version: VersionIntent::Exact(PinSelector::ModrinthVersion(
                    ModrinthVersionId::parse(version).unwrap(),
                )),
                placement: PlacementIntent::Automatic,
                requirements: required(),
            },
        );
    }
    write_intent(root.path(), &intent);
    sync(root.path(), &server.url(), false).await.unwrap();
    let lock = fs::read(root.path().join("project/empack.lock")).unwrap();
    for original in original_versions {
        original.remove_async().await;
    }
    for (id, version, filename) in [
        ("Root0001", "RootVer1", "first.jar"),
        ("Root0002", "RootVer2", "second.jar"),
    ] {
        server.mock("GET", format!("/version/{version}").as_str()).with_body(json!({
            "id":version,"project_id":id,"game_versions":["1.21.1"],"loaders":["fabric"],
            "files":[{"filename":filename,"primary":true,"size":7,"hashes":{"sha512":ExpectedDigest::Sha512(Sha512::digest(b"payload").into()).hex()},"url":format!("{}/{filename}",server.url())}],
            "dependencies":[],"date_published":"2026-01-01T00:00:00Z","status":"listed","version_type":"release"
        }).to_string()).expect(2).create_async().await;
    }
    let before = snapshot(root.path());
    synchronize_with_services(
        &session(root.path(), true),
        true,
        services(&server.url()),
        RuntimeCatalog::for_loopback_tests(&server.url()),
    )
    .await
    .unwrap();
    assert_eq!(snapshot(root.path()), before);
    let first = server
        .mock("GET", "/first.jar")
        .with_body("payload")
        .expect(2)
        .create_async()
        .await;
    let bad = server
        .mock("GET", "/second.jar")
        .with_body("changed")
        .expect(1)
        .create_async()
        .await;
    let error = synchronize_with_services(
        &session(root.path(), false),
        true,
        services(&server.url()),
        RuntimeCatalog::for_loopback_tests(&server.url()),
    )
    .await
    .unwrap_err();
    assert!(format!("{error:#}").contains("digest"), "{error:#}");
    assert_eq!(
        snapshot(root.path()),
        before,
        "failed acquisition must publish nothing"
    );
    bad.assert_async().await;
    bad.remove_async().await;
    let second = server
        .mock("GET", "/second.jar")
        .with_body("payload")
        .expect(1)
        .create_async()
        .await;
    synchronize_with_services(
        &session(root.path(), false),
        true,
        services(&server.url()),
        RuntimeCatalog::for_loopback_tests(&server.url()),
    )
    .await
    .unwrap();
    for name in ["first.jar", "second.jar"] {
        assert_eq!(
            fs::read(root.path().join(format!("project/pack/mods/{name}"))).unwrap(),
            b"payload"
        );
    }
    assert_eq!(
        fs::read(root.path().join("project/empack.lock")).unwrap(),
        lock
    );
    let published = snapshot(&root.path().join("project"));
    for _ in 0..2 {
        sync(root.path(), "http://127.0.0.1:9", false)
            .await
            .unwrap();
    }
    assert_eq!(snapshot(&root.path().join("project")), published);
    first.assert_async().await;
    second.assert_async().await;
}

#[tokio::test]
async fn automatic_layout_changes_move_content_while_explicit_placements_remain_stable() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path()).await;
    let mut intent = project(root.path()).intent().clone();
    intent.layout.insert(
        ContentKind::Config,
        PortableRelPath::parse("config", PathSyntax::ProjectContent).unwrap(),
    );
    fs::write(root.path().join("project/settings.toml"), b"settings").unwrap();
    for (key, placement) in [
        ("automatic", PlacementIntent::Automatic),
        ("explicit", placed("config/explicit.toml")),
    ] {
        intent.roots.insert(
            DependencyKey::parse(key).unwrap(),
            DependencyIntent {
                source: SourceIntent::Local(
                    PortableRelPath::parse("settings.toml", PathSyntax::ProjectContent).unwrap(),
                ),
                kind: ContentKind::Config,
                version: VersionIntent::FollowCompatible,
                placement,
                requirements: required(),
            },
        );
    }
    write_intent(root.path(), &intent);
    sync(root.path(), "http://127.0.0.1:9", false)
        .await
        .unwrap();
    let old = root.path().join("project/pack/config/settings.toml");
    assert!(old.exists());
    intent.layout.insert(
        ContentKind::Config,
        PortableRelPath::parse("config/nested", PathSyntax::ProjectContent).unwrap(),
    );
    write_intent(root.path(), &intent);
    let before = snapshot(root.path());
    sync(root.path(), "http://127.0.0.1:9", true).await.unwrap();
    assert_eq!(snapshot(root.path()), before);
    sync(root.path(), "http://127.0.0.1:9", false)
        .await
        .unwrap();
    assert!(
        !old.exists(),
        "old automatic layout survived changed directory intent"
    );
    assert_eq!(
        fs::read(root.path().join("project/pack/config/nested/settings.toml")).unwrap(),
        b"settings"
    );
    assert_eq!(
        fs::read(root.path().join("project/pack/config/explicit.toml")).unwrap(),
        b"settings"
    );
    let expected = snapshot(&root.path().join("project"));
    for _ in 0..2 {
        sync(root.path(), "http://127.0.0.1:9", false)
            .await
            .unwrap();
    }
    assert_eq!(snapshot(&root.path().join("project")), expected);
    intent.layout.remove(&ContentKind::Config);
    write_intent(root.path(), &intent);
    let before = snapshot(root.path());
    assert!(
        sync(root.path(), "http://127.0.0.1:9", false)
            .await
            .is_err(),
        "a kind without a default directory needs an explicit placement"
    );
    assert_eq!(snapshot(root.path()), before);
}
