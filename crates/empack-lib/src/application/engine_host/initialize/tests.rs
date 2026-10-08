use super::*;
use crate::{
    application::session_mocks::{
        MockCommandSession, MockConfigProvider, MockFileSystemProvider, MockInteractiveProvider,
    },
    engine::{
        api::{BuildOutput, BuildRequest, SyncRequest},
        content::SourceEvidencePolicy,
        documents::DocumentCodec,
        mrpack::OptionalConversion,
        packwiz::InstallerInteraction,
        templates::TemplateOptions,
    },
};
use empack_core::inventory::OptionalPolicy;
use std::{fs, path::PathBuf};

fn args() -> InitArgs {
    InitArgs {
        dir: Some("pack".into()),
        modloader: Some("none".into()),
        mc_version: Some("1.21.1".into()),
        pack_name: Some("A human display name".into()),
        pack_version: Some("2.0".into()),
        author: Some("Tester".into()),
        ..Default::default()
    }
}
fn session(root: &Path, yes: bool, dry_run: bool) -> MockCommandSession {
    MockCommandSession::new()
        .with_filesystem(MockFileSystemProvider::new().with_current_dir(root.to_path_buf()))
        .with_config(MockConfigProvider::new(AppConfig {
            workdir: Some(root.to_path_buf()),
            state_dir: Some(root.join("state")),
            yes,
            dry_run,
            ..Default::default()
        }))
}
fn snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    super::super::tests::snapshot(root)
}
fn read(root: &Path) -> empack_core::model::ResolvedProject {
    let intent = DocumentCodec
        .decode_intent(&fs::read(root.join("empack.yml")).unwrap(), "test")
        .unwrap();
    DocumentCodec
        .decode_lock(
            &fs::read(root.join("empack.lock")).unwrap(),
            &intent,
            "test",
        )
        .unwrap()
}
#[tokio::test]
async fn initialization_host_preserves_preview_decline_and_exact_replacement_footprints() {
    let root = tempfile::tempdir().unwrap();
    let mut options = args();
    initialize(&session(root.path(), true, true), &options)
        .await
        .unwrap();
    assert!(
        snapshot(root.path()).is_empty(),
        "Preview must create neither project nor host state"
    );
    initialize(&session(root.path(), false, false), &options)
        .await
        .unwrap();
    assert!(
        snapshot(root.path()).is_empty(),
        "Declined publication must remain read-only"
    );
    initialize(&session(root.path(), true, false), &options)
        .await
        .unwrap();
    let project = root.path().join("pack");
    fs::create_dir_all(project.join("pack/config")).unwrap();
    fs::write(project.join("pack/config/old.cfg"), b"old managed bytes").unwrap();
    fs::write(project.join("notes.txt"), b"retain unrelated notes").unwrap();
    fs::write(
        project.join("templates/server/server.properties.template"),
        b"user-owned template",
    )
    .unwrap();
    let original = snapshot(root.path());
    assert!(
        initialize(&session(root.path(), true, false), &options)
            .await
            .is_err()
    );
    assert_eq!(snapshot(root.path()), original);
    options.force = true;
    options.pack_name = Some("Replacement".into());
    for (yes, dry) in [(true, true), (false, false)] {
        initialize(&session(root.path(), yes, dry), &options)
            .await
            .unwrap();
        assert_eq!(snapshot(root.path()), original);
    }
    initialize(&session(root.path(), true, false), &options)
        .await
        .unwrap();
    assert!(!project.join("pack/config/old.cfg").exists());
    assert_eq!(
        fs::read(project.join("notes.txt")).unwrap(),
        b"retain unrelated notes"
    );
    assert_eq!(
        fs::read(project.join("templates/server/server.properties.template")).unwrap(),
        b"user-owned template"
    );
    assert_eq!(read(&project).intent().metadata.name, "Replacement");
}

