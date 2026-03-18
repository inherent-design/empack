//! Integration tests for `empack sync` workflow.
//!
//! These scenarios exercise real workflow planning with hermetic packwiz command
//! assertions instead of only checking that the command doesn't panic.

use anyhow::Result;
use empack_lib::application::cli::Commands;
use empack_lib::application::commands::execute_command_with_session;
use empack_lib::display::Display;
use empack_lib::terminal::TerminalCapabilities;
use empack_tests::test_env::ConditionalRule;
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
    - "sodium: Sodium|mod"
    - "fabric_api: Fabric API|mod"
  project_ids:
    sodium: "AANobbMI"
    fabric_api: "P7dR8mSH"
"#
}

/// Test: empack sync executes the planned add/remove actions hermetically.
#[tokio::test]
async fn test_sync_workflow_full() -> Result<()> {
    let (session, test_env) = HermeticSessionBuilder::new()?
        .with_mock_http_client()
        .with_empack_project("sync-pack", "1.21.1", "fabric")?
        .with_mock_executable(
            "packwiz",
            MockBehavior::Conditional {
                rules: vec![ConditionalRule {
                    args_pattern: vec!["list".to_string()],
                    behavior: MockBehavior::SucceedWithOutput {
                        stdout: "sodium.pw.toml\nold-mod.pw.toml".to_string(),
                        stderr: String::new(),
                    },
                }],
            },
        )?
        .build()?;

    let terminal_caps = TerminalCapabilities::detect_from_config(session.config().app_config())?;
    Display::init(terminal_caps)?;

    let workdir = session
        .config()
        .app_config()
        .workdir
        .clone()
        .expect("hermetic project should configure a workdir");
    std::env::set_current_dir(&workdir)?;
    fs::write(workdir.join("empack.yml"), sync_project_config())?;

    let sync_result = execute_command_with_session(Commands::Sync {}, &session).await;
    assert!(sync_result.is_ok(), "sync command failed: {sync_result:?}");

    let packwiz_calls = test_env.get_mock_calls("packwiz")?;
    assert!(
        packwiz_calls
            .iter()
            .any(|call| call.ends_with("packwiz list")),
        "sync should inspect installed mods via packwiz list: {packwiz_calls:?}"
    );
    assert!(
        packwiz_calls
            .iter()
            .any(|call| call.contains("packwiz mr add P7dR8mSH")),
        "sync should add the missing dependency by project id: {packwiz_calls:?}"
    );
    assert!(
        packwiz_calls
            .iter()
            .any(|call| call.contains("packwiz remove old_mod")),
        "sync should remove mods not declared in empack.yml: {packwiz_calls:?}"
    );
    assert!(
        !packwiz_calls
            .iter()
            .any(|call| call.contains("packwiz mr add AANobbMI")),
        "sync should not re-add dependencies that are already installed: {packwiz_calls:?}"
    );

    Ok(())
}

/// Test: empack sync --dry-run plans actions without mutating packwiz state.
#[tokio::test]
async fn test_sync_dry_run_no_modifications() -> Result<()> {
    let (session, test_env) = HermeticSessionBuilder::new()?
        .with_dry_run_flag()
        .with_mock_http_client()
        .with_empack_project("sync-pack-dry-run", "1.21.1", "fabric")?
        .with_mock_executable(
            "packwiz",
            MockBehavior::Conditional {
                rules: vec![ConditionalRule {
                    args_pattern: vec!["list".to_string()],
                    behavior: MockBehavior::SucceedWithOutput {
                        stdout: "old-mod.pw.toml".to_string(),
                        stderr: String::new(),
                    },
                }],
            },
        )?
        .build()?;

    let terminal_caps = TerminalCapabilities::detect_from_config(session.config().app_config())?;
    Display::init(terminal_caps)?;

    let workdir = session
        .config()
        .app_config()
        .workdir
        .clone()
        .expect("hermetic project should configure a workdir");
    std::env::set_current_dir(&workdir)?;
    fs::write(workdir.join("empack.yml"), sync_project_config())?;

    let sync_result = execute_command_with_session(Commands::Sync {}, &session).await;
    assert!(
        sync_result.is_ok(),
        "dry-run sync command failed: {sync_result:?}"
    );

    let packwiz_calls = test_env.get_mock_calls("packwiz")?;
    assert!(
        packwiz_calls
            .iter()
            .any(|call| call.ends_with("packwiz list")),
        "dry-run sync should still inspect installed mods: {packwiz_calls:?}"
    );
    assert!(
        !packwiz_calls.iter().any(|call| call.contains(" mr add ")),
        "dry-run sync must not add dependencies: {packwiz_calls:?}"
    );
    assert!(
        !packwiz_calls.iter().any(|call| call.contains(" remove ")),
        "dry-run sync must not remove dependencies: {packwiz_calls:?}"
    );

    Ok(())
}
