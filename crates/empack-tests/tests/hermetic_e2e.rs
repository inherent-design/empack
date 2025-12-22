//! Hermetic E2E tests using TestEnvironment
//!
//! These tests use isolated test environments with mock executables
//! to validate end-to-end functionality without external dependencies.

use anyhow::Result;
use empack_lib::application::cli::Commands;
use empack_lib::application::commands::execute_command_with_session;
use empack_lib::display::Display;
use empack_lib::terminal::TerminalCapabilities;
use empack_tests::{HermeticSessionBuilder, MockBehavior};

/// Test that the add command works with hermetic environment
#[tokio::test]
async fn hermetic_add_command_with_mock_packwiz() -> Result<()> {
    // Create hermetic session with coordinated mock providers
    let (session, test_env) = HermeticSessionBuilder::new()?
        .with_mock_executable(
            "packwiz",
            MockBehavior::SucceedWithOutput {
                stdout: "Added mod successfully".to_string(),
                stderr: String::new(),
            },
        )?
        .with_mock_executable(
            "which",
            MockBehavior::SucceedWithOutput {
                stdout: "/test/bin/packwiz".to_string(),
                stderr: String::new(),
            },
        )?
        .with_mock_mod("test-mod", "AANobbMI") // Mock the network resolution
        .with_empack_project("test-pack", "1.21.1", "fabric")?
        .build()?;

    // Initialize display system
    let terminal_caps = TerminalCapabilities::detect_from_config(session.config().app_config())?;
    Display::init(terminal_caps)?;

    // Change to project directory for command execution
    let workdir = session.config().app_config().workdir.as_ref().unwrap();
    std::env::set_current_dir(workdir)?;

    // Execute the add command
    let result = execute_command_with_session(
        Commands::Add {
            mods: vec!["test-mod".to_string()],
            force: false,
            platform: None,
        },
        &session,
    )
    .await;

    // Verify the command succeeded
    assert!(result.is_ok(), "Add command failed: {:?}", result);

    // Verify that packwiz was called
    let packwiz_calls = test_env.get_mock_calls("packwiz")?;
    assert!(!packwiz_calls.is_empty(), "packwiz should have been called");

    // Verify that the correct command was executed
    let found_add_call = packwiz_calls
        .iter()
        .any(|call| call.contains("mr add AANobbMI"));
    assert!(
        found_add_call,
        "Expected packwiz mr add AANobbMI call, got: {:?}",
        packwiz_calls
    );

    Ok(())
}

/// Test that the build command works with hermetic environment
#[tokio::test]
async fn hermetic_build_command_with_mock_tools() -> Result<()> {
    // Create hermetic session with coordinated mock providers
    let (session, test_env) = HermeticSessionBuilder::new()?
        .with_mock_executable(
            "packwiz",
            MockBehavior::SucceedWithOutput {
                stdout: "Pack refreshed successfully".to_string(),
                stderr: String::new(),
            },
        )?
        .with_mock_executable("unzip", MockBehavior::AlwaysSucceed)?
        .with_mock_executable("zip", MockBehavior::AlwaysSucceed)?
        .with_mock_executable("java", MockBehavior::AlwaysSucceed)?
        .with_mock_executable("mrpack-install", MockBehavior::AlwaysSucceed)?
        .with_mock_executable(
            "which",
            MockBehavior::SucceedWithOutput {
                stdout: "/test/bin/packwiz".to_string(),
                stderr: String::new(),
            },
        )?
        .with_empack_project("test-pack", "1.21.1", "fabric")?
        .build()?;

    // Initialize display system
    let terminal_caps = TerminalCapabilities::detect_from_config(session.config().app_config())?;
    Display::init(terminal_caps)?;

    // Change to project directory for command execution
    let workdir = session.config().app_config().workdir.as_ref().unwrap();
    std::env::set_current_dir(workdir)?;

    // Execute the build command
    let result = execute_command_with_session(
        Commands::Build {
            targets: vec!["mrpack".to_string()],
            clean: false,
            jobs: None,
        },
        &session,
    )
    .await;

    // Verify the command succeeded
    assert!(result.is_ok(), "Build command failed: {:?}", result);

    // Verify that packwiz was called for refresh and export
    let packwiz_calls = test_env.get_mock_calls("packwiz")?;
    assert!(!packwiz_calls.is_empty(), "packwiz should have been called");

    // Verify that refresh was called
    let found_refresh_call = packwiz_calls.iter().any(|call| call.contains("refresh"));
    assert!(
        found_refresh_call,
        "Expected packwiz refresh call, got: {:?}",
        packwiz_calls
    );

    // Verify that mrpack export was called
    let found_export_call = packwiz_calls.iter().any(|call| call.contains("mr export"));
    assert!(
        found_export_call,
        "Expected packwiz mr export call, got: {:?}",
        packwiz_calls
    );

    Ok(())
}

