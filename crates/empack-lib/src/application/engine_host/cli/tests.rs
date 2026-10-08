use super::*;
use crate::{
    application::{
        InitArgs,
        session_mocks::{MockCommandSession, MockConfigProvider, MockFileSystemProvider},
    },
    engine::{api::SyncRequest, documents::DocumentCodec},
};
use serde_json::json;
use sha2::{Digest, Sha512};
use std::fs;

pub(super) fn session(root: &Path, dry: bool) -> MockCommandSession {
    MockCommandSession::new()
        .with_filesystem(MockFileSystemProvider::new().with_current_dir(root.into()))
        .with_config(MockConfigProvider::new(AppConfig {
            workdir: Some("project".into()),
            state_dir: Some("state".into()),
            yes: true,
            dry_run: dry,
            curseforge_api_client_key: None,
            ..Default::default()
        }))
}
async fn fixture(root: &Path) {
    fs::create_dir(root.join("project")).unwrap();
    initialize(
        &session(root, false),
        &InitArgs {
            mc_version: Some("1.21.1".into()),
            modloader: Some("fabric".into()),
            loader_version: Some("0.16.14".into()),
            pack_name: Some("Frontend Pack".into()),
            pack_version: Some("1.0".into()),
            author: Some("Tester".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
}
fn options(value: &str) -> AddOptions {
    AddOptions {
        inputs: vec![value.into()],
        force: false,
        platform: Some(SearchPlatform::Modrinth),
        kind: None,
        version_id: Some("RootVer1".into()),
        file_id: None,
    }
}
pub(super) fn project(root: &Path) -> ResolvedProject {
    let intent = DocumentCodec
        .decode_intent(&fs::read(root.join("project/empack.yml")).unwrap(), "test")
        .unwrap();
    DocumentCodec
        .decode_lock(
            &fs::read(root.join("project/empack.lock")).unwrap(),
            &intent,
            "test",
        )
        .unwrap()
}
#[test]
fn pin_and_source_validation_never_reinterprets_invalid_input() {
    let mut selected = options("renderer");
    selected.inputs.push("another".into());
    assert!(selected.pin().is_err());
    selected.inputs.pop();
    selected.file_id = Some("123".into());
    assert!(selected.pin().is_err());
    selected.file_id = None;
    selected.platform = Some(SearchPlatform::Curseforge);
    assert!(selected.pin().is_err());
    let root = tempfile::tempdir().unwrap();
    for value in [
        "missing.jar",
        "./missing",
        "https://example.invalid/mod.jar?token=secret",
        "http://example.invalid/mod.jar",
    ] {
        assert!(files::classify(root.path(), value, false).is_err());
    }
    fs::create_dir(root.path().join("directory")).unwrap();
    assert!(files::classify(root.path(), "directory", false).is_err());
    assert!(
        files::classify(root.path(), "renderer", false)
            .unwrap()
            .is_none()
    );
    assert!(
        ProjectSelector::parse(ProviderKind::Modrinth, "https://modrinth.com/nope/renderer")
            .is_err()
    );
}
#[tokio::test]
async fn headless_search_and_conflicting_provider_url_preserve_entire_project() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path()).await;
    let before = super::super::tests::snapshot(root.path());
    let mut selected = options("renderer");
    selected.platform = None;
    selected.version_id = None;
    let error = add(&session(root.path(), false), selected)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("deliberate choice"));
    let mut selected = options("https://curseforge.com/minecraft/mc-mods/renderer");
    selected.platform = None;
    assert!(
        add(&session(root.path(), false), selected)
            .await
            .unwrap_err()
            .to_string()
            .contains("conflicts")
    );
    assert_eq!(super::super::tests::snapshot(root.path()), before);
}
#[tokio::test]
async fn slug_id_and_url_persist_one_canonical_identity_and_sync_twice_without_changes() {
    let mut server = mockito::Server::new_async().await;
    for selector in ["renderer", "Root0001"] {
        server.mock("GET", format!("/project/{selector}").as_str())
            .with_body(json!({"id":"Root0001","slug":"renderer","title":"Renderer","project_type":"mod","loaders":["fabric"]}).to_string())
            .create_async().await;
    }
    server.mock("GET", "/version/RootVer1").with_body(json!({
        "id":"RootVer1","project_id":"Root0001","game_versions":["1.21.1"],"loaders":["fabric"],
        "files":[{"filename":"renderer.jar","primary":true,"size":7,"hashes":{
            "sha1":"f07e5a815613c5abeddc4b682247a4c42d8a95df",
            "sha512":empack_core::digest::ExpectedDigest::Sha512(Sha512::digest(b"payload").into()).hex()},
            "url":"https://example.invalid/renderer.jar"}],"dependencies":[],
        "date_published":"2026-01-01T00:00:00Z","status":"listed","version_type":"release"
    }).to_string()).create_async().await;
    let mut prior = None;
    for input in ["renderer", "Root0001", "https://modrinth.com/mod/renderer"] {
        let root = tempfile::tempdir().unwrap();
        fixture(root.path()).await;
        let before = super::super::tests::snapshot(root.path());
        add_with_catalog(
            &session(root.path(), true),
            options(input),
            ProviderCatalog::for_loopback_tests(&server.url(), None),
        )
        .await
        .unwrap();
        assert_eq!(super::super::tests::snapshot(root.path()), before);
        add_with_catalog(
            &session(root.path(), false),
            options(input),
            ProviderCatalog::for_loopback_tests(&server.url(), None),
        )
        .await
        .unwrap();
        let current = DocumentCodec.encode_lock(&project(root.path())).unwrap();
        if let Some(expected) = prior {
            assert_eq!(current, expected);
        }
        prior = Some(current);
        let before = super::super::tests::snapshot(&root.path().join("project"));
        for _ in 0..2 {
            synchronize(
                &session(root.path(), false),
                SyncRequest::Recorded {
                    resolution: None,
                    evidence: SourceEvidencePolicy::Compatibility,
                },
            )
            .await
            .unwrap();
        }
        assert_eq!(
            super::super::tests::snapshot(&root.path().join("project")),
            before
        );
    }
}

