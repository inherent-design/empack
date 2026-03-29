use anyhow::Result;
use empack_lib::application::cli::{CliProjectType, Commands};
use empack_lib::application::commands::execute_command_with_session;
use empack_lib::application::config::AppConfig;
use empack_lib::application::session::{
    CommandSession, LiveConfigProvider, LiveFileSystemProvider, ProcessOutput,
};
use empack_lib::application::session_mocks::{
    MockInteractiveProvider, MockNetworkProvider, MockProcessProvider,
};
use empack_lib::display::Display;
use empack_lib::empack::search::ProjectInfo;
use empack_lib::primitives::ProjectPlatform;
use empack_lib::terminal::TerminalCapabilities;
use std::path::Path;
use tempfile::TempDir;

fn initialize_project(workdir: &Path, loader: &str) -> Result<()> {
    std::fs::create_dir_all(workdir.join("pack"))?;

    let empack_yml = format!(
        r#"empack:
  dependencies: {{}}
  minecraft_version: "1.21.4"
  loader: {loader}
  name: "Test Pack"
  author: "Test Author"
  version: "1.0.0"
"#
    );
    std::fs::write(workdir.join("empack.yml"), empack_yml)?;

    let pack_toml = format!(
        r#"name = "Test Pack"
author = "Test Author"
version = "1.0.0"
pack-format = "packwiz:1.1.0"

[index]
file = "index.toml"
hash-format = "sha256"
hash = ""

[versions]
minecraft = "1.21.4"
{loader} = "0.15.0"
"#
    );
    std::fs::write(workdir.join("pack").join("pack.toml"), pack_toml)?;

    let index_toml = r#"hash-format = "sha256"

[[files]]
file = "pack.toml"
hash = ""
"#;
    std::fs::write(workdir.join("pack").join("index.toml"), index_toml)?;

    Ok(())
}

#[tokio::test]
async fn test_add_type_resourcepack() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let workdir = temp_dir.path().to_path_buf();
    initialize_project(&workdir, "fabric")?;
    std::env::set_current_dir(&workdir)?;

    let app_config = AppConfig {
        workdir: Some(workdir.clone()),
        ..AppConfig::default()
    };

    let terminal_caps = TerminalCapabilities::detect_from_config(app_config.color)?;
    Display::init_or_get(terminal_caps);

    let mock_network = MockNetworkProvider::new().with_project_response(
        "faithless".to_string(),
        ProjectInfo {
            platform: ProjectPlatform::Modrinth,
            project_id: "faithless".to_string(),
            title: "Faithless".to_string(),
            downloads: 5000,
            confidence: 100,
            project_type: "resourcepack".to_string(),
        },
    );

    let mock_process = MockProcessProvider::new().with_packwiz_result(
        vec![
            "modrinth".to_string(),
            "add".to_string(),
            "--project-id".to_string(),
            "faithless".to_string(),
            "-y".to_string(),
        ],
        Ok(ProcessOutput {
            stdout: String::new(),
            stderr: String::new(),
            success: true,
        }),
    );

    let session = CommandSession::new_with_providers(
        LiveFileSystemProvider,
        mock_network,
        mock_process,
        LiveConfigProvider::new(app_config),
        MockInteractiveProvider::new(),
    );

    let result = execute_command_with_session(
        Commands::Add {
            mods: vec!["faithless".to_string()],
            force: false,
            platform: None,
            project_type: Some(CliProjectType::ResourcePack),
        },
        &session,
    )
    .await;

    assert!(
        result.is_ok(),
        "Add with --type resourcepack should succeed: {:?}",
        result
    );

    Ok(())
}

#[tokio::test]
async fn test_add_type_shader() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let workdir = temp_dir.path().to_path_buf();
    initialize_project(&workdir, "fabric")?;
    std::env::set_current_dir(&workdir)?;

    let app_config = AppConfig {
        workdir: Some(workdir.clone()),
        ..AppConfig::default()
    };

    let terminal_caps = TerminalCapabilities::detect_from_config(app_config.color)?;
    Display::init_or_get(terminal_caps);

    let mock_network = MockNetworkProvider::new().with_project_response(
        "complementary-shaders".to_string(),
        ProjectInfo {
            platform: ProjectPlatform::Modrinth,
            project_id: "complementary".to_string(),
            title: "Complementary Shaders".to_string(),
            downloads: 8000,
            confidence: 100,
            project_type: "shader".to_string(),
        },
    );

    let mock_process = MockProcessProvider::new().with_packwiz_result(
        vec![
            "modrinth".to_string(),
            "add".to_string(),
            "--project-id".to_string(),
            "complementary".to_string(),
            "-y".to_string(),
        ],
        Ok(ProcessOutput {
            stdout: String::new(),
            stderr: String::new(),
            success: true,
        }),
    );

    let session = CommandSession::new_with_providers(
        LiveFileSystemProvider,
        mock_network,
        mock_process,
        LiveConfigProvider::new(app_config),
        MockInteractiveProvider::new(),
    );

    let result = execute_command_with_session(
        Commands::Add {
            mods: vec!["complementary-shaders".to_string()],
            force: false,
            platform: None,
            project_type: Some(CliProjectType::Shader),
        },
        &session,
    )
    .await;

    assert!(
        result.is_ok(),
        "Add with --type shader should succeed: {:?}",
        result
    );

    Ok(())
}

#[tokio::test]
async fn test_add_dry_run() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let workdir = temp_dir.path().to_path_buf();
    initialize_project(&workdir, "fabric")?;
    std::env::set_current_dir(&workdir)?;

    let mut app_config = AppConfig {
        workdir: Some(workdir.clone()),
        ..AppConfig::default()
    };
    app_config.dry_run = true;

    let terminal_caps = TerminalCapabilities::detect_from_config(app_config.color)?;
    Display::init_or_get(terminal_caps);

    let mock_network = MockNetworkProvider::new().with_project_response(
        "sodium".to_string(),
        ProjectInfo {
            platform: ProjectPlatform::Modrinth,
            project_id: "AANobbMI".to_string(),
            title: "Sodium".to_string(),
            downloads: 50000,
            confidence: 100,
            project_type: "mod".to_string(),
        },
    );

    let mock_process = MockProcessProvider::new();

    let session = CommandSession::new_with_providers(
        LiveFileSystemProvider,
        mock_network,
        mock_process,
        LiveConfigProvider::new(app_config),
        MockInteractiveProvider::new(),
    );

    let result = execute_command_with_session(
        Commands::Add {
            mods: vec!["sodium".to_string()],
            force: false,
            platform: None,
            project_type: None,
        },
        &session,
    )
    .await;

    assert!(
        result.is_ok(),
        "Add with --dry-run should succeed: {:?}",
        result
    );

    let mods_dir = workdir.join("pack").join("mods");
    let has_pw_toml = mods_dir.exists()
        && std::fs::read_dir(&mods_dir)
            .map(|entries| entries.count() > 0)
            .unwrap_or(false);
    assert!(
        !has_pw_toml,
        "dry-run should not create any .pw.toml files in pack/mods/"
    );

    Ok(())
}
