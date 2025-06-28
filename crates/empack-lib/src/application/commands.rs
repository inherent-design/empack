//! Command execution handlers
//! 
//! Coordinates CLI commands with domain logic through the state machine.

use crate::application::{Commands, CliConfig};
use crate::empack::state::{ModpackStateManager, StateError};
use crate::platform::{ProgramFinder, GoCapabilities, ArchiverCapabilities};
use crate::primitives::{BuildTarget, ModpackState, StateTransition};
use anyhow::{Result, Context};
use std::env;

/// Execute CLI commands using the state machine
pub async fn execute_command(config: CliConfig) -> Result<()> {
    let command = match config.command {
        Some(cmd) => cmd,
        None => {
            println!("empack - Minecraft modpack management");
            println!("Run 'empack --help' for usage information");
            return Ok(());
        }
    };

    match command {
        Commands::Requirements => handle_requirements().await,
        Commands::Init { name, force } => handle_init(name, force).await,
        Commands::Sync { dry_run } => handle_sync(dry_run).await,
        Commands::Build { targets, clean, jobs: _ } => handle_build(targets, clean).await,
        Commands::Add { mods: _, force: _, platform: _ } => handle_add().await,
        Commands::Remove { mods: _, deps: _ } => handle_remove().await,
        Commands::Clean { targets } => handle_clean(targets).await,
    }
}

async fn handle_requirements() -> Result<()> {
    println!("🔧 Checking tool dependencies...");
    
    // Check packwiz
    let packwiz = check_packwiz();
    match packwiz {
        Ok((true, version)) => {
            println!("✅ packwiz: {}", version);
        },
        _ => {
            println!("❌ packwiz: not found");
            println!("   Install from: https://packwiz.infra.link/installation/");
            if GoCapabilities::detect().available {
                println!("   Or via Go: go install github.com/packwiz/packwiz@latest");
            }
        }
    }

    // Check archiving capabilities
    let create_caps = ArchiverCapabilities::detect_creation();
    let extract_caps = ArchiverCapabilities::detect_extraction();
    
    let can_create = create_caps.iter().any(|p| p.available);
    let can_extract = extract_caps.iter().any(|p| p.available);
    
    if can_create {
        let available: Vec<String> = create_caps.iter().filter(|p| p.available).map(|p| p.name.clone()).collect();
        println!("✅ archive creation: {} available", available.join(", "));
    } else {
        println!("❌ archive creation: no tools found");
    }
    
    if can_extract {
        let available: Vec<String> = extract_caps.iter().filter(|p| p.available).map(|p| p.name.clone()).collect();
        println!("✅ archive extraction: {} available", available.join(", "));
    } else {
        println!("❌ archive extraction: no tools found");
    }

    Ok(())
}

async fn handle_init(name: Option<String>, force: bool) -> Result<()> {
    let workdir = env::current_dir().context("Failed to get current directory")?;
    let manager = ModpackStateManager::new(workdir.clone());

    // Check if already initialized
    let current_state = manager.discover_state().map_err(StateError::from)?;
    if current_state != ModpackState::Uninitialized && !force {
        println!("❌ Directory already contains a modpack project");
        println!("   Use --force to overwrite existing files");
        return Ok(());
    }

    println!("🚀 Initializing modpack project...");
    
    if let Some(modpack_name) = name {
        println!("   Name: {}", modpack_name);
    }

    // Execute initialization
    let result = manager.execute_transition(StateTransition::Initialize)
        .context("Failed to initialize modpack project")?;

    match result {
        ModpackState::Configured => {
            println!("✅ Modpack project initialized successfully");
            println!("   📝 Edit empack.yml to configure your dependencies");
            println!("   🔧 Run 'empack sync' to sync with packwiz");
            println!("   🏗️  Run 'empack build all' to build distribution packages");
        },
        _ => return Err(anyhow::anyhow!("Unexpected state after initialization: {:?}", result)),
    }

    Ok(())
}

async fn handle_sync(dry_run: bool) -> Result<()> {
    let workdir = env::current_dir().context("Failed to get current directory")?;
    let manager = ModpackStateManager::new(workdir);

    // Verify we're in a configured state
    let current_state = manager.discover_state().map_err(StateError::from)?;
    if current_state == ModpackState::Uninitialized {
        println!("❌ Not in a modpack directory");
        println!("   Run 'empack init' to set up a modpack project");
        return Ok(());
    }

    if dry_run {
        println!("🔍 Dry run - showing planned changes...");
        // TODO: Implement dry run logic
        println!("   (Dry run functionality not yet implemented)");
        return Ok(());
    }

    println!("🔄 Synchronizing empack.yml with packwiz...");

    let result = manager.execute_transition(StateTransition::Synchronize)
        .context("Failed to synchronize configuration")?;

    match result {
        ModpackState::Configured => {
            println!("✅ Configuration synchronized successfully");
        },
        _ => return Err(anyhow::anyhow!("Unexpected state after sync: {:?}", result)),
    }

    Ok(())
}

