//! E2E tests for the add command
//!
//! These tests use real filesystems (tempfile) and mock providers
//! to validate that our abstractions correctly model the external world.

use anyhow::Result;
use empack_lib::application::cli::Commands;
use empack_lib::application::commands::execute_command_with_session;
use empack_lib::application::config::AppConfig;
use empack_lib::application::session::{
    CommandSession, LiveConfigProvider, LiveFileSystemProvider, LiveNetworkProvider, ProcessOutput,
};
use empack_lib::application::session_mocks::{
    MockInteractiveProvider, MockNetworkProvider, MockProcessProvider,
};
use empack_lib::display::Display;
use empack_lib::empack::config::DependencyEntry;
use empack_lib::empack::search::ProjectInfo;
use empack_lib::primitives::ProjectPlatform;
use empack_lib::terminal::TerminalCapabilities;
use empack_tests::e2e::{assert_pack_option_string, read_empack_config};
use mockito::Server;
use std::path::Path;
use tempfile::TempDir;

#[tokio::test]
async fn e2e_add_mod_successfully() -> Result<()> {
    // Setup: Create a real temporary directory
    let temp_dir = TempDir::new()?;
    let workdir = temp_dir.path().to_path_buf();

    // Initialize: Create a real empack project using live providers
    initialize_empack_project(&workdir).await?;

    // Set working directory for the test
    std::env::set_current_dir(&workdir)?;

    // Create hybrid session: Real filesystem + Mock process + Mock network (no live API calls)
    let app_config = AppConfig {
        workdir: Some(workdir.clone()),
        ..AppConfig::default()
    };

    // Initialize display system
    let terminal_caps = TerminalCapabilities::detect_from_config(app_config.color)?;
    Display::init_or_get(terminal_caps);

    // Use a mock process provider that simulates packwiz success
    let mock_process_provider = MockProcessProvider::new().with_packwiz_result(
        vec![
            "modrinth".to_string(),
            "add".to_string(),
            "--project-id".to_string(),
            "AANobbMI".to_string(),
            "-y".to_string(),
        ],
        Ok(ProcessOutput {
            stdout: String::new(),
            stderr: String::new(),
            success: true,
        }),
    );
    let mock_network_provider = MockNetworkProvider::new().with_project_response(
        "sodium".to_string(),
        ProjectInfo {
            platform: ProjectPlatform::Modrinth,
            project_id: "AANobbMI".to_string(),
            title: "Sodium".to_string(),
            downloads: 1_000_000,
            confidence: 95,
            project_type: "mod".to_string(),
        },
    );

    let session = CommandSession::new_with_providers(
        LiveFileSystemProvider,
        mock_network_provider,
        mock_process_provider,
        LiveConfigProvider::new(app_config),
        MockInteractiveProvider::new(),
    );

    // Execute the add command - this will use the mock server instead of live API calls
    let result = execute_command_with_session(
        Commands::Add {
            mods: vec!["sodium".to_string()],
            force: false,
            platform: None,
            project_type: None,
            version_id: None,
            file_id: None,
        },
        &session,
    )
    .await;

    assert!(result.is_ok(), "Add command failed: {:?}", result);

    // Verify empack.yml still exists and was not corrupted by the add
    let empack_yml = std::fs::read_to_string(workdir.join("empack.yml"))?;
    assert!(
        empack_yml.contains("loader: fabric"),
        "empack.yml should preserve its existing loader field after add, got: {}",
        empack_yml
    );

    Ok(())
}