#[tokio::test]
async fn initialization_host_retains_explicit_runtime_and_layout_choices_without_network() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("parent")).unwrap();
    let mut host = session(root.path(), true, false);
    host.config_provider.app_config.workdir = Some(PathBuf::from("parent"));
    let mut options = args();
    options.modloader = Some("forge".into());
    options.mc_version = Some("1.7.10".into());
    options.loader_version = Some("10.13.4.1614-1.7.10".into());
    options.game_versions = Some(vec!["1.7.10".into(), "1.7.2".into(), "1.7.2".into()]);
    options.datapack_folder = Some("saves/world/datapacks".into());
    initialize(&host, &options).await.unwrap();
    let project = read(&root.path().join("parent/pack"));
    assert_eq!(
        project
            .intent()
            .runtime
            .loader_version
            .as_ref()
            .unwrap()
            .as_str(),
        "10.13.4.1614"
    );
    assert_eq!(
        project.intent().runtime.loader_version,
        project.lock().runtime.loader_version
    );
    assert_eq!(
        project.intent().runtime.acceptable_versions,
        vec![GameVersion::parse("1.7.2").unwrap()]
    );
    assert_eq!(
        project.intent().layout[&ContentKind::DataPack].as_str(),
        "saves/world/datapacks"
    );
    assert_eq!(project.intent().metadata.author.as_deref(), Some("Tester"));
    assert_eq!(project.intent().distribution.targets.as_slice().len(), 5);
}

#[tokio::test]
async fn initialization_host_validates_options_and_interactive_indices_before_publication() {
    let root = tempfile::tempdir().unwrap();
    for change in [
        "loader",
        "vanilla-pin",
        "game",
        "datapack",
        "name",
        "version",
        "headless",
        "import",
    ] {
        let mut options = args();
        match change {
            "loader" => options.modloader = Some("mistyped".into()),
            "vanilla-pin" => options.loader_version = Some("1".into()),
            "game" => options.mc_version = Some("".into()),
            "datapack" => options.datapack_folder = Some("../escape".into()),
            "name" => options.pack_name = Some(" ".into()),
            "version" => options.pack_version = Some(" ".into()),
            "headless" => options.modloader = None,
            "import" => options.from_source = Some("not-an-empty-project".into()),
            _ => unreachable!(),
        }
        assert!(
            initialize(&session(root.path(), true, false), &options)
                .await
                .is_err(),
            "{change}"
        );
        assert!(snapshot(root.path()).is_empty(), "{change}");
    }
    let host = session(root.path(), false, false)
        .with_interactive(MockInteractiveProvider::new().with_select(999));
    assert!(choose_loader(&host, vec![(LoaderKind::Vanilla, None)]).is_err());
    assert!(snapshot(root.path()).is_empty());
    assert!(select_version(&host, "Version", &[]).is_err());
    let host = host.with_interactive(MockInteractiveProvider::new().with_fuzzy_select(999));
    assert!(select_version(&host, "Version", &["1.0".into()]).is_err());
}

#[tokio::test]
async fn initialized_project_syncs_twice_and_builds_current_scaffolding_through_engine() {
    let root = tempfile::tempdir().unwrap();
    let host = session(root.path(), true, false);
    initialize(&host, &args()).await.unwrap();
    let project = root.path().join("pack");
    let before = snapshot(&project);
    let engine = engine(&host.config_provider.app_config, root.path()).unwrap();
    for _ in 0..2 {
        let prepared = match engine
            .prepare(
                ProjectTarget::Existing(project.clone()),
                SyncRequest {
                    resolution: None,
                    content: BTreeMap::new(),
                },
            )
            .await
            .unwrap()
        {
            Preparation::Ready(value) => value,
            Preparation::NeedsInput(_) => panic!("Empty project needs no inputs"),
        };
        assert!(prepared.view().sync().unwrap().files.changes().is_empty());
        let grant = ExecutionGrant {
            plan: prepared.view().plan(),
            replacement: prepared.view().replacement(),
            network: NetworkPermission::Offline,
            run_installer: false,
        };
        let mut operation = engine.start(prepared.authorize(grant).unwrap()).unwrap();
        assert!(matches!(
            &*operation.wait().await,
            OperationOutcome::Completed(ExecutionOutcome::Completed(ExecutionReceipt::Sync(_)))
        ));
        engine.release_completed(operation.id());
        assert_eq!(snapshot(&project), before);
    }
    let request = BuildRequest {
        clean: false,
        outputs: NonEmpty::new(vec![
            BuildOutput {
                target: BuildTarget::Mrpack,
                artifact: PortableRelPath::parse("pack.mrpack", PathSyntax::ProjectContent)
                    .unwrap(),
            },
            BuildOutput {
                target: BuildTarget::ClientFull,
                artifact: PortableRelPath::parse("client.zip", PathSyntax::ProjectContent).unwrap(),
            },
        ])
        .unwrap(),
        archive: DistributionArchive::Zip,
        optional: OptionalPolicy::Preserve,
        mrpack_optional: OptionalConversion::RejectMetadataLoss,
        templates: TemplateOptions::default(),
        evidence: SourceEvidencePolicy::Compatibility,
        interaction: InstallerInteraction::Headless,
    };
    let prepared = match engine
        .prepare(ProjectTarget::Existing(project.clone()), request)
        .await
        .unwrap()
    {
        Preparation::Ready(value) => value,
        Preparation::NeedsInput(_) => panic!("Empty vanilla client build needs no inputs"),
    };
    let grant = ExecutionGrant {
        plan: prepared.view().plan(),
        replacement: prepared.view().replacement(),
        network: NetworkPermission::Offline,
        run_installer: false,
    };
    let mut operation = engine.start(prepared.authorize(grant).unwrap()).unwrap();
    let outcome = operation.wait().await;
    match &*outcome {
        OperationOutcome::Completed(ExecutionOutcome::Completed(ExecutionReceipt::Build(
            receipt,
        ))) => assert_eq!(receipt.artifacts.len(), 2),
        OperationOutcome::Completed(ExecutionOutcome::FailedBeforePublication(error)) => {
            panic!("{error:#}")
        }
        _ => panic!("Unexpected build outcome"),
    }
    let mut archive =
        zip::ZipArchive::new(fs::File::open(project.join("dist/client.zip")).unwrap()).unwrap();
    let mut cfg = String::new();
    std::io::Read::read_to_string(&mut archive.by_name("instance.cfg").unwrap(), &mut cfg).unwrap();
    assert!(cfg.contains("A human display name"));
    assert!(!cfg.contains("{{"));
    assert_eq!(
        fs::read(project.join("empack.yml")).unwrap(),
        before[&PathBuf::from("empack.yml")]
    );
    engine.shutdown().await;
}

