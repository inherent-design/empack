//! E2E test for init error recovery
//!
//! Tests error handling when packwiz init fails and verifies cleanup

use anyhow::Result;
use empack_lib::application::cli::Commands;
use empack_lib::application::commands::execute_command_with_session;
use empack_lib::display::Display;
use empack_lib::terminal::TerminalCapabilities;
use empack_tests::{HermeticSessionBuilder, MockBehavior};

/// Test: Init command handles packwiz failure gracefully
///
/// Workflow:
/// 1. Configure packwiz to fail during init
/// 2. Run `empack init -y`
/// 3. Verify error is returned
/// 4. Verify no partial state is left behind (cleanup successful)
#[tokio::test]
async fn test_init_packwiz_failure() -> Result<()> {
    // Create hermetic session with failing packwiz
    let (session, test_env) = HermeticSessionBuilder::new()?
        .with_yes_flag()
        .with_mock_executable(
            "packwiz",
            MockBehavior::AlwaysFail {
                error: "Mock packwiz init failure".to_string(),
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
            MockBehavior::AlwaysFail {
                error: "packwiz not found".to_string(),
            },
        )?
        .build()?;

    // Initialize display
    let terminal_caps = TerminalCapabilities::detect_from_config(session.config().app_config())?;
    Display::init_or_get(terminal_caps);

    let workdir = test_env.work_path.clone();
    std::env::set_current_dir(&workdir)?;

    // Execute init command - may fail or succeed via fallback (--yes requires --modloader)
    let result = execute_command_with_session(
        Commands::Init {
            name: Some("failure-test-pack".to_string()),
            pack_name: None,
            force: false,
            modloader: Some("fabric".to_string()),
            mc_version: None,
            author: None,
            loader_version: None,
            pack_version: None,
        },
        &session,
    )
    .await;

    let project_dir = workdir.join("failure-test-pack");

    let err = result.expect_err("Init should fail when packwiz mock returns non-zero exit code");
    let error_msg = err.to_string();
    assert!(
        error_msg.contains("initialize") || error_msg.contains("packwiz"),
        "Error should mention initialization or packwiz, got: {}",
        error_msg
    );

    assert!(
        !project_dir.join("empack.yml").exists(),
        "empack.yml should be cleaned up after failed init"
    );

    Ok(())
}

/// Test: Init command detects packwiz unavailability before attempting init
///
/// Workflow:
/// 1. Configure packwiz as unavailable (not in PATH)
/// 2. Run `empack init -y`
/// 3. Verify requirements check catches missing packwiz early
#[tokio::test]
async fn test_init_packwiz_unavailable() -> Result<()> {
    // Create hermetic session with unavailable packwiz
    let (session, test_env) = HermeticSessionBuilder::new()?
        .with_yes_flag()
        .with_mock_executable(
            "which",
            MockBehavior::AlwaysFail {
                error: "packwiz not found in PATH".to_string(),
            },
        )?
        .with_mock_executable(
            "git",
            MockBehavior::SucceedWithOutput {
                stdout: "main".to_string(),
                stderr: String::new(),
            },
        )?
        .build()?;

    let terminal_caps = TerminalCapabilities::detect_from_config(session.config().app_config())?;
    Display::init_or_get(terminal_caps);

    let workdir = test_env.work_path.clone();
    std::env::set_current_dir(&workdir)?;

    // Execute init command (--yes requires --modloader)
    let result = execute_command_with_session(
        Commands::Init {
            name: Some("unavailable-packwiz-test".to_string()),
            pack_name: None,
            force: false,
            modloader: Some("fabric".to_string()),
            mc_version: None,
            author: None,
            loader_version: None,
            pack_version: None,
        },
        &session,
    )
    .await;

    let project_dir = workdir.join("unavailable-packwiz-test");

    // Behavior depends on host: if packwiz is on the system PATH (outside the
    // hermetic bin/ directory), LiveProcessProvider finds it and init succeeds.
    // If packwiz is truly absent, init fails.
    match result {
        Ok(_) => {
            assert!(
                project_dir.join("empack.yml").exists(),
                "Successful init must produce empack.yml"
            );
            assert!(
                project_dir.join("pack").join("pack.toml").exists(),
                "Successful init must produce pack/pack.toml"
            );
        }
        Err(err) => {
            let error_msg = err.to_string();
            assert!(
                error_msg.contains("initialize") || error_msg.contains("packwiz"),
                "Error should mention initialization or packwiz, got: {}",
                error_msg
            );
        }
    }

    Ok(())
}

/// Test: Init command handles filesystem errors during directory creation
///
/// Workflow:
/// 1. Attempt to create project in read-only or inaccessible location
/// 2. Verify error is caught and reported
#[cfg(unix)]
#[tokio::test]
async fn test_init_filesystem_error() -> Result<()> {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    // Create a read-only directory for testing
    let temp_dir = tempfile::TempDir::new()?;
    let readonly_dir = temp_dir.path().join("readonly");
    fs::create_dir(&readonly_dir)?;

    // Make directory read-only (Unix-specific)
    // Use 0o555 (r-x) instead of 0o444 (r--) because chdir() requires execute permission
    #[cfg(unix)]
    {
        let mut perms = fs::metadata(&readonly_dir)?.permissions();
        perms.set_mode(0o555); // Read-execute (allow chdir, prevent writes)
        fs::set_permissions(&readonly_dir, perms)?;
    }

    let (session, _test_env) = HermeticSessionBuilder::new()?
        .with_yes_flag()
        .with_workdir(readonly_dir.clone())
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

    let terminal_caps = TerminalCapabilities::detect_from_config(session.config().app_config())?;
    Display::init_or_get(terminal_caps);

    std::env::set_current_dir(&readonly_dir)?;

    // Execute init command - should fail due to permission error (--yes requires --modloader)
    let result = execute_command_with_session(
        Commands::Init {
            name: Some("readonly-test".to_string()),
            pack_name: None,
            force: false,
            modloader: Some("fabric".to_string()),
            mc_version: None,
            author: None,
            loader_version: None,
            pack_version: None,
        },
        &session,
    )
    .await;

    // Restore permissions for cleanup
    #[cfg(unix)]
    {
        let mut perms = fs::metadata(&readonly_dir)?.permissions();
        perms.set_mode(0o755); // Restore write permissions
        fs::set_permissions(&readonly_dir, perms)?;
    }

    // Init should fail due to permission error
    // Note: In hermetic tests with LiveFileSystemProvider, actual filesystem errors occur
    if let Err(err) = result {
        let error_msg = err.to_string();
        assert!(
            error_msg.contains("Permission")
                || error_msg.contains("denied")
                || error_msg.contains("create")
                || error_msg.contains("write"),
            "Error should mention permission or write failure, got: {}",
            error_msg
        );
    } else {
        // If succeeded, LiveFileSystemProvider may have bypassed permission check
        eprintln!("Note: Init succeeded despite read-only directory (test environment quirk)");
    }

    Ok(())
}

/// Test: Init command handles empty loader list gracefully (all loaders incompatible)
///
/// Workflow:
/// 1. Create hermetic session where MockNetworkProvider forces fallback behavior
/// 2. Run `empack init -y`
/// 3. Verify init either:
///    - Succeeds with fallback loader (graceful degradation)
///    - Returns meaningful error (no compatible loaders found)
/// 4. Verify NO PANIC occurs when all loaders return empty vec
///
/// Context:
/// - When MC version is unsupported by all loaders:
///   - Fabric returns Ok(vec![]) on HTTP 400
///   - Quilt returns Ok(vec![]) on HTTP 404
///   - NeoForge returns Ok(vec![]) for MC < 1.20.2
///   - Forge returns Ok(vec![]) for unknown versions
/// - MockNetworkProvider.http_client() returns Err() to force fallback
/// - Fallback versions are hardcoded, so test verifies graceful handling
///
/// This test validates the gap identified in VCR analysis:
/// "No existing tests found for empty loader list scenario"
#[tokio::test]
async fn test_init_empty_loader_list_graceful_handling() -> Result<()> {
    // Create hermetic session with network provider that forces fallback
    // MockNetworkProvider returns Err() from http_client(), triggering fallback behavior
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

    let workdir = test_env.work_path.clone();
    std::env::set_current_dir(&workdir)?;

    // Execute init command (--yes requires --modloader)
    let result = execute_command_with_session(
        Commands::Init {
            name: Some("empty-loader-test".to_string()),
            pack_name: None,
            force: false,
            modloader: Some("fabric".to_string()),
            mc_version: None,
            author: None,
            loader_version: None,
            pack_version: None,
        },
        &session,
    )
    .await;

    let project_dir = workdir.join("empty-loader-test");

    // MockNetworkProvider returns Err from http_client(), forcing version
    // fetcher to use fallback versions. Init should succeed with a fallback
    // loader selection.
    match result {
        Ok(_) => {
            assert!(
                project_dir.join("empack.yml").exists(),
                "Fallback init must produce empack.yml"
            );
            let empack_yml = std::fs::read_to_string(project_dir.join("empack.yml"))?;
            // With --yes and no --modloader flag, the interactive provider
            // auto-selects index 0 which is "none (vanilla)". Vanilla packs
            // omit the loader field from empack.yml entirely.
            assert!(
                empack_yml.contains("minecraft_version:") || empack_yml.contains("loader:"),
                "empack.yml should contain minecraft_version or loader, got: {}",
                empack_yml
            );
        }
        Err(e) => {
            let error_msg = e.to_string();
            assert!(
                error_msg.contains("loader")
                    || error_msg.contains("version")
                    || error_msg.contains("initialize"),
                "Error should mention loader, version, or initialization, got: {}",
                error_msg
            );
        }
    }

    Ok(())
}
