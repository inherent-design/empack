//! Hermetic E2E tests for the server-full build target.

use anyhow::Result;
use empack_lib::application::Commands;
use empack_lib::application::cli::CliArchiveFormat;
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
        .with_mock_http_client()
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
                stdout: "Installed server-full mods".to_string(),
                stderr: String::new(),
            },
        )?
        .with_pre_cached_jars()?
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
async fn e2e_build_server_full_successfully() -> anyhow::Result<()> {
    let project_name = "workflow-server-full";
    let (session, test_env, workdir) = initialize_empack_project(project_name).await?;

    let templates_dir = workdir.join("templates").join("server");
    std::fs::create_dir_all(&templates_dir)?;
    std::fs::write(
        templates_dir.join("server.properties.template"),
        "server-port=25565\nmotd={{NAME}} v{{VERSION}}\n",
    )?;
    std::fs::write(
        templates_dir.join("install_pack.sh.template"),
        "#!/bin/bash\necho \"Installing {{NAME}}\"\n",
    )?;

    let result = execute_command_with_session(
        Commands::Build {
            targets: vec!["server-full".to_string()],
            clean: false,
            format: CliArchiveFormat::Zip,
        },
        &session,
    )
    .await;

    assert!(result.is_ok(), "Server-full build failed: {result:?}");

    let server_full_dir = workdir.join("dist").join("server-full");
    assert!(
        server_full_dir.exists(),
        "Server-full build directory should exist"
    );
    assert!(
        std::fs::read_to_string(server_full_dir.join("server.properties"))?.contains(project_name),
        "Server-full build should process template variables"
    );
    assert!(
        server_full_dir.join("pack").join("pack.toml").exists(),
        "Pack contents should be copied into server-full output"
    );
    assert!(
        server_full_dir.join("srv.jar").exists(),
        "Server-full build should materialize the server JAR"
    );
    assert!(
        server_full_dir
            .join("mods")
            .join("server-installed.txt")
            .exists(),
        "Mock installer should leave a deterministic server install marker"
    );
    assert!(
        workdir
            .join("dist")
            .join(format!("{project_name}-v1.0.0-server-full.zip"))
            .exists(),
        "Server-full archive should be created"
    );
    assert!(
        !workdir.join("dist").join("server").exists(),
        "Standalone server-full builds should not materialize the server target directory"
    );
    assert!(
        !workdir
            .join("dist")
            .join(format!("{project_name}-v1.0.0-server.zip"))
            .exists(),
        "Standalone server-full builds should not create a server archive"
    );

    let java_calls = test_env.get_mock_calls("java")?;
    assert!(
        java_calls.iter().any(|call| call.contains("-s server")
            && call.contains("--bootstrap-main-jar")
            && call.contains("pack.toml")),
        "server-full build should invoke packwiz installer for server side: {java_calls:?}"
    );

    Ok(())
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn e2e_build_server_full_missing_installer() -> anyhow::Result<()> {
    let project_name = "workflow-server-full-missing-installer";
    let (session, test_env) = HermeticSessionBuilder::new()?
        .with_empack_project(project_name, "1.21.1", "fabric")?
        .with_mock_executable(
            "packwiz",
            MockBehavior::SucceedWithOutput {
                stdout: build_packwiz_output(project_name),
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

    let templates_dir = workdir.join("templates").join("server");
    std::fs::create_dir_all(&templates_dir)?;

    let result = execute_command_with_session(
        Commands::Build {
            targets: vec!["server-full".to_string()],
            clean: false,
            format: CliArchiveFormat::Zip,
        },
        &session,
    )
    .await;

    assert!(
        result.is_err(),
        "Build should fail when HTTP client is unavailable"
    );
    let error = result.unwrap_err().to_string();
    assert!(
        error.contains("HTTP client unavailable") || error.contains("Mock HTTP client unavailable"),
        "Build should fail at HTTP client creation, got: {error}"
    );
    assert!(
        !workdir
            .join("dist")
            .join("workflow-server-full-missing-installer-v1.0.0-server-full.zip")
            .exists(),
        "No server-full archive should be produced when the build fails"
    );

    Ok(())
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn e2e_build_server_full_with_templates() -> anyhow::Result<()> {
    let project_name = "workflow-server-full-templates";
    let (session, _test_env, workdir) = initialize_empack_project(project_name).await?;

    let templates_dir = workdir.join("templates").join("server");
    std::fs::create_dir_all(&templates_dir)?;
    std::fs::write(
        templates_dir.join("server.properties.template"),
        "server-port=25565\nmotd={{NAME}} v{{VERSION}} by {{AUTHOR}}\nmax-players=20\n",
    )?;
    std::fs::write(
        templates_dir.join("eula.txt.template"),
        "eula=true\n# {{NAME}} server\n",
    )?;
    std::fs::write(
        templates_dir.join("start.sh.template"),
        "#!/bin/bash\necho \"Starting {{NAME}} server-full\"\njava -jar srv.jar nogui\n",
    )?;

    let result = execute_command_with_session(
        Commands::Build {
            targets: vec!["server-full".to_string()],
            clean: false,
            format: CliArchiveFormat::Zip,
        },
        &session,
    )
    .await;

    assert!(result.is_ok(), "Server-full build failed: {result:?}");

    let server_full_dir = workdir.join("dist").join("server-full");
    let properties = std::fs::read_to_string(server_full_dir.join("server.properties"))?;
    assert!(
        properties.contains(project_name),
        "Server name should be processed"
    );
    assert!(
        properties.contains("Test Author"),
        "Author should be processed"
    );
    assert!(
        !properties.contains("{{NAME}}"),
        "Template variables should be replaced"
    );

    let eula = std::fs::read_to_string(server_full_dir.join("eula.txt"))?;
    assert!(eula.contains("eula=true"), "EULA should be rendered");
    assert!(
        eula.contains(project_name),
        "EULA comment should be rendered"
    );

    let script = std::fs::read_to_string(server_full_dir.join("start.sh"))?;
    assert!(
        script.contains(&format!("Starting {project_name} server-full")),
        "Start script should be rendered"
    );
    assert!(
        script.contains("java -jar srv.jar nogui"),
        "Start script should retain the server launch command"
    );
    assert!(
        server_full_dir.join("srv.jar").exists(),
        "Server JAR should exist"
    );
    assert!(
        server_full_dir
            .join("mods")
            .join("server-installed.txt")
            .exists(),
        "Installer marker should confirm server-full download step"
    );

    Ok(())
}
