//! E2E test for complete lifecycle with Forge modloader
//!
//! Tests the full workflow: init -> add -> build -> clean with Forge modloader

use anyhow::Result;
use empack_lib::application::cli::{CliArchiveFormat, Commands};
use empack_lib::application::commands::execute_command_with_session;
use empack_lib::application::session_mocks::mock_root;
use empack_lib::display::Display;
use empack_lib::empack::search::ProjectInfo;
use empack_lib::primitives::ProjectPlatform;
use empack_lib::terminal::TerminalCapabilities;
use empack_tests::MockSessionBuilder;

/// Test: Complete lifecycle with Forge modloader
///
/// Workflow:
/// 1. Run `empack init -y --loader forge` in mock directory
/// 2. Add mods: sodium (Modrinth), jei (CurseForge)
/// 3. Build all targets
/// 4. Clean builds
/// 5. Verify all operations succeeded
#[tokio::test]
async fn test_lifecycle_forge_full() -> Result<()> {
    let workdir = mock_root().join("workdir");

    // Build session with search results for add step.
    let mut session = MockSessionBuilder::new()
        .with_yes_flag()
        .with_mock_http_client()
        .with_mock_search_result(
            "sodium",
            ProjectInfo {
                platform: ProjectPlatform::Modrinth,
                project_id: "AANobbMI".to_string(),
                title: "Sodium".to_string(),
                downloads: 1_000_000,
                confidence: 95,
                project_type: "mod".to_string(),
            },
        )
        .with_mock_search_result(
            "jei",
            ProjectInfo {
                platform: ProjectPlatform::CurseForge,
                project_id: "238222".to_string(),
                title: "Just Enough Items".to_string(),
                downloads: 500_000,
                confidence: 90,
                project_type: "mod".to_string(),
            },
        )
        .with_packwiz_add_slug("AANobbMI".to_string(), "AANobbMI".to_string())
        .with_packwiz_add_slug("238222".to_string(), "238222".to_string())
        .build();

    Display::init_or_get(TerminalCapabilities::minimal());

    // Step 1: Initialize with Forge modloader
    let init_result = execute_command_with_session(
        Commands::Init {
            name: Some("forge-test-pack".to_string()),
            pack_name: Some("forge-test-pack".to_string()),
            force: false,
            modloader: Some("forge".to_string()),
            mc_version: Some("1.21.1".to_string()),
            author: Some("Workflow Test".to_string()),
            loader_version: None,
            pack_version: None,
        },
        &session,
    )
    .await;
    assert!(
        init_result.is_ok(),
        "Init command failed: {:?}",
        init_result
    );

    // Init creates a subdirectory with the project name
    let project_dir = workdir.join("forge-test-pack");

    // Verify empack.yml was created in project directory
    let empack_yml_path = project_dir.join("empack.yml");
    assert!(
        session.filesystem().exists(&empack_yml_path),
        "empack.yml should be created in project directory"
    );

    // Verify pack/ directory was created in project directory
    let pack_dir = project_dir.join("pack");
    assert!(
        session.filesystem().exists(&pack_dir),
        "pack/ directory should be created"
    );

    // Verify pack.toml exists
    let pack_toml_path = pack_dir.join("pack.toml");
    assert!(
        session.filesystem().exists(&pack_toml_path),
        "pack.toml should exist after packwiz init"
    );
    let empack_yml = session.filesystem().read_to_string(&empack_yml_path)?;
    assert!(
        empack_yml.contains("loader: forge"),
        "empack.yml should record the Forge loader, got: {empack_yml}"
    );

    // Pivot the session's working directory into the initialized project so
    // subsequent commands (add, build, clean) resolve empack.yml correctly.
    session.filesystem_provider.current_dir = project_dir.clone();
    session.config_provider.app_config.workdir = Some(project_dir.clone());
    session.packwiz_provider.current_dir = project_dir.clone();

    // Add server templates for the build step
    let templates_dir = project_dir.join("templates").join("server");
    session.filesystem().create_dir_all(&templates_dir)?;
    session.filesystem().write_file(
        &templates_dir.join("server.properties.template"),
        "motd={{NAME}}\nmax-players=10\n",
    )?;

    // Pre-populate JAR cache so full builds skip real HTTP downloads.
    // MockPackwizOps resolves cache relative to its current_dir.
    let cache_dir = project_dir.join("cache");
    session.filesystem().create_dir_all(&cache_dir)?;
    session.filesystem().write_file(
        &cache_dir.join("packwiz-installer-bootstrap.jar"),
        "mock-bootstrap-jar",
    )?;
    session.filesystem().write_file(
        &cache_dir.join("packwiz-installer.jar"),
        "mock-installer-jar",
    )?;

    // Deferred server JAR stubs: injected when the build creates dist directories.
    let dist = project_dir.join("dist");
    {
        let mut deferred = session.filesystem_provider.deferred_files.lock().unwrap();
        deferred
            .entry(dist.join("server"))
            .or_default()
            .push(("srv.jar".to_string(), "mock-server-jar".to_string()));
        deferred
            .entry(dist.join("server-full"))
            .or_default()
            .push(("srv.jar".to_string(), "mock-server-jar".to_string()));
    }

    // Step 2: Add mods (sodium via Modrinth, jei via CurseForge)
    let add_sodium_result = execute_command_with_session(
        Commands::Add {
            mods: vec!["sodium".to_string()],
            force: false,
            platform: None,
            project_type: None,
        },
        &session,
    )
    .await;

    assert!(
        add_sodium_result.is_ok(),
        "Add sodium should succeed: {add_sodium_result:?}"
    );
    assert!(
        session
            .filesystem()
            .exists(&pack_dir.join("mods").join("AANobbMI.pw.toml")),
        "Modrinth add should leave deterministic mod metadata"
    );

    let add_jei_result = execute_command_with_session(
        Commands::Add {
            mods: vec!["jei".to_string()],
            force: false,
            platform: None,
            project_type: None,
        },
        &session,
    )
    .await;

    assert!(
        add_jei_result.is_ok(),
        "Add JEI should succeed: {add_jei_result:?}"
    );
    assert!(
        session
            .filesystem()
            .exists(&pack_dir.join("mods").join("238222.pw.toml")),
        "CurseForge add should leave deterministic mod metadata"
    );

    // Step 3: Build all targets
    let build_result = execute_command_with_session(
        Commands::Build {
            targets: vec!["all".to_string()],
            clean: false,
            format: CliArchiveFormat::Zip,
        },
        &session,
    )
    .await;

    assert!(
        build_result.is_ok(),
        "Build all should succeed: {build_result:?}"
    );

    // Verify mrpack artifact in mock filesystem (created by packwiz mr export side effect)
    let mrpack_path = dist.join("forge-test-pack-v1.0.0.mrpack");
    assert!(
        session.filesystem().exists(&mrpack_path),
        "Lifecycle build should create the mrpack artifact"
    );

    // Verify zip archives via MockArchiveProvider create_calls spy
    {
        let create_calls = session.archive_provider.create_calls.lock().unwrap();
        for suffix in [
            "client.zip",
            "server.zip",
            "client-full.zip",
            "server-full.zip",
        ] {
            assert!(
                create_calls
                    .iter()
                    .any(|(_, dest)| dest.to_string_lossy().contains(suffix)),
                "Lifecycle build should create archive with suffix '{}': {create_calls:?}",
                suffix
            );
        }
    }

    // Verify server-full and client-full install markers from java installer side effects
    assert!(
        session.filesystem().exists(
            &dist
                .join("server-full")
                .join("mods")
                .join("server-installed.txt")
        ),
        "Server-full build should include full install marker"
    );
    assert!(
        session.filesystem().exists(
            &dist
                .join("client-full")
                .join("mods")
                .join("both-installed.txt")
        ),
        "Client-full build should include full install marker"
    );

    // Step 4: Clean builds
    let clean_result = execute_command_with_session(
        Commands::Clean {
            targets: vec!["all".to_string()],
        },
        &session,
    )
    .await;

    assert!(
        clean_result.is_ok(),
        "Clean command failed: {:?}",
        clean_result
    );
    assert!(
        !session.filesystem().exists(&dist),
        "Clean should remove the build output directory after a successful lifecycle"
    );

    // Verify packwiz was called during workflow via process provider spy
    let packwiz_calls = session.process_provider.get_calls_for_command("packwiz");
    assert!(
        packwiz_calls.iter().any(|call| {
            call.args.iter().any(|a| a == "modrinth")
                && call.args.iter().any(|a| a == "add")
                && call.args.iter().any(|a| a == "--project-id")
                && call.args.iter().any(|a| a == "AANobbMI")
        }),
        "Lifecycle should add the Modrinth dependency through packwiz: {packwiz_calls:?}"
    );
    assert!(
        packwiz_calls.iter().any(|call| {
            call.args.iter().any(|a| a == "curseforge")
                && call.args.iter().any(|a| a == "add")
                && call.args.iter().any(|a| a == "--addon-id")
                && call.args.iter().any(|a| a == "238222")
        }),
        "Lifecycle should add the CurseForge dependency through packwiz: {packwiz_calls:?}"
    );
    assert!(
        packwiz_calls
            .iter()
            .any(|call| call.args.iter().any(|a| a == "mr")
                && call.args.iter().any(|a| a == "export")),
        "Lifecycle should export the mrpack during full builds: {packwiz_calls:?}"
    );

    Ok(())
}

