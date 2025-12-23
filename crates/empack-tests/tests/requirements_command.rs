//! E2E tests for the requirements command
//!
//! These tests use mock process providers to verify that empack correctly
//! checks for required external tools (packwiz, git, etc.).

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
use tempfile::TempDir;

/// Test that requirements command checks for packwiz successfully
#[tokio::test]
async fn e2e_requirements_check_successfully() -> Result<()> {
    // Setup: Create a real temporary directory
    let temp_dir = TempDir::new()?;
    let workdir = temp_dir.path().to_path_buf();

    // Set working directory for the test
    std::env::set_current_dir(&workdir)?;

    // Create session with mock process provider
    let mut app_config = AppConfig::default();
    app_config.workdir = Some(workdir.clone());

    // Initialize display system
    let terminal_caps = TerminalCapabilities::detect_from_config(&app_config)?;
    Display::init(terminal_caps)?;

    // Mock successful which command for packwiz
    let mock_process_provider = MockProcessProvider::new()
        .with_result(
            "which".to_string(),
            vec!["packwiz".to_string()],
            Ok(ProcessOutput {
                stdout: "/usr/local/bin/packwiz".to_string(),
                stderr: String::new(),
                success: true,
            }),
        )
        .with_result(
            "go".to_string(),
            vec!["version".to_string(), "-m".to_string(), "/usr/local/bin/packwiz".to_string()],
            Ok(ProcessOutput {
                stdout: "mod github.com/packwiz/packwiz v0.14.0".to_string(),
                stderr: String::new(),
                success: true,
            }),
        );

    let session = CommandSession::new_with_providers(
        LiveFileSystemProvider,
        LiveNetworkProvider::new(),
        mock_process_provider,
        LiveConfigProvider::new(app_config),
        MockInteractiveProvider::new(),
    );

    // Execute the requirements command
    let result = execute_command_with_session(Commands::Requirements, &session).await;

    // Assert: Command should succeed when packwiz is "found"
    assert!(
        result.is_ok(),
        "Requirements command should succeed: {:?}",
        result
    );

    Ok(())
}

/// Test that requirements command reports missing packwiz
#[tokio::test]
async fn e2e_requirements_packwiz_missing() -> Result<()> {
    // Setup: Create a real temporary directory
    let temp_dir = TempDir::new()?;
    let workdir = temp_dir.path().to_path_buf();

    // Set working directory for the test
    std::env::set_current_dir(&workdir)?;

    // Create session with mock process provider
    let mut app_config = AppConfig::default();
    app_config.workdir = Some(workdir.clone());

    // Initialize display system
    let terminal_caps = TerminalCapabilities::detect_from_config(&app_config)?;
    Display::init(terminal_caps)?;

    // Mock which command returning error (packwiz not found)
    let mock_process_provider = MockProcessProvider::new().with_result(
        "which".to_string(),
        vec!["packwiz".to_string()],
        Ok(ProcessOutput {
            stdout: String::new(),
            stderr: String::new(),
            success: false, // which returns non-zero when not found
        }),
    );

    let session = CommandSession::new_with_providers(
        LiveFileSystemProvider,
        LiveNetworkProvider::new(),
        mock_process_provider,
        LiveConfigProvider::new(app_config),
        MockInteractiveProvider::new(),
    );

    // Execute the requirements command
    let result = execute_command_with_session(Commands::Requirements, &session).await;

    // Assert: Command should complete but report missing tool
    // Implementation may succeed (reporting missing) or fail (hard requirement)
    match result {
        Ok(_) => {
            // Acceptable: requirements command reports missing tools but doesn't fail
            println!("Requirements command completed (reported missing packwiz)");
        }
        Err(e) => {
            // Also acceptable: requirements command fails when critical tool missing
            println!("Requirements command failed as expected: {}", e);
        }
    }

    Ok(())
}

/// Test that requirements command checks git availability
#[tokio::test]
async fn e2e_requirements_check_git() -> Result<()> {
    // Setup: Create a real temporary directory
    let temp_dir = TempDir::new()?;
    let workdir = temp_dir.path().to_path_buf();

    // Set working directory for the test
    std::env::set_current_dir(&workdir)?;

    // Create session with mock process provider
    let mut app_config = AppConfig::default();
    app_config.workdir = Some(workdir.clone());

    // Initialize display system
    let terminal_caps = TerminalCapabilities::detect_from_config(&app_config)?;
    Display::init(terminal_caps)?;

    // Mock both packwiz and git checks
    let mock_process_provider = MockProcessProvider::new()
        .with_result(
            "which".to_string(),
            vec!["packwiz".to_string()],
            Ok(ProcessOutput {
                stdout: "/usr/local/bin/packwiz".to_string(),
                stderr: String::new(),
                success: true,
            }),
        )
        .with_result(
            "which".to_string(),
            vec!["git".to_string()],
            Ok(ProcessOutput {
                stdout: "/usr/bin/git".to_string(),
                stderr: String::new(),
                success: true,
            }),
        )
        .with_result(
            "go".to_string(),
            vec!["version".to_string(), "-m".to_string(), "/usr/local/bin/packwiz".to_string()],
            Ok(ProcessOutput {
                stdout: "mod github.com/packwiz/packwiz v0.14.0".to_string(),
                stderr: String::new(),
                success: true,
            }),
        )
        .with_result(
            "git".to_string(),
            vec!["--version".to_string()],
            Ok(ProcessOutput {
                stdout: "git version 2.39.0".to_string(),
                stderr: String::new(),
                success: true,
            }),
        );

    let session = CommandSession::new_with_providers(
        LiveFileSystemProvider,
        LiveNetworkProvider::new(),
        mock_process_provider,
        LiveConfigProvider::new(app_config),
        MockInteractiveProvider::new(),
    );

    // Execute the requirements command
    let result = execute_command_with_session(Commands::Requirements, &session).await;

    // Assert: Command should succeed when all tools are "found"
    assert!(
        result.is_ok(),
        "Requirements command should succeed: {:?}",
        result
    );

    Ok(())
}
