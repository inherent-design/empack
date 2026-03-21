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
#[cfg(unix)]
#[tokio::test]
async fn test_init_zero_config() -> Result<()> {
    // Create hermetic session with mock packwiz and --yes flag
    let (session, test_env) = HermeticSessionBuilder::new()?
        .with_yes_flag() // Enable non-interactive mode
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
    Display::init_or_get(terminal_caps);

    // Use test work directory as working directory
    let workdir = test_env.work_path.clone();
    std::env::set_current_dir(&workdir)?;

    // Execute init command (--yes flag is now global in session.config)
    let result = execute_command_with_session(
        Commands::Init {
            name: None,
            pack_name: None,
            force: false,
            modloader: None,
            mc_version: None,
            author: None,
            loader_version: None,
        },
        &session,
    )
    .await;

    // Verify init succeeded
    assert!(result.is_ok(), "Init command failed: {:?}", result);

    // When no name is provided via CLI, the interactively-entered name
    // (defaulting to the directory name) becomes the target subdirectory.
    let dir_name = workdir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("Pack");
    let project_dir = workdir.join(dir_name);

    // Verify empack.yml was created inside the subdirectory
    let empack_yml_path = project_dir.join("empack.yml");
    assert!(
        empack_yml_path.exists(),
        "empack.yml should be created in subdirectory named after the modpack"
    );

    // Verify pack/ directory was created inside the subdirectory
    let pack_dir = project_dir.join("pack");
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

/// Test: empack init with explicit CLI configuration
///
/// Workflow:
/// 1. Run `empack init matrix-fabric --pack-name "Matrix Fabric" --modloader fabric --mc-version 1.21.1`
/// 2. Verify the generated empack.yml reflects the explicit inputs
/// 3. Verify the packwiz invocation received the expected progressive-init flags
#[cfg(unix)]
#[tokio::test]
async fn test_init_with_explicit_flags() -> Result<()> {
    let (session, test_env) = HermeticSessionBuilder::new()?
        .with_yes_flag()
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
    Display::init_or_get(terminal_caps);

    // Use test work directory
    let workdir = test_env.work_path.clone();
    std::env::set_current_dir(&workdir)?;

    // Execute init with explicit CLI configuration.
    let result = execute_command_with_session(
        Commands::Init {
            name: Some("matrix-fabric".to_string()),
            pack_name: Some("Matrix Fabric".to_string()),
            force: false,
            modloader: Some("fabric".to_string()),
            mc_version: Some("1.21.1".to_string()),
            author: Some("Workflow Test".to_string()),
            loader_version: None,
        },
        &session,
    )
    .await;

    assert!(result.is_ok(), "Init with name failed: {:?}", result.err());

    // When name is provided, init creates a subdirectory
    let project_dir = workdir.join("matrix-fabric");
    assert!(
        project_dir.exists(),
        "matrix-fabric directory should be created"
    );

    // Verify empack.yml was created inside the configured project directory.
    let empack_yml_path = project_dir.join("empack.yml");
    assert!(
        empack_yml_path.exists(),
        "empack.yml should be created inside matrix-fabric/"
    );

    let empack_yml = fs::read_to_string(&empack_yml_path)?;
    assert!(
        empack_yml.contains("name: \"Matrix Fabric\""),
        "empack.yml should persist the explicit pack name"
    );
    assert!(
        empack_yml.contains("author: \"Workflow Test\""),
        "empack.yml should persist the explicit author"
    );
    assert!(
        empack_yml.contains("minecraft_version: \"1.21.1\""),
        "empack.yml should persist the explicit Minecraft version"
    );
    assert!(
        empack_yml.contains("loader: fabric"),
        "empack.yml should persist the explicit loader"
    );

    // Verify pack/ directory created inside matrix-fabric/
    let pack_dir = project_dir.join("pack");
    assert!(
        pack_dir.exists(),
        "pack/ directory should be created inside matrix-fabric/"
    );

    // Verify packwiz was called
    let packwiz_calls = test_env.get_mock_invocations("packwiz")?;
    assert!(
        !packwiz_calls.is_empty(),
        "packwiz should have been called for initialization"
    );

    let init_call = packwiz_calls
        .iter()
        .find(|call| call.args.first().map(String::as_str) == Some("init"))
        .expect("packwiz init invocation should be logged");
    assert!(
        init_call.contains_args(&["--name", "Matrix Fabric"]),
        "packwiz init should receive the explicit pack name: {init_call:?}"
    );
    assert!(
        init_call.contains_args(&["--author", "Workflow Test"]),
        "packwiz init should receive the explicit author: {init_call:?}"
    );
    assert!(
        init_call.contains_args(&["--mc-version", "1.21.1"]),
        "packwiz init should receive the explicit Minecraft version: {init_call:?}"
    );
    assert!(
        init_call.contains_args(&["--modloader", "fabric"]),
        "packwiz init should receive the explicit loader: {init_call:?}"
    );
    assert!(
        init_call.args.iter().any(|arg| arg == "--fabric-version"),
        "packwiz init should resolve and pass a Fabric loader version: {init_call:?}"
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
#[cfg(unix)]
#[tokio::test]
async fn test_init_creates_directory_from_name() -> Result<()> {
    // Create hermetic session with --yes flag
    let (session, test_env) = HermeticSessionBuilder::new()?
        .with_yes_flag() // Enable non-interactive mode
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
    Display::init_or_get(terminal_caps);

    // Use test work directory
    let workdir = test_env.work_path.clone();
    std::env::set_current_dir(&workdir)?;

    // Execute init with directory name argument (--yes flag is now global in session.config)
    let result = execute_command_with_session(
        Commands::Init {
            name: Some("my-pack".to_string()),
            pack_name: None,
            force: false,
            modloader: None,
            mc_version: None,
            author: None,
            loader_version: None,
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
    assert!(project_dir.exists(), "my-pack directory should be created");
    assert!(project_dir.is_dir(), "my-pack should be a directory");

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
#[cfg(unix)]
#[tokio::test]
async fn test_init_existing_project_error() -> Result<()> {
    // Create hermetic session with --yes flag
    let (session, test_env) = HermeticSessionBuilder::new()?
        .with_yes_flag() // Enable non-interactive mode
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
    Display::init_or_get(terminal_caps);

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
            pack_name: None,
            force: false,
            modloader: None,
            mc_version: None,
            author: None, // No force flag - should fail or prompt
            loader_version: None,
        },
        &session,
    )
    .await;

    assert!(
        result.is_ok(),
        "Existing project without --force should return cleanly after refusing to overwrite"
    );

    // Existing project should be preserved and init should short-circuit before packwiz runs.
    let empack_yml_content = fs::read_to_string(&empack_yml_path)?;
    assert!(
        empack_yml_content.contains("existing-pack"),
        "Original empack.yml should be preserved (not overwritten)"
    );
    assert!(
        empack_yml_content.contains("Existing Author"),
        "Original author should be preserved"
    );
    assert!(
        test_env.get_mock_calls("packwiz")?.is_empty(),
        "Existing project detection should refuse early instead of invoking packwiz"
    );
    assert!(
        !workdir.join("pack").exists(),
        "Refused init should not create pack metadata in an existing project"
    );

    Ok(())
}