/// Test: Forge modloader initialization validates loader selection
///
/// This test specifically validates that Forge modloader can be initialized
/// and that the modloader choice propagates through the configuration.
#[tokio::test]
async fn test_forge_modloader_initialization() -> Result<()> {
    let session = MockSessionBuilder::new().with_yes_flag().build();

    Display::init_or_get(TerminalCapabilities::minimal());

    let workdir = session.filesystem().current_dir()?;

    // Execute init with explicit Forge loader (--yes requires --modloader)
    let result = execute_command_with_session(
        Commands::Init {
            name: Some("forge-loader-test".to_string()),
            pack_name: None,
            force: false,
            modloader: Some("forge".to_string()),
            mc_version: Some("1.21.1".to_string()),
            author: None,
            loader_version: None,
            pack_version: None,
        },
        &session,
    )
    .await;

    assert!(
        result.is_ok(),
        "Init with Forge loader failed: {:?}",
        result
    );

    // Init creates a subdirectory with the project name
    let project_dir = workdir.join("forge-loader-test");

    // Verify empack.yml exists in project directory
    let empack_yml_path = project_dir.join("empack.yml");
    assert!(
        session.filesystem().exists(&empack_yml_path),
        "empack.yml should exist"
    );

    let empack_yml_content = session.filesystem().read_to_string(&empack_yml_path)?;
    assert!(
        empack_yml_content.contains("loader: forge"),
        "empack.yml should contain 'loader: forge', got: {}",
        empack_yml_content
    );

    Ok(())
}
