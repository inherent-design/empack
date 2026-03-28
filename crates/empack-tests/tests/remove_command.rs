use anyhow::Result;
use empack_lib::application::cli::Commands;
use empack_lib::application::commands::execute_command_with_session;
use empack_lib::application::session_mocks::mock_root;
use empack_lib::display::Display;
use empack_lib::terminal::TerminalCapabilities;
use empack_tests::MockSessionBuilder;

fn remove_project_config() -> &'static str {
    r#"empack:
  name: "Remove Pack"
  author: "Test Author"
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
async fn e2e_remove_single_mod() -> Result<()> {
    let workdir = mock_root().join("workdir");
    let session = MockSessionBuilder::new()
        .with_empack_project("remove-single", "1.21.1", "fabric")
        .with_yes_flag()
        .with_file(workdir.join("empack.yml"), remove_project_config().to_string())
        .build();

    Display::init_or_get(TerminalCapabilities::minimal());

    let result = execute_command_with_session(
        Commands::Remove {
            mods: vec!["sodium".to_string()],
            deps: false,
        },
        &session,
    )
    .await;

    assert!(result.is_ok(), "remove command failed: {result:?}");

    let packwiz_calls = session.process_provider.get_calls_for_command("packwiz");
    assert!(
        packwiz_calls
            .iter()
            .any(|call| call.args.iter().map(String::as_str).collect::<Vec<_>>()
                == ["remove", "-y", "sodium"]),
        "remove should invoke packwiz remove -y sodium: {packwiz_calls:?}"
    );

    let config_content = session.filesystem().read_to_string(&workdir.join("empack.yml"))?;
    assert!(
        !config_content.contains("sodium"),
        "sodium should be removed from empack.yml after remove command"
    );
    assert!(
        config_content.contains("fabric_api"),
        "fabric_api should remain in empack.yml"
    );

    Ok(())
}

#[tokio::test]
async fn e2e_remove_multiple_mods() -> Result<()> {
    let workdir = mock_root().join("workdir");
    let session = MockSessionBuilder::new()
        .with_empack_project("remove-multi", "1.21.1", "fabric")
        .with_yes_flag()
        .with_file(workdir.join("empack.yml"), remove_project_config().to_string())
        .build();

    Display::init_or_get(TerminalCapabilities::minimal());

    let result = execute_command_with_session(
        Commands::Remove {
            mods: vec!["sodium".to_string(), "fabric_api".to_string()],
            deps: false,
        },
        &session,
    )
    .await;

    assert!(result.is_ok(), "remove command failed: {result:?}");

    let packwiz_calls = session.process_provider.get_calls_for_command("packwiz");
    assert!(
        packwiz_calls
            .iter()
            .any(|call| call.args.iter().map(String::as_str).collect::<Vec<_>>()
                == ["remove", "-y", "sodium"]),
        "should invoke packwiz remove for sodium: {packwiz_calls:?}"
    );
    assert!(
        packwiz_calls
            .iter()
            .any(|call| call.args.iter().map(String::as_str).collect::<Vec<_>>()
                == ["remove", "-y", "fabric_api"]),
        "should invoke packwiz remove for fabric_api: {packwiz_calls:?}"
    );

    let config_content = session.filesystem().read_to_string(&workdir.join("empack.yml"))?;
    assert!(
        !config_content.contains("sodium"),
        "sodium should be removed from empack.yml"
    );
    assert!(
        !config_content.contains("fabric_api"),
        "fabric_api should be removed from empack.yml"
    );

    Ok(())
}

#[tokio::test]
async fn e2e_remove_empty_mods_is_noop() -> Result<()> {
    let workdir = mock_root().join("workdir");
    let session = MockSessionBuilder::new()
        .with_empack_project("remove-empty", "1.21.1", "fabric")
        .with_yes_flag()
        .with_file(workdir.join("empack.yml"), remove_project_config().to_string())
        .build();

    Display::init_or_get(TerminalCapabilities::minimal());

    let result = execute_command_with_session(
        Commands::Remove {
            mods: vec![],
            deps: false,
        },
        &session,
    )
    .await;

    assert!(
        result.is_ok(),
        "remove with empty mods should not fail: {result:?}"
    );

    let packwiz_calls = session.process_provider.get_calls_for_command("packwiz");
    assert!(
        packwiz_calls.is_empty(),
        "remove with empty mods should not call packwiz: {packwiz_calls:?}"
    );

    let config_content = session.filesystem().read_to_string(&workdir.join("empack.yml"))?;
    assert!(
        config_content.contains("sodium"),
        "empack.yml should be unchanged after empty remove"
    );

    Ok(())
}
