//! Integration tests for `empack sync` workflow.
//!
//! These scenarios exercise real workflow planning with hermetic packwiz command
//! assertions instead of only checking that the command doesn't panic.

use anyhow::Result;
use empack_lib::application::cli::Commands;
use empack_lib::application::commands::execute_command_with_session;
use empack_lib::display::Display;
use empack_lib::terminal::TerminalCapabilities;
use empack_tests::{HermeticSessionBuilder, MockBehavior};
use std::fs;

fn sync_project_config() -> &'static str {
    r#"empack:
  name: "Sync Pack"
  author: "Workflow Test"
  version: "1.0.0"
  minecraft_version: "1.21.1"
  loader: fabric
  dependencies:
    sodium:
      status: resolved
      title: Sodium
      platform: modrinth
      project_id: AANobbMI
      type: mod
    fabric_api:
      status: resolved
      title: Fabric API
      platform: modrinth
      project_id: P7dR8mSH
      type: mod
"#
}

/// Minimal .pw.toml content to mark a mod as installed
fn pw_toml_stub(name: &str) -> String {
    format!(
        r#"name = "{name}"
filename = "{name}.jar"
side = "both"

[download]
url = ""
hash = ""
"#
    )
}

/// Test: empack sync executes the planned add/remove actions hermetically.
#[cfg(unix)]
#[tokio::test]
async fn test_sync_workflow_full() -> Result<()> {
    let (session, test_env) = HermeticSessionBuilder::new()?
        .with_mock_http_client()
        .with_empack_project("sync-pack", "1.21.1", "fabric")?
        .with_mock_executable("packwiz", MockBehavior::AlwaysSucceed)?
        .build()?;

    let terminal_caps = TerminalCapabilities::detect_from_config(session.config().app_config())?;
    Display::init_or_get(terminal_caps);

    let workdir = session
        .config()
        .app_config()
        .workdir
        .clone()
        .expect("hermetic project should configure a workdir");
    std::env::set_current_dir(&workdir)?;
    fs::write(workdir.join("empack.yml"), sync_project_config())?;

    // Simulate sodium already installed, old-mod is extra
    let mods_dir = workdir.join("pack").join("mods");
    fs::create_dir_all(&mods_dir)?;
    fs::write(mods_dir.join("sodium.pw.toml"), pw_toml_stub("sodium"))?;
    fs::write(mods_dir.join("old-mod.pw.toml"), pw_toml_stub("old-mod"))?;

    let sync_result = execute_command_with_session(Commands::Sync {}, &session).await;
    assert!(sync_result.is_ok(), "sync command failed: {sync_result:?}");

    let packwiz_calls = test_env.get_mock_invocations("packwiz")?;
    assert!(
        packwiz_calls.iter().any(|call| call.contains_args(&[
            "modrinth",
            "add",
            "--project-id",
            "P7dR8mSH",
            "-y"
        ])),
        "sync should add the missing dependency by project id: {packwiz_calls:?}"
    );
    assert!(
        packwiz_calls
            .iter()
            .any(|call| call.contains_args(&["remove", "old-mod"])),
        "sync should remove mods not declared in empack.yml: {packwiz_calls:?}"
    );
    assert!(
        !packwiz_calls.iter().any(|call| call.contains_args(&[
            "modrinth",
            "add",
            "--project-id",
            "AANobbMI",
            "-y"
        ])),
        "sync should not re-add dependencies that are already installed: {packwiz_calls:?}"
    );

    Ok(())
}

/// Test: empack sync --dry-run plans actions without mutating packwiz state.
#[cfg(unix)]
#[tokio::test]
async fn test_sync_dry_run_no_modifications() -> Result<()> {
    let (session, test_env) = HermeticSessionBuilder::new()?
        .with_dry_run_flag()
        .with_mock_http_client()
        .with_empack_project("sync-pack-dry-run", "1.21.1", "fabric")?
        .with_mock_executable("packwiz", MockBehavior::AlwaysSucceed)?
        .build()?;

    let terminal_caps = TerminalCapabilities::detect_from_config(session.config().app_config())?;
    Display::init_or_get(terminal_caps);

    let workdir = session
        .config()
        .app_config()
        .workdir
        .clone()
        .expect("hermetic project should configure a workdir");
    std::env::set_current_dir(&workdir)?;
    fs::write(workdir.join("empack.yml"), sync_project_config())?;

    // Simulate old-mod installed (will be planned for removal but not executed)
    let mods_dir = workdir.join("pack").join("mods");
    fs::create_dir_all(&mods_dir)?;
    fs::write(mods_dir.join("old-mod.pw.toml"), pw_toml_stub("old-mod"))?;

    let sync_result = execute_command_with_session(Commands::Sync {}, &session).await;
    assert!(
        sync_result.is_ok(),
        "dry-run sync command failed: {sync_result:?}"
    );

    let packwiz_calls = test_env.get_mock_invocations("packwiz")?;
    assert!(
        !packwiz_calls.iter().any(|call| {
            call.args
                .windows(2)
                .any(|window| window == ["modrinth", "add"] || window == ["curseforge", "add"])
        }),
        "dry-run sync must not add dependencies: {packwiz_calls:?}"
    );
    assert!(
        !packwiz_calls
            .iter()
            .any(|call| call.args.first().map(String::as_str) == Some("remove")),
        "dry-run sync must not remove dependencies: {packwiz_calls:?}"
    );

    Ok(())
}

/// Test: sync matches installed .pw.toml filenames by slug key (no normalization).
#[cfg(unix)]
#[tokio::test]
async fn test_sync_normalized_installed_names_noop() -> Result<()> {
    let (session, test_env) = HermeticSessionBuilder::new()?
        .with_mock_http_client()
        .with_empack_project("sync-pack-normalized", "1.21.1", "fabric")?
        .with_mock_executable("packwiz", MockBehavior::AlwaysSucceed)?
        .build()?;

    let terminal_caps = TerminalCapabilities::detect_from_config(session.config().app_config())?;
    Display::init_or_get(terminal_caps);

    let workdir = session
        .config()
        .app_config()
        .workdir
        .clone()
        .expect("hermetic project should configure a workdir");
    std::env::set_current_dir(&workdir)?;
    fs::write(workdir.join("empack.yml"), sync_project_config())?;

    // Both mods installed with exact slug-matching filenames
    let mods_dir = workdir.join("pack").join("mods");
    fs::create_dir_all(&mods_dir)?;
    fs::write(mods_dir.join("sodium.pw.toml"), pw_toml_stub("sodium"))?;
    fs::write(
        mods_dir.join("fabric_api.pw.toml"),
        pw_toml_stub("fabric_api"),
    )?;

    let sync_result = execute_command_with_session(Commands::Sync {}, &session).await;
    assert!(
        sync_result.is_ok(),
        "slug-matching installed names should produce a no-op sync: {sync_result:?}"
    );

    let packwiz_calls = test_env.get_mock_invocations("packwiz")?;
    assert!(
        packwiz_calls.is_empty(),
        "all-installed sync should not call packwiz at all: {packwiz_calls:?}"
    );

    Ok(())
}
