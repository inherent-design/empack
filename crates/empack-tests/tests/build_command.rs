//! E2E tests for the build command
//!
//! These tests use real filesystems (tempfile) and mock process providers
//! to validate build workflows without requiring external packwiz installation.

use anyhow::Result;
use empack_lib::application::cli::{CliArchiveFormat, Commands};
use empack_lib::application::commands::execute_command_with_session;
use empack_lib::application::config::AppConfig;
use empack_lib::application::session::{
    CommandSession, LiveConfigProvider, LiveFileSystemProvider, LiveNetworkProvider, ProcessOutput,
};
use empack_lib::application::session_mocks::{MockInteractiveProvider, MockProcessProvider};
use empack_lib::display::Display;
use empack_lib::terminal::TerminalCapabilities;
use empack_tests::fixtures::{WorkflowArtifact, WorkflowProjectFixture};
use empack_tests::{HermeticSessionBuilder, MockBehavior};
use tempfile::TempDir;

/// Test that the build command works end-to-end with mock packwiz
#[cfg(unix)]
#[tokio::test]
async fn e2e_build_mrpack_successfully() -> Result<()> {
    let fixture = WorkflowProjectFixture::new("workflow-build-pack");
    let (session, test_env) = HermeticSessionBuilder::new()?
        .with_empack_project(
            &fixture.pack_name,
            &fixture.minecraft_version,
            &fixture.loader,
        )?
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
    Display::init_or_get(terminal_caps);

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
            format: CliArchiveFormat::Zip,
        },
        &session,
    )
    .await;

    assert!(result.is_ok(), "Build command failed: {:?}", result);

    let pack_file = workdir.join("pack").join("pack.toml");
    let mrpack_path = fixture.artifact_path(&workdir, WorkflowArtifact::Mrpack);
    assert!(
        mrpack_path.exists(),
        "mrpack build should create an artifact in dist/: {}",
        mrpack_path.display()
    );

    let packwiz_calls = test_env.get_mock_invocations("packwiz")?;
    assert!(
        packwiz_calls.iter().any(|call| {
            call.args
                == vec![
                    "--pack-file".to_string(),
                    pack_file.to_string_lossy().to_string(),
                    "refresh".to_string(),
                ]
        }),
        "build should refresh the pack before exporting: {packwiz_calls:?}"
    );
    assert!(
        packwiz_calls.iter().any(|call| {
            call.args
                == vec![
                    "--pack-file".to_string(),
                    pack_file.to_string_lossy().to_string(),
                    "mr".to_string(),
                    "export".to_string(),
                    "-o".to_string(),
                    mrpack_path.to_string_lossy().to_string(),
                ]
        }),
        "build should export the mrpack artifact through packwiz: {packwiz_calls:?}"
    );

    Ok(())
}

/// Test that clean-before-build rebuilds the mrpack artifact without removing config files
#[cfg(unix)]
#[tokio::test]
async fn e2e_build_clean_recreates_mrpack_and_preserves_configuration() -> Result<()> {
    let fixture = WorkflowProjectFixture::new("workflow-build-clean");
    let (session, test_env) = HermeticSessionBuilder::new()?
        .with_empack_project(
            &fixture.pack_name,
            &fixture.minecraft_version,
            &fixture.loader,
        )?
        .with_mock_executable(
            "packwiz",
            MockBehavior::SucceedWithOutput {
                stdout: format!(
                    "Refreshed packwiz index\nExported to {}",
                    fixture.artifact_file_name(WorkflowArtifact::Mrpack)
                ),
                stderr: String::new(),
            },
        )?
        .build()?;

    let terminal_caps = TerminalCapabilities::detect_from_config(session.config().app_config())?;
    Display::init_or_get(terminal_caps);

    let workdir = session
        .config()
        .app_config()
        .workdir
        .clone()
        .expect("hermetic project should configure a workdir");
    std::env::set_current_dir(&workdir)?;

    let stale_server_dir = workdir.join("dist").join("server");
    std::fs::create_dir_all(&stale_server_dir)?;
    std::fs::write(stale_server_dir.join("stale.txt"), "stale build output")?;

    let mrpack_path = fixture.artifact_path(&workdir, WorkflowArtifact::Mrpack);
    std::fs::write(&mrpack_path, "stale mrpack artifact")?;

    let sentinel = workdir.join("sentinel.txt");
    std::fs::write(&sentinel, "preserve me")?;

    let pack_file = workdir.join("pack").join("pack.toml");
    let result = execute_command_with_session(
        Commands::Build {
            targets: vec!["mrpack".to_string()],
            clean: true,
            format: CliArchiveFormat::Zip,
        },
        &session,
    )
    .await;

    assert!(
        result.is_ok(),
        "clean-before-build should rebuild the mrpack artifact: {result:?}"
    );
    assert!(
        mrpack_path.exists(),
        "clean-before-build should recreate the mrpack artifact in dist/: {}",
        mrpack_path.display()
    );
    let rebuilt_bytes = std::fs::read(&mrpack_path)?;
    assert_ne!(
        rebuilt_bytes,
        b"stale mrpack artifact",
        "clean-before-build should replace stale artifact contents with the rebuilt artifact"
    );
    assert!(
        rebuilt_bytes.starts_with(b"PK"),
        "rebuilt mrpack should be a valid zip archive"
    );
    assert!(
        !stale_server_dir.exists(),
        "clean-before-build should remove stale sibling build directories under dist/"
    );
    assert!(
        workdir.join("empack.yml").exists(),
        "clean-before-build should preserve empack.yml"
    );
    assert!(
        workdir.join("pack").join("pack.toml").exists(),
        "clean-before-build should preserve pack.toml"
    );
    assert!(
        workdir.join("pack").join("index.toml").exists(),
        "clean-before-build should preserve index.toml"
    );
    assert!(
        sentinel.exists(),
        "clean-before-build should not remove unrelated project files outside dist/"
    );

    let packwiz_calls = test_env.get_mock_invocations("packwiz")?;
    assert!(
        packwiz_calls.iter().any(|call| {
            call.args
                == vec![
                    "--pack-file".to_string(),
                    pack_file.to_string_lossy().to_string(),
                    "refresh".to_string(),
                ]
        }),
        "clean-before-build should refresh the pack after cleaning: {packwiz_calls:?}"
    );
    assert!(
        packwiz_calls.iter().any(|call| {
            call.args
                == vec![
                    "--pack-file".to_string(),
                    pack_file.to_string_lossy().to_string(),
                    "mr".to_string(),
                    "export".to_string(),
                    "-o".to_string(),
                    mrpack_path.to_string_lossy().to_string(),
                ]
        }),
        "clean-before-build should export the rebuilt mrpack artifact: {packwiz_calls:?}"
    );

    Ok(())
}

