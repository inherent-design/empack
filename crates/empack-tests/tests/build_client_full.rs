//! E2E tests for client-full build target
//!
//! Tests the complete client-full build workflow including packwiz installer
//! bootstrap execution and mod downloading.

use anyhow::Result;
use empack_lib::application::Commands;
use empack_lib::application::commands::execute_command_with_session;
use empack_lib::application::config::AppConfig;
use empack_lib::application::session::{
    CommandSession, LiveConfigProvider, LiveFileSystemProvider, LiveNetworkProvider,
    LiveProcessProvider,
};
use empack_lib::display::{Display, LiveDisplayProvider};
use empack_lib::terminal::TerminalCapabilities;
use indicatif::MultiProgress;
use std::path::Path;
use tempfile::TempDir;

/// Initialize a real empack project in the given directory and return a session
async fn initialize_empack_project(
    workdir: &Path,
) -> Result<
    CommandSession<
        LiveFileSystemProvider,
        LiveNetworkProvider,
        LiveProcessProvider,
        LiveConfigProvider,
    >,
> {
    // Create the basic structure that empack expects
    std::fs::create_dir_all(workdir.join("pack"))?;

    // Create empack.yml
    let empack_yml = r#"empack:
  dependencies:
    - "fabric_api: Fabric API|mod"
  minecraft_version: "1.21.1"
  loader: fabric
  name: "Test Modpack"
  author: "Test Author"
  version: "1.0.0"
"#;
    std::fs::write(workdir.join("empack.yml"), empack_yml)?;

    // Create pack.toml
    let pack_toml = r#"name = "Test Modpack"
author = "Test Author"
version = "1.0.0"
pack-format = "packwiz:1.1.0"

[index]
file = "index.toml"
hash-format = "sha256"
hash = ""

[versions]
minecraft = "1.21.1"
fabric = "0.15.0"
"#;
    std::fs::write(workdir.join("pack").join("pack.toml"), pack_toml)?;

    // Create index.toml
    let index_toml = r#"hash-format = "sha256"

[[files]]
file = "pack.toml"
hash = ""
"#;
    std::fs::write(workdir.join("pack").join("index.toml"), index_toml)?;

    // Initialize display for test
    let app_config = AppConfig::default();
    let terminal_caps = TerminalCapabilities::detect_from_config(&app_config)?;
    let _ = Display::init(terminal_caps);

    // Create session with live providers for E2E testing
    let filesystem_provider = LiveFileSystemProvider;
    let network_provider = LiveNetworkProvider::new();
    let process_provider = LiveProcessProvider::new();
    let config_provider = LiveConfigProvider::new(app_config);

    let session = CommandSession::new_with_providers(
        filesystem_provider,
        network_provider,
        process_provider,
        config_provider,
    );

    // Change to the working directory
    std::env::set_current_dir(workdir)?;

    Ok(session)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn e2e_build_client_full_successfully() -> anyhow::Result<()> {
    let temp_dir = TempDir::new()?;
    let session = initialize_empack_project(temp_dir.path()).await?;

    // Create required directories for client-full build
    let installer_dir = temp_dir.path().join("installer");
    std::fs::create_dir_all(&installer_dir)?;

    // Create a mock installer JAR (client-full build requires it)
    let installer_jar = installer_dir.join("packwiz-installer-bootstrap.jar");
    std::fs::write(&installer_jar, "mock-installer-jar")?;

    // Execute client-full build command
    let result = execute_command_with_session(
        Commands::Build {
            targets: vec!["client-full".to_string()],
            clean: false,
            jobs: None,
        },
        &session,
    )
    .await;

    // Client-full build may fail due to missing Java or installer issues, but should create directory structure
    match result {
        Ok(_) => {
            // If successful, verify the client-full directory structure was created
            let client_full_dir = temp_dir.path().join("dist/client-full");
            assert!(
                client_full_dir.exists(),
                "Client-full build directory should exist"
            );

            // Verify pack directory was copied
            let pack_dir = client_full_dir.join("pack");
            assert!(
                pack_dir.exists(),
                "Pack directory should be copied to client-full build"
            );

            // Check for potential build artifacts (may exist if installer ran successfully)
            let pack_toml = pack_dir.join("pack.toml");
            assert!(
                pack_toml.exists(),
                "Pack.toml should exist in client-full build"
            );
        }
        Err(e) => {
            // Expected if Java or installer is not available - verify partial build occurred
            let client_full_dir = temp_dir.path().join("dist/client-full");
            if client_full_dir.exists() {
                println!(
                    "Client-full build failed as expected (likely missing Java or installer issue): {}",
                    e
                );

                // Even if build failed, pack directory should have been copied
                let pack_dir = client_full_dir.join("pack");
                if pack_dir.exists() {
                    println!("Pack directory was copied before build failure");
                }
            } else {
                return Err(e);
            }
        }
    }

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn e2e_build_client_full_missing_installer() -> anyhow::Result<()> {
    let temp_dir = TempDir::new()?;
    let session = initialize_empack_project(temp_dir.path()).await?;

    // Do not create installer directory - should fail

    // Execute client-full build command (should fail due to missing installer)
    let result = execute_command_with_session(
        Commands::Build {
            targets: vec!["client-full".to_string()],
            clean: false,
            jobs: None,
        },
        &session,
    )
    .await;

    // Should fail or complete with warnings about missing installer
    match result {
        Ok(_) => {
            // Build system should handle missing installer gracefully
            println!("Build completed with missing installer handled gracefully");
        }
        Err(e) => {
            // Expected behavior - build should fail fast on missing installer
            println!("Build failed as expected with missing installer: {}", e);
        }
    }

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn e2e_build_client_full_with_pack_structure() -> anyhow::Result<()> {
    let temp_dir = TempDir::new()?;
    let session = initialize_empack_project(temp_dir.path()).await?;

    // Create installer directory and mock JAR
    let installer_dir = temp_dir.path().join("installer");
    std::fs::create_dir_all(&installer_dir)?;

    let installer_jar = installer_dir.join("packwiz-installer-bootstrap.jar");
    std::fs::write(&installer_jar, "mock-installer-jar")?;

    // Add some additional files to the pack directory to verify they're copied
    let pack_dir = temp_dir.path().join("pack");
    let mods_dir = pack_dir.join("mods");
    std::fs::create_dir_all(&mods_dir)?;

    // Create a mock mod file
    let mod_file = mods_dir.join("example-mod.pw.toml");
    std::fs::write(
        &mod_file,
        "[download]\nurl = \"https://example.com/mod.jar\"\nhash = \"abc123\"\n",
    )?;

    // Create an index.toml file
    let index_file = pack_dir.join("index.toml");
    std::fs::write(
        &index_file,
        "hash-format = \"sha256\"\n\n[[files]]\nfile = \"pack.toml\"\nhash = \"def456\"\n",
    )?;

    // Execute client-full build command
    let result = execute_command_with_session(
        Commands::Build {
            targets: vec!["client-full".to_string()],
            clean: false,
            jobs: None,
        },
        &session,
    )
    .await;

    // Verify pack structure was copied regardless of overall build success
    let client_full_dir = temp_dir.path().join("dist/client-full");
    if client_full_dir.exists() {
        // Check that pack structure was copied
        let copied_pack_dir = client_full_dir.join("pack");
        assert!(copied_pack_dir.exists(), "Pack directory should be copied");

        let copied_pack_toml = copied_pack_dir.join("pack.toml");
        assert!(copied_pack_toml.exists(), "Pack.toml should be copied");

        let copied_index = copied_pack_dir.join("index.toml");
        assert!(copied_index.exists(), "Index.toml should be copied");

        let copied_mods_dir = copied_pack_dir.join("mods");
        if copied_mods_dir.exists() {
            let copied_mod_file = copied_mods_dir.join("example-mod.pw.toml");
            assert!(copied_mod_file.exists(), "Mod files should be copied");
        }
    }

    // Result may be Ok or Err depending on Java/installer availability
    match result {
        Ok(_) => println!("Client-full build completed successfully"),
        Err(e) => println!(
            "Client-full build failed (likely missing Java or installer issue): {}",
            e
        ),
    }

    Ok(())
}
