//! Hermetic E2E tests for the server build target.

use anyhow::Result;
use empack_lib::application::Commands;
use empack_lib::application::commands::execute_command_with_session;
use empack_lib::application::session::{
    CommandSession, LiveConfigProvider, LiveFileSystemProvider, LiveProcessProvider,
};
use empack_lib::application::session_mocks::MockInteractiveProvider;
use empack_lib::display::Display;
use empack_lib::terminal::TerminalCapabilities;
use empack_tests::{
    HermeticSessionBuilder, MockBehavior, MockNetworkProvider, TestEnvironment,
};
use std::path::PathBuf;

type HermeticSession = CommandSession<
    LiveFileSystemProvider,
    MockNetworkProvider,
    LiveProcessProvider,
    LiveConfigProvider,
    MockInteractiveProvider,
>;

fn build_packwiz_output(project_name: &str) -> String {
    format!(
        "Refreshed packwiz index\nExported to {project_name}-v1.0.0.mrpack"
    )
}

fn init_display(session: &HermeticSession) -> Result<()> {
    let terminal_caps = TerminalCapabilities::detect_from_config(session.config().app_config())?;
    let _ = Display::init(terminal_caps);
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
            "mrpack-install",
            MockBehavior::SucceedWithOutput {
                stdout: "Installed mock server jar".to_string(),
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
                stdout: "Created server archive".to_string(),
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

    Ok((session, test_env, workdir))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn e2e_build_server_successfully() -> anyhow::Result<()> {
    let project_name = "workflow-server";
    let (session, test_env, workdir) = initialize_empack_project(project_name).await?;

    let templates_dir = workdir.join("templates/server");
    std::fs::create_dir_all(&templates_dir)?;
    std::fs::write(
        templates_dir.join("server.properties.template"),
        "server-port=25565\nmotd={{NAME}} v{{VERSION}}\n",
    )?;
    std::fs::write(
        templates_dir.join("install_pack.sh.template"),
        "#!/bin/bash\necho \"Installing {{NAME}}\"\n",
    )?;

    let installer_dir = workdir.join("installer");
    std::fs::create_dir_all(&installer_dir)?;
    std::fs::write(
        installer_dir.join("packwiz-installer-bootstrap.jar"),
        "mock-installer-jar",
    )?;

    let result = execute_command_with_session(
        Commands::Build {
            targets: vec!["server".to_string()],
            clean: false,
            jobs: None,
        },
        &session,
    )
    .await;

    assert!(result.is_ok(), "Server build failed: {result:?}");

    let server_dir = workdir.join("dist/server");
    assert!(server_dir.exists(), "Server build directory should exist");
    assert!(
        std::fs::read_to_string(server_dir.join("server.properties"))?.contains(project_name),
        "Server templates should be rendered"
    );
    assert!(
        server_dir.join("packwiz-installer-bootstrap.jar").exists(),
        "Bootstrap installer should be copied into server output"
    );
    assert!(
        server_dir.join("srv.jar").exists(),
        "Server build should materialize the server JAR"
    );
    assert!(
        server_dir.join("config/generated.txt").exists(),
        "Mock unzip should supply deterministic override content"
    );
    assert!(
        workdir
            .join("dist")
            .join(format!("{project_name}-v1.0.0-server.zip"))
            .exists(),
        "Server archive should be created"
    );

    let packwiz_calls = test_env.get_mock_calls("packwiz")?;
    assert!(
        packwiz_calls.iter().any(|call| call.contains(" mr export ")),
        "Server build should export an mrpack before extraction: {packwiz_calls:?}"
    );

    let unzip_calls = test_env.get_mock_calls("unzip")?;
    assert!(
        !unzip_calls.is_empty(),
        "Server build should extract the generated mrpack: {unzip_calls:?}"
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn e2e_build_server_missing_installer() -> anyhow::Result<()> {
    let (session, _test_env, workdir) =
        initialize_empack_project("workflow-server-missing-installer").await?;

    let templates_dir = workdir.join("templates/server");
    std::fs::create_dir_all(&templates_dir)?;

    let result = execute_command_with_session(
        Commands::Build {
            targets: vec!["server".to_string()],
            clean: false,
            jobs: None,
        },
        &session,
    )
    .await;

    assert!(result.is_err(), "Build should fail when installer JAR is unavailable");
    let error = result.unwrap_err().to_string();
    assert!(
        error.contains("Failed to execute build pipeline"),
        "Missing installer should surface as a pipeline failure, got: {error}"
    );
    assert!(
        !workdir
            .join("dist")
            .join("workflow-server-missing-installer-v1.0.0-server.zip")
            .exists(),
        "No server archive should be produced when the installer is missing"
    );
    assert!(
        !workdir.join("dist/server/srv.jar").exists(),
        "The server jar should not be materialized when the installer bootstrap is missing"
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn e2e_build_server_with_templates() -> anyhow::Result<()> {
    let project_name = "workflow-server-templates";
    let (session, _test_env, workdir) = initialize_empack_project(project_name).await?;

    let templates_dir = workdir.join("templates/server");
    std::fs::create_dir_all(&templates_dir)?;
    std::fs::write(
        templates_dir.join("server.properties.template"),
        "server-port=25565\nmotd={{NAME}} v{{VERSION}} by {{AUTHOR}}\nmax-players=20\n",
    )?;
    std::fs::write(
        templates_dir.join("README.md.template"),
        "# {{NAME}}\n\nVersion: {{VERSION}}\nAuthor: {{AUTHOR}}\nMinecraft: {{MC_VERSION}}\n",
    )?;
    std::fs::write(
        templates_dir.join("start.sh.template"),
        "#!/bin/bash\necho \"Starting {{NAME}} server\"\njava -jar srv.jar\n",
    )?;

    let installer_dir = workdir.join("installer");
    std::fs::create_dir_all(&installer_dir)?;
    std::fs::write(
        installer_dir.join("packwiz-installer-bootstrap.jar"),
        "mock-installer-jar",
    )?;

    let result = execute_command_with_session(
        Commands::Build {
            targets: vec!["server".to_string()],
            clean: false,
            jobs: None,
        },
        &session,
    )
    .await;

    assert!(result.is_ok(), "Server build failed: {result:?}");

    let server_dir = workdir.join("dist/server");
    let properties = std::fs::read_to_string(server_dir.join("server.properties"))?;
    assert!(properties.contains(project_name), "Server name should be processed");
    assert!(properties.contains("Test Author"), "Author should be processed");
    assert!(
        !properties.contains("{{NAME}}"),
        "Template variables should be replaced"
    );

    let readme = std::fs::read_to_string(server_dir.join("README.md"))?;
    assert!(readme.contains(&format!("# {project_name}")), "README should be rendered");
    assert!(
        !readme.contains("{{VERSION}}"),
        "README template variables should be replaced"
    );

    let script = std::fs::read_to_string(server_dir.join("start.sh"))?;
    assert!(
        script.contains(&format!("Starting {project_name} server")),
        "Script should be processed"
    );
    assert!(
        script.contains("java -jar srv.jar"),
        "Script should contain the server launch command"
    );
    assert!(server_dir.join("srv.jar").exists(), "Server JAR should exist");
    assert!(
        server_dir.join("config/generated.txt").exists(),
        "Override content should be copied into the rendered server build"
    );

    Ok(())
}
