use anyhow::Result;
use empack_lib::application::cli::{BuildArgs, Commands};
use empack_lib::application::commands::execute_command_with_session;
use empack_lib::application::session::ProcessOutput;
use empack_lib::application::session_mocks::mock_root;
use empack_lib::display::Display;
use empack_lib::terminal::TerminalCapabilities;
use empack_tests::MockSessionBuilder;
use std::path::{Path, PathBuf};

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
    setup_server_session(project_name, loader)
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

    let packwiz_calls = session
        .process_provider
        .get_calls_for_command(empack_lib::empack::packwiz::PACKWIZ_BIN);
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

    let packwiz_calls = session
        .process_provider
        .get_calls_for_command(empack_lib::empack::packwiz::PACKWIZ_BIN);
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

    let packwiz_calls = session
        .process_provider
        .get_calls_for_command(empack_lib::empack::packwiz::PACKWIZ_BIN);
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

    let packwiz_calls = session
        .process_provider
        .get_calls_for_command(empack_lib::empack::packwiz::PACKWIZ_BIN);
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
// Forge tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_build_forge_mrpack() -> Result<()> {
    let project_name = "matrix-forge-mrpack";
    let workdir = mock_root().join("workdir");
    let session = setup_mrpack_session(project_name, "forge").build();

    Display::init_or_get(TerminalCapabilities::minimal());

    let result = execute_command_with_session(
        Commands::Build(BuildArgs {
            targets: vec!["mrpack".to_string()],
            ..Default::default()
        }),
        &session,
    )
    .await;

    assert!(result.is_ok(), "Forge mrpack build failed: {result:?}");

    let mrpack_path = workdir
        .join("dist")
        .join(format!("{project_name}-v1.0.0.mrpack"));
    assert!(
        session.filesystem().exists(&mrpack_path),
        "mrpack artifact should be created in dist/"
    );

    let packwiz_calls = session
        .process_provider
        .get_calls_for_command(empack_lib::empack::packwiz::PACKWIZ_BIN);
    assert!(
        packwiz_calls
            .iter()
            .any(|call| call.args.iter().any(|a| a == "mr")
                && call.args.iter().any(|a| a == "export")),
        "Forge mrpack build should export via packwiz: {packwiz_calls:?}"
    );

    Ok(())
}

