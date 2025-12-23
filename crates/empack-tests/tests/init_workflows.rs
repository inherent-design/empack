//! Integration tests for `empack init` workflow
//!
//! Tests the complete initialization workflow across different scenarios:
//! - Zero-config init with defaults
//! - Explicit configuration via flags
//! - Directory creation from name argument
//! - Error handling for existing projects

use anyhow::Result;
use empack_lib::application::cli::Commands;
use empack_lib::application::commands::execute_command_with_session;
use empack_lib::display::Display;
use empack_lib::terminal::TerminalCapabilities;
use empack_tests::{HermeticSessionBuilder, MockBehavior};
use std::fs;

/// Test: empack init -y (zero-config with API-driven defaults)
///
/// Workflow:
/// 1. Run `empack init -y` in temp directory
/// 2. Verify empack.yml created with reasonable defaults
/// 3. Verify pack/ directory created
/// 4. Verify packwiz init was called
/// 5. Verify project structure matches expected layout
#[tokio::test]
async fn test_init_zero_config() -> Result<()> {
    // Create hermetic session with mock packwiz and --yes flag
    let (session, test_env) = HermeticSessionBuilder::new()?
        .with_yes_flag()  // Enable non-interactive mode
        .with_mock_executable(
            "packwiz",
            MockBehavior::SucceedWithOutput {
                stdout: "Initialized packwiz project".to_string(),
                stderr: String::new(),
            },
        )?
        .with_mock_executable(
            "git",
            MockBehavior::SucceedWithOutput {
                stdout: "main".to_string(),
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
        .build()?;

    // Initialize display
    let terminal_caps = TerminalCapabilities::detect_from_config(session.config().app_config())?;
    Display::init(terminal_caps)?;

    // Use test work directory as working directory
    let workdir = test_env.work_path.clone();
    std::env::set_current_dir(&workdir)?;

    // Execute init command (--yes flag is now global in session.config)
    let result = execute_command_with_session(
        Commands::Init {
            name: None,
            force: false,
        },
        &session,
    )
    .await;

    // Verify init succeeded
    assert!(result.is_ok(), "Init command failed: {:?}", result);

    // Verify empack.yml was created
    let empack_yml_path = workdir.join("empack.yml");
    assert!(
        empack_yml_path.exists(),
        "empack.yml should be created in work directory"
    );

    // Verify pack/ directory was created
    let pack_dir = workdir.join("pack");
    assert!(pack_dir.exists(), "pack/ directory should be created");

    // Verify packwiz init was called
    let packwiz_calls = test_env.get_mock_calls("packwiz")?;
    assert!(
        !packwiz_calls.is_empty(),
        "packwiz should have been called for initialization"
    );

    // Verify pack.toml exists in pack/
    let pack_toml_path = pack_dir.join("pack.toml");
    assert!(
        pack_toml_path.exists(),
        "pack.toml should exist after packwiz init"
    );

    Ok(())
}

/// Test: empack init with explicit name and interactive prompts
///
/// Workflow:
/// 1. Run `empack init test-pack` (name provided, yes=false to trigger prompts)
/// 2. Mock interactive provider will return defaults for all prompts
/// 3. Verify empack.yml created with expected configuration
///
/// Note: MockInteractiveProvider returns the default value when no specific response is configured.
/// This tests that the interactive flow works correctly with defaults.
#[tokio::test]
async fn test_init_with_explicit_flags() -> Result<()> {
    use empack_lib::application::session_mocks::MockInteractiveProvider;

    // Configure mock interactive responses to return defaults
    // (not setting specific responses means defaults will be used)
    let interactive = MockInteractiveProvider::new()
        .with_select(0)           // Select first loader
        .with_fuzzy_select(0);    // Select first MC version

    // Create hermetic session with configured interactive provider
    let (session, test_env) = HermeticSessionBuilder::new()?
        .with_interactive_provider(interactive)
        .with_mock_executable(
            "packwiz",
            MockBehavior::SucceedWithOutput {
                stdout: "Initialized packwiz project".to_string(),
                stderr: String::new(),
            },
        )?
        .with_mock_executable(
            "git",
            MockBehavior::SucceedWithOutput {
                stdout: "main".to_string(),
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
        .build()?;

    // Initialize display
    let terminal_caps = TerminalCapabilities::detect_from_config(session.config().app_config())?;
    Display::init(terminal_caps)?;

    // Use test work directory
    let workdir = test_env.work_path.clone();
    std::env::set_current_dir(&workdir)?;

    // Execute init with name specified (interactive prompts will use mock provider)
    let result = execute_command_with_session(
        Commands::Init {
            name: Some("test-pack".to_string()),
            force: false,
        },
        &session,
    )
    .await;

    assert!(
        result.is_ok(),
        "Init with name failed: {:?}",
        result.err()
    );

    // When name is provided, init creates a subdirectory
    let project_dir = workdir.join("test-pack");
    assert!(
        project_dir.exists(),
        "test-pack directory should be created"
    );

    // Verify empack.yml was created inside test-pack/
    let empack_yml_path = project_dir.join("empack.yml");
    assert!(
        empack_yml_path.exists(),
        "empack.yml should be created inside test-pack/"
    );

    // Verify pack/ directory created inside test-pack/
    let pack_dir = project_dir.join("pack");
    assert!(
        pack_dir.exists(),
        "pack/ directory should be created inside test-pack/"
    );

    // Verify packwiz was called
    let packwiz_calls = test_env.get_mock_calls("packwiz")?;
    assert!(
        !packwiz_calls.is_empty(),
        "packwiz should have been called for initialization"
    );

    Ok(())
}

/// Test: empack init my-pack (creates directory and initializes inside)
///
/// Workflow:
/// 1. Run `empack init my-pack -y` where my-pack doesn't exist
/// 2. Verify my-pack/ directory was created
/// 3. Verify empack.yml exists inside my-pack/
/// 4. Verify pack/ directory created inside my-pack/
#[tokio::test]
async fn test_init_creates_directory_from_name() -> Result<()> {
    // Create hermetic session with --yes flag
    let (session, test_env) = HermeticSessionBuilder::new()?
        .with_yes_flag()  // Enable non-interactive mode
        .with_mock_executable(
            "packwiz",
            MockBehavior::SucceedWithOutput {
                stdout: "Initialized packwiz project".to_string(),
                stderr: String::new(),
            },
        )?
        .with_mock_executable(
            "git",
            MockBehavior::SucceedWithOutput {
                stdout: "main".to_string(),
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
        .build()?;

    // Initialize display
    let terminal_caps = TerminalCapabilities::detect_from_config(session.config().app_config())?;
    Display::init(terminal_caps)?;

    // Use test work directory
    let workdir = test_env.work_path.clone();
    std::env::set_current_dir(&workdir)?;

    // Execute init with directory name argument (--yes flag is now global in session.config)
    let result = execute_command_with_session(
        Commands::Init {
            name: Some("my-pack".to_string()),
            force: false,
        },
        &session,
    )
    .await;

    assert!(
        result.is_ok(),
        "Init with name argument failed: {:?}",
        result.err()
    );

    // Verify my-pack directory was created
    let project_dir = workdir.join("my-pack");
    assert!(
        project_dir.exists(),
        "my-pack directory should be created"
    );
    assert!(
        project_dir.is_dir(),
        "my-pack should be a directory"
    );

    // Verify empack.yml exists inside my-pack/
    let empack_yml_path = project_dir.join("empack.yml");
    assert!(
        empack_yml_path.exists(),
        "empack.yml should exist inside my-pack/"
    );

    // Verify pack/ directory exists inside my-pack/
    let pack_dir = project_dir.join("pack");
    assert!(
        pack_dir.exists(),
        "pack/ directory should exist inside my-pack/"
    );

    Ok(())
}

/// Test: empack init in directory with existing empack.yml (error handling)
///
/// Workflow:
/// 1. Create empack.yml in work directory
/// 2. Run `empack init -y` (should detect existing project)
/// 3. Verify appropriate error handling (either prompt or fail gracefully)
///
/// Note: Without --force flag, init should detect existing project
#[tokio::test]
async fn test_init_existing_project_error() -> Result<()> {
    // Create hermetic session with --yes flag
    let (session, test_env) = HermeticSessionBuilder::new()?
        .with_yes_flag()  // Enable non-interactive mode
        .with_mock_executable(
            "packwiz",
            MockBehavior::SucceedWithOutput {
                stdout: "Initialized packwiz project".to_string(),
                stderr: String::new(),
            },
        )?
        .with_mock_executable(
            "git",
            MockBehavior::SucceedWithOutput {
                stdout: "main".to_string(),
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
        .build()?;

    // Initialize display
    let terminal_caps = TerminalCapabilities::detect_from_config(session.config().app_config())?;
    Display::init(terminal_caps)?;

    // Use test work directory
    let workdir = test_env.work_path.clone();
    std::env::set_current_dir(&workdir)?;

    // Create existing empack.yml to simulate existing project
    let empack_yml_path = workdir.join("empack.yml");
    fs::write(
        &empack_yml_path,
        r#"empack:
  dependencies: []
  minecraft_version: "1.21.1"
  loader: fabric
  name: "existing-pack"
  author: "Existing Author"
  version: "1.0.0"
"#,
    )?;

    // Execute init command (should detect existing project)
    let result = execute_command_with_session(
        Commands::Init {
            name: None,
            force: false, // No force flag - should fail or prompt
        },
        &session,
    )
    .await;

    // Init should either:
    // 1. Return error (preferred for non-interactive mode)
    // 2. Skip initialization gracefully
    // Either way, the original empack.yml should be unchanged
    let empack_yml_content = fs::read_to_string(&empack_yml_path)?;
    assert!(
        empack_yml_content.contains("existing-pack"),
        "Original empack.yml should be preserved (not overwritten)"
    );
    assert!(
        empack_yml_content.contains("Existing Author"),
        "Original author should be preserved"
    );

    // If result is error, that's expected behavior
    if result.is_err() {
        // Error is acceptable for existing project without --force
        println!("Init correctly detected existing project and returned error");
    }

    Ok(())
}
