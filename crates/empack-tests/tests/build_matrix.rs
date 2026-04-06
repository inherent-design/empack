use anyhow::Result;
use empack_lib::application::Commands;
use empack_lib::application::cli::BuildArgs;
use empack_lib::application::commands::execute_command_with_session;
use empack_lib::application::session_mocks::mock_root;
use empack_lib::display::Display;
use empack_lib::terminal::TerminalCapabilities;
use empack_tests::MockSessionBuilder;
use std::path::Path;

fn create_server_templates(builder: MockSessionBuilder, workdir: &Path) -> MockSessionBuilder {
    let templates_dir = workdir.join("templates").join("server");
    builder
        .with_file(
            templates_dir.join("server.properties.template"),
            "server-port=25565\nmotd={{NAME}} v{{VERSION}}\n".to_string(),
        )
        .with_file(
            templates_dir.join("install_pack.sh.template"),
            "#!/bin/bash\necho \"Installing {{NAME}}\"\n".to_string(),
        )
}

fn loader_version_for_pack_toml(loader: &str) -> &'static str {
    match loader {
        "fabric" => "0.16.14",
        "neoforge" => "21.4.157",
        "quilt" => "0.27.1-beta.5",
        "forge" => "49.2.0",
        _ => "0.15.0",
    }
}

fn setup_mrpack_session(project_name: &str, loader: &str) -> MockSessionBuilder {
    let workdir = mock_root().join("workdir");
    let version = loader_version_for_pack_toml(loader);
    let pack_toml = workdir.join("pack").join("pack.toml");
    let default_pack_toml_content = format!(
        r#"name = "{project_name}"
author = "Test Author"
version = "1.0.0"
pack-format = "packwiz:1.1.0"

[index]
file = "index.toml"
hash-format = "sha256"
hash = ""

[versions]
minecraft = "1.21.4"
{loader} = "{version}"
"#
    );

    MockSessionBuilder::new()
        .with_empack_project(project_name, "1.21.4", loader)
        .with_file(pack_toml, default_pack_toml_content)
}

fn setup_server_session(project_name: &str, loader: &str) -> MockSessionBuilder {
    let workdir = mock_root().join("workdir");
    let builder = setup_mrpack_session(project_name, loader)
        .with_pre_cached_jars()
        .with_server_jar_stub();
    create_server_templates(builder, &workdir)
}

fn setup_server_full_session(project_name: &str, loader: &str) -> MockSessionBuilder {
    let workdir = mock_root().join("workdir");
    let builder = setup_mrpack_session(project_name, loader)
        .with_pre_cached_jars()
        .with_server_jar_stub();
    create_server_templates(builder, &workdir)
}

fn setup_client_session(project_name: &str, loader: &str) -> MockSessionBuilder {
    setup_mrpack_session(project_name, loader).with_pre_cached_jars()
}

// ---------------------------------------------------------------------------
// NeoForge tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_build_neoforge_mrpack() -> Result<()> {
    let project_name = "matrix-neoforge-mrpack";
    let workdir = mock_root().join("workdir");
    let session = setup_mrpack_session(project_name, "neoforge").build();

    Display::init_or_get(TerminalCapabilities::minimal());

    let result = execute_command_with_session(
        Commands::Build(BuildArgs {
            targets: vec!["mrpack".to_string()],
            ..Default::default()
        }),
        &session,
    )
    .await;

    assert!(result.is_ok(), "NeoForge mrpack build failed: {result:?}");

    let mrpack_path = workdir
        .join("dist")
        .join(format!("{project_name}-v1.0.0.mrpack"));
    assert!(
        session.filesystem().exists(&mrpack_path),
        "mrpack artifact should be created in dist/"
    );

    let packwiz_calls = session.process_provider.get_calls_for_command("packwiz");
    assert!(
        packwiz_calls
            .iter()
            .any(|call| call.args.iter().any(|a| a == "mr")
                && call.args.iter().any(|a| a == "export")),
        "NeoForge mrpack build should export via packwiz: {packwiz_calls:?}"
    );

    Ok(())
}

