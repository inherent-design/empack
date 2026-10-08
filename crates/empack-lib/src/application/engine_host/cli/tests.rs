use super::*;
use crate::{
    application::{
        InitArgs,
        session_mocks::{MockCommandSession, MockConfigProvider, MockInvocationProvider},
    },
    engine::documents::DocumentCodec,
};
use serde_json::json;
use sha2::{Digest, Sha512};
use std::{collections::BTreeMap, fs};

pub(super) fn session(root: &Path, dry: bool) -> MockCommandSession {
    MockCommandSession::new()
        .with_invocation(MockInvocationProvider::new().with_current_dir(root.into()))
        .with_config(MockConfigProvider::new(AppConfig {
            workdir: Some("project".into()),
            state_dir: Some("state".into()),
            cache_dir: Some("cache".into()),
            yes: true,
            dry_run: dry,
            curseforge_api_client_key: None,
            ..Default::default()
        }))
}
pub(super) async fn fixture(root: &Path) {
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
        file_plan: None,
        download_as_local: false,
        continue_independent: false,
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
        let keys = project(root.path())
            .intent()
            .roots
            .keys()
            .map(|key| key.as_str().to_owned())
            .collect();
        let intent_before = fs::read(root.path().join("project/empack.yml")).unwrap();
        let catalog = ProviderCatalog::for_loopback_tests(&server.url(), None);
        update::update_with_services(
            &session(root.path(), false),
            keys,
            dependencies::AdditionServices {
                transport: catalog.configure_acquisition(HttpAcquisition::new().unwrap()),
                catalog,
                files: DirectFileLimits::default(),
            },
        )
        .await
        .unwrap();
        assert_eq!(
            fs::read(root.path().join("project/empack.yml")).unwrap(),
            intent_before
        );
        assert_eq!(
            DocumentCodec.encode_lock(&project(root.path())).unwrap(),
            prior.clone().unwrap()
        );
        let before = super::super::tests::snapshot(&root.path().join("project"));
        for _ in 0..2 {
            synchronize(&session(root.path(), false), false)
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
            cache_dir: Some("cache".into()),
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
        None,
    )
    .await
    .unwrap();
    let resolved = initialize::discover(&selected_session, move |mut scope| async move {
        catalog
            .resolve_selector(&mut scope, selector.0, Default::default())
            .await
    })
    .await
    .unwrap();
    assert_eq!(resolved.id.to_string(), "Right001");
    request.assert_async().await;
    resolution.assert_async().await;
    assert_eq!(super::super::tests::snapshot(root.path()), before);
}

