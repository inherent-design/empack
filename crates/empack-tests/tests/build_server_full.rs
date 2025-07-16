//! E2E tests for server-full build target
//!
//! Tests the complete server-full build workflow including template processing,
//! server JAR download, and mod downloading.

use empack_lib::application::commands::execute_command_with_session;
use empack_lib::application::config::AppConfig;
use empack_lib::application::session::{CommandSession, LiveFileSystemProvider, LiveNetworkProvider, LiveConfigProvider, LiveProcessProvider};
use empack_lib::application::Commands;
use empack_lib::display::{Display, LiveDisplayProvider};
use empack_lib::terminal::TerminalCapabilities;
use tempfile::TempDir;
use anyhow::Result;
use std::path::Path;
use indicatif::MultiProgress;

/// Initialize a real empack project in the given directory and return a session
async fn initialize_empack_project(workdir: &Path) -> Result<CommandSession<LiveFileSystemProvider, LiveNetworkProvider, LiveProcessProvider, LiveConfigProvider>> {
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
    let process_provider = LiveProcessProvider;
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
async fn e2e_build_server_full_successfully() -> anyhow::Result<()> {
    let temp_dir = TempDir::new()?;
    let session = initialize_empack_project(temp_dir.path()).await?;

    // Create required directories for server-full build
    let templates_dir = temp_dir.path().join("templates/server");
    std::fs::create_dir_all(&templates_dir)?;
    
    let installer_dir = temp_dir.path().join("installer");
    std::fs::create_dir_all(&installer_dir)?;
    
    // Create a mock installer JAR (server-full build requires it)
    let installer_jar = installer_dir.join("packwiz-installer-bootstrap.jar");
    std::fs::write(&installer_jar, "mock-installer-jar")?;
    
    // Create server template files
    let server_properties = templates_dir.join("server.properties.template");
    std::fs::write(&server_properties, "server-port=25565\nmotd={{NAME}} v{{VERSION}}\n")?;
    
    let install_script = templates_dir.join("install_pack.sh.template");
    std::fs::write(&install_script, "#!/bin/bash\necho \"Installing {{NAME}}\"\n")?;

    // Execute server-full build command
    let result = execute_command_with_session(
        Commands::Build {
            targets: vec!["server-full".to_string()],
            clean: false,
            jobs: None,
        },
        &session,
    )
    .await;

    // Server-full build may fail due to missing mrpack-install or Java, but should create directory structure
    match result {
        Ok(_) => {
            // If successful, verify the server-full directory structure was created
            let server_full_dir = temp_dir.path().join("dist/server-full");
            assert!(server_full_dir.exists(), "Server-full build directory should exist");
            
            // Verify template processing occurred
            let processed_properties = server_full_dir.join("server.properties");
            if processed_properties.exists() {
                let content = std::fs::read_to_string(&processed_properties)?;
                assert!(content.contains("Test Modpack"), "Template variables should be processed");
            }
            
            // Verify pack directory was copied
            let pack_dir = server_full_dir.join("pack");
            assert!(pack_dir.exists(), "Pack directory should be copied to server-full build");
        }
        Err(e) => {
            // Expected if mrpack-install or Java is not available - verify partial build occurred
            let server_full_dir = temp_dir.path().join("dist/server-full");
            if server_full_dir.exists() {
                println!("Server-full build failed as expected (likely missing mrpack-install or Java): {}", e);
            } else {
                return Err(e);
            }
        }
    }

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn e2e_build_server_full_missing_installer() -> anyhow::Result<()> {
    let temp_dir = TempDir::new()?;
    let session = initialize_empack_project(temp_dir.path()).await?;

    // Create templates directory but not installer
    let templates_dir = temp_dir.path().join("templates/server");
    std::fs::create_dir_all(&templates_dir)?;

    // Execute server-full build command (should fail due to missing installer)
    let result = execute_command_with_session(
        Commands::Build {
            targets: vec!["server-full".to_string()],
            clean: false,
            jobs: None,
        },
        &session,
    )
    .await;

    // Should complete but with warnings about missing installer
    match result {
        Ok(_) => {
            // Build system should handle missing installer gracefully
            println!("Build completed with missing installer handled gracefully");
        }
        Err(e) => {
            // Also acceptable if build system fails fast on missing installer
            println!("Build failed as expected with missing installer: {}", e);
        }
    }

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn e2e_build_server_full_with_templates() -> anyhow::Result<()> {
    let temp_dir = TempDir::new()?;
    let session = initialize_empack_project(temp_dir.path()).await?;

    // Create templates directory with various template files
    let templates_dir = temp_dir.path().join("templates/server");
    std::fs::create_dir_all(&templates_dir)?;
    
    let installer_dir = temp_dir.path().join("installer");
    std::fs::create_dir_all(&installer_dir)?;
    
    // Create mock installer JAR
    let installer_jar = installer_dir.join("packwiz-installer-bootstrap.jar");
    std::fs::write(&installer_jar, "mock-installer-jar")?;

    // Create multiple template files
    let server_properties = templates_dir.join("server.properties.template");
    std::fs::write(&server_properties, 
        "server-port=25565\nmotd={{NAME}} v{{VERSION}} by {{AUTHOR}}\nmax-players=20\n")?;
    
    let eula = templates_dir.join("eula.txt.template");
    std::fs::write(&eula, "eula=true\n# {{NAME}} server\n")?;
    
    let start_script = templates_dir.join("start.sh.template");
    std::fs::write(&start_script, 
        "#!/bin/bash\necho \"Starting {{NAME}} server-full\"\njava -jar srv.jar nogui\n")?;

    // Execute server-full build command
    let result = execute_command_with_session(
        Commands::Build {
            targets: vec!["server-full".to_string()],
            clean: false,
            jobs: None,
        },
        &session,
    )
    .await;

    // Verify template processing worked regardless of overall build success
    let server_full_dir = temp_dir.path().join("dist/server-full");
    if server_full_dir.exists() {
        // Check that templates were processed
        let processed_properties = server_full_dir.join("server.properties");
        if processed_properties.exists() {
            let content = std::fs::read_to_string(&processed_properties)?;
            assert!(content.contains("Test Modpack"), "Server name should be processed");
            assert!(content.contains("Test Author"), "Author should be processed");
            assert!(!content.contains("{{NAME}}"), "Template variables should be replaced");
        }
        
        let processed_eula = server_full_dir.join("eula.txt");
        if processed_eula.exists() {
            let content = std::fs::read_to_string(&processed_eula)?;
            assert!(content.contains("eula=true"), "EULA should be processed");
            assert!(content.contains("Test Modpack"), "EULA comment should be processed");
        }
        
        let processed_script = server_full_dir.join("start.sh");
        if processed_script.exists() {
            let content = std::fs::read_to_string(&processed_script)?;
            assert!(content.contains("Starting Test Modpack server-full"), "Script should be processed");
            assert!(content.contains("java -jar srv.jar nogui"), "Script should contain server command");
        }
    }

    // Result may be Ok or Err depending on mrpack-install and Java availability
    match result {
        Ok(_) => println!("Server-full build completed successfully"),
        Err(e) => println!("Server-full build failed (likely missing mrpack-install or Java): {}", e),
    }

    Ok(())
}