#[tokio::test]
async fn initialization_discovery_preserves_unpinned_intent_and_provider_failures_publish_nothing()
{
    let mut server = mockito::Server::new_async().await;
    let games = server.mock("GET", "/games").with_status(200)
        .with_body(serde_json::json!({"latest":{"release":"1.21.1"},"versions":[{"id":"1.21.1","type":"release"}]}).to_string())
        .expect(4).create_async().await;
    let loaders = server.mock("GET", "/fabric/1.21.1").with_status(200)
        .with_body(serde_json::json!([
            {"loader":{"version":"0.16.0","stable":true},"intermediary":{"version":"1.21.1"}},
            {"loader":{"version":"0.17.0-beta.1","stable":false},"intermediary":{"version":"1.21.1"}}
        ]).to_string()).expect(3).create_async().await;
    let failure = server
        .mock("GET", "/quilt/1.21.1")
        .with_status(403)
        .create_async()
        .await;
    let root = tempfile::tempdir().unwrap();
    let mut options = args();
    options.mc_version = None;
    options.modloader = Some("fabric".into());
    let catalog = RuntimeCatalog::for_loopback_tests(&server.url());
    initialize_with_catalog(&session(root.path(), true, true), &options, catalog.clone())
        .await
        .unwrap();
    assert!(snapshot(root.path()).is_empty());
    initialize_with_catalog(
        &session(root.path(), true, false),
        &options,
        catalog.clone(),
    )
    .await
    .unwrap();
    let resolved = read(&root.path().join("pack"));
    assert_eq!(
        resolved
            .lock()
            .runtime
            .loader_version
            .as_ref()
            .unwrap()
            .as_str(),
        "0.16.0"
    );
    assert!(resolved.intent().runtime.loader_version.is_none());
    assert_eq!(resolved.intent().runtime.minecraft.as_str(), "1.21.1");
    let host = session(root.path(), false, false).with_interactive(
        MockInteractiveProvider::new()
            .queue_fuzzy_select(Some(0))
            .queue_fuzzy_select(Some(1))
            .queue_confirm(true),
    );
    options.dir = Some("explicit-prerelease".into());
    initialize_with_catalog(&host, &options, catalog.clone())
        .await
        .unwrap();
    let resolved = read(&root.path().join("explicit-prerelease"));
    assert_eq!(
        resolved
            .lock()
            .runtime
            .loader_version
            .as_ref()
            .unwrap()
            .as_str(),
        "0.17.0-beta.1"
    );
    assert!(resolved.intent().runtime.loader_version.is_none());
    options.dir = Some("unavailable".into());
    options.modloader = Some("quilt".into());
    let before = snapshot(root.path());
    let error = initialize_with_catalog(&session(root.path(), true, false), &options, catalog)
        .await
        .unwrap_err();
    assert!(
        matches!(
            error.downcast_ref::<crate::engine::acquisition::TransferError>(),
            Some(crate::engine::acquisition::TransferError::Unauthorized)
        ),
        "{error:#}"
    );
    assert_eq!(snapshot(root.path()), before);
    games.assert_async().await;
    loaders.assert_async().await;
    failure.assert_async().await;
}

