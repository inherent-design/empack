//! E2E tests for the build command
//!
//! These tests use real filesystems (tempfile) and mock process providers
//! to validate build workflows without requiring external packwiz installation.

use anyhow::Result;
use empack_lib::application::cli::Commands;
use empack_lib::application::commands::execute_command_with_session;
use empack_lib::application::config::AppConfig;
use empack_lib::application::session::{
    CommandSession, LiveConfigProvider, LiveFileSystemProvider, LiveNetworkProvider, ProcessOutput,
};
use empack_lib::application::session_mocks::{MockInteractiveProvider, MockProcessProvider};
use empack_lib::display::Display;
use empack_lib::terminal::TerminalCapabilities;
use empack_tests::{HermeticSessionBuilder, MockBehavior};
use std::path::Path;
use tempfile::TempDir;

/// Initialize a real empack project in the given directory for build testing
async fn initialize_empack_project(workdir: &Path) -> Result<()> {
    // Create the basic structure that empack expects
    std::fs::create_dir_all(workdir.join("pack"))?;

    // Create empack.yml
    let empack_yml = r#"empack:
  dependencies:
    - "fabric_api: Fabric API|mod"
  minecraft_version: "1.21.1"
  loader: fabric
  name: "Test Modpack"
  author: "Test Author"
  version: "1.0.0"
"#;
    std::fs::write(workdir.join("empack.yml"), empack_yml)?;

    // Create pack.toml
    let pack_toml = r#"name = "Test Modpack"
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

    // Create index.toml
    let index_toml = r#"hash-format = "sha256"

[[files]]
file = "pack.toml"
hash = ""
"#;
    std::fs::write(workdir.join("pack").join("index.toml"), index_toml)?;

    Ok(())
}

/// Test that the build command works end-to-end with mock packwiz
#[tokio::test]
async fn e2e_build_mrpack_successfully() -> Result<()> {
    let (session, test_env) = HermeticSessionBuilder::new()?
        .with_empack_project("workflow-build-pack", "1.21.1", "fabric")?
        .with_mock_executable(
            "packwiz",
            MockBehavior::SucceedWithOutput {
                stdout: "Refreshed packwiz index\nExported to workflow-build-pack-v1.0.0.mrpack"
                    .to_string(),
                stderr: String::new(),
            },
        )?
        .build()?;

    let terminal_caps = TerminalCapabilities::detect_from_config(session.config().app_config())?;
    Display::init(terminal_caps)?;

    let workdir = session
        .config()
        .app_config()
        .workdir
        .clone()
        .expect("hermetic project should configure a workdir");
    std::env::set_current_dir(&workdir)?;

    let result = execute_command_with_session(
        Commands::Build {
            targets: vec!["mrpack".to_string()],
            clean: false,
            jobs: None,
        },
        &session,
    )
    .await;

    assert!(result.is_ok(), "Build command failed: {:?}", result);

    let mrpack_path = workdir
        .join("dist")
        .join("workflow-build-pack-v1.0.0.mrpack");
    assert!(
        mrpack_path.exists(),
        "mrpack build should create an artifact in dist/: {}",
        mrpack_path.display()
    );

    let packwiz_calls = test_env.get_mock_calls("packwiz")?;
    assert!(
        packwiz_calls
            .iter()
            .any(|call| call.contains("packwiz --pack-file") && call.contains(" refresh")),
        "build should refresh the pack before exporting: {packwiz_calls:?}"
    );
    assert!(
        packwiz_calls.iter().any(|call| {
            call.contains("packwiz --pack-file")
                && call.contains(" mr export ")
                && call.contains("workflow-build-pack-v1.0.0.mrpack")
        }),
        "build should export the mrpack artifact through packwiz: {packwiz_calls:?}"
    );

    Ok(())
}

/// Test that build command fails gracefully when packwiz refresh fails
#[tokio::test]
async fn e2e_build_packwiz_refresh_fails() -> Result<()> {
    // Setup: Create a real temporary directory
    let temp_dir = TempDir::new()?;
    let workdir = temp_dir.path().to_path_buf();

    // Initialize: Create a real empack project
    initialize_empack_project(&workdir).await?;

    // Set working directory for the test
    std::env::set_current_dir(&workdir)?;

    // Create hybrid session with failing packwiz mock
    let mut app_config = AppConfig::default();
    app_config.workdir = Some(workdir.clone());

    // Initialize display system
    let terminal_caps = TerminalCapabilities::detect_from_config(&app_config)?;
    let _ = Display::init(terminal_caps);

    // Mock packwiz refresh failure
    let pack_file = workdir.join("pack/pack.toml");
    let mock_process_provider = MockProcessProvider::new().with_packwiz_result(
        vec![
            "--pack-file".to_string(),
            pack_file.to_string_lossy().to_string(),
            "refresh".to_string(),
        ],
        Ok(ProcessOutput {
            stdout: String::new(),
            stderr: "Error: pack.toml is corrupted".to_string(),
            success: false,
        }),
    );

    let session = CommandSession::new_with_providers(
        LiveFileSystemProvider,
        LiveNetworkProvider::new(),
        mock_process_provider,
        LiveConfigProvider::new(app_config),
        MockInteractiveProvider::new(),
    );

    // Execute the build command (may succeed with warnings or fail)
    let result = execute_command_with_session(
        Commands::Build {
            targets: vec!["mrpack".to_string()],
            clean: false,
            jobs: None,
        },
        &session,
    )
    .await;

    assert!(
        result.is_err(),
        "Build should fail fast when packwiz refresh returns a non-zero exit code"
    );
    let error = result.unwrap_err().to_string();
    assert!(
        error.contains("Failed to execute build pipeline"),
        "Refresh failure should propagate a clear packwiz error, got: {error}"
    );
    assert!(
        !workdir.join("dist/test-modpack-v1.0.0.mrpack").exists(),
        "No mrpack artifact should be produced after a failed refresh"
    );

    Ok(())
}
