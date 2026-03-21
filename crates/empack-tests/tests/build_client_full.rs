//! Hermetic E2E tests for the client-full build target.

use anyhow::Result;
use empack_lib::application::Commands;
use empack_lib::application::commands::execute_command_with_session;
use empack_lib::application::session::{
    CommandSession, LiveConfigProvider, LiveFileSystemProvider, LiveProcessProvider,
};
use empack_lib::application::session_mocks::MockInteractiveProvider;
use empack_lib::display::Display;
use empack_lib::terminal::TerminalCapabilities;
use empack_tests::{HermeticSessionBuilder, MockBehavior, MockNetworkProvider, TestEnvironment};
use std::path::PathBuf;

type HermeticSession = CommandSession<
    LiveFileSystemProvider,
    MockNetworkProvider,
    LiveProcessProvider,
    LiveConfigProvider,
    MockInteractiveProvider,
>;

fn build_packwiz_output(project_name: &str) -> String {
    format!("Refreshed packwiz index\nExported to {project_name}-v1.0.0.mrpack")
}

fn init_display(session: &HermeticSession) -> Result<()> {
    let terminal_caps = TerminalCapabilities::detect_from_config(session.config().app_config())?;
    Display::init_or_get(terminal_caps);
    Ok(())
}

async fn initialize_empack_project(
    project_name: &str,
) -> Result<(HermeticSession, TestEnvironment, PathBuf)> {
    let (session, test_env) = HermeticSessionBuilder::new()?
        .with_empack_project(project_name, "1.21.1", "fabric")?
        .with_mock_executable(
            "packwiz",
            MockBehavior::SucceedWithOutput {
                stdout: build_packwiz_output(project_name),
                stderr: String::new(),
            },
        )?
        .with_mock_executable(
            "java",
            MockBehavior::SucceedWithOutput {
                stdout: "Installed client-full mods".to_string(),
                stderr: String::new(),
            },
        )?
        .with_mock_executable(
            "unzip",
            MockBehavior::SucceedWithOutput {
                stdout: "Extracted mock mrpack".to_string(),
                stderr: String::new(),
            },
        )?
        .with_mock_executable(
            "zip",
            MockBehavior::SucceedWithOutput {
                stdout: "Created client-full archive".to_string(),
                stderr: String::new(),
            },
        )?
        .build()?;

    init_display(&session)?;

    let workdir = session
        .config()
        .app_config()
        .workdir
        .clone()
        .expect("hermetic project should configure a workdir");
    std::env::set_current_dir(&workdir)?;
    unsafe {
        std::env::set_var("HOME", &test_env.root_path);
    }

    Ok((session, test_env, workdir))
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn e2e_build_client_full_successfully() -> anyhow::Result<()> {
    let project_name = "workflow-client-full";
    let (session, test_env, workdir) = initialize_empack_project(project_name).await?;

    let jar_cache = empack_lib::platform::cache::cache_root()?.join("jars");
    std::fs::create_dir_all(&jar_cache)?;
    std::fs::write(
        jar_cache.join("packwiz-installer-bootstrap.jar"),
        "mock-bootstrap-jar",
    )?;
    std::fs::write(
        jar_cache.join("packwiz-installer.jar"),
        "mock-installer-jar",
    )?;

    let result = execute_command_with_session(
        Commands::Build {
            targets: vec!["client-full".to_string()],
            clean: false,
            jobs: None,
        },
        &session,
    )
    .await;

    assert!(result.is_ok(), "Client-full build failed: {result:?}");

    let client_full_dir = workdir.join("dist").join("client-full");
    assert!(
        client_full_dir.exists(),
        "Client-full build directory should exist"
    );
    assert!(
        client_full_dir.join("pack").join("pack.toml").exists(),
        "Pack metadata should be copied into client-full output"
    );
    assert!(
        client_full_dir.join("mods").join("both-installed.txt").exists(),
        "Mock installer should leave a deterministic install marker"
    );

    let archive = workdir
        .join("dist")
        .join(format!("{project_name}-v1.0.0-client-full.zip"));
    assert!(archive.exists(), "Client-full archive should be created");
    assert!(
        !workdir.join("dist").join("client").exists(),
        "Standalone client-full builds should not materialize the client target directory"
    );
    assert!(
        !workdir
            .join("dist")
            .join(format!("{project_name}-v1.0.0-client.zip"))
            .exists(),
        "Standalone client-full builds should not create a client archive"
    );

    let packwiz_calls = test_env.get_mock_calls("packwiz")?;
    assert!(
        packwiz_calls.iter().any(|call| call.contains(" refresh")),
        "build should refresh pack metadata before client-full build: {packwiz_calls:?}"
    );

    let java_calls = test_env.get_mock_calls("java")?;
    assert!(
        java_calls.iter().any(|call| call.contains("-s both")
            && call.contains("--bootstrap-main-jar")
            && call.contains("pack.toml")),
        "client-full build should invoke packwiz installer for both sides: {java_calls:?}"
    );

    Ok(())
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn e2e_build_client_full_missing_installer() -> anyhow::Result<()> {
    let (session, _test_env, workdir) =
        initialize_empack_project("workflow-client-full-missing-installer").await?;

    let result = execute_command_with_session(
        Commands::Build {
            targets: vec!["client-full".to_string()],
            clean: false,
            jobs: None,
        },
        &session,
    )
    .await;

    assert!(
        result.is_err(),
        "Build should fail when installer JAR is unavailable"
    );
    let error = result.unwrap_err().to_string();
    assert!(
        error.contains("Mock HTTP client unavailable (test mode)"),
        "Missing installer should fail while resolving the bootstrap JAR, got: {error}"
    );
    assert!(
        !workdir
            .join("dist")
            .join("workflow-client-full-missing-installer-v1.0.0-client-full.zip")
            .exists(),
        "No client-full archive should be produced when the installer is missing"
    );
    assert!(
        !workdir
            .join("dist").join("client-full").join("mods").join("both-installed.txt")
            .exists(),
        "The full installer step should not run when the installer bootstrap is missing"
    );
    assert!(
        !workdir.join("dist").join("client").exists(),
        "Standalone client-full failures should not create the client target directory"
    );
    assert!(
        !workdir
            .join("dist")
            .join("workflow-client-full-missing-installer-v1.0.0-client.zip")
            .exists(),
        "Standalone client-full failures should not create a client archive"
    );

    Ok(())
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn e2e_build_client_full_with_pack_structure() -> anyhow::Result<()> {
    let project_name = "workflow-client-full-structure";
    let (session, _test_env, workdir) = initialize_empack_project(project_name).await?;

    let jar_cache = empack_lib::platform::cache::cache_root()?.join("jars");
    std::fs::create_dir_all(&jar_cache)?;
    std::fs::write(
        jar_cache.join("packwiz-installer-bootstrap.jar"),
        "mock-bootstrap-jar",
    )?;
    std::fs::write(
        jar_cache.join("packwiz-installer.jar"),
        "mock-installer-jar",
    )?;

    let pack_dir = workdir.join("pack");
    let mods_dir = pack_dir.join("mods");
    std::fs::create_dir_all(&mods_dir)?;
    std::fs::write(
        mods_dir.join("example-mod.pw.toml"),
        "[download]\nurl = \"https://example.com/mod.jar\"\nhash = \"abc123\"\n",
    )?;

    let result = execute_command_with_session(
        Commands::Build {
            targets: vec!["client-full".to_string()],
            clean: false,
            jobs: None,
        },
        &session,
    )
    .await;

    assert!(result.is_ok(), "Client-full build failed: {result:?}");

    let client_full_dir = workdir.join("dist").join("client-full");
    assert!(
        client_full_dir.exists(),
        "Client-full build directory should exist"
    );
    assert!(
        client_full_dir
            .join("pack").join("mods").join("example-mod.pw.toml")
            .exists(),
        "Existing pack structure should be copied into client-full output"
    );
    assert!(
        client_full_dir.join("mods").join("both-installed.txt").exists(),
        "Installer marker should confirm the mocked full download step"
    );
    assert!(
        workdir
            .join("dist")
            .join(format!("{project_name}-v1.0.0-client-full.zip"))
            .exists(),
        "Client-full archive should be created for the structured pack scenario"
    );
    assert!(
        !workdir.join("dist").join("client").exists(),
        "Structured standalone client-full builds should not materialize the client target directory"
    );

    Ok(())
}