#[tokio::test]
async fn e2e_add_direct_zip_datapack_tracks_local_dependency_and_sets_folder() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let workdir = temp_dir.path().to_path_buf();

    initialize_empack_project(&workdir).await?;

    let app_config = AppConfig {
        workdir: Some(workdir.clone()),
        ..AppConfig::default()
    };

    let terminal_caps = TerminalCapabilities::detect_from_config(app_config.color)?;
    Display::init_or_get(terminal_caps);

    let mut server = Server::new_async().await;
    let _datapack = server
        .mock("GET", "/downloads/example-datapack.zip")
        .with_status(200)
        .with_header("content-type", "application/zip")
        .with_body("zip-bytes")
        .create_async()
        .await;

    let session = CommandSession::new_with_providers(
        LiveFileSystemProvider,
        LiveNetworkProvider::new_for_test(None, None),
        MockProcessProvider::new(),
        LiveConfigProvider::new(app_config),
        MockInteractiveProvider::new(),
    );

    let result = execute_command_with_session(
        Commands::Add {
            mods: vec![format!("{}/downloads/example-datapack.zip", server.url())],
            force: false,
            platform: None,
            project_type: Some(empack_lib::application::cli::CliProjectType::Datapack),
            version_id: None,
            file_id: None,
        },
        &session,
    )
    .await;

    assert!(result.is_ok(), "direct datapack zip add failed: {result:?}");

    let config = read_empack_config(&workdir);
    assert_eq!(
        config.empack.datapack_folder.as_deref(),
        Some("datapacks"),
        "datapack add should initialize datapack_folder"
    );

    let dependency = config
        .empack
        .dependencies
        .get("example-datapack")
        .expect("tracked local datapack dependency");

    match dependency {
        DependencyEntry::Local(record) => {
            assert_eq!(record.path, "pack/datapacks/example-datapack.zip");
            assert_eq!(
                record.source_url.as_deref(),
                Some(format!("{}/downloads/example-datapack.zip", server.url()).as_str())
            );
            assert!(!record.sha256.is_empty(), "sha256 should be recorded");
        }
        other => panic!("expected local dependency entry, got {other:?}"),
    }

    assert_pack_option_string(&workdir, "datapack-folder", "datapacks");
    assert!(
        workdir.join("pack/datapacks/example-datapack.zip").exists(),
        "downloaded datapack should be written into the datapack folder"
    );

    Ok(())
}

#[tokio::test]
async fn e2e_add_direct_zip_shader_tracks_local_dependency() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let workdir = temp_dir.path().to_path_buf();

    initialize_empack_project(&workdir).await?;

    let app_config = AppConfig {
        workdir: Some(workdir.clone()),
        ..AppConfig::default()
    };

    let terminal_caps = TerminalCapabilities::detect_from_config(app_config.color)?;
    Display::init_or_get(terminal_caps);

    let mut server = Server::new_async().await;
    let _shader = server
        .mock("GET", "/downloads/example-shader.zip")
        .with_status(200)
        .with_header("content-type", "application/zip")
        .with_body("zip-bytes")
        .create_async()
        .await;

    let session = CommandSession::new_with_providers(
        LiveFileSystemProvider,
        LiveNetworkProvider::new_for_test(None, None),
        MockProcessProvider::new(),
        LiveConfigProvider::new(app_config),
        MockInteractiveProvider::new(),
    );

    let result = execute_command_with_session(
        Commands::Add {
            mods: vec![format!("{}/downloads/example-shader.zip", server.url())],
            force: false,
            platform: None,
            project_type: Some(empack_lib::application::cli::CliProjectType::Shader),
            version_id: None,
            file_id: None,
        },
        &session,
    )
    .await;

    assert!(result.is_ok(), "direct shader zip add failed: {result:?}");

    let config = read_empack_config(&workdir);
    let dependency = config
        .empack
        .dependencies
        .get("example-shader")
        .expect("tracked local shader dependency");

    match dependency {
        DependencyEntry::Local(record) => {
            assert_eq!(record.path, "pack/shaderpacks/example-shader.zip");
            assert_eq!(
                record.source_url.as_deref(),
                Some(format!("{}/downloads/example-shader.zip", server.url()).as_str())
            );
            assert!(!record.sha256.is_empty(), "sha256 should be recorded");
        }
        other => panic!("expected local dependency entry, got {other:?}"),
    }

    assert!(
        workdir.join("pack/shaderpacks/example-shader.zip").exists(),
        "downloaded shader should be written into the shaderpacks folder"
    );

    Ok(())
}

async fn initialize_empack_project(workdir: &Path) -> Result<()> {
    std::fs::create_dir_all(workdir.join("pack"))?;

    let empack_yml = r#"empack:
  dependencies: {}
  minecraft_version: "1.21.1"
  loader: fabric
  name: "Test Pack"
  author: "Test Author"
  version: "1.0.0"
"#;
    std::fs::write(workdir.join("empack.yml"), empack_yml)?;

    let pack_toml = r#"name = "Test Pack"
author = "Test Author"
version = "1.0.0"
pack-format = "packwiz:1.1.0"

[index]
file = "index.toml"
hash-format = "sha256"
hash = ""

[versions]
minecraft = "1.21.1"
fabric = "0.15.0"
"#;
    std::fs::write(workdir.join("pack").join("pack.toml"), pack_toml)?;

    let index_toml = r#"hash-format = "sha256"

[[files]]
file = "pack.toml"
hash = ""
"#;
    std::fs::write(workdir.join("pack").join("index.toml"), index_toml)?;

    Ok(())
}