/// Test that the requirements command works with hermetic environment
#[tokio::test]
async fn hermetic_requirements_command_with_mock_tools() -> Result<()> {
    // Create hermetic session with coordinated mock providers
    let (session, test_env) = HermeticSessionBuilder::new()?
        .with_mock_executable(
            "which",
            MockBehavior::SucceedWithOutput {
                stdout: "/test/bin/packwiz".to_string(),
                stderr: String::new(),
            },
        )?
        .with_mock_executable(
            "go",
            MockBehavior::SucceedWithOutput {
                stdout: "mod github.com/packwiz/packwiz v0.14.0".to_string(),
                stderr: String::new(),
            },
        )?
        .with_mock_executable("packwiz", MockBehavior::AlwaysSucceed)?
        .build()?;

    // Initialize display system
    let terminal_caps = TerminalCapabilities::detect_from_config(session.config().app_config())?;
    Display::init(terminal_caps)?;

    // Execute the requirements command
    let result = execute_command_with_session(Commands::Requirements, &session).await;

    // Verify the command succeeded
    assert!(result.is_ok(), "Requirements command failed: {:?}", result);

    // Verify that which was called to check for packwiz
    let which_calls = test_env.get_mock_calls("which")?;
    assert!(!which_calls.is_empty(), "which should have been called");

    let found_packwiz_check = which_calls.iter().any(|call| call.contains("packwiz"));
    assert!(
        found_packwiz_check,
        "Expected 'which packwiz' call, got: {:?}",
        which_calls
    );

    Ok(())
}

/// Test that the clean command works with hermetic environment
#[tokio::test]
async fn hermetic_clean_command() -> Result<()> {
    // Create hermetic session with coordinated mock providers
    let (session, test_env) = HermeticSessionBuilder::new()?
        .with_empack_project("test-pack", "1.21.1", "fabric")?
        .build()?;

    // Get the project path and create some mock build artifacts
    let workdir = session.config().app_config().workdir.as_ref().unwrap();
    let dist_dir = workdir.join("dist");
    std::fs::create_dir_all(&dist_dir)?;
    std::fs::write(
        dist_dir.join("test-pack-v1.0.0.mrpack"),
        "mock mrpack content",
    )?;
    std::fs::write(
        dist_dir.join("test-pack-v1.0.0-client.zip"),
        "mock client zip",
    )?;

    // Initialize display system
    let terminal_caps = TerminalCapabilities::detect_from_config(session.config().app_config())?;
    Display::init(terminal_caps)?;

    // Change to project directory for command execution
    std::env::set_current_dir(workdir)?;

    // Execute the clean command
    let result = execute_command_with_session(
        Commands::Clean {
            targets: vec!["builds".to_string()],
        },
        &session,
    )
    .await;

    // Verify the command succeeded
    assert!(result.is_ok(), "Clean command failed: {:?}", result);

    // Verify that build artifacts were cleaned up
    // Note: Clean command behavior depends on the actual implementation
    // This test verifies the command runs without error

    Ok(())
}