#[tokio::test]
async fn historical_initialization_menu_offers_only_catalog_supported_loaders() {
    let mut server = mockito::Server::new_async().await;
    let fabric = server
        .mock("GET", "/fabric/1.7.10")
        .with_status(200)
        .with_body("[]")
        .create_async()
        .await;
    let quilt = server
        .mock("GET", "/quilt/1.7.10")
        .with_status(403)
        .create_async()
        .await;
    let forge = server
        .mock("GET", "/forge")
        .with_status(200)
        .with_body(r#"{"1.7.10":["1.7.10-10.13.4.1614-1.7.10"]}"#)
        .create_async()
        .await;
    let neo = server
        .mock("GET", "/neoforge")
        .with_status(200)
        .with_body(r#"{"versions":["21.1.1"]}"#)
        .create_async()
        .await;
    let root = tempfile::tempdir().unwrap();
    let mut options = args();
    options.modloader = None;
    options.mc_version = Some("1.7.10".into());
    let host = session(root.path(), false, false).with_interactive(
        MockInteractiveProvider::new()
            .queue_select(1)
            .queue_fuzzy_select(Some(0))
            .queue_confirm(true),
    );
    initialize_with_catalog(
        &host,
        &options,
        RuntimeCatalog::for_loopback_tests(&server.url()),
    )
    .await
    .unwrap();
    let resolved = read(&root.path().join("pack"));
    assert_eq!(resolved.lock().runtime.loader, LoaderKind::Forge);
    assert_eq!(
        resolved
            .lock()
            .runtime
            .loader_version
            .as_ref()
            .unwrap()
            .as_str(),
        "10.13.4.1614"
    );
    assert!(resolved.intent().runtime.loader_version.is_none());
    for response in [fabric, quilt, forge, neo] {
        response.assert_async().await;
    }
}

#[tokio::test]
async fn loader_pin_menu_excludes_vanilla_and_families_without_the_requested_pin() {
    let mut server = mockito::Server::new_async().await;
    let mut responses = Vec::new();
    for (path, body) in [
        ("/neoforge", r#"{"versions":["21.1.1"]}"#),
        (
            "/fabric/1.7.10",
            r#"[{"loader":{"version":"0.16.0","stable":true},"intermediary":{"version":"1.7.10"}}]"#,
        ),
        ("/quilt/1.7.10", "[]"),
        ("/forge", r#"{"1.7.10":["1.7.10-10.13.4.1614-1.7.10"]}"#),
    ] {
        responses.push(
            server
                .mock("GET", path)
                .with_status(200)
                .with_body(body)
                .expect(2)
                .create_async()
                .await,
        );
    }
    let root = tempfile::tempdir().unwrap();
    let mut options = args();
    options.modloader = None;
    options.mc_version = Some("1.7.10".into());
    options.loader_version = Some("10.13.4.1614-1.7.10".into());
    let host = session(root.path(), false, false).with_interactive(
        MockInteractiveProvider::new()
            .queue_select(0)
            .queue_confirm(true),
    );
    initialize_with_catalog(
        &host,
        &options,
        RuntimeCatalog::for_loopback_tests(&server.url()),
    )
    .await
    .unwrap();
    let project = read(&root.path().join("pack"));
    assert_eq!(project.lock().runtime.loader, LoaderKind::Forge);
    assert_eq!(
        project
            .intent()
            .runtime
            .loader_version
            .as_ref()
            .unwrap()
            .as_str(),
        "10.13.4.1614"
    );
    let before = snapshot(root.path());
    options.dir = Some("unavailable-pin".into());
    options.loader_version = Some("unknown-pin".into());
    let error = initialize_with_catalog(
        &host,
        &options,
        RuntimeCatalog::for_loopback_tests(&server.url()),
    )
    .await
    .unwrap_err();
    assert!(format!("{error:#}").contains("No compatible loader family"));
    assert_eq!(snapshot(root.path()), before);
    for response in responses {
        response.assert_async().await;
    }
}

#[tokio::test]
async fn loader_menu_shares_one_deadline_and_retains_an_earlier_supported_family() {
    let mut server = mockito::Server::new_async().await;
    let neo = server
        .mock("GET", "/neoforge")
        .with_status(200)
        .with_body(r#"{"versions":["21.1.1"]}"#)
        .create_async()
        .await;
    let mut fabric_server = mockito::Server::new_async().await;
    let fabric = fabric_server
        .mock("GET", "/fabric/1.21.1")
        .with_status(200)
        .with_chunked_body(|writer| {
            std::thread::sleep(Duration::from_millis(300));
            writer.write_all(b"[]")
        })
        .create_async()
        .await;
    let mut catalog = RuntimeCatalog::for_loopback_tests(&server.url())
        .with_loader_test_origin(LoaderKind::Fabric, &fabric_server.url());
    let mut later = Vec::new();
    let mut other_servers = Vec::new();
    for (path, family) in [
        ("/forge", LoaderKind::Forge),
        ("/quilt/1.21.1", LoaderKind::Quilt),
    ] {
        let mut other = mockito::Server::new_async().await;
        catalog = catalog.with_loader_test_origin(family, &other.url());
        later.push(
            other
                .mock("GET", path)
                .with_status(200)
                .with_chunked_body(|writer| {
                    std::thread::sleep(Duration::from_millis(300));
                    writer.write_all(b"{}")
                })
                .expect(1)
                .create_async()
                .await,
        );
        other_servers.push(other);
    }
    let root = tempfile::tempdir().unwrap();
    let host = session(root.path(), false, false)
        .with_interactive(MockInteractiveProvider::new().queue_select(1));
    let mut limits = RuntimeCatalogLimits::default();
    limits.transfer.deadline = Duration::from_millis(150);
    let (family, choices) = compatible_loader(
        &host,
        catalog,
        GameVersion::parse("1.21.1").unwrap(),
        None,
        limits,
    )
    .await
    .unwrap();
    assert_eq!(family, LoaderKind::NeoForge);
    assert_eq!(
        choices
            .unwrap()
            .resolve(None)
            .unwrap()
            .loader_version
            .unwrap()
            .as_str(),
        "21.1.1"
    );
    neo.assert_async().await;
    fabric.assert_async().await;
    for response in later {
        response.assert_async().await;
    }
    assert!(snapshot(root.path()).is_empty());
}

#[tokio::test]
async fn slow_earlier_provider_cannot_starve_a_later_family_matching_the_pin() {
    let mut server = mockito::Server::new_async().await;
    let mut stalled = mockito::Server::new_async().await;
    let slow = stalled
        .mock("GET", "/neoforge")
        .with_status(200)
        .with_chunked_body(|writer| {
            std::thread::sleep(Duration::from_millis(500));
            writer.write_all(b"{}")
        })
        .create_async()
        .await;
    let fabric = server
        .mock("GET", "/fabric/1.7.10")
        .with_status(200)
        .with_body("[]")
        .create_async()
        .await;
    let forge = server
        .mock("GET", "/forge")
        .with_status(200)
        .with_body(r#"{"1.7.10":["1.7.10-10.13.4.1614-1.7.10"]}"#)
        .create_async()
        .await;
    let quilt = server
        .mock("GET", "/quilt/1.7.10")
        .with_status(200)
        .with_body("[]")
        .create_async()
        .await;
    let root = tempfile::tempdir().unwrap();
    let host = session(root.path(), false, false)
        .with_interactive(MockInteractiveProvider::new().queue_select(0));
    let mut limits = RuntimeCatalogLimits::default();
    limits.transfer.deadline = Duration::from_millis(200);
    let (family, versions) = compatible_loader(
        &host,
        RuntimeCatalog::for_loopback_tests(&server.url())
            .with_loader_test_origin(LoaderKind::NeoForge, &stalled.url()),
        GameVersion::parse("1.7.10").unwrap(),
        Some(LoaderVersion::parse("10.13.4.1614").unwrap()),
        limits,
    )
    .await
    .unwrap();
    assert_eq!(family, LoaderKind::Forge);
    assert_eq!(
        versions
            .unwrap()
            .resolve(None)
            .unwrap()
            .loader_version
            .unwrap()
            .as_str(),
        "10.13.4.1614"
    );
    for response in [slow, fabric, forge, quilt] {
        response.assert_async().await;
    }
    assert!(snapshot(root.path()).is_empty());
}
