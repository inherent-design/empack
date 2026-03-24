//! E2E test for complete lifecycle with Forge modloader
//!
//! Tests the full workflow: init → add → build → clean with Forge modloader

use anyhow::Result;
use empack_lib::application::cli::Commands;
use empack_lib::application::commands::execute_command_with_session;
use empack_lib::display::Display;
use empack_lib::empack::search::ProjectInfo;
use empack_lib::primitives::ProjectPlatform;
use empack_lib::terminal::TerminalCapabilities;
use empack_tests::fixtures::{WorkflowArtifact, WorkflowProjectFixture};
use empack_tests::{HermeticSessionBuilder, MockBehavior};
use std::fs;

/// Test: Complete lifecycle with Forge modloader
///
/// Workflow:
/// 1. Run `empack init -y --loader forge` in temp directory
/// 2. Add mods: sodium (Modrinth), jei (CurseForge)
/// 3. Build all targets
/// 4. Clean builds
/// 5. Verify all operations succeeded
#[cfg(unix)]
#[tokio::test]
async fn test_lifecycle_forge_full() -> Result<()> {
    let fixture = WorkflowProjectFixture::new("forge-test-pack");
    // Create hermetic session with mocked toolchain and deterministic add results
    let (session, test_env) = HermeticSessionBuilder::new()?
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
        .with_mock_executable(
            "packwiz",
            MockBehavior::SucceedWithOutput {
                stdout: "Initialized packwiz project\nRefreshed packwiz index\nExported to forge-test-pack-v1.0.0.mrpack".to_string(),
                stderr: String::new(),
            },
        )?
        .with_mock_executable(
            "git",
            MockBehavior::SucceedWithOutput {
                stdout: "main".to_string(),
                stderr: String::new(),
            },
        )?
        .with_mock_executable(
            "java",
            MockBehavior::SucceedWithOutput {
                stdout: "Installed full client and server mods".to_string(),
                stderr: String::new(),
            },
        )?
        .with_mock_executable(
            "curl",
            MockBehavior::SucceedWithOutput {
                stdout: String::new(),
                stderr: String::new(),
            },
        )?
        .with_mock_executable(
            "zip",
            MockBehavior::SucceedWithOutput {
                stdout: "Created distribution archive".to_string(),
                stderr: String::new(),
            },
        )?
        .with_mock_executable(
            "unzip",
            MockBehavior::SucceedWithOutput {
                stdout: "Archive extracted".to_string(),
                stderr: String::new(),
            },
        )?
        .build()?;

    // Initialize display
    let terminal_caps = TerminalCapabilities::detect_from_config(session.config().app_config())?;
    Display::init_or_get(terminal_caps);

    // Use test work directory as working directory
    let workdir = test_env.work_path.clone();
    std::env::set_current_dir(&workdir)?;

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
        empack_yml_path.exists(),
        "empack.yml should be created in project directory"
    );

    // Verify pack/ directory was created in project directory
    let pack_dir = project_dir.join("pack");
    assert!(pack_dir.exists(), "pack/ directory should be created");

    // Verify pack.toml exists
    let pack_toml_path = pack_dir.join("pack.toml");
    assert!(
        pack_toml_path.exists(),
        "pack.toml should exist after packwiz init"
    );
    let empack_yml = fs::read_to_string(&empack_yml_path)?;
    assert!(
        empack_yml.contains("loader: forge"),
        "empack.yml should record the Forge loader, got: {empack_yml}"
    );

    // Run the remaining lifecycle steps from inside the initialized project.
    std::env::set_current_dir(&project_dir)?;

    let templates_dir = project_dir.join("templates").join("server");
    std::fs::create_dir_all(&templates_dir)?;
    std::fs::write(
        templates_dir.join("server.properties.template"),
        "motd={{NAME}}\nmax-players=10\n",
    )?;

    let jar_cache = empack_lib::platform::cache::cache_root()?.join("jars");
    std::fs::create_dir_all(&jar_cache)?;
    std::fs::write(
        jar_cache.join("packwiz-installer-bootstrap.jar"),
        "mock-installer-jar",
    )?;

    // Step 2: Add mods (sodium via Modrinth, jei via CurseForge)
    // Note: This validates multi-platform mod addition in Forge context

    // Add sodium (Modrinth platform, Forge-compatible versions exist)
    let add_sodium_result = execute_command_with_session(
        Commands::Add {
            mods: vec!["sodium".to_string()],
            force: false,
            platform: None, // Should auto-detect Modrinth
        },
        &session,
    )
    .await;

    assert!(
        add_sodium_result.is_ok(),
        "Add sodium should resolve and succeed in the hermetic lifecycle: {add_sodium_result:?}"
    );
    assert!(
        project_dir.join("pack").join("mods").join("AANobbMI.pw.toml").exists(),
        "Modrinth add should leave deterministic mod metadata"
    );

    // Add JEI (CurseForge platform, Forge-native mod)
    let add_jei_result = execute_command_with_session(
        Commands::Add {
            mods: vec!["jei".to_string()],
            force: false,
            platform: None, // Should auto-detect or fallback to CurseForge
        },
        &session,
    )
    .await;

    assert!(
        add_jei_result.is_ok(),
        "Add JEI should resolve and succeed in the hermetic lifecycle: {add_jei_result:?}"
    );
    assert!(
        project_dir.join("pack").join("mods").join("238222.pw.toml").exists(),
        "CurseForge add should leave deterministic mod metadata"
    );

    // Step 3: Build all targets
    let build_result = execute_command_with_session(
        Commands::Build {
            targets: vec!["all".to_string()],
            clean: false,
            jobs: None,
        },
        &session,
    )
    .await;

    assert!(
        build_result.is_ok(),
        "Build all should succeed with hermetic tool mocks: {build_result:?}"
    );

    let dist_dir = project_dir.join("dist");
    for artifact in [
        WorkflowArtifact::Mrpack,
        WorkflowArtifact::Client,
        WorkflowArtifact::Server,
        WorkflowArtifact::ClientFull,
        WorkflowArtifact::ServerFull,
    ] {
        let artifact_path = fixture.artifact_path(&project_dir, artifact);
        assert!(
            artifact_path.exists(),
            "Lifecycle build should materialize {}",
            artifact_path.display()
        );
    }
    assert!(
        dist_dir.join("server").join("config").join("generated.txt").exists(),
        "Server build should include extracted override content"
    );
    assert!(
        dist_dir
            .join("server-full").join("mods").join("server-installed.txt")
            .exists(),
        "Server-full build should include full install marker"
    );
    assert!(
        dist_dir
            .join("client-full").join("mods").join("both-installed.txt")
            .exists(),
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

    // Clean should succeed even if builds didn't complete
    assert!(
        clean_result.is_ok(),
        "Clean command failed: {:?}",
        clean_result
    );
    assert!(
        !dist_dir.exists(),
        "Clean should remove the build output directory after a successful lifecycle"
    );

    // Verify packwiz was called during workflow
    let packwiz_calls = test_env.get_mock_invocations("packwiz")?;
    assert!(
        packwiz_calls.iter().any(|call| call.contains_args(&[
            "modrinth",
            "add",
            "--project-id",
            "AANobbMI",
            "-y"
        ])),
        "Lifecycle should add the Modrinth dependency through packwiz: {packwiz_calls:?}"
    );
    assert!(
        packwiz_calls.iter().any(|call| call.contains_args(&[
            "curseforge",
            "add",
            "--addon-id",
            "238222",
            "-y"
        ])),
        "Lifecycle should add the CurseForge dependency through packwiz: {packwiz_calls:?}"
    );
    assert!(
        packwiz_calls
            .iter()
            .any(|call| call.contains_args(&["mr", "export"])),
        "Lifecycle should export the mrpack during full builds: {packwiz_calls:?}"
    );

    Ok(())
}

