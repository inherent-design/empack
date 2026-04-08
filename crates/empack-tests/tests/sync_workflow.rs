use anyhow::Result;
use empack_lib::application::cli::Commands;
use empack_lib::application::commands::execute_command_with_session;
use empack_lib::application::session_mocks::mock_root;
use empack_lib::display::Display;
use empack_lib::terminal::TerminalCapabilities;
use empack_tests::MockSessionBuilder;
use std::collections::HashSet;

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

#[tokio::test]
async fn test_sync_workflow_full() -> Result<()> {
    let workdir = mock_root().join("workdir");

    let session = MockSessionBuilder::new()
        .with_empack_project("sync-pack", "1.21.1", "fabric")
        .with_mock_http_client()
        .with_yes_flag()
        .with_file(
            workdir.join("empack.yml"),
            sync_project_config().to_string(),
        )
        .with_installed_mods(HashSet::from(["sodium".to_string(), "old-mod".to_string()]))
        .build();

    Display::init_or_get(TerminalCapabilities::minimal());

    let sync_result = execute_command_with_session(Commands::Sync {}, &session).await;
    assert!(sync_result.is_ok(), "sync command failed: {sync_result:?}");

    let packwiz_calls = session
        .process_provider
        .get_calls_for_command(empack_lib::empack::packwiz::PACKWIZ_BIN);
    assert!(
        packwiz_calls.iter().any(|call| {
            let args: Vec<&str> = call.args.iter().map(String::as_str).collect();
            args.windows(5)
                .any(|w| w == ["modrinth", "add", "--project-id", "P7dR8mSH", "-y"])
        }),
        "sync should add the missing dependency by project id: {packwiz_calls:?}"
    );
    assert!(
        packwiz_calls.iter().any(|call| {
            let args: Vec<&str> = call.args.iter().map(String::as_str).collect();
            args.windows(3).any(|w| w == ["remove", "-y", "old-mod"])
        }),
        "sync should remove mods not declared in empack.yml: {packwiz_calls:?}"
    );
    assert!(
        !packwiz_calls.iter().any(|call| {
            let args: Vec<&str> = call.args.iter().map(String::as_str).collect();
            args.windows(5)
                .any(|w| w == ["modrinth", "add", "--project-id", "AANobbMI", "-y"])
        }),
        "sync should not re-add dependencies that are already installed: {packwiz_calls:?}"
    );

    Ok(())
}

#[tokio::test]
async fn test_sync_dry_run_no_modifications() -> Result<()> {
    let workdir = mock_root().join("workdir");

    let session = MockSessionBuilder::new()
        .with_empack_project("sync-pack-dry-run", "1.21.1", "fabric")
        .with_mock_http_client()
        .with_dry_run_flag()
        .with_file(
            workdir.join("empack.yml"),
            sync_project_config().to_string(),
        )
        .with_installed_mods(HashSet::from(["old-mod".to_string()]))
        .build();

    Display::init_or_get(TerminalCapabilities::minimal());

    let sync_result = execute_command_with_session(Commands::Sync {}, &session).await;
    assert!(
        sync_result.is_ok(),
        "dry-run sync command failed: {sync_result:?}"
    );

    let packwiz_calls = session
        .process_provider
        .get_calls_for_command(empack_lib::empack::packwiz::PACKWIZ_BIN);
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

#[tokio::test]
async fn test_sync_normalized_installed_names_noop() -> Result<()> {
    let workdir = mock_root().join("workdir");

    let session = MockSessionBuilder::new()
        .with_empack_project("sync-pack-normalized", "1.21.1", "fabric")
        .with_mock_http_client()
        .with_yes_flag()
        .with_file(
            workdir.join("empack.yml"),
            sync_project_config().to_string(),
        )
        .with_installed_mods(HashSet::from([
            "sodium".to_string(),
            "fabric_api".to_string(),
        ]))
        .build();

    Display::init_or_get(TerminalCapabilities::minimal());

    let sync_result = execute_command_with_session(Commands::Sync {}, &session).await;
    assert!(
        sync_result.is_ok(),
        "slug-matching installed names should produce a no-op sync: {sync_result:?}"
    );

    let packwiz_calls = session
        .process_provider
        .get_calls_for_command(empack_lib::empack::packwiz::PACKWIZ_BIN);
    assert!(
        packwiz_calls.is_empty(),
        "all-installed sync should not call packwiz at all: {packwiz_calls:?}"
    );

    Ok(())
}