#[tokio::test]
async fn test_build_forge_server() -> Result<()> {
    let project_name = "matrix-forge-server";
    let workdir = mock_root().join("workdir");
    let session = setup_server_session(project_name, "forge").build();

    Display::init_or_get(TerminalCapabilities::minimal());

    let result = execute_command_with_session(
        Commands::Build(BuildArgs {
            targets: vec!["server".to_string()],
            ..Default::default()
        }),
        &session,
    )
    .await;

    assert!(result.is_ok(), "Forge server build failed: {result:?}");

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

    let packwiz_calls = session
        .process_provider
        .get_calls_for_command(empack_lib::empack::packwiz::PACKWIZ_BIN);
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
async fn test_build_forge_server_full() -> Result<()> {
    let project_name = "matrix-forge-server-full";
    let workdir = mock_root().join("workdir");
    let session = setup_server_full_session(project_name, "forge").build();

    Display::init_or_get(TerminalCapabilities::minimal());

    let result = execute_command_with_session(
        Commands::Build(BuildArgs {
            targets: vec!["server-full".to_string()],
            ..Default::default()
        }),
        &session,
    )
    .await;

    assert!(result.is_ok(), "Forge server-full build failed: {result:?}");

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

    let packwiz_calls = session
        .process_provider
        .get_calls_for_command(empack_lib::empack::packwiz::PACKWIZ_BIN);
    assert!(
        packwiz_calls
            .iter()
            .any(|call| call.args.iter().any(|a| a == "refresh")),
        "build should refresh pack metadata before client build: {packwiz_calls:?}"
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// NeoForge client-full test
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_build_neoforge_client_full() -> Result<()> {
    let project_name = "matrix-neoforge-client-full";
    let workdir = mock_root().join("workdir");

    let session = MockSessionBuilder::new()
        .with_empack_project(project_name, "1.21.4", "neoforge")
        .with_pre_cached_jars()
        .build();

    Display::init_or_get(TerminalCapabilities::minimal());

    let result = execute_command_with_session(
        Commands::Build(BuildArgs {
            targets: vec!["client-full".to_string()],
            ..Default::default()
        }),
        &session,
    )
    .await;

    assert!(
        result.is_ok(),
        "NeoForge client-full build failed: {result:?}"
    );

    let client_full_dir = workdir.join("dist").join("client-full");
    assert!(
        session.filesystem().exists(&client_full_dir),
        "Client-full build directory should exist"
    );
    assert!(
        session
            .filesystem()
            .exists(&client_full_dir.join("pack").join("pack.toml")),
        "Pack metadata should be copied into client-full output"
    );
    assert!(
        session
            .filesystem()
            .exists(&client_full_dir.join("mods").join("both-installed.txt")),
        "Mock installer should leave a deterministic install marker"
    );

    let create_calls = session.archive_provider.create_calls.lock().unwrap();
    assert!(
        create_calls.iter().any(|(_, dest)| dest
            .to_string_lossy()
            .contains(&format!("{project_name}-v1.0.0-client-full.zip"))),
        "Client-full archive should be created: {create_calls:?}"
    );

    let java_calls = session.process_provider.get_calls_for_command("java");
    assert!(
        java_calls
            .iter()
            .any(|call| call.args.iter().any(|a| a == "-s")
                && call.args.iter().any(|a| a == "both")
                && call.args.iter().any(|a| a == "--bootstrap-main-jar")
                && call.args.iter().any(|a| a.contains("pack.toml"))),
        "client-full build should invoke packwiz installer for both sides: {java_calls:?}"
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Build command tests (from build_command.rs)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn e2e_build_mrpack_successfully() -> Result<()> {
    let session = MockSessionBuilder::new()
        .with_empack_project("workflow-build-pack", "1.21.1", "fabric")
        .build();

    Display::init_or_get(TerminalCapabilities::minimal());

    let workdir = session.filesystem().current_dir()?;
    let pack_file = workdir.join("pack").join("pack.toml");
    let mrpack_path = workdir
        .join("dist")
        .join("workflow-build-pack-v1.0.0.mrpack");

    let result = execute_command_with_session(
        Commands::Build(BuildArgs {
            targets: vec!["mrpack".to_string()],
            ..Default::default()
        }),
        &session,
    )
    .await;

    assert!(result.is_ok(), "Build command failed: {:?}", result);

    assert!(
        session.filesystem().exists(&mrpack_path),
        "mrpack build should create an artifact in dist/"
    );

    let packwiz_calls = session
        .process_provider
        .get_calls_for_command(empack_lib::empack::packwiz::PACKWIZ_BIN);
    let pack_file_str = pack_file.to_string_lossy();
    let mrpack_path_str = mrpack_path.to_string_lossy();
    assert!(
        packwiz_calls
            .iter()
            .any(|call| { call.args == vec!["--pack-file", pack_file_str.as_ref(), "refresh"] }),
        "build should refresh the pack before exporting: {packwiz_calls:?}"
    );
    assert!(
        packwiz_calls.iter().any(|call| {
            call.args
                == vec![
                    "--pack-file",
                    pack_file_str.as_ref(),
                    "mr",
                    "export",
                    "-o",
                    mrpack_path_str.as_ref(),
                ]
        }),
        "build should export the mrpack artifact through packwiz: {packwiz_calls:?}"
    );

    Ok(())
}

#[tokio::test]
async fn e2e_build_clean_recreates_mrpack_and_preserves_configuration() -> Result<()> {
    let workdir = mock_root().join("workdir");
    let stale_server_dir = workdir.join("dist").join("server");
    let mrpack_path = workdir
        .join("dist")
        .join("workflow-build-clean-v1.0.0.mrpack");
    let sentinel = workdir.join("sentinel.txt");

    let session = MockSessionBuilder::new()
        .with_empack_project("workflow-build-clean", "1.21.1", "fabric")
        .with_file(
            stale_server_dir.join("stale.txt"),
            "stale build output".to_string(),
        )
        .with_file(mrpack_path.clone(), "stale mrpack artifact".to_string())
        .with_file(sentinel.clone(), "preserve me".to_string())
        .build();

    Display::init_or_get(TerminalCapabilities::minimal());

    let pack_file = workdir.join("pack").join("pack.toml");
    let result = execute_command_with_session(
        Commands::Build(BuildArgs {
            targets: vec!["mrpack".to_string()],
            clean: true,
            ..Default::default()
        }),
        &session,
    )
    .await;

    assert!(
        result.is_ok(),
        "clean-before-build should rebuild the mrpack artifact: {result:?}"
    );
    assert!(
        session.filesystem().exists(&mrpack_path),
        "clean-before-build should recreate the mrpack artifact in dist/"
    );
    let rebuilt_content = session.filesystem().read_to_string(&mrpack_path)?;
    assert_ne!(
        rebuilt_content, "stale mrpack artifact",
        "clean-before-build should replace stale artifact contents with the rebuilt artifact"
    );
    assert!(
        !session.filesystem().exists(&stale_server_dir),
        "clean-before-build should remove stale sibling build directories under dist/"
    );
    assert!(
        session.filesystem().exists(&workdir.join("empack.yml")),
        "clean-before-build should preserve empack.yml"
    );
    assert!(
        session.filesystem().exists(&pack_file),
        "clean-before-build should preserve pack.toml"
    );
    assert!(
        session
            .filesystem()
            .exists(&workdir.join("pack").join("index.toml")),
        "clean-before-build should preserve index.toml"
    );
    assert!(
        session.filesystem().exists(&sentinel),
        "clean-before-build should not remove unrelated project files outside dist/"
    );

    let packwiz_calls = session
        .process_provider
        .get_calls_for_command(empack_lib::empack::packwiz::PACKWIZ_BIN);
    let pack_file_str = pack_file.to_string_lossy();
    let mrpack_path_str = mrpack_path.to_string_lossy();
    assert!(
        packwiz_calls
            .iter()
            .any(|call| { call.args == vec!["--pack-file", pack_file_str.as_ref(), "refresh"] }),
        "clean-before-build should refresh the pack after cleaning: {packwiz_calls:?}"
    );
    assert!(
        packwiz_calls.iter().any(|call| {
            call.args
                == vec![
                    "--pack-file",
                    pack_file_str.as_ref(),
                    "mr",
                    "export",
                    "-o",
                    mrpack_path_str.as_ref(),
                ]
        }),
        "clean-before-build should export the rebuilt mrpack artifact: {packwiz_calls:?}"
    );

    Ok(())
}

#[tokio::test]
async fn e2e_build_packwiz_refresh_fails() -> Result<()> {
    let workdir = mock_root().join("workdir");
    let pack_file = workdir.join("pack").join("pack.toml");

    let session = MockSessionBuilder::new()
        .with_empack_project("workflow-build-refresh-fails", "1.21.1", "fabric")
        .with_packwiz_result(
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
        )
        .build();

    Display::init_or_get(TerminalCapabilities::minimal());

    let result = execute_command_with_session(
        Commands::Build(BuildArgs {
            targets: vec!["mrpack".to_string()],
            ..Default::default()
        }),
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
    let mrpack_path = workdir
        .join("dist")
        .join("workflow-build-refresh-fails-v1.0.0.mrpack");
    assert!(
        !session.filesystem().exists(&mrpack_path),
        "No mrpack artifact should be produced after a failed refresh"
    );

    Ok(())
}

#[tokio::test]
async fn e2e_build_packwiz_export_fails() -> Result<()> {
    let workdir = mock_root().join("workdir");
    let pack_file = workdir.join("pack").join("pack.toml");
    let mrpack_path = workdir
        .join("dist")
        .join("workflow-build-export-fails-v1.0.0.mrpack");

    let session = MockSessionBuilder::new()
        .with_empack_project("workflow-build-export-fails", "1.21.1", "fabric")
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
        )
        .build();

    Display::init_or_get(TerminalCapabilities::minimal());

    let result = execute_command_with_session(
        Commands::Build(BuildArgs {
            targets: vec!["mrpack".to_string()],
            ..Default::default()
        }),
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
        !session.filesystem().exists(&mrpack_path),
        "Export failure should not leave a partial mrpack artifact"
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Client-full tests (from build_client_full.rs)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn e2e_build_client_full_successfully() -> Result<()> {
    let project_name = "workflow-client-full";
    let workdir = mock_root().join("workdir");

    let session = MockSessionBuilder::new()
        .with_empack_project(project_name, "1.21.1", "fabric")
        .with_pre_cached_jars()
        .build();

    Display::init_or_get(TerminalCapabilities::minimal());

    let result = execute_command_with_session(
        Commands::Build(BuildArgs {
            targets: vec!["client-full".to_string()],
            ..Default::default()
        }),
        &session,
    )
    .await;

    assert!(result.is_ok(), "Client-full build failed: {result:?}");

    let client_full_dir = workdir.join("dist").join("client-full");
    assert!(
        session.filesystem().exists(&client_full_dir),
        "Client-full build directory should exist"
    );
    assert!(
        session
            .filesystem()
            .exists(&client_full_dir.join("pack").join("pack.toml")),
        "Pack metadata should be copied into client-full output"
    );
    assert!(
        session
            .filesystem()
            .exists(&client_full_dir.join("mods").join("both-installed.txt")),
        "Mock installer should leave a deterministic install marker"
    );

    let create_calls = session.archive_provider.create_calls.lock().unwrap();
    assert!(
        create_calls.iter().any(|(_, dest)| dest
            .to_string_lossy()
            .contains(&format!("{project_name}-v1.0.0-client-full.zip"))),
        "Client-full archive should be created: {create_calls:?}"
    );
    assert!(
        !session
            .filesystem()
            .exists(&workdir.join("dist").join("client")),
        "Standalone client-full builds should not materialize the client target directory"
    );

    let packwiz_calls = session
        .process_provider
        .get_calls_for_command(empack_lib::empack::packwiz::PACKWIZ_BIN);
    assert!(
        packwiz_calls
            .iter()
            .any(|call| call.args.iter().any(|a| a == "refresh")),
        "build should refresh pack metadata before client-full build: {packwiz_calls:?}"
    );

    let java_calls = session.process_provider.get_calls_for_command("java");
    assert!(
        java_calls
            .iter()
            .any(|call| call.args.iter().any(|a| a == "-s")
                && call.args.iter().any(|a| a == "both")
                && call.args.iter().any(|a| a == "--bootstrap-main-jar")
                && call.args.iter().any(|a| a.contains("pack.toml"))),
        "client-full build should invoke packwiz installer for both sides: {java_calls:?}"
    );

    Ok(())
}

#[tokio::test]
async fn e2e_build_client_full_missing_installer() -> Result<()> {
    let project_name = "workflow-client-full-missing-installer";
    let workdir = mock_root().join("workdir");

    let session = MockSessionBuilder::new()
        .with_empack_project(project_name, "1.21.1", "fabric")
        .build();

    Display::init_or_get(TerminalCapabilities::minimal());

    let result = execute_command_with_session(
        Commands::Build(BuildArgs {
            targets: vec!["client-full".to_string()],
            ..Default::default()
        }),
        &session,
    )
    .await;

    assert!(
        result.is_err(),
        "Build should fail when installer JAR is unavailable"
    );
    let error = result.unwrap_err().to_string();
    assert!(
        error.contains("Mock HTTP client unavailable (test mode)")
            || error.contains("Failed to read file:"),
        "Missing installer should fail while resolving the bootstrap JAR, got: {error}"
    );
    assert!(
        !session.filesystem().exists(
            &workdir
                .join("dist")
                .join(format!("{project_name}-v1.0.0-client-full.zip"))
        ),
        "No client-full archive should be produced when the installer is missing"
    );
    assert!(
        !session.filesystem().exists(
            &workdir
                .join("dist")
                .join("client-full")
                .join("mods")
                .join("both-installed.txt")
        ),
        "The full installer step should not run when the installer bootstrap is missing"
    );
    assert!(
        !session
            .filesystem()
            .exists(&workdir.join("dist").join("client")),
        "Standalone client-full failures should not create the client target directory"
    );

    Ok(())
}

#[tokio::test]
async fn e2e_build_client_full_with_pack_structure() -> Result<()> {
    let project_name = "workflow-client-full-structure";
    let workdir = mock_root().join("workdir");
    let mods_dir = workdir.join("pack").join("mods");

    let session = MockSessionBuilder::new()
        .with_empack_project(project_name, "1.21.1", "fabric")
        .with_pre_cached_jars()
        .with_file(
            mods_dir.join("example-mod.pw.toml"),
            "[download]\nurl = \"https://example.com/mod.jar\"\nhash = \"abc123\"\n".to_string(),
        )
        .build();

    Display::init_or_get(TerminalCapabilities::minimal());

    let result = execute_command_with_session(
        Commands::Build(BuildArgs {
            targets: vec!["client-full".to_string()],
            ..Default::default()
        }),
        &session,
    )
    .await;

    assert!(result.is_ok(), "Client-full build failed: {result:?}");

    let client_full_dir = workdir.join("dist").join("client-full");
    assert!(
        session.filesystem().exists(&client_full_dir),
        "Client-full build directory should exist"
    );
    assert!(
        session.filesystem().exists(
            &client_full_dir
                .join("pack")
                .join("mods")
                .join("example-mod.pw.toml")
        ),
        "Existing pack structure should be copied into client-full output"
    );
    assert!(
        session
            .filesystem()
            .exists(&client_full_dir.join("mods").join("both-installed.txt")),
        "Installer marker should confirm the mocked full download step"
    );

    let create_calls = session.archive_provider.create_calls.lock().unwrap();
    assert!(
        create_calls.iter().any(|(_, dest)| dest
            .to_string_lossy()
            .contains(&format!("{project_name}-v1.0.0-client-full.zip"))),
        "Client-full archive should be created: {create_calls:?}"
    );
    assert!(
        !session
            .filesystem()
            .exists(&workdir.join("dist").join("client")),
        "Structured standalone client-full builds should not materialize the client target directory"
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Server tests (from build_server.rs)
// ---------------------------------------------------------------------------

fn server_template_files(workdir: &Path) -> Vec<(PathBuf, String)> {
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

    for (path, content) in server_template_files(&workdir) {
        builder = builder.with_file(path, content);
    }

    let session = builder.build();

    Display::init_or_get(TerminalCapabilities::minimal());

    let result = execute_command_with_session(
        Commands::Build(BuildArgs {
            targets: vec!["server".to_string()],
            ..Default::default()
        }),
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

    let packwiz_calls = session
        .process_provider
        .get_calls_for_command(empack_lib::empack::packwiz::PACKWIZ_BIN);
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
        Commands::Build(BuildArgs {
            targets: vec!["server".to_string()],
            ..Default::default()
        }),
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
        Commands::Build(BuildArgs {
            targets: vec!["server".to_string()],
            ..Default::default()
        }),
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

// ---------------------------------------------------------------------------
// Server-full tests (from build_server_full.rs)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn e2e_build_server_full_successfully() -> Result<()> {
    let project_name = "workflow-server-full";
    let workdir = mock_root().join("workdir");

    let mut builder = MockSessionBuilder::new()
        .with_empack_project(project_name, "1.21.1", "fabric")
        .with_pre_cached_jars()
        .with_server_jar_stub();

    for (path, content) in server_template_files(&workdir) {
        builder = builder.with_file(path, content);
    }

    let session = builder.build();

    Display::init_or_get(TerminalCapabilities::minimal());

    let result = execute_command_with_session(
        Commands::Build(BuildArgs {
            targets: vec!["server-full".to_string()],
            ..Default::default()
        }),
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
        !session
            .filesystem()
            .exists(&workdir.join("dist").join("server")),
        "Standalone server-full builds should not materialize the server target directory"
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

#[tokio::test]
async fn e2e_build_server_full_missing_installer() -> Result<()> {
    let project_name = "workflow-server-full-missing-installer";
    let workdir = mock_root().join("workdir");

    let session = MockSessionBuilder::new()
        .with_empack_project(project_name, "1.21.1", "fabric")
        .build();

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
        Commands::Build(BuildArgs {
            targets: vec!["server-full".to_string()],
            ..Default::default()
        }),
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
