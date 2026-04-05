//! E2E tests for the server build target.

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
async fn e2e_build_server_successfully() -> Result<()> {
    let project_name = "workflow-server";
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
            targets: vec!["server".to_string()],
            clean: false,
            format: CliArchiveFormat::Zip,
            downloads_dir: None,
        },
        &session,
    )
    .await;

    assert!(result.is_ok(), "Server build failed: {result:?}");

    let server_dir = workdir.join("dist").join("server");
    assert!(
        session.filesystem().exists(&server_dir),
        "Server build directory should exist"
    );
    let properties = session
        .filesystem()
        .read_to_string(&server_dir.join("server.properties"))?;
    assert!(
        properties.contains(project_name),
        "Server templates should be rendered"
    );
    assert!(
        session
            .filesystem()
            .exists(&server_dir.join("packwiz-installer-bootstrap.jar")),
        "Bootstrap installer should be copied into server output"
    );
    assert!(
        session.filesystem().exists(&server_dir.join("srv.jar")),
        "Server build should materialize the server JAR"
    );

    let extract_calls = session.archive_provider.extract_calls.lock().unwrap();
    assert!(
        !extract_calls.is_empty(),
        "Server build should extract the mrpack archive"
    );

    let create_calls = session.archive_provider.create_calls.lock().unwrap();
    assert!(
        create_calls.iter().any(|(_, dest)| dest
            .to_string_lossy()
            .contains(&format!("{project_name}-v1.0.0-server.zip"))),
        "Server archive should be created: {create_calls:?}"
    );

    let packwiz_calls = session.process_provider.get_calls_for_command("packwiz");
    assert!(
        packwiz_calls
            .iter()
            .any(|call| call.args.iter().any(|a| a == "mr")
                && call.args.iter().any(|a| a == "export")),
        "Server build should export an mrpack before extraction: {packwiz_calls:?}"
    );

    Ok(())
}

#[tokio::test]
async fn e2e_build_server_missing_installer() -> Result<()> {
    let project_name = "workflow-server-missing-installer";
    let workdir = mock_root().join("workdir");

    let session = MockSessionBuilder::new()
        .with_empack_project(project_name, "1.21.1", "fabric")
        .build();

    Display::init_or_get(TerminalCapabilities::minimal());

    let result = execute_command_with_session(
        Commands::Build {
            targets: vec!["server".to_string()],
            clean: false,
            format: CliArchiveFormat::Zip,
            downloads_dir: None,
        },
        &session,
    )
    .await;

    assert!(
        result.is_err(),
        "Build should fail when HTTP client is unavailable"
    );
    let error = format!("{:#}", result.unwrap_err());
    assert!(
        error.contains("HTTP client unavailable")
            || error.contains("Mock HTTP client unavailable")
            || error.contains("Failed to read file:"),
        "Build should fail at HTTP or bootstrap JAR resolution, got: {error}"
    );
    assert!(
        !session.filesystem().exists(
            &workdir
                .join("dist")
                .join(format!("{project_name}-v1.0.0-server.zip"))
        ),
        "No server archive should be produced when the build fails"
    );

    Ok(())
}

#[tokio::test]
async fn e2e_build_server_with_templates() -> Result<()> {
    let project_name = "workflow-server-templates";
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
            templates_dir.join("README.md.template"),
            "# {{NAME}}\n\nVersion: {{VERSION}}\nAuthor: {{AUTHOR}}\nMinecraft: {{MC_VERSION}}\n"
                .to_string(),
        )
        .with_file(
            templates_dir.join("start.sh.template"),
            "#!/bin/bash\necho \"Starting {{NAME}} server\"\njava -jar srv.jar\n".to_string(),
        )
        .build();

    Display::init_or_get(TerminalCapabilities::minimal());

    let result = execute_command_with_session(
        Commands::Build {
            targets: vec!["server".to_string()],
            clean: false,
            format: CliArchiveFormat::Zip,
            downloads_dir: None,
        },
        &session,
    )
    .await;

    assert!(result.is_ok(), "Server build failed: {result:?}");

    let server_dir = workdir.join("dist").join("server");
    let properties = session
        .filesystem()
        .read_to_string(&server_dir.join("server.properties"))?;
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

    let readme = session
        .filesystem()
        .read_to_string(&server_dir.join("README.md"))?;
    assert!(
        readme.contains(&format!("# {project_name}")),
        "README should be rendered"
    );
    assert!(
        !readme.contains("{{VERSION}}"),
        "README template variables should be replaced"
    );

    let script = session
        .filesystem()
        .read_to_string(&server_dir.join("start.sh"))?;
    assert!(
        script.contains(&format!("Starting {project_name} server")),
        "Script should be processed"
    );
    assert!(
        script.contains("java -jar srv.jar"),
        "Script should contain the server launch command"
    );
    assert!(
        session.filesystem().exists(&server_dir.join("srv.jar")),
        "Server JAR should exist"
    );

    Ok(())
}