/// Test: Forge modloader initialization validates loader selection
///
/// This test specifically validates that Forge modloader can be initialized
/// and that the modloader choice propagates through the configuration.
#[cfg(unix)]
#[tokio::test]
async fn test_forge_modloader_initialization() -> Result<()> {
    use empack_lib::application::session_mocks::MockInteractiveProvider;

    // Create interactive provider that selects Forge loader
    let interactive = MockInteractiveProvider::new()
        .with_select(2) // Assume Forge is index 2 in loader list (Fabric, NeoForge, Forge, Quilt)
        .with_fuzzy_select(0); // Select first MC version

    let (session, test_env) = HermeticSessionBuilder::new()?
        .with_yes_flag()
        .with_mock_executable(
            "packwiz",
            MockBehavior::SucceedWithOutput {
                stdout: "Initialized packwiz project".to_string(),
                stderr: String::new(),
            },
        )?
        .with_mock_executable(
            "git",
            MockBehavior::SucceedWithOutput {
                stdout: "main".to_string(),
                stderr: String::new(),
            },
        )?
        .with_mock_executable(
            "which",
            MockBehavior::SucceedWithOutput {
                stdout: "/test/bin/packwiz".to_string(),
                stderr: String::new(),
            },
        )?
        .with_interactive_provider(interactive)
        .build()?;

    // Initialize display
    let terminal_caps = TerminalCapabilities::detect_from_config(session.config().app_config())?;
    Display::init_or_get(terminal_caps);

    let workdir = test_env.work_path.clone();
    std::env::set_current_dir(&workdir)?;

    // Execute init without explicit loader (interactive provider will select Forge)
    let result = execute_command_with_session(
        Commands::Init {
            name: Some("forge-loader-test".to_string()),
            pack_name: None,
            force: false,
            modloader: None,
            mc_version: None,
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
    assert!(empack_yml_path.exists(), "empack.yml should exist");

    let empack_yml_content = fs::read_to_string(&empack_yml_path)?;
    assert!(
        empack_yml_content.contains("loader: forge"),
        "empack.yml should contain 'loader: forge', got: {}",
        empack_yml_content
    );

    Ok(())
}