/// Test that build command fails gracefully when packwiz refresh fails
#[tokio::test]
async fn e2e_build_packwiz_refresh_fails() -> Result<()> {
    // Setup: Create a real temporary directory
    let temp_dir = TempDir::new()?;
    let workdir = temp_dir.path().to_path_buf();
    let fixture = WorkflowProjectFixture::new("workflow-build-refresh-fails");

    // Initialize: Create a real empack project
    fixture.write_to(&workdir)?;

    // Set working directory for the test
    std::env::set_current_dir(&workdir)?;

    // Create hybrid session with failing packwiz mock
    let app_config = AppConfig {
        workdir: Some(workdir.clone()),
        ..AppConfig::default()
    };

    // Initialize display system
    let terminal_caps = TerminalCapabilities::detect_from_config(&app_config)?;
    Display::init_or_get(terminal_caps);

    // Mock packwiz refresh failure
    let pack_file = workdir.join("pack").join("pack.toml");
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
            format: CliArchiveFormat::Zip,
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
        !fixture
            .artifact_path(&workdir, WorkflowArtifact::Mrpack)
            .exists(),
        "No mrpack artifact should be produced after a failed refresh"
    );

    Ok(())
}

/// Test that build command surfaces packwiz export failures without leaving stale artifacts.
#[tokio::test]
async fn e2e_build_packwiz_export_fails() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let workdir = temp_dir.path().to_path_buf();
    let fixture = WorkflowProjectFixture::new("workflow-build-export-fails");
    fixture.write_to(&workdir)?;

    std::env::set_current_dir(&workdir)?;

    let app_config = AppConfig {
        workdir: Some(workdir.clone()),
        ..AppConfig::default()
    };

    let terminal_caps = TerminalCapabilities::detect_from_config(&app_config)?;
    Display::init_or_get(terminal_caps);

    let pack_file = workdir.join("pack").join("pack.toml");
    let mrpack_path = fixture.artifact_path(&workdir, WorkflowArtifact::Mrpack);
    let mock_process_provider = MockProcessProvider::new()
        .with_packwiz_result(
            vec![
                "--pack-file".to_string(),
                pack_file.to_string_lossy().to_string(),
                "refresh".to_string(),
            ],
            Ok(ProcessOutput {
                stdout: "Refreshed packwiz index".to_string(),
                stderr: String::new(),
                success: true,
            }),
        )
        .with_packwiz_result(
            vec![
                "--pack-file".to_string(),
                pack_file.to_string_lossy().to_string(),
                "mr".to_string(),
                "export".to_string(),
                "-o".to_string(),
                mrpack_path.to_string_lossy().to_string(),
            ],
            Ok(ProcessOutput {
                stdout: String::new(),
                stderr: "Error: mrpack export failed".to_string(),
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

    let result = execute_command_with_session(
        Commands::Build {
            targets: vec!["mrpack".to_string()],
            clean: false,
            format: CliArchiveFormat::Zip,
        },
        &session,
    )
    .await;

    assert!(
        result.is_err(),
        "Build should fail when packwiz export returns a non-zero exit code"
    );
    let error = result.unwrap_err().to_string();
    assert!(
        error.contains("Failed to execute build pipeline")
            || error.contains("Build failed for target Mrpack"),
        "Export failure should surface a clear build error, got: {error}"
    );
    assert!(
        !mrpack_path.exists(),
        "Export failure should not leave a partial mrpack artifact"
    );

    Ok(())
}
