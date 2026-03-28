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
use std::path::{Path, PathBuf};

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

fn create_server_templates(workdir: &Path) -> Result<()> {
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
    Ok(())
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

fn fix_pack_toml_loader_version(workdir: &Path, loader: &str) -> Result<()> {
    let version = loader_version_for_pack_toml(loader);
    let pack_toml_path = workdir.join("pack").join("pack.toml");
    let content = std::fs::read_to_string(&pack_toml_path)?;
    let fixed = content.replace(
        &format!("{loader} = \"0.15.0\""),
        &format!("{loader} = \"{version}\""),
    );
    std::fs::write(&pack_toml_path, fixed)?;
    Ok(())
}

async fn setup_mrpack_session(
    project_name: &str,
    loader: &str,
) -> Result<(HermeticSession, TestEnvironment, PathBuf)> {
    let builder =
        HermeticSessionBuilder::new()?.with_empack_project(project_name, "1.21.4", loader)?;

    let workdir_early = builder.test_env().work_path.join(project_name);
    fix_pack_toml_loader_version(&workdir_early, loader)?;

    let (session, test_env) = builder
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

    Ok((session, test_env, workdir))
}

async fn setup_server_session(
    project_name: &str,
    loader: &str,
) -> Result<(HermeticSession, TestEnvironment, PathBuf)> {
    let builder =
        HermeticSessionBuilder::new()?.with_empack_project(project_name, "1.21.4", loader)?;

    let workdir_early = builder.test_env().work_path.join(project_name);
    fix_pack_toml_loader_version(&workdir_early, loader)?;

    let (session, test_env) = builder
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
                stdout: "Installed server".to_string(),
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

async fn setup_server_full_session(
    project_name: &str,
    loader: &str,
) -> Result<(HermeticSession, TestEnvironment, PathBuf)> {
    let builder =
        HermeticSessionBuilder::new()?.with_empack_project(project_name, "1.21.4", loader)?;

    let workdir_early = builder.test_env().work_path.join(project_name);
    fix_pack_toml_loader_version(&workdir_early, loader)?;

    let (session, test_env) = builder
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

async fn setup_client_session(
    project_name: &str,
    loader: &str,
) -> Result<(HermeticSession, TestEnvironment, PathBuf)> {
    let builder =
        HermeticSessionBuilder::new()?.with_empack_project(project_name, "1.21.4", loader)?;

    let workdir_early = builder.test_env().work_path.join(project_name);
    fix_pack_toml_loader_version(&workdir_early, loader)?;

    let (session, test_env) = builder
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
                stdout: "Installed client mods".to_string(),
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

// ---------------------------------------------------------------------------
// NeoForge tests
// ---------------------------------------------------------------------------

#[cfg(unix)]
#[tokio::test]
async fn test_build_neoforge_mrpack() -> anyhow::Result<()> {
    let project_name = "matrix-neoforge-mrpack";
    let (session, test_env, workdir) = setup_mrpack_session(project_name, "neoforge").await?;

    let result = execute_command_with_session(
        Commands::Build {
            targets: vec!["mrpack".to_string()],
            clean: false,
            format: CliArchiveFormat::Zip,
        },
        &session,
    )
    .await;

    assert!(result.is_ok(), "NeoForge mrpack build failed: {result:?}");

    let mrpack_path = workdir
        .join("dist")
        .join(format!("{project_name}-v1.0.0.mrpack"));
    assert!(
        mrpack_path.exists(),
        "mrpack artifact should be created in dist/"
    );

    let packwiz_calls = test_env.get_mock_calls("packwiz")?;
    assert!(
        packwiz_calls
            .iter()
            .any(|call| call.contains(" mr export ")),
        "NeoForge mrpack build should export via packwiz: {packwiz_calls:?}"
    );

    Ok(())
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_build_neoforge_server() -> anyhow::Result<()> {
    let project_name = "matrix-neoforge-server";
    let (session, test_env, workdir) = setup_server_session(project_name, "neoforge").await?;

    create_server_templates(&workdir)?;

    let result = execute_command_with_session(
        Commands::Build {
            targets: vec!["server".to_string()],
            clean: false,
            format: CliArchiveFormat::Zip,
        },
        &session,
    )
    .await;

    assert!(result.is_ok(), "NeoForge server build failed: {result:?}");

    let server_dir = workdir.join("dist").join("server");
    assert!(server_dir.exists(), "Server build directory should exist");
    assert!(
        server_dir.join("packwiz-installer-bootstrap.jar").exists(),
        "Bootstrap installer should be copied into server output"
    );
    assert!(
        server_dir.join("srv.jar").exists(),
        "Server build should materialize srv.jar"
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
        packwiz_calls
            .iter()
            .any(|call| call.contains(" mr export ")),
        "Server build should export an mrpack before extraction: {packwiz_calls:?}"
    );

    Ok(())
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_build_neoforge_server_full() -> anyhow::Result<()> {
    let project_name = "matrix-neoforge-server-full";
    let (session, test_env, workdir) = setup_server_full_session(project_name, "neoforge").await?;

    create_server_templates(&workdir)?;

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
        result.is_ok(),
        "NeoForge server-full build failed: {result:?}"
    );

    let server_full_dir = workdir.join("dist").join("server-full");
    assert!(
        server_full_dir.exists(),
        "Server-full build directory should exist"
    );
    assert!(
        server_full_dir.join("srv.jar").exists(),
        "Server-full build should materialize srv.jar"
    );
    assert!(
        server_full_dir.join("pack").join("pack.toml").exists(),
        "Pack contents should be copied into server-full output"
    );
    assert!(
        server_full_dir
            .join("mods")
            .join("server-installed.txt")
            .exists(),
        "Mock installer should leave a server install marker"
    );
    assert!(
        workdir
            .join("dist")
            .join(format!("{project_name}-v1.0.0-server-full.zip"))
            .exists(),
        "Server-full archive should be created"
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

// ---------------------------------------------------------------------------
// Quilt tests
// ---------------------------------------------------------------------------

#[cfg(unix)]
#[tokio::test]
async fn test_build_quilt_mrpack() -> anyhow::Result<()> {
    let project_name = "matrix-quilt-mrpack";
    let (session, test_env, workdir) = setup_mrpack_session(project_name, "quilt").await?;

    let result = execute_command_with_session(
        Commands::Build {
            targets: vec!["mrpack".to_string()],
            clean: false,
            format: CliArchiveFormat::Zip,
        },
        &session,
    )
    .await;

    assert!(result.is_ok(), "Quilt mrpack build failed: {result:?}");

    let mrpack_path = workdir
        .join("dist")
        .join(format!("{project_name}-v1.0.0.mrpack"));
    assert!(
        mrpack_path.exists(),
        "mrpack artifact should be created in dist/"
    );

    let packwiz_calls = test_env.get_mock_calls("packwiz")?;
    assert!(
        packwiz_calls
            .iter()
            .any(|call| call.contains(" mr export ")),
        "Quilt mrpack build should export via packwiz: {packwiz_calls:?}"
    );

    Ok(())
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_build_quilt_server() -> anyhow::Result<()> {
    let project_name = "matrix-quilt-server";
    let (session, test_env, workdir) = setup_server_session(project_name, "quilt").await?;

    create_server_templates(&workdir)?;

    let result = execute_command_with_session(
        Commands::Build {
            targets: vec!["server".to_string()],
            clean: false,
            format: CliArchiveFormat::Zip,
        },
        &session,
    )
    .await;

    assert!(result.is_ok(), "Quilt server build failed: {result:?}");

    let server_dir = workdir.join("dist").join("server");
    assert!(server_dir.exists(), "Server build directory should exist");
    assert!(
        server_dir.join("packwiz-installer-bootstrap.jar").exists(),
        "Bootstrap installer should be copied into server output"
    );
    assert!(
        server_dir.join("srv.jar").exists(),
        "Server build should materialize srv.jar"
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
        packwiz_calls
            .iter()
            .any(|call| call.contains(" mr export ")),
        "Server build should export an mrpack before extraction: {packwiz_calls:?}"
    );

    Ok(())
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_build_quilt_server_full() -> anyhow::Result<()> {
    let project_name = "matrix-quilt-server-full";
    let (session, test_env, workdir) = setup_server_full_session(project_name, "quilt").await?;

    create_server_templates(&workdir)?;

    let result = execute_command_with_session(
        Commands::Build {
            targets: vec!["server-full".to_string()],
            clean: false,
            format: CliArchiveFormat::Zip,
        },
        &session,
    )
    .await;

    assert!(result.is_ok(), "Quilt server-full build failed: {result:?}");

    let server_full_dir = workdir.join("dist").join("server-full");
    assert!(
        server_full_dir.exists(),
        "Server-full build directory should exist"
    );
    assert!(
        server_full_dir.join("srv.jar").exists(),
        "Server-full build should materialize srv.jar"
    );
    assert!(
        server_full_dir.join("pack").join("pack.toml").exists(),
        "Pack contents should be copied into server-full output"
    );
    assert!(
        server_full_dir
            .join("mods")
            .join("server-installed.txt")
            .exists(),
        "Mock installer should leave a server install marker"
    );
    assert!(
        workdir
            .join("dist")
            .join(format!("{project_name}-v1.0.0-server-full.zip"))
            .exists(),
        "Server-full archive should be created"
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

// ---------------------------------------------------------------------------
// Vanilla tests
// ---------------------------------------------------------------------------

#[cfg(unix)]
#[tokio::test]
async fn test_build_vanilla_mrpack() -> anyhow::Result<()> {
    let project_name = "matrix-vanilla-mrpack";
    let (session, test_env, workdir) = setup_mrpack_session(project_name, "none").await?;

    let result = execute_command_with_session(
        Commands::Build {
            targets: vec!["mrpack".to_string()],
            clean: false,
            format: CliArchiveFormat::Zip,
        },
        &session,
    )
    .await;

    assert!(result.is_ok(), "Vanilla mrpack build failed: {result:?}");

    let mrpack_path = workdir
        .join("dist")
        .join(format!("{project_name}-v1.0.0.mrpack"));
    assert!(
        mrpack_path.exists(),
        "mrpack artifact should be created in dist/"
    );

    let packwiz_calls = test_env.get_mock_calls("packwiz")?;
    assert!(
        packwiz_calls
            .iter()
            .any(|call| call.contains(" mr export ")),
        "Vanilla mrpack build should export via packwiz: {packwiz_calls:?}"
    );

    Ok(())
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_build_vanilla_server() -> anyhow::Result<()> {
    let project_name = "matrix-vanilla-server";
    let (session, test_env, workdir) = setup_server_session(project_name, "none").await?;

    create_server_templates(&workdir)?;

    let result = execute_command_with_session(
        Commands::Build {
            targets: vec!["server".to_string()],
            clean: false,
            format: CliArchiveFormat::Zip,
        },
        &session,
    )
    .await;

    assert!(result.is_ok(), "Vanilla server build failed: {result:?}");

    let server_dir = workdir.join("dist").join("server");
    assert!(server_dir.exists(), "Server build directory should exist");
    assert!(
        server_dir.join("packwiz-installer-bootstrap.jar").exists(),
        "Bootstrap installer should be copied into server output"
    );
    assert!(
        server_dir.join("srv.jar").exists(),
        "Vanilla server build should download the server JAR"
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
        packwiz_calls
            .iter()
            .any(|call| call.contains(" mr export ")),
        "Server build should export an mrpack before extraction: {packwiz_calls:?}"
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Fabric client test
// ---------------------------------------------------------------------------

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_build_fabric_client() -> anyhow::Result<()> {
    let project_name = "matrix-fabric-client";
    let (session, test_env, workdir) = setup_client_session(project_name, "fabric").await?;

    let result = execute_command_with_session(
        Commands::Build {
            targets: vec!["client".to_string()],
            clean: false,
            format: CliArchiveFormat::Zip,
        },
        &session,
    )
    .await;

    assert!(result.is_ok(), "Fabric client build failed: {result:?}");

    let client_dir = workdir.join("dist").join("client");
    assert!(client_dir.exists(), "Client build directory should exist");

    let minecraft_dir = client_dir.join(".minecraft");
    assert!(
        minecraft_dir
            .join("packwiz-installer-bootstrap.jar")
            .exists(),
        "Bootstrap installer should be copied into .minecraft/"
    );
    assert!(
        minecraft_dir.join("pack").join("pack.toml").exists(),
        "Pack metadata should be copied into .minecraft/pack/"
    );

    let archive = workdir
        .join("dist")
        .join(format!("{project_name}-v1.0.0-client.zip"));
    assert!(archive.exists(), "Client archive should be created");

    let packwiz_calls = test_env.get_mock_calls("packwiz")?;
    assert!(
        packwiz_calls.iter().any(|call| call.contains(" refresh")),
        "build should refresh pack metadata before client build: {packwiz_calls:?}"
    );

    Ok(())
}
