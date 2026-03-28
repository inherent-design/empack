use anyhow::Result;
use empack_lib::application::cli::Commands;
use empack_lib::application::commands::execute_command_with_session;
use empack_lib::application::config::AppConfig;
use empack_lib::application::session::{
    CommandSession, LiveConfigProvider, LiveFileSystemProvider, LiveNetworkProvider,
};
use empack_lib::application::session_mocks::{
    mock_root, MockInteractiveProvider, MockProcessProvider,
};
use empack_lib::display::Display;
use empack_lib::terminal::TerminalCapabilities;
use empack_tests::fixtures::{WorkflowArtifact, WorkflowProjectFixture};
use empack_tests::MockSessionBuilder;
use tempfile::TempDir;

#[tokio::test]
async fn test_clean_dry_run() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let workdir = temp_dir.path().to_path_buf();
    let fixture = WorkflowProjectFixture::new("clean-dry-run-pack");
    let paths = fixture.write_to(&workdir)?;

    std::fs::create_dir_all(&paths.dist_dir)?;
    let mrpack_file = fixture.artifact_path(&workdir, WorkflowArtifact::Mrpack);
    std::fs::write(&mrpack_file, "mock mrpack content")?;
    let client_zip = fixture.artifact_path(&workdir, WorkflowArtifact::Client);
    std::fs::write(&client_zip, "mock client zip content")?;

    assert!(mrpack_file.exists(), "mrpack should exist before dry-run");
    assert!(
        client_zip.exists(),
        "client zip should exist before dry-run"
    );

    std::env::set_current_dir(&workdir)?;

    let mut app_config = AppConfig {
        workdir: Some(workdir.clone()),
        ..AppConfig::default()
    };
    app_config.dry_run = true;

    let terminal_caps = TerminalCapabilities::detect_from_config(&app_config)?;
    Display::init_or_get(terminal_caps);

    let session = CommandSession::new_with_providers(
        LiveFileSystemProvider,
        LiveNetworkProvider::new(),
        MockProcessProvider::new(),
        LiveConfigProvider::new(app_config),
        MockInteractiveProvider::new(),
    );

    let result = execute_command_with_session(
        Commands::Clean {
            targets: vec!["builds".to_string()],
        },
        &session,
    )
    .await;

    assert!(
        result.is_ok(),
        "Clean --dry-run should succeed: {:?}",
        result
    );

    assert!(
        mrpack_file.exists(),
        "dry-run should not delete mrpack file"
    );
    assert!(client_zip.exists(), "dry-run should not delete client zip");
    assert!(
        paths.dist_dir.exists(),
        "dry-run should not remove dist/ directory"
    );
    assert!(
        paths.empack_yml.exists(),
        "dry-run should preserve empack.yml"
    );
    assert!(
        paths.pack_toml.exists(),
        "dry-run should preserve pack.toml"
    );

    Ok(())
}

#[tokio::test]
async fn test_remove_dry_run() -> Result<()> {
    let workdir = mock_root().join("workdir");
    let custom_config = r#"empack:
  name: "Remove Dry Run Pack"
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
"#;

    let session = MockSessionBuilder::new()
        .with_empack_project("remove-dry-run", "1.21.1", "fabric")
        .with_dry_run_flag()
        .with_file(workdir.join("empack.yml"), custom_config.to_string())
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

    assert!(
        result.is_ok(),
        "Remove --dry-run should succeed: {:?}",
        result
    );

    let packwiz_calls = session.process_provider.get_calls_for_command("packwiz");
    let remove_calls: Vec<_> = packwiz_calls
        .iter()
        .filter(|call| call.args.contains(&"remove".to_string()))
        .collect();
    assert!(
        remove_calls.is_empty(),
        "dry-run should not invoke packwiz remove: {:?}",
        remove_calls
    );

    let config_content = session
        .filesystem()
        .read_to_string(&workdir.join("empack.yml"))?;
    assert!(
        config_content.contains("sodium"),
        "dry-run should not modify empack.yml; sodium should remain"
    );
    assert!(
        config_content.contains("fabric_api"),
        "dry-run should not modify empack.yml; fabric_api should remain"
    );

    Ok(())
}