#[tokio::test]
async fn direct_cli_files_publish_together_and_failed_member_keeps_previous_tree() {
    use std::io::Write;
    let root = tempfile::tempdir().unwrap();
    fixture(root.path()).await;
    let mut archive = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    archive
        .start_file("fabric.mod.json", zip::write::SimpleFileOptions::default())
        .unwrap();
    archive
        .write_all(br#"{"schemaVersion":1,"id":"fixture","version":"1"}"#)
        .unwrap();
    let bytes = archive.finish().unwrap().into_inner();
    fs::write(root.path().join("fixture.jar"), &bytes).unwrap();
    fs::write(root.path().join("broken.jar"), b"not an archive").unwrap();
    let mut selected = options("fixture.jar");
    selected.platform = None;
    selected.version_id = None;
    selected.inputs.push("broken.jar".into());
    let before = super::super::tests::snapshot(root.path());
    assert!(add(&session(root.path(), false), selected).await.is_err());
    assert_eq!(super::super::tests::snapshot(root.path()), before);
    let mut selected = options("fixture.jar");
    selected.platform = None;
    selected.version_id = None;
    add(&session(root.path(), false), selected).await.unwrap();
    assert_eq!(
        fs::read(root.path().join("project/pack/mods/fixture.jar")).unwrap(),
        bytes
    );
    assert_eq!(project(root.path()).intent().roots.len(), 1);
}

#[tokio::test]
async fn interactive_search_preserves_selected_identity_without_publishing() {
    use crate::application::session_mocks::MockInteractiveProvider;
    let root = tempfile::tempdir().unwrap();
    fixture(root.path()).await;
    let mut selected_session = session(root.path(), false);
    selected_session = selected_session
        .with_config(MockConfigProvider::new(AppConfig {
            workdir: Some("project".into()),
            state_dir: Some("state".into()),
            yes: false,
            curseforge_api_client_key: None,
            ..Default::default()
        }))
        .with_interactive(MockInteractiveProvider::new().with_fuzzy_select(1));
    let mut server = mockito::Server::new_async().await;
    let hit = |id: &str| json!({"project_id":id,"slug":id,"title":id,"project_type":"mod","all_project_types":["mod"],"categories":["fabric"],"versions":["1.21.1"]});
    let request = server
        .mock("GET", "/search")
        .match_query(mockito::Matcher::Any)
        .with_body(
            json!({"hits":[hit("Wrong001"),hit("Right001")],"offset":0,"limit":20,"total_hits":2})
                .to_string(),
        )
        .expect(1)
        .create_async()
        .await;
    let resolution = server.mock("GET", "/project/Right001")
        .with_body(json!({"id":"Right001","slug":"chosen","title":"Chosen","project_type":"mod","loaders":["fabric"]}).to_string())
        .expect(1).create_async().await;
    let before = super::super::tests::snapshot(root.path());
    let catalog = ProviderCatalog::for_loopback_tests(&server.url(), None);
    let selector = search(
        &selected_session,
        &catalog,
        &project(root.path()),
        "renderer",
        Some(ContentKind::Mod),
    )
    .await
    .unwrap();
    let resolved = initialize::discover(&selected_session, move |mut scope| async move {
        catalog
            .resolve_selector(&mut scope, selector, Default::default())
            .await
    })
    .await
    .unwrap();
    assert_eq!(resolved.id.to_string(), "Right001");
    request.assert_async().await;
    resolution.assert_async().await;
    assert_eq!(super::super::tests::snapshot(root.path()), before);
}