#[tokio::test]
async fn test_build_neoforge_server() -> Result<()> {
    let project_name = "matrix-neoforge-server";
    let workdir = mock_root().join("workdir");
    let session = setup_server_session(project_name, "neoforge").build();

    Display::init_or_get(TerminalCapabilities::minimal());

    let result = execute_command_with_session(
        Commands::Build(BuildArgs {
            targets: vec!["server".to_string()],
            ..Default::default()
        }),
        &session,
    )
    .await;

    assert!(result.is_ok(), "NeoForge server build failed: {result:?}");

    let server_dir = workdir.join("dist").join("server");
    assert!(
        session.filesystem().exists(&server_dir),
        "Server build directory should exist"
    );
    assert!(
        session
            .filesystem()
            .exists(&server_dir.join("packwiz-installer-bootstrap.jar")),
        "Bootstrap installer should be copied into server output"
    );
    assert!(
        session.filesystem().exists(&server_dir.join("srv.jar")),
        "Server build should materialize srv.jar"
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
async fn test_build_neoforge_server_full() -> Result<()> {
    let project_name = "matrix-neoforge-server-full";
    let workdir = mock_root().join("workdir");
    let session = setup_server_full_session(project_name, "neoforge").build();

    Display::init_or_get(TerminalCapabilities::minimal());

    let result = execute_command_with_session(
        Commands::Build(BuildArgs {
            targets: vec!["server-full".to_string()],
            ..Default::default()
        }),
        &session,
    )
    .await;

    assert!(
        result.is_ok(),
        "NeoForge server-full build failed: {result:?}"
    );

    let server_full_dir = workdir.join("dist").join("server-full");
    assert!(
        session.filesystem().exists(&server_full_dir),
        "Server-full build directory should exist"
    );
    assert!(
        session
            .filesystem()
            .exists(&server_full_dir.join("srv.jar")),
        "Server-full build should materialize srv.jar"
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
            .exists(&server_full_dir.join("mods").join("server-installed.txt")),
        "Mock installer should leave a server install marker"
    );

    let create_calls = session.archive_provider.create_calls.lock().unwrap();
    assert!(
        create_calls.iter().any(|(_, dest)| dest
            .to_string_lossy()
            .contains(&format!("{project_name}-v1.0.0-server-full.zip"))),
        "Server-full archive should be created: {create_calls:?}"
    );

    let java_calls = session.process_provider.get_calls_for_command("java");
    assert!(
        java_calls
            .iter()
            .any(|call| call.args.iter().any(|a| a == "-s")
                && call.args.iter().any(|a| a == "server")
                && call.args.iter().any(|a| a == "--bootstrap-main-jar")
                && call.args.iter().any(|a| a.contains("pack.toml"))),
        "server-full build should invoke packwiz installer for server side: {java_calls:?}"
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Quilt tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_build_quilt_mrpack() -> Result<()> {
    let project_name = "matrix-quilt-mrpack";
    let workdir = mock_root().join("workdir");
    let session = setup_mrpack_session(project_name, "quilt").build();

    Display::init_or_get(TerminalCapabilities::minimal());

    let result = execute_command_with_session(
        Commands::Build(BuildArgs {
            targets: vec!["mrpack".to_string()],
            ..Default::default()
        }),
        &session,
    )
    .await;

    assert!(result.is_ok(), "Quilt mrpack build failed: {result:?}");

    let mrpack_path = workdir
        .join("dist")
        .join(format!("{project_name}-v1.0.0.mrpack"));
    assert!(
        session.filesystem().exists(&mrpack_path),
        "mrpack artifact should be created in dist/"
    );

    let packwiz_calls = session.process_provider.get_calls_for_command("packwiz");
    assert!(
        packwiz_calls
            .iter()
            .any(|call| call.args.iter().any(|a| a == "mr")
                && call.args.iter().any(|a| a == "export")),
        "Quilt mrpack build should export via packwiz: {packwiz_calls:?}"
    );

    Ok(())
}

#[tokio::test]
async fn test_build_quilt_server() -> Result<()> {
    let project_name = "matrix-quilt-server";
    let workdir = mock_root().join("workdir");
    let session = setup_server_session(project_name, "quilt").build();

    Display::init_or_get(TerminalCapabilities::minimal());

    let result = execute_command_with_session(
        Commands::Build(BuildArgs {
            targets: vec!["server".to_string()],
            ..Default::default()
        }),
        &session,
    )
    .await;

    assert!(result.is_ok(), "Quilt server build failed: {result:?}");

    let server_dir = workdir.join("dist").join("server");
    assert!(
        session.filesystem().exists(&server_dir),
        "Server build directory should exist"
    );
    assert!(
        session
            .filesystem()
            .exists(&server_dir.join("packwiz-installer-bootstrap.jar")),
        "Bootstrap installer should be copied into server output"
    );
    assert!(
        session.filesystem().exists(&server_dir.join("srv.jar")),
        "Server build should materialize srv.jar"
    );

    let create_calls = session.archive_provider.create_calls.lock().unwrap();
    assert!(
        create_calls.iter().any(|(_, dest)| dest
            .to_string_lossy()
            .contains(&format!("{project_name}-v1.0.0-server.zip"))),
        "Server archive should be created: {create_calls:?}"
    );

    Ok(())
}

#[tokio::test]
async fn test_build_quilt_server_full() -> Result<()> {
    let project_name = "matrix-quilt-server-full";
    let workdir = mock_root().join("workdir");
    let session = setup_server_full_session(project_name, "quilt").build();

    Display::init_or_get(TerminalCapabilities::minimal());

    let result = execute_command_with_session(
        Commands::Build(BuildArgs {
            targets: vec!["server-full".to_string()],
            ..Default::default()
        }),
        &session,
    )
    .await;

    assert!(result.is_ok(), "Quilt server-full build failed: {result:?}");

    let server_full_dir = workdir.join("dist").join("server-full");
    assert!(
        session.filesystem().exists(&server_full_dir),
        "Server-full build directory should exist"
    );
    assert!(
        session
            .filesystem()
            .exists(&server_full_dir.join("srv.jar")),
        "Server-full build should materialize srv.jar"
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
            .exists(&server_full_dir.join("mods").join("server-installed.txt")),
        "Mock installer should leave a server install marker"
    );

    let create_calls = session.archive_provider.create_calls.lock().unwrap();
    assert!(
        create_calls.iter().any(|(_, dest)| dest
            .to_string_lossy()
            .contains(&format!("{project_name}-v1.0.0-server-full.zip"))),
        "Server-full archive should be created: {create_calls:?}"
    );

    let java_calls = session.process_provider.get_calls_for_command("java");
    assert!(
        java_calls
            .iter()
            .any(|call| call.args.iter().any(|a| a == "-s")
                && call.args.iter().any(|a| a == "server")
                && call.args.iter().any(|a| a == "--bootstrap-main-jar")
                && call.args.iter().any(|a| a.contains("pack.toml"))),
        "server-full build should invoke packwiz installer for server side: {java_calls:?}"
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Vanilla tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_build_vanilla_mrpack() -> Result<()> {
    let project_name = "matrix-vanilla-mrpack";
    let workdir = mock_root().join("workdir");
    let session = setup_mrpack_session(project_name, "none").build();

    Display::init_or_get(TerminalCapabilities::minimal());

    let result = execute_command_with_session(
        Commands::Build(BuildArgs {
            targets: vec!["mrpack".to_string()],
            ..Default::default()
        }),
        &session,
    )
    .await;

    assert!(result.is_ok(), "Vanilla mrpack build failed: {result:?}");

    let mrpack_path = workdir
        .join("dist")
        .join(format!("{project_name}-v1.0.0.mrpack"));
    assert!(
        session.filesystem().exists(&mrpack_path),
        "mrpack artifact should be created in dist/"
    );

    let packwiz_calls = session.process_provider.get_calls_for_command("packwiz");
    assert!(
        packwiz_calls
            .iter()
            .any(|call| call.args.iter().any(|a| a == "mr")
                && call.args.iter().any(|a| a == "export")),
        "Vanilla mrpack build should export via packwiz: {packwiz_calls:?}"
    );

    Ok(())
}

#[tokio::test]
async fn test_build_vanilla_server() -> Result<()> {
    let project_name = "matrix-vanilla-server";
    let workdir = mock_root().join("workdir");
    let session = setup_server_session(project_name, "none").build();

    Display::init_or_get(TerminalCapabilities::minimal());

    let result = execute_command_with_session(
        Commands::Build(BuildArgs {
            targets: vec!["server".to_string()],
            ..Default::default()
        }),
        &session,
    )
    .await;

    assert!(result.is_ok(), "Vanilla server build failed: {result:?}");

    let server_dir = workdir.join("dist").join("server");
    assert!(
        session.filesystem().exists(&server_dir),
        "Server build directory should exist"
    );
    assert!(
        session
            .filesystem()
            .exists(&server_dir.join("packwiz-installer-bootstrap.jar")),
        "Bootstrap installer should be copied into server output"
    );
    assert!(
        session.filesystem().exists(&server_dir.join("srv.jar")),
        "Vanilla server build should have the server JAR"
    );

    let create_calls = session.archive_provider.create_calls.lock().unwrap();
    assert!(
        create_calls.iter().any(|(_, dest)| dest
            .to_string_lossy()
            .contains(&format!("{project_name}-v1.0.0-server.zip"))),
        "Server archive should be created: {create_calls:?}"
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Fabric client test
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_build_fabric_client() -> Result<()> {
    let project_name = "matrix-fabric-client";
    let workdir = mock_root().join("workdir");
    let session = setup_client_session(project_name, "fabric").build();

    Display::init_or_get(TerminalCapabilities::minimal());

    let result = execute_command_with_session(
        Commands::Build(BuildArgs {
            targets: vec!["client".to_string()],
            ..Default::default()
        }),
        &session,
    )
    .await;

    assert!(result.is_ok(), "Fabric client build failed: {result:?}");

    let client_dir = workdir.join("dist").join("client");
    assert!(
        session.filesystem().exists(&client_dir),
        "Client build directory should exist"
    );

    let minecraft_dir = client_dir.join(".minecraft");
    assert!(
        session
            .filesystem()
            .exists(&minecraft_dir.join("packwiz-installer-bootstrap.jar")),
        "Bootstrap installer should be copied into .minecraft/"
    );
    assert!(
        session
            .filesystem()
            .exists(&minecraft_dir.join("pack").join("pack.toml")),
        "Pack metadata should be copied into .minecraft/pack/"
    );

    let create_calls = session.archive_provider.create_calls.lock().unwrap();
    assert!(
        create_calls.iter().any(|(_, dest)| dest
            .to_string_lossy()
            .contains(&format!("{project_name}-v1.0.0-client.zip"))),
        "Client archive should be created: {create_calls:?}"
    );

    let packwiz_calls = session.process_provider.get_calls_for_command("packwiz");
    assert!(
        packwiz_calls
            .iter()
            .any(|call| call.args.iter().any(|a| a == "refresh")),
        "build should refresh pack metadata before client build: {packwiz_calls:?}"
    );

    Ok(())
}
