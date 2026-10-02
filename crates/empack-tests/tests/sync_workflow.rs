use anyhow::Result;
use empack_lib::application::cli::Commands;
use empack_lib::application::commands::execute_command_with_session;
use empack_lib::application::session_mocks::mock_root;
use empack_lib::display::Display;
use empack_lib::empack::search::ProjectInfo;
use empack_lib::primitives::ProjectPlatform;
use empack_lib::terminal::TerminalCapabilities;
use empack_tests::MockSessionBuilder;
use std::collections::HashSet;

fn search_project_config() -> &'static str {
    r#"# Preserve this comment during a preview.
empack:
  minecraft_version: "1.21.1"
  loader: fabric
  dependencies:
    sodium:
      title: Sodium
      type: mod
      platform: modrinth
"#
}

fn sodium_result() -> ProjectInfo {
    ProjectInfo {
        platform: ProjectPlatform::Modrinth,
        project_id: "AANobbMI".to_string(),
        title: "Sodium".to_string(),
        downloads: 100,
        confidence: 100,
        project_type: "mod".to_string(),
    }
}

#[tokio::test]
async fn test_sync_search_dry_run_preserves_manifest() -> Result<()> {
    let workdir = mock_root().join("workdir");
    let session = MockSessionBuilder::new()
        .with_empack_project("search-preview", "1.21.1", "fabric")
        .with_mock_http_client()
        .with_dry_run_flag()
        .with_mock_search_result("Sodium", sodium_result())
        .with_file(
            workdir.join("empack.yml"),
            search_project_config().to_string(),
        )
        .build();

    execute_command_with_session(Commands::Sync {}, &session).await?;

    assert_eq!(
        session
            .filesystem()
            .read_to_string(&workdir.join("empack.yml"))?,
        search_project_config(),
        "dry-run search resolution must preserve the manifest byte for byte"
    );
    assert!(session.process_provider.get_calls().is_empty());
    Ok(())
}

#[tokio::test]
async fn test_sync_unresolved_search_fails_and_preserves_installed_dependency() -> Result<()> {
    let workdir = mock_root().join("workdir");
    for dry_run in [false, true] {
        let mut session = MockSessionBuilder::new()
            .with_empack_project("unresolved-search", "1.21.1", "fabric")
            .with_mock_http_client()
            .with_file(
                workdir.join("empack.yml"),
                search_project_config().to_string(),
            )
            .with_installed_mods(HashSet::from(["sodium".to_string()]))
            .build();
        session.config_provider.app_config.dry_run = dry_run;

        let result = execute_command_with_session(Commands::Sync {}, &session).await;

        assert!(
            result.is_err(),
            "unresolved search must fail; dry_run={dry_run}"
        );
        assert!(session.process_provider.get_calls().is_empty());
        assert_eq!(
            session
                .filesystem()
                .read_to_string(&workdir.join("empack.yml"))?,
            search_project_config()
        );
    }
    Ok(())
}

#[tokio::test]
async fn test_sync_resolution_write_failure_preserves_manifest_and_installed_dependency() -> Result<()> {
    let workdir = mock_root().join("workdir");
    for dry_run in [false, true] {
        let mut session = MockSessionBuilder::new()
            .with_empack_project("search-write-failure", "1.21.1", "fabric")
            .with_mock_http_client()
            .with_mock_search_result("Sodium", sodium_result())
            .with_installed_mods(HashSet::from(["sodium".to_string()]))
            .with_file(workdir.join("empack.yml"), search_project_config().to_string())
            .build();
        session.config_provider.app_config.dry_run = dry_run;
        session.filesystem_provider.add_write_failure(workdir.join("empack.yml"), "manifest is read-only");

        let result = execute_command_with_session(Commands::Sync {}, &session).await;
        if dry_run {
            result?;
        } else {
            let error = result.expect_err("failed resolution write must fail sync");
            assert!(format!("{error:#}").contains("manifest is read-only"));
        }
        assert!(session.process_provider.get_calls().is_empty());
        assert_eq!(session.filesystem().read_to_string(&workdir.join("empack.yml"))?, search_project_config());
    }
    Ok(())
}

#[tokio::test]
async fn test_sync_search_applies_resolution_and_adds_dependency() -> Result<()> {
    let workdir = mock_root().join("workdir");
    let session = MockSessionBuilder::new()
        .with_empack_project("search-apply", "1.21.1", "fabric")
        .with_mock_http_client()
        .with_mock_search_result("Sodium", sodium_result())
        .with_file(
            workdir.join("empack.yml"),
            search_project_config().to_string(),
        )
        .build();

    execute_command_with_session(Commands::Sync {}, &session).await?;

    let config = session
        .filesystem()
        .config_manager(workdir.clone())
        .load_empack_config()?;
    assert!(matches!(
        &config.empack.dependencies["sodium"],
        empack_lib::empack::config::DependencyEntry::Resolved(record)
            if record.project_id == "AANobbMI"
    ));
    assert!(session.process_provider.verify_call(
        empack_lib::empack::packwiz::PACKWIZ_BIN,
        &["modrinth", "add", "--project-id", "AANobbMI", "-y"],
        &workdir.join("pack")
    ));
    Ok(())
}

#[tokio::test]
async fn test_sync_partial_resolution_reports_failure_with_valid_actions() -> Result<()> {
    let workdir = mock_root().join("workdir");
    for dry_run in [false, true] {
        for unresolved in [
            "    missing:\n      title: Missing\n      type: mod\n",
            "    missing:\n      status: resolved\n      title: Missing\n      type: mod\n      platform: modrinth\n      project_id: \"\"\n",
        ] {
            let manifest = format!("{}{unresolved}", search_project_config());
            let mut session = MockSessionBuilder::new()
                .with_empack_project("partial-resolution", "1.21.1", "fabric")
                .with_mock_http_client()
                .with_mock_search_result("Sodium", sodium_result())
                .with_file(workdir.join("empack.yml"), manifest.clone())
                .build();
            session.config_provider.app_config.dry_run = dry_run;

            let result = execute_command_with_session(Commands::Sync {}, &session).await;

            assert!(
                result.is_err(),
                "partial resolution must fail; dry_run={dry_run}"
            );
            let calls = session.process_provider.get_calls();
            if dry_run {
                assert!(calls.is_empty());
                assert_eq!(
                    session
                        .filesystem()
                        .read_to_string(&workdir.join("empack.yml"))?,
                    manifest
                );
            } else {
                assert_eq!(calls.len(), 1, "only Sodium should be added: {calls:?}");
                assert!(session.process_provider.verify_call(
                    empack_lib::empack::packwiz::PACKWIZ_BIN,
                    &["modrinth", "add", "--project-id", "AANobbMI", "-y"],
                    &workdir.join("pack")
                ));
            }
        }
    }
    Ok(())
}

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