async fn handle_build(targets: Vec<String>, clean: bool) -> Result<()> {
    let workdir = env::current_dir().context("Failed to get current directory")?;
    let manager = ModpackStateManager::new(workdir);

    // Verify we're in a configured state
    let current_state = manager.discover_state().map_err(StateError::from)?;
    if current_state == ModpackState::Uninitialized {
        println!("❌ Not in a modpack directory");
        println!("   Run 'empack init' to set up a modpack project");
        return Ok(());
    }

    // Clean if requested
    if clean {
        println!("🧹 Cleaning build artifacts...");
        manager.execute_transition(StateTransition::Clean)
            .context("Failed to clean build artifacts")?;
    }

    // Parse build targets
    let build_targets = parse_build_targets(targets)?;
    
    println!("🏗️  Building targets: {:?}", build_targets);

    let result = manager.execute_transition(StateTransition::Build(build_targets))
        .context("Failed to build modpack")?;

    match result {
        ModpackState::Built => {
            println!("✅ Build completed successfully");
            println!("   📦 Check dist/ directory for build artifacts");
        },
        _ => return Err(anyhow::anyhow!("Unexpected state after build: {:?}", result)),
    }

    Ok(())
}

async fn handle_add() -> Result<()> {
    println!("➕ Add command not yet implemented");
    println!("   This will integrate with the mod search and resolution system");
    Ok(())
}

async fn handle_remove() -> Result<()> {
    println!("➖ Remove command not yet implemented");
    println!("   This will integrate with dependency management");
    Ok(())
}

async fn handle_clean(targets: Vec<String>) -> Result<()> {
    let workdir = env::current_dir().context("Failed to get current directory")?;
    let manager = ModpackStateManager::new(workdir);

    if targets.is_empty() || targets.contains(&"builds".to_string()) || targets.contains(&"all".to_string()) {
        println!("🧹 Cleaning build artifacts...");
        
        let current_state = manager.discover_state().map_err(StateError::from)?;
        if current_state == ModpackState::Built {
            manager.execute_transition(StateTransition::Clean)
                .context("Failed to clean build artifacts")?;
            println!("✅ Build artifacts cleaned");
        } else {
            println!("   No build artifacts to clean");
        }
    }

    if targets.contains(&"cache".to_string()) || targets.contains(&"all".to_string()) {
        println!("🧹 Cleaning cache...");
        println!("   (Cache cleaning not yet implemented)");
    }

    Ok(())
}

fn parse_build_targets(targets: Vec<String>) -> Result<Vec<BuildTarget>> {
    if targets.is_empty() {
        return Err(anyhow::anyhow!("No build targets specified"));
    }

    let mut build_targets = Vec::new();

    for target in targets {
        match target.as_str() {
            "all" => {
                return Ok(vec![
                    BuildTarget::Mrpack,
                    BuildTarget::Client,
                    BuildTarget::Server,
                    BuildTarget::ClientFull,
                    BuildTarget::ServerFull,
                ]);
            },
            "mrpack" => build_targets.push(BuildTarget::Mrpack),
            "client" => build_targets.push(BuildTarget::Client),
            "server" => build_targets.push(BuildTarget::Server),
            "client-full" => build_targets.push(BuildTarget::ClientFull),
            "server-full" => build_targets.push(BuildTarget::ServerFull),
            _ => return Err(anyhow::anyhow!("Unknown build target: {}", target)),
        }
    }

    Ok(build_targets)
}

fn check_packwiz() -> Result<(bool, String)> {
    match std::process::Command::new("packwiz").arg("--help").output() {
        Ok(output) if output.status.success() => {
            let version = get_packwiz_version().unwrap_or_else(|| "unknown".to_string());
            Ok((true, version))
        },
        _ => Ok((false, "not found".to_string())),
    }
}

fn get_packwiz_version() -> Option<String> {
    let packwiz_path = std::process::Command::new("which")
        .arg("packwiz")
        .output()
        .ok()?
        .stdout;
    
    if packwiz_path.is_empty() {
        return None;
    }
    
    let path_str = String::from_utf8_lossy(&packwiz_path).trim().to_string();
    
    let output = std::process::Command::new("go")
        .arg("version")
        .arg("-m")
        .arg(&path_str)
        .output()
        .ok()?;
    
    if !output.status.success() {
        return None;
    }
    
    let version_output = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = version_output.lines().collect();
    if lines.len() >= 3 {
        let third_line = lines[2];
        let fields: Vec<&str> = third_line.split_whitespace().collect();
        if fields.len() >= 3 {
            return Some(fields[2].to_string());
        }
    }
    
    None
}