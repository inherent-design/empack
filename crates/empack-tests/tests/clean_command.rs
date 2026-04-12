//! E2E tests for the clean command
//!
//! These tests use real filesystems (tempfile) to verify that the clean
//! command correctly removes build artifacts and distribution files.

use anyhow::Result;
use empack_lib::application::cli::Commands;
use empack_lib::application::commands::execute_command_with_session;
use empack_lib::application::config::AppConfig;
use empack_lib::application::session::{
    CommandSession, LiveConfigProvider, LiveFileSystemProvider, LiveNetworkProvider,
};
use empack_lib::application::session_mocks::{MockInteractiveProvider, MockProcessProvider};
use empack_lib::display::Display;
use empack_lib::terminal::TerminalCapabilities;
use empack_tests::e2e::empack_cmd;
use empack_tests::fixtures::{WorkflowArtifact, WorkflowProjectFixture};
use tempfile::TempDir;

fn combined_output(output: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[tokio::test]
async fn e2e_clean_builds_successfully() -> Result<()> {
    // Setup: Create a real temporary directory
    let temp_dir = TempDir::new()?;
    let workdir = temp_dir.path().to_path_buf();
    let fixture = WorkflowProjectFixture::new("workflow-clean-pack");

    // Initialize: Create a real empack project
    let paths = fixture.write_to(&workdir)?;

    // Create mock build artifacts that clean should remove
    std::fs::create_dir_all(&paths.dist_dir)?;

    let mrpack_file = fixture.artifact_path(&workdir, WorkflowArtifact::Mrpack);
    std::fs::write(&mrpack_file, "mock mrpack content")?;

    let client_zip = fixture.artifact_path(&workdir, WorkflowArtifact::Client);
    std::fs::write(&client_zip, "mock client zip content")?;

    let server_zip = fixture.artifact_path(&workdir, WorkflowArtifact::Server);
    std::fs::write(&server_zip, "mock server zip content")?;

    // Verify artifacts exist before clean
    assert!(
        mrpack_file.exists(),
        "mrpack file should exist before clean"
    );
    assert!(client_zip.exists(), "client zip should exist before clean");
    assert!(server_zip.exists(), "server zip should exist before clean");

    // Set working directory for the test
    std::env::set_current_dir(&workdir)?;

    // Create session (no external commands needed for clean)
    let app_config = AppConfig {
        workdir: Some(workdir.clone()),
        ..AppConfig::default()
    };

    // Initialize display system
    let terminal_caps = TerminalCapabilities::detect_from_config(app_config.color)?;
    Display::init_or_get(terminal_caps);

    let session = CommandSession::new_with_providers(
        LiveFileSystemProvider,
        LiveNetworkProvider::new(),
        MockProcessProvider::new(),
        LiveConfigProvider::new(app_config),
        MockInteractiveProvider::new(),
    );

    // Execute the clean command
    let result = execute_command_with_session(
        Commands::Clean {
            targets: vec!["builds".to_string()],
        },
        &session,
    )
    .await;

    // Assert: Command should succeed
    assert!(result.is_ok(), "Clean command failed: {:?}", result);

    assert!(
        !paths.dist_dir.exists(),
        "Clean should remove the canonical dist/ artifact root"
    );
    assert!(
        paths.empack_yml.exists(),
        "Clean should preserve empack.yml"
    );
    assert!(
        paths.pack_toml.exists(),
        "Clean should preserve pack metadata"
    );
    assert!(
        paths.index_toml.exists(),
        "Clean should preserve index metadata"
    );

    Ok(())
}

#[tokio::test]
async fn e2e_clean_no_artifacts() -> Result<()> {
    // Setup: Create a real temporary directory
    let temp_dir = TempDir::new()?;
    let workdir = temp_dir.path().to_path_buf();
    let fixture = WorkflowProjectFixture::new("workflow-clean-no-artifacts");

    // Initialize: Create a real empack project (no build artifacts)
    let paths = fixture.write_to(&workdir)?;

    let output = empack_cmd(&workdir).args(["clean", "builds"]).output()?;
    let combined = combined_output(&output);

    assert!(
        output.status.success(),
        "Clean command should succeed with no artifacts.\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.contains("No build artifacts to clean"),
        "combined output did not mention 'No build artifacts to clean'\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        paths.empack_yml.exists(),
        "No-op clean should preserve empack.yml"
    );
    assert!(
        paths.pack_toml.exists(),
        "No-op clean should preserve pack.toml"
    );
    assert!(
        !paths.dist_dir.exists(),
        "No-op clean should not materialize a dist directory"
    );

    Ok(())
}

#[tokio::test]
async fn e2e_clean_specific_targets() -> Result<()> {
    // Setup: Create a real temporary directory
    let temp_dir = TempDir::new()?;
    let workdir = temp_dir.path().to_path_buf();
    let fixture = WorkflowProjectFixture::new("workflow-clean-targets");

    // Initialize: Create a real empack project
    let paths = fixture.write_to(&workdir)?;

    // Create mock build artifacts for different targets
    std::fs::create_dir_all(&paths.dist_dir)?;

    let mrpack_file = fixture.artifact_path(&workdir, WorkflowArtifact::Mrpack);
    std::fs::write(&mrpack_file, "mock mrpack content")?;

    let client_dir = paths.dist_dir.join("client");
    std::fs::create_dir_all(&client_dir)?;
    std::fs::write(client_dir.join("instance.cfg"), "mock client config")?;

    let server_dir = paths.dist_dir.join("server");
    std::fs::create_dir_all(&server_dir)?;
    std::fs::write(server_dir.join("server.properties"), "mock server config")?;

    let readme_path = workdir.join("README.md");
    std::fs::write(&readme_path, "keep me")?;

    // Set working directory for the test
    std::env::set_current_dir(&workdir)?;

    // Create session
    let app_config = AppConfig {
        workdir: Some(workdir.clone()),
        ..AppConfig::default()
    };

    // Initialize display system
    let terminal_caps = TerminalCapabilities::detect_from_config(app_config.color)?;
    Display::init_or_get(terminal_caps);

    let session = CommandSession::new_with_providers(
        LiveFileSystemProvider,
        LiveNetworkProvider::new(),
        MockProcessProvider::new(),
        LiveConfigProvider::new(app_config),
        MockInteractiveProvider::new(),
    );

    // Execute the clean command with all target types
    let result = execute_command_with_session(
        Commands::Clean {
            targets: vec!["builds".to_string()],
        },
        &session,
    )
    .await;

    // Assert: Command should succeed
    assert!(result.is_ok(), "Clean command failed: {:?}", result);

    assert!(
        !paths.dist_dir.exists(),
        "Cleaning build targets should remove nested dist target directories too"
    );
    assert!(
        readme_path.exists(),
        "Clean should not remove unrelated project files outside dist/"
    );

    Ok(())
}
