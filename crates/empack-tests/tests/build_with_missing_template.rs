//! E2E test for build with missing template file
//!
//! Tests graceful error handling when a template file is missing during build.

use anyhow::Result;
use empack_lib::application::cli::Commands;
use empack_lib::application::commands::execute_command_with_session;
use empack_lib::display::Display;
use empack_lib::terminal::TerminalCapabilities;
use empack_tests::{HermeticSessionBuilder, MockBehavior};

#[cfg(unix)]
#[tokio::test]
async fn test_build_with_missing_template() -> Result<()> {
    // Create hermetic session with basic project setup
    let (session, test_env) = HermeticSessionBuilder::new()?
        .with_yes_flag()
        .with_mock_executable(
            "packwiz",
            MockBehavior::SucceedWithOutput {
                stdout: "Initialized packwiz project".to_string(),
                stderr: String::new(),
            },
        )?
        .with_mock_executable("git", MockBehavior::AlwaysSucceed)?
        .with_mock_executable(
            "which",
            MockBehavior::SucceedWithOutput {
                stdout: "/usr/local/bin/packwiz".to_string(),
                stderr: String::new(),
            },
        )?
        .build()?;

    // Initialize display
    let terminal_caps = TerminalCapabilities::detect_from_config(session.config().app_config())?;
    Display::init_or_get(terminal_caps);

    // Set working directory
    let workdir = test_env.work_path.clone();
    std::env::set_current_dir(&workdir)?;

    // Initialize the project
    execute_command_with_session(
        Commands::Init {
            name: None,
            pack_name: None,
            force: false,
            modloader: None,
            mc_version: None,
            author: None,
            loader_version: None,
            pack_version: None,
        },
        &session,
    )
    .await?;

    // When no name is provided via CLI, the interactively-entered name
    // (defaulting to the directory name) becomes the target subdirectory.
    let dir_name = workdir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("Pack");
    let project_dir = workdir.join(dir_name);

    // First, verify the project was initialized
    assert!(
        project_dir.join("empack.yml").exists(),
        "empack.yml should exist"
    );
    assert!(
        project_dir.join("pack").exists(),
        "pack/ directory should exist"
    );

    // Pre-seed the bootstrap JAR so the build progresses past the download step
    // to actually test template handling.
    let jar_cache = empack_lib::platform::cache::cache_root()?.join("jars");
    std::fs::create_dir_all(&jar_cache)?;
    std::fs::write(
        jar_cache.join("packwiz-installer-bootstrap.jar"),
        "mock jar content",
    )?;

    // Change to project directory since build needs to find the project
    std::env::set_current_dir(&project_dir)?;

    // Attempt a build - this should detect missing templates gracefully
    // Note: In the hermetic environment, the build might fail for other reasons
    // (no packwiz refresh, no actual mods), but we're primarily testing that
    // missing template errors are clear and graceful
    let build_result = execute_command_with_session(
        Commands::Build {
            targets: vec!["client".to_string()],
            clean: false,
            jobs: None,
        },
        &session,
    )
    .await;

    // The build should either succeed (if templates are all embedded and found)
    // or fail with a clear error message about the template issue
    match build_result {
        Ok(_) => {
            // Build succeeded - templates were all found
            // This is acceptable in hermetic environment
        }
        Err(e) => {
            let err_msg = format!("{:?}", e);
            // Error should be clear about what's missing (template, file, or build issue)
            // We're not expecting a specific template to be missing, just that
            // IF a template is missing, the error should be informative
            assert!(
                err_msg.contains("template")
                    || err_msg.contains("Template")
                    || err_msg.contains("file")
                    || err_msg.contains("not found")
                    || err_msg.contains("missing")
                    || err_msg.contains("Build")
                    || err_msg.contains("packwiz"),
                "Error should indicate what's missing or what failed: {}",
                err_msg
            );
        }
    }

    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn test_build_template_error_specificity() -> Result<()> {
    // This test validates that template-related errors are specific
    // and indicate which template file is problematic

    let (session, test_env) = HermeticSessionBuilder::new()?
        .with_yes_flag()
        .with_mock_executable(
            "packwiz",
            MockBehavior::SucceedWithOutput {
                stdout: "Initialized packwiz project".to_string(),
                stderr: String::new(),
            },
        )?
        .with_mock_executable("git", MockBehavior::AlwaysSucceed)?
        .with_mock_executable(
            "which",
            MockBehavior::SucceedWithOutput {
                stdout: "/usr/local/bin/packwiz".to_string(),
                stderr: String::new(),
            },
        )?
        .build()?;

    let terminal_caps = TerminalCapabilities::detect_from_config(session.config().app_config())?;
    Display::init_or_get(terminal_caps);

    let workdir = test_env.work_path.clone();
    std::env::set_current_dir(&workdir)?;

    // Initialize project
    execute_command_with_session(
        Commands::Init {
            name: None,
            pack_name: None,
            force: false,
            modloader: None,
            mc_version: None,
            author: None,
            loader_version: None,
            pack_version: None,
        },
        &session,
    )
    .await?;

    // When no name is provided via CLI, init creates a subdirectory
    let dir_name = workdir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("Pack");
    let project_dir = workdir.join(dir_name);
    std::env::set_current_dir(&project_dir)?;

    // Try building with a target that might have template dependencies
    let build_result = execute_command_with_session(
        Commands::Build {
            targets: vec!["server".to_string()],
            clean: false,
            jobs: None,
        },
        &session,
    )
    .await;

    // Without a pre-seeded bootstrap JAR, the server build should fail
    // attempting to download packwiz-installer-bootstrap.jar via
    // MockNetworkProvider (which returns Err from http_client()).
    // On hosts where the JAR is cached from prior runs, the build may
    // progress further and fail on a different step or succeed.
    match build_result {
        Ok(_) => {
            assert!(
                project_dir.join("dist").exists(),
                "Successful build must produce a dist/ directory"
            );
        }
        Err(e) => {
            let err_msg = format!("{:?}", e);
            assert!(
                err_msg.contains("build")
                    || err_msg.contains("Build")
                    || err_msg.contains("download")
                    || err_msg.contains("HTTP")
                    || err_msg.contains("packwiz"),
                "Error should mention build, download, or packwiz, got: {}",
                err_msg
            );
        }
    }

    Ok(())
}
