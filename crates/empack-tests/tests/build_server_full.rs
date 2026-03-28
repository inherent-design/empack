//! E2E tests for the server-full build target.

use anyhow::Result;
use empack_lib::application::Commands;
use empack_lib::application::cli::CliArchiveFormat;
use empack_lib::application::commands::execute_command_with_session;
use empack_lib::application::session_mocks::mock_root;
use empack_lib::display::Display;
use empack_lib::terminal::TerminalCapabilities;
use empack_tests::MockSessionBuilder;
use std::path::{Path, PathBuf};

fn server_templates(workdir: &Path) -> Vec<(PathBuf, String)> {
    let templates_dir = workdir.join("templates").join("server");
    vec![
        (
            templates_dir.join("server.properties.template"),
            "server-port=25565\nmotd={{NAME}} v{{VERSION}}\n".to_string(),
        ),
        (
            templates_dir.join("install_pack.sh.template"),
            "#!/bin/bash\necho \"Installing {{NAME}}\"\n".to_string(),
        ),
    ]
}

#[tokio::test]
async fn e2e_build_server_full_successfully() -> Result<()> {
    let project_name = "workflow-server-full";
    let workdir = mock_root().join("workdir");

    let mut builder = MockSessionBuilder::new()
        .with_empack_project(project_name, "1.21.1", "fabric")
        .with_pre_cached_jars()
        .with_server_jar_stub();

    for (path, content) in server_templates(&workdir) {
        builder = builder.with_file(path, content);
    }

    let session = builder.build();

    Display::init_or_get(TerminalCapabilities::minimal());

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
        session.filesystem().exists(&server_full_dir),
        "Server-full build directory should exist"
    );
    let properties = session
        .filesystem()
        .read_to_string(&server_full_dir.join("server.properties"))?;
    assert!(
        properties.contains(project_name),
        "Server-full build should process template variables"
    );
    assert!(
        session
            .filesystem()
            .exists(&server_full_dir.join("pack").join("pack.toml")),
        "Pack contents should be copied into server-full output"
    );
    assert!(
        session
            .filesystem()
            .exists(&server_full_dir.join("srv.jar")),
        "Server-full build should materialize the server JAR"
    );
    assert!(
        session
            .filesystem()
            .exists(&server_full_dir.join("mods").join("server-installed.txt")),
        "Mock installer should leave a deterministic server install marker"
    );

    let create_calls = session.archive_provider.create_calls.lock().unwrap();
    assert!(
        create_calls.iter().any(|(_, dest)| dest
            .to_string_lossy()
            .contains(&format!("{project_name}-v1.0.0-server-full.zip"))),
        "Server-full archive should be created: {create_calls:?}"
    );
    assert!(
        !session.filesystem().exists(&workdir.join("dist").join("server")),
        "Standalone server-full builds should not materialize the server target directory"
    );

    let java_calls = session.process_provider.get_calls_for_command("java");
    assert!(
        java_calls.iter().any(|call| call
            .args
            .iter()
            .any(|a| a == "-s")
            && call.args.iter().any(|a| a == "server")
            && call.args.iter().any(|a| a == "--bootstrap-main-jar")
            && call.args.iter().any(|a| a.contains("pack.toml"))),
        "server-full build should invoke packwiz installer for server side: {java_calls:?}"
    );

    Ok(())
}

#[tokio::test]
async fn e2e_build_server_full_missing_installer() -> Result<()> {
    let project_name = "workflow-server-full-missing-installer";
    let workdir = mock_root().join("workdir");

    let session = MockSessionBuilder::new()
        .with_empack_project(project_name, "1.21.1", "fabric")
        .build();

    Display::init_or_get(TerminalCapabilities::minimal());

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
        !session.filesystem().exists(
            &workdir
                .join("dist")
                .join(format!("{project_name}-v1.0.0-server-full.zip"))
        ),
        "No server-full archive should be produced when the build fails"
    );

    Ok(())
}

#[tokio::test]
async fn e2e_build_server_full_with_templates() -> Result<()> {
    let project_name = "workflow-server-full-templates";
    let workdir = mock_root().join("workdir");
    let templates_dir = workdir.join("templates").join("server");

    let session = MockSessionBuilder::new()
        .with_empack_project(project_name, "1.21.1", "fabric")
        .with_pre_cached_jars()
        .with_server_jar_stub()
        .with_file(
            templates_dir.join("server.properties.template"),
            "server-port=25565\nmotd={{NAME}} v{{VERSION}} by {{AUTHOR}}\nmax-players=20\n"
                .to_string(),
        )
        .with_file(
            templates_dir.join("eula.txt.template"),
            "eula=true\n# {{NAME}} server\n".to_string(),
        )
        .with_file(
            templates_dir.join("start.sh.template"),
            "#!/bin/bash\necho \"Starting {{NAME}} server-full\"\njava -jar srv.jar nogui\n"
                .to_string(),
        )
        .build();

    Display::init_or_get(TerminalCapabilities::minimal());

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
    let properties = session
        .filesystem()
        .read_to_string(&server_full_dir.join("server.properties"))?;
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

    let eula = session
        .filesystem()
        .read_to_string(&server_full_dir.join("eula.txt"))?;
    assert!(eula.contains("eula=true"), "EULA should be rendered");
    assert!(
        eula.contains(project_name),
        "EULA comment should be rendered"
    );

    let script = session
        .filesystem()
        .read_to_string(&server_full_dir.join("start.sh"))?;
    assert!(
        script.contains(&format!("Starting {project_name} server-full")),
        "Start script should be rendered"
    );
    assert!(
        script.contains("java -jar srv.jar nogui"),
        "Start script should retain the server launch command"
    );
    assert!(
        session
            .filesystem()
            .exists(&server_full_dir.join("srv.jar")),
        "Server JAR should exist"
    );
    assert!(
        session
            .filesystem()
            .exists(&server_full_dir.join("mods").join("server-installed.txt")),
        "Installer marker should confirm server-full download step"
    );

    Ok(())
}
