// NOTE: `set_current_dir` mutates process-global state but is safe here because
// nextest runs each test in its own process, preventing cross-test interference.
// This matches the pattern used across all hermetic tests in this crate.

use anyhow::Result;
use empack_lib::application::cli::Commands;
use empack_lib::application::commands::execute_command_with_session;
use empack_lib::display::Display;
use empack_lib::terminal::TerminalCapabilities;
use empack_tests::{HermeticSessionBuilder, MockBehavior};
use std::fs;

fn standard_packwiz_mock() -> MockBehavior {
    MockBehavior::SucceedWithOutput {
        stdout: "Initialized packwiz project".to_string(),
        stderr: String::new(),
    }
}

fn standard_git_mock() -> MockBehavior {
    MockBehavior::SucceedWithOutput {
        stdout: "main".to_string(),
        stderr: String::new(),
    }
}

fn standard_which_mock() -> MockBehavior {
    MockBehavior::SucceedWithOutput {
        stdout: "/test/bin/packwiz".to_string(),
        stderr: String::new(),
    }
}

#[cfg(unix)]
#[tokio::test]
async fn test_init_neoforge() -> Result<()> {
    let (session, test_env) = HermeticSessionBuilder::new()?
        .with_yes_flag()
        .with_mock_executable("packwiz", standard_packwiz_mock())?
        .with_mock_executable("git", standard_git_mock())?
        .with_mock_executable("which", standard_which_mock())?
        .build()?;

    let terminal_caps = TerminalCapabilities::detect_from_config(session.config().app_config())?;
    Display::init_or_get(terminal_caps);

    let workdir = test_env.work_path.clone();
    std::env::set_current_dir(&workdir)?;

    let result = execute_command_with_session(
        Commands::Init {
            name: Some("neoforge-pack".to_string()),
            pack_name: None,
            force: false,
            modloader: Some("neoforge".to_string()),
            mc_version: Some("1.21.4".to_string()),
            author: None,
            loader_version: None,
            pack_version: None,
        },
        &session,
    )
    .await;

    assert!(result.is_ok(), "Init with NeoForge failed: {:?}", result);

    let project_dir = workdir.join("neoforge-pack");
    assert!(project_dir.exists(), "neoforge-pack directory should exist");

    let empack_yml = fs::read_to_string(project_dir.join("empack.yml"))?;
    assert!(
        empack_yml.contains("loader: neoforge"),
        "empack.yml should have loader: neoforge, got: {}",
        empack_yml
    );
    assert!(
        empack_yml.contains("minecraft_version: 1.21.4")
            || empack_yml.contains("minecraft_version: \"1.21.4\""),
        "empack.yml should have minecraft_version 1.21.4, got: {}",
        empack_yml
    );

    let pack_toml = fs::read_to_string(project_dir.join("pack").join("pack.toml"))?;
    assert!(
        pack_toml.contains("neoforge = "),
        "pack.toml [versions] should contain neoforge entry, got: {}",
        pack_toml
    );

    let packwiz_calls = test_env.get_mock_invocations("packwiz")?;
    let init_call = packwiz_calls
        .iter()
        .find(|call| call.args.first().map(String::as_str) == Some("init"))
        .expect("packwiz init should have been called");
    assert!(
        init_call.contains_args(&["--modloader", "neoforge"]),
        "packwiz should receive --modloader neoforge: {init_call:?}"
    );
    assert!(
        init_call.args.iter().any(|arg| arg == "--neoforge-version"),
        "packwiz should receive --neoforge-version flag: {init_call:?}"
    );

    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn test_init_quilt() -> Result<()> {
    let (session, test_env) = HermeticSessionBuilder::new()?
        .with_yes_flag()
        .with_mock_executable("packwiz", standard_packwiz_mock())?
        .with_mock_executable("git", standard_git_mock())?
        .with_mock_executable("which", standard_which_mock())?
        .build()?;

    let terminal_caps = TerminalCapabilities::detect_from_config(session.config().app_config())?;
    Display::init_or_get(terminal_caps);

    let workdir = test_env.work_path.clone();
    std::env::set_current_dir(&workdir)?;

    let result = execute_command_with_session(
        Commands::Init {
            name: Some("quilt-pack".to_string()),
            pack_name: None,
            force: false,
            modloader: Some("quilt".to_string()),
            mc_version: Some("1.21.4".to_string()),
            author: None,
            loader_version: None,
            pack_version: None,
        },
        &session,
    )
    .await;

    assert!(result.is_ok(), "Init with Quilt failed: {:?}", result);

    let project_dir = workdir.join("quilt-pack");
    assert!(project_dir.exists(), "quilt-pack directory should exist");

    let empack_yml = fs::read_to_string(project_dir.join("empack.yml"))?;
    assert!(
        empack_yml.contains("loader: quilt"),
        "empack.yml should have loader: quilt, got: {}",
        empack_yml
    );
    assert!(
        empack_yml.contains("minecraft_version: 1.21.4")
            || empack_yml.contains("minecraft_version: \"1.21.4\""),
        "empack.yml should have minecraft_version 1.21.4, got: {}",
        empack_yml
    );

    let pack_toml = fs::read_to_string(project_dir.join("pack").join("pack.toml"))?;
    assert!(
        pack_toml.contains("quilt = "),
        "pack.toml [versions] should contain quilt entry, got: {}",
        pack_toml
    );

    let packwiz_calls = test_env.get_mock_invocations("packwiz")?;
    let init_call = packwiz_calls
        .iter()
        .find(|call| call.args.first().map(String::as_str) == Some("init"))
        .expect("packwiz init should have been called");
    assert!(
        init_call.contains_args(&["--modloader", "quilt"]),
        "packwiz should receive --modloader quilt: {init_call:?}"
    );
    assert!(
        init_call.args.iter().any(|arg| arg == "--quilt-version"),
        "packwiz should receive --quilt-version flag: {init_call:?}"
    );

    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn test_init_vanilla() -> Result<()> {
    let (session, test_env) = HermeticSessionBuilder::new()?
        .with_yes_flag()
        .with_mock_executable("packwiz", standard_packwiz_mock())?
        .with_mock_executable("git", standard_git_mock())?
        .with_mock_executable("which", standard_which_mock())?
        .build()?;

    let terminal_caps = TerminalCapabilities::detect_from_config(session.config().app_config())?;
    Display::init_or_get(terminal_caps);

    let workdir = test_env.work_path.clone();
    std::env::set_current_dir(&workdir)?;

    let result = execute_command_with_session(
        Commands::Init {
            name: Some("vanilla-pack".to_string()),
            pack_name: None,
            force: false,
            modloader: Some("none".to_string()),
            mc_version: Some("1.21.4".to_string()),
            author: None,
            loader_version: None,
            pack_version: None,
        },
        &session,
    )
    .await;

    assert!(result.is_ok(), "Init with vanilla failed: {:?}", result);

    let project_dir = workdir.join("vanilla-pack");
    assert!(project_dir.exists(), "vanilla-pack directory should exist");

    let empack_yml = fs::read_to_string(project_dir.join("empack.yml"))?;
    assert!(
        !empack_yml.contains("loader:"),
        "vanilla empack.yml should not have a loader field, got: {}",
        empack_yml
    );

    let packwiz_calls = test_env.get_mock_invocations("packwiz")?;
    let init_call = packwiz_calls
        .iter()
        .find(|call| call.args.first().map(String::as_str) == Some("init"))
        .expect("packwiz init should have been called");
    assert!(
        init_call.contains_args(&["--modloader", "none"]),
        "packwiz should receive --modloader none: {init_call:?}"
    );
    let has_loader_version_flag = init_call.args.iter().any(|arg| {
        arg == "--fabric-version"
            || arg == "--neoforge-version"
            || arg == "--forge-version"
            || arg == "--quilt-version"
    });
    assert!(
        !has_loader_version_flag,
        "vanilla init should not pass any loader version flag: {init_call:?}"
    );

    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn test_init_fabric_older_mc() -> Result<()> {
    let (session, test_env) = HermeticSessionBuilder::new()?
        .with_yes_flag()
        .with_mock_executable("packwiz", standard_packwiz_mock())?
        .with_mock_executable("git", standard_git_mock())?
        .with_mock_executable("which", standard_which_mock())?
        .build()?;

    let terminal_caps = TerminalCapabilities::detect_from_config(session.config().app_config())?;
    Display::init_or_get(terminal_caps);

    let workdir = test_env.work_path.clone();
    std::env::set_current_dir(&workdir)?;

    let result = execute_command_with_session(
        Commands::Init {
            name: Some("fabric-old-mc".to_string()),
            pack_name: None,
            force: false,
            modloader: Some("fabric".to_string()),
            mc_version: Some("1.20.1".to_string()),
            author: None,
            loader_version: None,
            pack_version: None,
        },
        &session,
    )
    .await;

    assert!(
        result.is_ok(),
        "Init with Fabric 1.20.1 failed: {:?}",
        result
    );

    let project_dir = workdir.join("fabric-old-mc");
    assert!(
        project_dir.exists(),
        "fabric-old-mc directory should exist"
    );

    let empack_yml = fs::read_to_string(project_dir.join("empack.yml"))?;
    assert!(
        empack_yml.contains("loader: fabric"),
        "empack.yml should have loader: fabric, got: {}",
        empack_yml
    );
    assert!(
        empack_yml.contains("minecraft_version: 1.20.1")
            || empack_yml.contains("minecraft_version: \"1.20.1\""),
        "empack.yml should have minecraft_version 1.20.1, got: {}",
        empack_yml
    );

    let pack_toml = fs::read_to_string(project_dir.join("pack").join("pack.toml"))?;
    assert!(
        pack_toml.contains("1.20.1"),
        "pack.toml should reference minecraft 1.20.1, got: {}",
        pack_toml
    );
    assert!(
        pack_toml.contains("fabric = "),
        "pack.toml [versions] should contain fabric entry, got: {}",
        pack_toml
    );

    let packwiz_calls = test_env.get_mock_invocations("packwiz")?;
    let init_call = packwiz_calls
        .iter()
        .find(|call| call.args.first().map(String::as_str) == Some("init"))
        .expect("packwiz init should have been called");
    assert!(
        init_call.contains_args(&["--mc-version", "1.20.1"]),
        "packwiz should receive --mc-version 1.20.1: {init_call:?}"
    );
    assert!(
        init_call.contains_args(&["--modloader", "fabric"]),
        "packwiz should receive --modloader fabric: {init_call:?}"
    );

    Ok(())
}