#[tokio::test]
async fn identified_cli_file_preserves_supplied_bytes_and_rejects_unverified_batches() {
    use std::io::Write;
    let mut archive = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    archive
        .start_file("fabric.mod.json", zip::write::SimpleFileOptions::default())
        .unwrap();
    archive
        .write_all(br#"{"schemaVersion":1,"id":"renderer","version":"1"}"#)
        .unwrap();
    let bytes = archive.finish().unwrap().into_inner();
    for outcome in ["exact", "unknown", "mismatch", "changed-resolution"] {
        let root = tempfile::tempdir().unwrap();
        fixture(root.path()).await;
        fs::write(root.path().join("renamed.jar"), &bytes).unwrap();
        let mut server = mockito::Server::new_async().await;
        let mut version = json!({
            "id":"RootVer1","project_id":"Root0001","game_versions":["1.21.1"],"loaders":["fabric"],
            "files":[{"filename":"renderer.jar","primary":true,"size":bytes.len(),"hashes":{
                "sha1":empack_core::digest::ExpectedDigest::Sha1(sha1::Sha1::digest(&bytes).into()).hex(),
                "sha512":empack_core::digest::ExpectedDigest::Sha512(Sha512::digest(&bytes).into()).hex()},
                "url":"https://example.invalid/must-not-download.jar"}],"dependencies":[],
            "date_published":"2026-01-01T00:00:00Z","status":"listed","version_type":"release"
        });
        if outcome == "mismatch" {
            version["files"][0]["hashes"]["sha512"] = json!("00".repeat(64));
        }
        server
            .mock("GET", mockito::Matcher::Regex("^/version_file/.*".into()))
            .with_status(if outcome == "unknown" { 404 } else { 200 })
            .with_body(version.to_string())
            .create_async()
            .await;
        if outcome == "changed-resolution" {
            version["files"][0]["hashes"]["sha512"] = json!("00".repeat(64));
        }
        server
            .mock("GET", "/version/RootVer1")
            .with_body(version.to_string())
            .create_async()
            .await;
        server.mock("GET", "/project/Root0001")
            .with_body(json!({"id":"Root0001","slug":"renderer","title":"Renderer","project_type":"mod","loaders":["fabric"]}).to_string())
            .create_async().await;
        let selected = || AddOptions {
            inputs: vec!["renamed.jar".into()],
            force: false,
            platform: Some(SearchPlatform::Modrinth),
            kind: None,
            version_id: None,
            file_id: None,
            file_plan: None,
            download_as_local: false,
            continue_independent: false,
        };
        let before = super::super::tests::snapshot(root.path());
        let result = add_with_catalog(
            &session(root.path(), true),
            selected(),
            ProviderCatalog::for_loopback_tests(&server.url(), None),
        )
        .await;
        assert_eq!(result.is_ok(), outcome == "exact", "{outcome}: {result:?}");
        assert_eq!(super::super::tests::snapshot(root.path()), before);
        let result = add_with_catalog(
            &session(root.path(), false),
            selected(),
            ProviderCatalog::for_loopback_tests(&server.url(), None),
        )
        .await;
        if outcome != "exact" {
            assert!(result.is_err());
            assert_eq!(super::super::tests::snapshot(root.path()), before);
            continue;
        }
        result.unwrap();
        assert_eq!(
            fs::read(root.path().join("project/pack/mods/renderer.jar")).unwrap(),
            bytes
        );
        let resolved = project(root.path());
        assert!(matches!(
            resolved.intent().roots.values().next().unwrap().source,
            empack_core::model::SourceIntent::Provider(_)
        ));
        assert!(matches!(
            resolved.intent().roots.values().next().unwrap().version,
            empack_core::model::VersionIntent::Exact(_)
        ));
        let before = super::super::tests::snapshot(&root.path().join("project"));
        for _ in 0..2 {
            synchronize(&session(root.path(), false), false)
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
async fn provider_adoption_uses_observed_pin_and_verifies_bytes_without_upgrading_or_rewriting_them()
 {
    use empack_core::{digest::ExpectedDigest, model::*};
    let root = tempfile::tempdir().unwrap();
    fixture(root.path()).await;
    let mut server = mockito::Server::new_async().await;
    for id in ["renderer", "Root0001"] {
        server.mock("GET", format!("/project/{id}").as_str())
            .with_body(json!({"id":"Root0001","slug":"renderer","title":"Renderer","project_type":"mod","loaders":["fabric"]}).to_string())
            .create_async().await;
    }
    for (pin, bytes) in [("RootVer1", b"payload"), ("RootVer2", b"updated")] {
        server.mock("GET", format!("/version/{pin}").as_str()).with_body(json!({
            "id":pin,"project_id":"Root0001","game_versions":["1.21.1"],"loaders":["fabric"],
            "files":[{"filename":"renderer.jar","primary":true,"size":7,"hashes":{"sha512":ExpectedDigest::Sha512(Sha512::digest(bytes).into()).hex()},"url":"https://example.invalid/renderer.jar"}],
            "dependencies":[],"date_published":"2026-01-01T00:00:00Z","status":"listed","version_type":"release"
        }).to_string()).create_async().await;
    }
    let latest = server
        .mock("GET", "/project/Root0001/version")
        .match_query(mockito::Matcher::Any)
        .expect(0)
        .create_async()
        .await;
    add_with_catalog(
        &session(root.path(), false),
        options("renderer"),
        ProviderCatalog::for_loopback_tests(&server.url(), None),
    )
    .await
    .unwrap();
    let recorded = project(root.path());
    let key = recorded.intent().roots.keys().next().unwrap().clone();
    let mut intent = recorded.intent().clone();
    intent.roots.get_mut(&key).unwrap().version = VersionIntent::FollowCompatible;
    fs::write(
        root.path().join("project/empack.yml"),
        DocumentCodec.encode_intent(&intent).unwrap(),
    )
    .unwrap();
    synchronize(&session(root.path(), false), false)
        .await
        .unwrap();
    // Reference-only addition does not install packwiz metadata. Simulate an external
    // tool selecting a different exact version and materializing its payload.
    fs::create_dir_all(root.path().join("project/pack/mods")).unwrap();
    let metadata = root.path().join("project/pack/mods/renderer.pw.toml");
    let wire = format!(
        "name='Renderer'\nfilename='renderer.jar'\nside='both'\n[download]\nurl='https://example.invalid/renderer.jar'\nhash-format='sha512'\nhash='{}'\n[update.modrinth]\nmod-id='Root0001'\nversion='RootVer2'\n",
        ExpectedDigest::Sha512(Sha512::digest(b"updated").into()).hex()
    );
    fs::write(&metadata, wire).unwrap();
    let installed = root.path().join("project/pack/mods/renderer.jar");
    fs::write(&installed, b"corrupt").unwrap();
    let services = || dependencies::AdditionServices {
        catalog: ProviderCatalog::for_loopback_tests(&server.url(), None),
        transport: HttpAcquisition::for_loopback_tests(),
        files: DirectFileLimits::default(),
    };
    let before = super::super::tests::snapshot(root.path());
    assert!(
        update::adopt_with_services(
            &session(root.path(), false),
            vec![key.as_str().into()],
            services()
        )
        .await
        .is_err()
    );
    assert_eq!(super::super::tests::snapshot(root.path()), before);
    fs::write(&installed, b"updated").unwrap();
    // A selected installation cannot override an authored exact pin.
    let intent_path = root.path().join("project/empack.yml");
    let lock_path = root.path().join("project/empack.lock");
    let intent_bytes = fs::read(&intent_path).unwrap();
    let lock_bytes = fs::read(&lock_path).unwrap();
    fs::write(
        &intent_path,
        DocumentCodec.encode_intent(recorded.intent()).unwrap(),
    )
    .unwrap();
    fs::write(&lock_path, DocumentCodec.encode_lock(&recorded).unwrap()).unwrap();
    let pinned = super::super::tests::snapshot(root.path());
    assert!(
        update::adopt_with_services(
            &session(root.path(), false),
            vec![key.as_str().into()],
            services()
        )
        .await
        .is_err()
    );
    assert_eq!(super::super::tests::snapshot(root.path()), pinned);
    fs::write(&intent_path, intent_bytes).unwrap();
    fs::write(&lock_path, lock_bytes).unwrap();
    let metadata_bytes = fs::read_to_string(&metadata).unwrap();
    fs::write(&metadata, metadata_bytes.replace("Root0001", "Other001")).unwrap();
    let foreign = super::super::tests::snapshot(root.path());
    let error = update::adopt_with_services(
        &session(root.path(), false),
        vec![key.as_str().into()],
        services(),
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("another provider identity"));
    assert_eq!(super::super::tests::snapshot(root.path()), foreign);
    fs::write(&metadata, metadata_bytes).unwrap();
    let before = super::super::tests::snapshot(root.path());
    update::adopt_with_services(
        &session(root.path(), true),
        vec![key.as_str().into()],
        services(),
    )
    .await
    .unwrap();
    assert_eq!(super::super::tests::snapshot(root.path()), before);
    update::adopt_with_services(
        &session(root.path(), false),
        vec![key.as_str().into()],
        services(),
    )
    .await
    .unwrap();
    assert_eq!(fs::read(&installed).unwrap(), b"updated");
    assert_eq!(
        fs::read(&metadata).unwrap(),
        before[metadata.strip_prefix(root.path()).unwrap()]
    );
    let adopted = project(root.path());
    assert_eq!(adopted.intent(), &intent);
    assert_eq!(
        adopted.lock().dependencies[&key]
            .selected
            .as_ref()
            .unwrap()
            .selection,
        PinSelector::ModrinthVersion(ModrinthVersionId::parse("RootVer2").unwrap())
    );
    let before = super::super::tests::snapshot(&root.path().join("project"));
    for _ in 0..2 {
        synchronize(&session(root.path(), false), false)
            .await
            .unwrap();
    }
    assert_eq!(
        super::super::tests::snapshot(&root.path().join("project")),
        before
    );
    // The same observed selection establishes a missing first lock; no latest query is allowed.
    fs::remove_file(root.path().join("project/empack.lock")).unwrap();
    let missing = super::super::tests::snapshot(root.path());
    update::adopt_with_services(
        &session(root.path(), true),
        vec![key.as_str().into()],
        services(),
    )
    .await
    .unwrap();
    assert_eq!(super::super::tests::snapshot(root.path()), missing);
    update::adopt_with_services(
        &session(root.path(), false),
        vec![key.as_str().into()],
        services(),
    )
    .await
    .unwrap();
    assert_eq!(project(root.path()).intent(), &intent);
    assert_eq!(
        project(root.path()).lock().dependencies[&key].selected,
        adopted.lock().dependencies[&key].selected
    );
    let restored = super::super::tests::snapshot(&root.path().join("project"));
    for _ in 0..2 {
        synchronize(&session(root.path(), false), false)
            .await
            .unwrap();
    }
    assert_eq!(
        super::super::tests::snapshot(&root.path().join("project")),
        restored
    );
    latest.assert_async().await;
}

#[tokio::test]
async fn supplied_provider_zip_discovers_kind_before_requesting_a_type_choice() {
    use std::io::Write;
    let root = tempfile::tempdir().unwrap();
    fixture(root.path()).await;
    let mut archive = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    archive
        .start_file("pack.mcmeta", zip::write::SimpleFileOptions::default())
        .unwrap();
    archive
        .write_all(br#"{"pack":{"pack_format":34,"description":"assets"}}"#)
        .unwrap();
    let bytes = archive.finish().unwrap().into_inner();
    fs::write(root.path().join("renamed.zip"), &bytes).unwrap();
    let mut server = mockito::Server::new_async().await;
    let version = json!({
        "id":"AssetVer","project_id":"Assets01","game_versions":["1.21.1"],"loaders":["minecraft"],
        "files":[{"filename":"assets.zip","primary":true,"size":bytes.len(),"hashes":{"sha512":empack_core::digest::ExpectedDigest::Sha512(Sha512::digest(&bytes).into()).hex()},"url":"https://example.invalid/assets.zip"}],
        "dependencies":[],"date_published":"2026-01-01T00:00:00Z","status":"listed","version_type":"release"
    });
    server
        .mock("GET", mockito::Matcher::Regex("^/version_file/.*".into()))
        .with_body(version.to_string())
        .create_async()
        .await;
    server
        .mock("GET", "/version/AssetVer")
        .with_body(version.to_string())
        .create_async()
        .await;
    server.mock("GET", "/project/Assets01").with_body(json!({"id":"Assets01","slug":"assets","title":"Assets","project_type":"resourcepack","loaders":["minecraft"]}).to_string()).create_async().await;
    let selected = || AddOptions {
        inputs: vec!["renamed.zip".into()],
        force: false,
        platform: Some(SearchPlatform::Modrinth),
        kind: None,
        version_id: None,
        file_id: None,
        file_plan: None,
        download_as_local: false,
        continue_independent: false,
    };
    let before = super::super::tests::snapshot(root.path());
    add_with_catalog(
        &session(root.path(), true),
        selected(),
        ProviderCatalog::for_loopback_tests(&server.url(), None),
    )
    .await
    .unwrap();
    assert_eq!(super::super::tests::snapshot(root.path()), before);
    add_with_catalog(
        &session(root.path(), false),
        selected(),
        ProviderCatalog::for_loopback_tests(&server.url(), None),
    )
    .await
    .unwrap();
    let resolved = project(root.path());
    let root_entry = resolved.intent().roots.values().next().unwrap();
    assert_eq!(root_entry.kind, ContentKind::ResourcePack);
    assert_eq!(root_entry.requirements.server, Requirement::Unsupported);
    assert_eq!(
        fs::read(root.path().join("project/pack/resourcepacks/assets.zip")).unwrap(),
        bytes
    );
    let before = super::super::tests::snapshot(&root.path().join("project"));
    for _ in 0..2 {
        synchronize(&session(root.path(), false), false)
            .await
            .unwrap();
    }
    assert_eq!(
        super::super::tests::snapshot(&root.path().join("project")),
        before
    );
    let mut wrong = selected();
    wrong.kind = Some(CliProjectType::Mod);
    assert!(
        add_with_catalog(
            &session(root.path(), false),
            wrong,
            ProviderCatalog::for_loopback_tests(&server.url(), None)
        )
        .await
        .is_err()
    );
    assert_eq!(
        super::super::tests::snapshot(&root.path().join("project")),
        before
    );
}

#[tokio::test]
async fn provider_file_plan_preserves_companions_destinations_and_optional_choices() {
    let mut server = mockito::Server::new_async().await;
    server.mock("GET", "/project/renderer")
        .with_body(json!({"id":"Root0001","slug":"renderer","title":"Renderer","project_type":"mod","loaders":["fabric"]}).to_string()).create_async().await;
    server.mock("GET", "/project/Root0001")
        .with_body(json!({"id":"Root0001","slug":"renderer","title":"Renderer","project_type":"mod","loaders":["fabric"]}).to_string()).create_async().await;
    let file = |name: &str, primary: bool, role: Option<&str>| {
        json!({
            "filename":name,"primary":primary,"size":7,"file_type":role,
            "hashes":{"sha512":empack_core::digest::ExpectedDigest::Sha512(Sha512::digest(b"payload").into()).hex()},
            "url":format!("https://example.invalid/{name}")
        })
    };
    server.mock("GET", "/version/RootVer1").with_body(json!({
        "id":"RootVer1","project_id":"Root0001","game_versions":["1.21.1"],"loaders":["fabric"],
        "files":[file("renderer.jar",true,None),file("assets.zip",false,Some("required-resource-pack")),file("extras.zip",false,Some("optional-resource-pack"))],
        "dependencies":[],"date_published":"2026-01-01T00:00:00Z","status":"listed","version_type":"release"
    }).to_string()).create_async().await;
    let root = tempfile::tempdir().unwrap();
    fixture(root.path()).await;
    let environment = json!({"client":"required","server":"unsupported"});
    let placement = |path: &str, environment: serde_json::Value| {
        json!([{
            "destination":path,"layer":"common","environment":environment
        }])
    };
    let mut plan = json!({"schema":1,"environment":environment,"files":{
        "renderer.jar":placement("mods/renamed.jar",environment.clone()),
        "assets.zip":placement("resourcepacks/required.zip",environment.clone()),
        "extras.zip":placement("resourcepacks/extra.zip",json!({"client":{"optional":"extra-art","default-enabled":false,"description":"Extra artwork"},"server":"unsupported"}))
    }});
    let selected = || {
        let mut options = options("renderer");
        options.file_plan = Some("files.yml".into());
        options
    };
    // Missing required companions fail the whole request before publication.
    let companion = plan["files"]
        .as_object_mut()
        .unwrap()
        .remove("assets.zip")
        .unwrap();
    fs::write(root.path().join("files.yml"), plan.to_string()).unwrap();
    let before = super::super::tests::snapshot(root.path());
    let error = add_with_catalog(
        &session(root.path(), false),
        selected(),
        ProviderCatalog::for_loopback_tests(&server.url(), None),
    )
    .await
    .unwrap_err();
    assert!(format!("{error:#}").contains("required companion"));
    assert_eq!(super::super::tests::snapshot(root.path()), before);
    plan["files"]["assets.zip"] = companion;
    fs::write(root.path().join("files.yml"), plan.to_string()).unwrap();
    let before = super::super::tests::snapshot(root.path());
    add_with_catalog(
        &session(root.path(), true),
        selected(),
        ProviderCatalog::for_loopback_tests(&server.url(), None),
    )
    .await
    .unwrap();
    assert_eq!(super::super::tests::snapshot(root.path()), before);
    add_with_catalog(
        &session(root.path(), false),
        selected(),
        ProviderCatalog::for_loopback_tests(&server.url(), None),
    )
    .await
    .unwrap();
    let resolved = project(root.path());
    let record = resolved.lock().dependencies.values().next().unwrap();
    assert_eq!(record.files.as_slice().len(), 3);
    let extra = record
        .files
        .as_slice()
        .iter()
        .find(|file| file.slot.as_str() == "extras.zip")
        .unwrap();
    let placement = &extra.placements.as_slice()[0];
    assert_eq!(
        placement.destination.relative().as_str(),
        "resourcepacks/extra.zip"
    );
    assert_eq!(placement.requirements.server, Requirement::Unsupported);
    let Requirement::Optional(choice) = &placement.requirements.client else {
        panic!("lost optional requirement")
    };
    assert_eq!(choice.key.as_str(), "extra-art");
    assert!(!choice.default_enabled);
    for _ in 0..2 {
        let before = super::super::tests::snapshot(&root.path().join("project"));
        synchronize(&session(root.path(), false), false)
            .await
            .unwrap();
        assert_eq!(
            super::super::tests::snapshot(&root.path().join("project")),
            before
        );
    }
}

#[tokio::test]
async fn file_plans_reject_unbounded_unsafe_and_unrecognized_selections() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path()).await;
    let path = root.path().join("files.yml");
    for bytes in [
        b"schema: 1\nfiles: {}\nenvironment: {client: required, server: unsupported}\ntypo: true"
            .to_vec(),
        vec![b'x'; (1 << 20) + 1],
    ] {
        fs::write(&path, bytes).unwrap();
        let before = super::super::tests::snapshot(root.path());
        assert!(
            file_plan::read(&session(root.path(), false), path.clone())
                .await
                .is_err()
        );
        assert_eq!(super::super::tests::snapshot(root.path()), before);
    }
    #[cfg(unix)]
    {
        let linked = root.path().join("linked.yml");
        std::os::unix::fs::symlink(&path, &linked).unwrap();
        assert!(
            file_plan::read(&session(root.path(), false), linked)
                .await
                .is_err()
        );
    }
    let mut selected = options("renderer");
    selected.file_plan = Some("files.yml".into());
    selected.inputs.push("other".into());
    assert!(selected.pin().is_err());
}

#[tokio::test]
async fn world_archive_members_remain_one_identity_across_commands() {
    use empack_core::{
        model::{ContentKind, SourceIntent},
        path::{PathSyntax, PortableRelPath},
    };
    use std::io::{Read, Write};
    let root = tempfile::tempdir().unwrap();
    fixture(root.path()).await;
    let mut intent = project(root.path()).intent().clone();
    intent.layout.insert(
        ContentKind::World,
        PortableRelPath::parse("saves", PathSyntax::ProjectContent).unwrap(),
    );
    fs::write(
        root.path().join("project/empack.yml"),
        DocumentCodec.encode_intent(&intent).unwrap(),
    )
    .unwrap();
    synchronize(&session(root.path(), false), false)
        .await
        .unwrap();
    let write_archive = |members: &[(&str, &[u8])]| {
        let mut archive = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (name, bytes) in members {
            archive
                .start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            archive.write_all(bytes).unwrap();
        }
        fs::write(
            root.path().join("adventure.zip"),
            archive.finish().unwrap().into_inner(),
        )
        .unwrap();
    };
    let selected = || AddOptions {
        inputs: vec!["adventure.zip".into()],
        force: false,
        platform: None,
        kind: Some(CliProjectType::World),
        version_id: None,
        file_id: None,
        file_plan: None,
        download_as_local: false,
        continue_independent: false,
    };
    for bad in [
        vec![
            ("Wrapped/level.dat", b"world".as_slice()),
            ("outside.txt", b"outside".as_slice()),
        ],
        vec![
            ("first/level.dat", b"a".as_slice()),
            ("second/level.dat", b"b".as_slice()),
        ],
        vec![("level.dat", b"".as_slice())],
    ] {
        write_archive(&bad);
        let before = super::super::tests::snapshot(root.path());
        assert!(add(&session(root.path(), false), selected()).await.is_err());
        assert_eq!(super::super::tests::snapshot(root.path()), before);
    }
    write_archive(&[
        ("Wrapped/level.dat", b"world"),
        ("Wrapped/region/r.0.0.mca", b"region"),
    ]);
    let before = super::super::tests::snapshot(root.path());
    add(&session(root.path(), true), selected()).await.unwrap();
    assert_eq!(super::super::tests::snapshot(root.path()), before);
    add(&session(root.path(), false), selected()).await.unwrap();
    let resolved = project(root.path());
    assert_eq!(resolved.intent().roots.len(), 1);
    assert!(
        matches!(&resolved.intent().roots.values().next().unwrap().source,SourceIntent::LocalFiles(members) if members.len()==2)
    );
    let world = root.path().join("project/pack/saves/adventure");
    assert_eq!(fs::read(world.join("region/r.0.0.mca")).unwrap(), b"region");
    assert_eq!(
        resolved
            .lock()
            .dependencies
            .values()
            .next()
            .unwrap()
            .files
            .as_slice()
            .len(),
        2
    );
    for _ in 0..2 {
        let before = super::super::tests::snapshot(&root.path().join("project"));
        synchronize(&session(root.path(), false), false)
            .await
            .unwrap();
        assert_eq!(
            super::super::tests::snapshot(&root.path().join("project")),
            before
        );
    }
    build(
        &session(root.path(), false),
        &crate::application::BuildArgs {
            targets: vec!["client-full".into()],
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let artifact = fs::read_dir(root.path().join("project/dist"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.extension().is_some_and(|ext| ext == "zip"))
        .unwrap();
    let mut archive = zip::ZipArchive::new(fs::File::open(artifact).unwrap()).unwrap();
    let mut bytes = vec![];
    archive
        .by_name(".minecraft/saves/adventure/region/r.0.0.mca")
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    assert_eq!(bytes, b"region");
    fs::write(world.join("region/r.0.0.mca"), b"changed-region").unwrap();
    let before = super::super::tests::snapshot(root.path());
    assert!(
        synchronize(&session(root.path(), false), false)
            .await
            .is_err()
    );
    assert_eq!(super::super::tests::snapshot(root.path()), before);
    adopt(&session(root.path(), false), vec!["adventure".into()])
        .await
        .unwrap();
    synchronize(&session(root.path(), false), false)
        .await
        .unwrap();
    // Losing the lock does not authorize claiming existing files as new installations.
    let saved_lock = fs::read(root.path().join("project/empack.lock")).unwrap();
    fs::remove_file(root.path().join("project/empack.lock")).unwrap();
    let missing_lock = super::super::tests::snapshot(root.path());
    assert!(
        synchronize(&session(root.path(), false), false)
            .await
            .is_err()
    );
    assert_eq!(super::super::tests::snapshot(root.path()), missing_lock);
    fs::write(root.path().join("project/empack.lock"), saved_lock).unwrap();
    let rebuilt = project(root.path());
    let mut invalid = rebuilt.lock().clone();
    let dependency = invalid.dependencies.values_mut().next().unwrap();
    dependency.files =
        empack_core::model::NonEmpty::new(vec![dependency.files.as_slice()[0].clone()]).unwrap();
    assert!(
        empack_core::model::ResolvedProject::validate(
            rebuilt.intent().clone(),
            invalid,
            rebuilt.lock().intent_revision
        )
        .is_err()
    );
    let mut invalid = rebuilt.lock().clone();
    let dependency = invalid.dependencies.values_mut().next().unwrap();
    let mut members = dependency.files.clone().into_vec();
    members[0].acquisition = empack_core::model::AcquisitionSpec::Local(
        PortableRelPath::parse("pack/unrelated", PathSyntax::ProjectContent).unwrap(),
    );
    dependency.files = empack_core::model::NonEmpty::new(members).unwrap();
    assert!(
        empack_core::model::ResolvedProject::validate(
            rebuilt.intent().clone(),
            invalid,
            rebuilt.lock().intent_revision
        )
        .is_err()
    );
    crate::application::execute_command_with_session(
        crate::application::Commands::Remove {
            mods: vec!["adventure".into()],
            deps: false,
            forget: true,
            acknowledge_unknown: false,
        },
        &session(root.path(), false),
    )
    .await
    .unwrap();
    assert!(project(root.path()).intent().roots.is_empty());
    update(&session(root.path(), false), vec!["adventure".into()])
        .await
        .unwrap();
    fs::write(world.join("region/r.0.0.mca"), b"demoted edit").unwrap();
    adopt(&session(root.path(), false), vec!["adventure".into()])
        .await
        .unwrap();
    assert!(project(root.path()).intent().roots.is_empty());
    let retained = super::super::tests::snapshot(&root.path().join("project"));
    for _ in 0..2 {
        synchronize(&session(root.path(), false), false)
            .await
            .unwrap();
    }
    assert!(super::super::tests::snapshot(&root.path().join("project")) == retained);
    fs::write(world.join("untracked.txt"), b"keep me").unwrap();
    crate::application::execute_command_with_session(
        crate::application::Commands::Remove {
            mods: vec!["adventure".into()],
            deps: false,
            forget: false,
            acknowledge_unknown: true,
        },
        &session(root.path(), false),
    )
    .await
    .unwrap();
    assert!(!world.join("level.dat").exists());
    assert!(!world.join("region/r.0.0.mca").exists());
    assert_eq!(fs::read(world.join("untracked.txt")).unwrap(), b"keep me");
}

#[tokio::test]
async fn provider_world_cannot_be_mistaken_for_an_installed_zip() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("GET", "/mods/42")
        .with_body(
            json!({"data":{"id":42,"gameId":432,"name":"World","slug":"world","classId":17}})
                .to_string(),
        )
        .create_async()
        .await;
    let root = tempfile::tempdir().unwrap();
    fixture(root.path()).await;
    let mut selected = options("42");
    selected.platform = Some(SearchPlatform::Curseforge);
    selected.version_id = None;
    let before = super::super::tests::snapshot(root.path());
    let error = add_with_catalog(
        &session(root.path(), false),
        selected,
        ProviderCatalog::for_loopback_tests(&server.url(), Some("test-key".into())),
    )
    .await
    .unwrap_err();
    assert!(format!("{error:#}").contains("world archives require member interpretation"));
    assert_eq!(super::super::tests::snapshot(root.path()), before);
}

#[tokio::test]
async fn multiple_worlds_and_large_member_updates_share_admission_without_splitting_identity() {
    use empack_core::path::{PathSyntax, PortableRelPath};
    use std::io::Write;
    let root = tempfile::tempdir().unwrap();
    fixture(root.path()).await;
    let mut intent = project(root.path()).intent().clone();
    intent.layout.insert(
        ContentKind::World,
        PortableRelPath::parse("saves", PathSyntax::ProjectContent).unwrap(),
    );
    fs::write(
        root.path().join("project/empack.yml"),
        DocumentCodec.encode_intent(&intent).unwrap(),
    )
    .unwrap();
    synchronize(&session(root.path(), false), false)
        .await
        .unwrap();
    let mut archive = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    archive
        .start_file("level.dat", zip::write::SimpleFileOptions::default())
        .unwrap();
    archive.write_all(b"world").unwrap();
    for i in 0..130 {
        archive
            .start_file(
                format!("region/r.{i}.0.mca"),
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        archive.write_all(b"region").unwrap();
    }
    let bytes = archive.finish().unwrap().into_inner();
    for name in ["first.zip", "second.zip"] {
        fs::write(root.path().join(name), &bytes).unwrap();
    }
    add(
        &session(root.path(), false),
        AddOptions {
            inputs: vec!["first.zip".into(), "second.zip".into()],
            force: false,
            platform: None,
            kind: Some(CliProjectType::World),
            version_id: None,
            file_id: None,
            file_plan: None,
            download_as_local: false,
            continue_independent: false,
        },
    )
    .await
    .unwrap();
    assert_eq!(project(root.path()).lock().dependencies.len(), 2);
    let before = super::super::tests::snapshot(&root.path().join("project"));
    update(&session(root.path(), false), vec!["first".into()])
        .await
        .unwrap();
    synchronize(&session(root.path(), false), false)
        .await
        .unwrap();
    assert_eq!(
        super::super::tests::snapshot(&root.path().join("project")),
        before
    );
    assert!(
        project(root.path())
            .lock()
            .dependencies
            .values()
            .all(|dependency| dependency.files.as_slice().len() == 131)
    );
}

#[tokio::test]
async fn url_adoption_preserves_origins_and_side_placements_without_remote_acquisition() {
    use crate::engine::addition::{
        DirectFileInput, DirectFileSource, FileEvidence, FileKindPolicy,
    };
    use empack_core::{
        model::*,
        path::InstallDestination,
        requirements::{Requirement, Requirements},
    };
    let root = tempfile::tempdir().unwrap();
    fixture(root.path()).await;
    let mut server = mockito::Server::new_async().await;
    let download = server
        .mock("GET", "/original")
        .with_body("original")
        .expect(1)
        .create_async()
        .await;
    let services = || dependencies::AdditionServices {
        catalog: ProviderCatalog::for_loopback_tests(&server.url(), None),
        transport: HttpAcquisition::for_loopback_tests(),
        files: DirectFileLimits::default(),
    };
    let key = DependencyKey::parse("url-config").unwrap();
    let origins = NonEmpty::new(vec!["https://example.invalid/original".into()]).unwrap();
    let requirements = Requirements {
        client: Requirement::Required,
        server: Requirement::Unsupported,
    };
    dependencies::add_with_services(
        &session(root.path(), false),
        NonEmpty::new(vec![AddHostInput::File(DirectFileInput {
            role: crate::engine::addition::FileInputRole::Primary,
            key: key.clone(),
            title: "URL config".into(),
            source: DirectFileSource::Download {
                origins: origins.clone(),
                alternatives: NonEmpty::new(vec![format!("{}/original", server.url())]).unwrap(),
            },
            evidence: FileEvidence::AcceptObserved,
            kind: ContentKind::Config,
            kind_policy: FileKindPolicy::AcceptUnrecognized,
            requirements: requirements.clone(),
            placements: NonEmpty::new(vec![Placement {
                destination: InstallDestination::parse("config/custom.txt").unwrap(),
                layer: ContentLayer::Client,
                requirements,
            }])
            .unwrap(),
        })])
        .unwrap(),
        ReleasePolicy::PreferStable,
        SourceEvidencePolicy::Compatibility,
        ExistingDependencyPolicy::RejectExisting,
        services(),
    )
    .await
    .unwrap();
    let payload = root
        .path()
        .join("project/overrides/client/config/custom.txt");
    assert_eq!(fs::read(&payload).unwrap(), b"original");
    // Imported URL files use their own stable role; adoption must not rename it to primary.
    let recorded = project(root.path());
    let mut intent = recorded.intent().clone();
    let mut lock = recorded.lock().clone();
    let mut files = lock.dependencies[&key].files.as_slice().to_vec();
    files[0].slot = FileSlot::parse("imported-content").unwrap();
    intent.roots.get_mut(&key).unwrap().placement = PlacementIntent::ByFile(BTreeMap::from([(
        files[0].slot.clone(),
        files[0].placements.clone(),
    )]));
    lock.dependencies.get_mut(&key).unwrap().files = NonEmpty::new(files).unwrap();
    let wire = DocumentCodec.encode_intent(&intent).unwrap();
    let source = DocumentCodec.decode_intent(&wire, "imported role").unwrap();
    lock.intent_revision = source.semantic_revision();
    let recorded = ResolvedProject::validate(intent, lock, source.semantic_revision()).unwrap();
    fs::write(root.path().join("project/empack.yml"), wire).unwrap();
    fs::write(
        root.path().join("project/empack.lock"),
        DocumentCodec.encode_lock(&recorded).unwrap(),
    )
    .unwrap();
    let before = project(root.path());
    fs::write(&payload, b"changed locally").unwrap();
    let changed = super::super::tests::snapshot(root.path());
    update::adopt_with_services(
        &session(root.path(), true),
        vec![key.as_str().into()],
        services(),
    )
    .await
    .unwrap();
    assert_eq!(super::super::tests::snapshot(root.path()), changed);
    update::adopt_with_services(
        &session(root.path(), false),
        vec![key.as_str().into()],
        services(),
    )
    .await
    .unwrap();
    let after = project(root.path());
    assert_eq!(after.intent(), before.intent());
    let dependency = &after.lock().dependencies[&key];
    assert_eq!(dependency.identity, ResolvedIdentity::Url(key.clone()));
    assert_eq!(
        dependency.files.as_slice()[0].slot.as_str(),
        "imported-content"
    );
    assert_eq!(
        dependency.files.as_slice()[0].acquisition,
        AcquisitionSpec::Url(origins)
    );
    assert_eq!(
        dependency.files.as_slice()[0].placements,
        before.lock().dependencies[&key].files.as_slice()[0].placements
    );
    assert_ne!(
        dependency.files.as_slice()[0].expected,
        before.lock().dependencies[&key].files.as_slice()[0].expected
    );
    assert_eq!(fs::read(&payload).unwrap(), b"changed locally");
    let adopted = super::super::tests::snapshot(&root.path().join("project"));
    for _ in 0..2 {
        synchronize(&session(root.path(), false), false)
            .await
            .unwrap();
    }
    assert_eq!(
        super::super::tests::snapshot(&root.path().join("project")),
        adopted
    );
    // Explicit source pins still bind adoption; accepting local drift cannot silently relax them.
    let mut intent = after.intent().clone();
    let mut lock = after.lock().clone();
    let digest = empack_core::digest::ExpectedDigest::Sha256(
        *dependency.files.as_slice()[0]
            .expected
            .accepted_observation
            .as_ref()
            .unwrap()
            .bytes(),
    );
    let digests = empack_core::digest::DigestSet::new(vec![digest]).unwrap();
    intent.roots.get_mut(&key).unwrap().version = VersionIntent::ContentPinned(digests.clone());
    let mut files = lock.dependencies[&key].files.as_slice().to_vec();
    files[0].expected.digests = Some(digests);
    lock.dependencies.get_mut(&key).unwrap().files = NonEmpty::new(files).unwrap();
    let wire = DocumentCodec.encode_intent(&intent).unwrap();
    let decoded = DocumentCodec
        .decode_intent(&wire, "pinned fixture")
        .unwrap();
    lock.intent_revision = decoded.semantic_revision();
    let pinned = ResolvedProject::validate(intent, lock, decoded.semantic_revision()).unwrap();
    fs::write(root.path().join("project/empack.yml"), wire).unwrap();
    fs::write(
        root.path().join("project/empack.lock"),
        DocumentCodec.encode_lock(&pinned).unwrap(),
    )
    .unwrap();
    fs::write(&payload, b"conflicts with authored pin").unwrap();
    let before_failure = super::super::tests::snapshot(root.path());
    assert!(
        update::adopt_with_services(
            &session(root.path(), false),
            vec![key.as_str().into()],
            services()
        )
        .await
        .is_err()
    );
    assert!(super::super::tests::snapshot(root.path()) == before_failure);
    #[cfg(unix)]
    {
        let outside = root.path().join("outside");
        fs::write(&outside, b"outside sentinel").unwrap();
        fs::remove_file(&payload).unwrap();
        std::os::unix::fs::symlink(&outside, &payload).unwrap();
        assert!(
            update::adopt_with_services(
                &session(root.path(), false),
                vec![key.as_str().into()],
                services()
            )
            .await
            .is_err()
        );
        assert_eq!(fs::read(&outside).unwrap(), b"outside sentinel");
        assert!(
            fs::symlink_metadata(&payload)
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }
    download.assert_async().await;
}

#[tokio::test]
async fn provider_side_adoption_identifies_exact_bytes_and_keeps_every_placement() {
    use empack_core::{digest::ExpectedDigest, model::*};
    let root = tempfile::tempdir().unwrap();
    fixture(root.path()).await;
    let mut server = mockito::Server::new_async().await;
    for id in ["renderer", "Root0001", "Other001"] {
        let canonical = if id == "Other001" {
            "Other001"
        } else {
            "Root0001"
        };
        server.mock("GET", format!("/project/{id}").as_str())
            .with_body(json!({"id":canonical,"slug":"renderer","title":"Renderer","project_type":"mod","loaders":["fabric"]}).to_string()).create_async().await;
    }
    for (pin, bytes, owner) in [
        ("RootVer1", "original", "Root0001"),
        ("RootVer2", "changed", "Root0001"),
        ("OtherVer", "unrelated", "Other001"),
    ] {
        let hash = ExpectedDigest::Sha512(Sha512::digest(bytes.as_bytes()).into()).hex();
        let version = json!({"id":pin,"project_id":owner,"game_versions":["1.21.1"],"loaders":["fabric"],
            "files":[{"filename":"renderer.jar","primary":true,"size":bytes.len(),"hashes":{"sha512":hash},"url":"https://example.invalid/renderer.jar"}],
            "dependencies":[],"date_published":"2026-01-01T00:00:00Z","status":"listed","version_type":"release"});
        server
            .mock("GET", format!("/version/{pin}").as_str())
            .with_body(version.to_string())
            .create_async()
            .await;
        server
            .mock("GET", format!("/version_file/{hash}").as_str())
            .match_query(mockito::Matcher::Any)
            .with_body(version.to_string())
            .create_async()
            .await;
    }
    let latest = server
        .mock("GET", "/project/Root0001/version")
        .match_query(mockito::Matcher::Any)
        .expect(0)
        .create_async()
        .await;
    let environment = json!({"client":"required","server":"unsupported"});
    fs::write(
        root.path().join("files.yml"),
        json!({"schema":1,"environment":environment,"files":{"renderer.jar":[
            {"destination":"mods/selected.jar","layer":"client","environment":environment},
            {"destination":"mods/copied.jar","layer":"client","environment":environment}
        ]}})
        .to_string(),
    )
    .unwrap();
    let mut selected = options("renderer");
    selected.file_plan = Some("files.yml".into());
    add_with_catalog(
        &session(root.path(), false),
        selected,
        ProviderCatalog::for_loopback_tests(&server.url(), None),
    )
    .await
    .unwrap();
    let recorded = project(root.path());
    let key = recorded.intent().roots.keys().next().unwrap().clone();
    let mut intent = recorded.intent().clone();
    intent.roots.get_mut(&key).unwrap().version = VersionIntent::FollowCompatible;
    fs::write(
        root.path().join("project/empack.yml"),
        DocumentCodec.encode_intent(&intent).unwrap(),
    )
    .unwrap();
    synchronize(&session(root.path(), false), false)
        .await
        .unwrap();
    let dir = root.path().join("project/overrides/client/mods");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("selected.jar"), b"changed").unwrap();
    fs::write(dir.join("copied.jar"), b"changed").unwrap();
    let services = || dependencies::AdditionServices {
        catalog: ProviderCatalog::for_loopback_tests(&server.url(), None),
        transport: HttpAcquisition::for_loopback_tests(),
        files: DirectFileLimits::default(),
    };
    let before = super::super::tests::snapshot(root.path());
    update::adopt_with_services(
        &session(root.path(), true),
        vec![key.as_str().into()],
        services(),
    )
    .await
    .unwrap();
    assert!(super::super::tests::snapshot(root.path()) == before);
    for (first, second) in [("unrelated", "unrelated"), ("changed", "original")] {
        fs::write(dir.join("selected.jar"), first).unwrap();
        fs::write(dir.join("copied.jar"), second).unwrap();
        let bad = super::super::tests::snapshot(root.path());
        assert!(
            update::adopt_with_services(
                &session(root.path(), false),
                vec![key.as_str().into()],
                services()
            )
            .await
            .is_err()
        );
        assert!(super::super::tests::snapshot(root.path()) == bad);
    }
    fs::write(dir.join("selected.jar"), b"changed").unwrap();
    fs::write(dir.join("copied.jar"), b"changed").unwrap();
    update::adopt_with_services(
        &session(root.path(), false),
        vec![key.as_str().into()],
        services(),
    )
    .await
    .unwrap();
    let adopted = project(root.path());
    assert_eq!(adopted.intent(), &intent);
    assert_eq!(
        adopted.lock().dependencies[&key]
            .selected
            .as_ref()
            .unwrap()
            .selection,
        PinSelector::ModrinthVersion(ModrinthVersionId::parse("RootVer2").unwrap())
    );
    let after = super::super::tests::snapshot(&root.path().join("project"));
    for _ in 0..2 {
        synchronize(&session(root.path(), false), false)
            .await
            .unwrap();
    }
    assert!(super::super::tests::snapshot(&root.path().join("project")) == after);
    // The same observed selection establishes a missing first lock; no latest query is allowed.
    fs::remove_file(root.path().join("project/empack.lock")).unwrap();
    let missing = super::super::tests::snapshot(root.path());
    update::adopt_with_services(
        &session(root.path(), true),
        vec![key.as_str().into()],
        services(),
    )
    .await
    .unwrap();
    assert_eq!(super::super::tests::snapshot(root.path()), missing);
    update::adopt_with_services(
        &session(root.path(), false),
        vec![key.as_str().into()],
        services(),
    )
    .await
    .unwrap();
    assert_eq!(project(root.path()).intent(), adopted.intent());
    assert_eq!(
        project(root.path()).lock().dependencies[&key].selected,
        adopted.lock().dependencies[&key].selected
    );
    let restored = super::super::tests::snapshot(&root.path().join("project"));
    for _ in 0..2 {
        synchronize(&session(root.path(), false), false)
            .await
            .unwrap();
    }
    assert_eq!(
        super::super::tests::snapshot(&root.path().join("project")),
        restored
    );
    latest.assert_async().await;
}

#[tokio::test]
async fn new_adoption_inputs_verify_installed_bytes_without_adding_or_downloading_payloads() {
    use std::io::Write;
    for missing_lock in [false, true] {
        let root = tempfile::tempdir().unwrap();
        fixture(root.path()).await;
        if missing_lock {
            fs::remove_file(root.path().join("project/empack.lock")).unwrap();
        }
        let mut archive = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        archive
            .start_file("fabric.mod.json", zip::write::SimpleFileOptions::default())
            .unwrap();
        archive
            .write_all(br#"{"schemaVersion":1,"id":"fixture","version":"1"}"#)
            .unwrap();
        let bytes = archive.finish().unwrap().into_inner();
        fs::write(root.path().join("fixture.jar"), &bytes).unwrap();
        fs::create_dir_all(root.path().join("project/pack/mods")).unwrap();
        fs::write(root.path().join("project/pack/mods/fixture.jar"), b"wrong").unwrap();
        fs::write(root.path().join("project/pack/mods/remote.jar"), &bytes).unwrap();
        let host = root.path();
        let adopt = |dry| async move {
            let mut selected = options("fixture.jar");
            selected.platform = None;
            selected.version_id = None;
            selected
                .inputs
                .push("https://example.invalid/remote.jar".into());
            selected_with_catalog(
                &session(host, dry),
                selected,
                ProviderCatalog::for_loopback_tests("http://127.0.0.1:9", None),
                InputOperation::Adopt,
            )
            .await
        };
        let before = super::super::tests::snapshot(root.path());
        assert!(adopt(false).await.is_err());
        assert_eq!(super::super::tests::snapshot(root.path()), before);
        fs::write(root.path().join("project/pack/mods/fixture.jar"), &bytes).unwrap();
        let before = super::super::tests::snapshot(root.path());
        adopt(true).await.unwrap();
        assert_eq!(super::super::tests::snapshot(root.path()), before);
        adopt(false).await.unwrap();
        let current = project(root.path());
        assert_eq!(current.intent().roots.len(), 2);
        assert!(matches!(
            current.intent().roots[&empack_core::model::DependencyKey::parse("remote").unwrap()]
                .source,
            empack_core::model::SourceIntent::Url(_)
        ));
        for name in ["fixture.jar", "remote.jar"] {
            assert_eq!(
                fs::read(root.path().join("project/pack/mods").join(name)).unwrap(),
                bytes
            );
        }
        let adopted = super::super::tests::snapshot(&root.path().join("project"));
        for _ in 0..2 {
            synchronize(&session(root.path(), false), false)
                .await
                .unwrap();
        }
        assert_eq!(
            super::super::tests::snapshot(&root.path().join("project")),
            adopted
        );
        let mut latest = options("renderer");
        latest.version_id = None;
        assert!(
            selected_with_catalog(
                &session(root.path(), false),
                latest,
                ProviderCatalog::for_loopback_tests("http://127.0.0.1:9", None),
                InputOperation::Adopt
            )
            .await
            .unwrap_err()
            .to_string()
            .contains("exact --version-id")
        );
        assert_eq!(
            super::super::tests::snapshot(&root.path().join("project")),
            adopted
        );
    }
}

#[tokio::test]
async fn new_provider_adoption_reuses_explicit_file_plans_and_rejects_incomplete_copies() {
    use empack_core::digest::ExpectedDigest;
    use std::io::Write;
    for identify in [false, true] {
        let mut archive = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        archive
            .start_file("fabric.mod.json", zip::write::SimpleFileOptions::default())
            .unwrap();
        archive
            .write_all(br#"{"schemaVersion":1,"id":"renderer","version":"1"}"#)
            .unwrap();
        let payload = archive.finish().unwrap().into_inner();
        let hash = ExpectedDigest::Sha512(Sha512::digest(&payload).into()).hex();
        let root = tempfile::tempdir().unwrap();
        fixture(root.path()).await;
        let mut server = mockito::Server::new_async().await;
        for selector in ["renderer", "Root0001"] {
            server.mock("GET", format!("/project/{selector}").as_str()).with_body(json!({"id":"Root0001","slug":"renderer","title":"Renderer","project_type":"mod","loaders":["fabric"]}).to_string()).create_async().await;
        }
        let version = json!({
            "id":"RootVer1","project_id":"Root0001","game_versions":["1.21.1"],"loaders":["fabric"],
            "files":[{"filename":"renderer.jar","primary":true,"size":payload.len(),"hashes":{"sha512":hash},"url":"https://example.invalid/no-download.jar"}],
            "dependencies":[],"date_published":"2026-01-01T00:00:00Z","status":"listed","version_type":"release"
        });
        server
            .mock("GET", "/version/RootVer1")
            .with_body(version.to_string())
            .create_async()
            .await;
        server
            .mock("GET", format!("/version_file/{hash}").as_str())
            .match_query(mockito::Matcher::Any)
            .with_body(version.to_string())
            .create_async()
            .await;
        fs::write(root.path().join("renderer.jar"), &payload).unwrap();
        let latest = server
            .mock("GET", "/project/Root0001/version")
            .match_query(mockito::Matcher::Any)
            .expect(0)
            .create_async()
            .await;
        let environment = json!({"client":"required","server":"unsupported"});
        fs::write(
            root.path().join("files.yml"),
            json!({"schema":1,"environment":environment,"files":{"renderer.jar":[
                {"destination":"mods/renamed.jar","layer":"client","environment":environment},
                {"destination":"mods/copy.jar","layer":"client","environment":environment}
            ]}})
            .to_string(),
        )
        .unwrap();
        fs::create_dir_all(root.path().join("project/overrides/client/mods")).unwrap();
        fs::write(
            root.path()
                .join("project/overrides/client/mods/renamed.jar"),
            &payload,
        )
        .unwrap();
        let host = root.path();
        let endpoint = server.url();
        let adopt = |dry| {
            let endpoint = endpoint.clone();
            async move {
                let mut selected = options(if identify { "renderer.jar" } else { "renderer" });
                if identify {
                    selected.version_id = None;
                }
                selected.file_plan = Some("files.yml".into());
                selected_with_catalog(
                    &session(host, dry),
                    selected,
                    ProviderCatalog::for_loopback_tests(&endpoint, None),
                    InputOperation::Adopt,
                )
                .await
            }
        };
        let incomplete = super::super::tests::snapshot(root.path());
        assert!(adopt(false).await.is_err());
        assert_eq!(super::super::tests::snapshot(root.path()), incomplete);
        fs::write(
            root.path().join("project/overrides/client/mods/copy.jar"),
            &payload,
        )
        .unwrap();
        let before = super::super::tests::snapshot(root.path());
        adopt(true).await.unwrap();
        assert_eq!(super::super::tests::snapshot(root.path()), before);
        adopt(false).await.unwrap();
        let resolved = project(root.path());
        let selected = resolved.lock().dependencies.values().next().unwrap();
        assert_eq!(
            selected.selected.as_ref().unwrap().selection,
            PinSelector::ModrinthVersion(ModrinthVersionId::parse("RootVer1").unwrap())
        );
        assert_eq!(selected.files.as_slice()[0].placements.as_slice().len(), 2);
        let committed = super::super::tests::snapshot(&root.path().join("project"));
        for _ in 0..2 {
            synchronize(&session(root.path(), false), false)
                .await
                .unwrap();
        }
        assert_eq!(
            super::super::tests::snapshot(&root.path().join("project")),
            committed
        );
        latest.assert_async().await;
    }
}
