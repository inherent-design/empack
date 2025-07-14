//! Command execution handlers
//! 
//! Coordinates CLI commands with domain logic through the state machine.

use crate::application::{Commands, CliConfig};
use crate::application::cli::SearchPlatform;
use crate::empack::state::{ModpackStateManager, StateError};
use crate::empack::search::{ProjectResolver, Platform};
use crate::empack::config::ConfigManager;
use crate::platform::{GoCapabilities, ArchiverCapabilities};
use crate::primitives::{BuildTarget, ModpackState, StateTransition};
use anyhow::{Result, Context};
use std::env;
use std::process::Command;
use std::collections::HashSet;

/// Actions to be taken during sync
#[derive(Debug, Clone)]
enum SyncAction {
    Add {
        key: String,
        title: String,
        command: Vec<String>,
    },
    Remove {
        key: String,
        title: String,
    },
}

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
        Commands::Version => handle_version().await,
        Commands::Init { name, force } => handle_init(name, force).await,
        Commands::Sync { dry_run } => handle_sync(dry_run, &config.app_config).await,
        Commands::Build { targets, clean, jobs: _ } => handle_build(targets, clean).await,
        Commands::Add { mods, force, platform } => handle_add(mods, force, platform, &config.app_config).await,
        Commands::Remove { mods, deps } => handle_remove(mods, deps).await,
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

async fn handle_version() -> Result<()> {
    println!("empack {}", env!("CARGO_PKG_VERSION"));
    println!("A Minecraft modpack development and distribution tool");
    println!();
    println!("Built from commit: {}", option_env!("GIT_HASH").unwrap_or("unknown"));
    println!("Build date: {}", option_env!("BUILD_DATE").unwrap_or("unknown"));
    println!("Target: {}", std::env::consts::ARCH);
    
    Ok(())
}

async fn handle_init(name: Option<String>, force: bool) -> Result<()> {
    let workdir = env::current_dir().context("Failed to get current directory")?;
    let manager = ModpackStateManager::new_default(workdir.clone());

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

async fn handle_sync(dry_run: bool, app_config: &crate::application::config::AppConfig) -> Result<()> {
    let workdir = env::current_dir().context("Failed to get current directory")?;
    let manager = ModpackStateManager::new_default(workdir.clone());

    // Verify we're in a configured state
    let current_state = manager.discover_state().map_err(StateError::from)?;
    if current_state == ModpackState::Uninitialized {
        println!("❌ Not in a modpack directory");
        println!("   Run 'empack init' to set up a modpack project");
        return Ok(());
    }

    // Create config manager
    let config_manager = ConfigManager::new(workdir.clone());

    // Load project plan from empack.yml
    let project_plan = config_manager.create_project_plan()
        .context("Failed to load empack.yml configuration")?;

    // Create HTTP client for API requests
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .context("Failed to create HTTP client")?;

    // Get CurseForge API key from app configuration
    let curseforge_api_key = app_config.curseforge_api_client_key.clone();

    // Create project resolver
    let resolver = ProjectResolver::new(client, curseforge_api_key);

    println!("🔄 Synchronizing empack.yml with packwiz...");
    println!("   Target: {} v{}", project_plan.minecraft_version, project_plan.loader_version);

    // Get currently installed mods
    let installed_mods = match get_installed_mods() {
        Ok(mods) => {
            println!("   📋 Found {} currently installed mods", mods.len());
            mods
        }
        Err(e) => {
            println!("   ⚠️  Could not read installed mods: {}", e);
            println!("   ℹ️  Assuming empty pack (add-only mode)");
            HashSet::new()
        }
    };

    // Collect expected mods from empack.yml
    let mut expected_mods = HashSet::new();
    let mut planned_actions = Vec::new();

    // Process each dependency in empack.yml
    for dep_spec in &project_plan.dependencies {
        println!("\n🔍 Processing dependency: {}", dep_spec.key);

        // Normalize the key for comparison with installed mods
        let normalized_key = dep_spec.key
            .to_lowercase()
            .replace(' ', "_")
            .replace('-', "_");
        expected_mods.insert(normalized_key.clone());

        // Check if this mod is already installed
        if installed_mods.contains(&normalized_key) {
            println!("   ✅ Already installed: {}", dep_spec.key);
            continue; // Skip mods that are already installed
        }

        // Resolve the project if we don't have a direct project_id
        let (project_id, command) = if let Some(existing_id) = &dep_spec.project_id {
            // Use existing project ID - default to Modrinth command
            (existing_id.clone(), vec!["mr".to_string(), "add".to_string(), existing_id.clone()])
        } else {
            // Use project plan context
            let minecraft_version = Some(dep_spec.minecraft_version.as_str());
            let mod_loader = Some(match dep_spec.loader {
                crate::empack::parsing::ModLoader::Fabric => "fabric",
                crate::empack::parsing::ModLoader::Forge => "forge",
                crate::empack::parsing::ModLoader::Quilt => "quilt",
                crate::empack::parsing::ModLoader::NeoForge => "neoforge",
            });

            match resolver.resolve_project(&dep_spec.search_query, 
                                         Some(match dep_spec.project_type {
                                             crate::primitives::ProjectType::Mod => "mod",
                                             crate::primitives::ProjectType::Datapack => "datapack",
                                             crate::primitives::ProjectType::ResourcePack => "resourcepack",
                                             crate::primitives::ProjectType::Shader => "shader",
                                         }), 
                                         minecraft_version, mod_loader).await {
                Ok(project_info) => {
                    println!("   ✅ Resolved: {} on {}", project_info.title, project_info.platform);
                    
                    // Create appropriate packwiz add command based on platform
                    let command = match project_info.platform {
                        Platform::Modrinth => vec!["mr".to_string(), "add".to_string(), project_info.project_id.clone()],
                        Platform::CurseForge => vec!["cf".to_string(), "add".to_string(), project_info.project_id.clone()],
                        Platform::Forge => vec!["cf".to_string(), "add".to_string(), project_info.project_id.clone()],
                    };
                    
                    (project_info.project_id, command)
                }
                Err(e) => {
                    println!("   ❌ Failed to resolve: {}", e);
                    continue;
                }
            }
        };

        planned_actions.push(SyncAction::Add {
            key: dep_spec.key.clone(),
            title: dep_spec.search_query.clone(),
            command,
        });
    }

    // Find mods that are installed but not in empack.yml (need to be removed)
    for installed_mod in &installed_mods {
        if !expected_mods.contains(installed_mod) {
            planned_actions.push(SyncAction::Remove {
                key: installed_mod.clone(),
                title: installed_mod.clone(),
            });
        }
    }

    // Show planned actions
    if planned_actions.is_empty() {
        println!("\n✅ No changes needed - empack.yml already in sync");
        return Ok(());
    }

    println!("\n📋 Planned Actions:");
    for action in &planned_actions {
        match action {
            SyncAction::Add { key, title, command } => {
                println!("   ➕ Add: {} ({})", title, key);
                if dry_run {
                    println!("      Command: packwiz {}", command.join(" "));
                }
            }
            SyncAction::Remove { key, title } => {
                println!("   ➖ Remove: {} ({})", title, key);
                if dry_run {
                    println!("      Command: packwiz remove {}", key);
                }
            }
        }
    }

    if dry_run {
        println!("\n🔍 Dry run complete - no changes applied");
        return Ok(());
    }

    // Execute planned actions
    println!("\n🚀 Executing sync actions...");
    let mut success_count = 0;
    let mut failure_count = 0;

    for action in planned_actions {
        match action {
            SyncAction::Add { key, title, command } => {
                println!("   ➕ Adding: {}", title);
                match execute_packwiz_command(&command.iter().map(|s| s.as_str()).collect::<Vec<_>>()) {
                    Ok(_) => {
                        println!("      ✅ Added successfully");
                        success_count += 1;
                    }
                    Err(e) => {
                        println!("      ❌ Failed: {}", e);
                        failure_count += 1;
                    }
                }
            }
            SyncAction::Remove { key, title: _ } => {
                println!("   ➖ Removing: {}", key);
                match execute_packwiz_command(&["remove", &key]) {
                    Ok(_) => {
                        println!("      ✅ Removed successfully");
                        success_count += 1;
                    }
                    Err(e) => {
                        println!("      ❌ Failed: {}", e);
                        failure_count += 1;
                    }
                }
            }
        }
    }

    // Show summary
    println!("\n📊 Sync Summary:");
    println!("   ✅ Successful actions: {}", success_count);
    println!("   ❌ Failed actions: {}", failure_count);

    if failure_count == 0 {
        println!("✅ empack.yml synchronized successfully with packwiz");
    } else {
        println!("⚠️  Sync completed with {} failures", failure_count);
    }

    Ok(())
}

async fn handle_build(targets: Vec<String>, clean: bool) -> Result<()> {
    let workdir = env::current_dir().context("Failed to get current directory")?;
    let manager = ModpackStateManager::new_default(workdir);

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

async fn handle_add(mods: Vec<String>, force: bool, platform: Option<SearchPlatform>, app_config: &crate::application::config::AppConfig) -> Result<()> {
    if mods.is_empty() {
        println!("❌ No mods specified to add");
        println!("   Usage: empack add <mod1> [mod2] [mod3]...");
        return Ok(());
    }

    let workdir = env::current_dir().context("Failed to get current directory")?;
    let manager = ModpackStateManager::new_default(workdir.clone());

    // Verify we're in a configured state
    let current_state = manager.discover_state().map_err(StateError::from)?;
    if current_state == ModpackState::Uninitialized {
        println!("❌ Not in a modpack directory");
        println!("   Run 'empack init' to set up a modpack project");
        return Ok(());
    }

    // Create config manager
    let config_manager = ConfigManager::new(workdir);

    // Try to load existing project plan to get context
    let project_plan = match config_manager.create_project_plan() {
        Ok(plan) => Some(plan),
        Err(_) => {
            println!("⚠️  No empack.yml found, using defaults");
            None
        }
    };

    // Create HTTP client for API requests
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .context("Failed to create HTTP client")?;

    // Get CurseForge API key from app configuration
    let curseforge_api_key = app_config.curseforge_api_client_key.clone();

    // Create project resolver
    let resolver = ProjectResolver::new(client, curseforge_api_key);

    println!("➕ Adding {} mod(s) to modpack...", mods.len());

    let mut added_mods = Vec::new();
    let mut failed_mods = Vec::new();

    for mod_query in mods {
        println!("\n🔍 Resolving mod: {}", mod_query);

        // Use project plan context if available
        let minecraft_version = project_plan.as_ref().map(|p| p.minecraft_version.as_str());
        let mod_loader = project_plan.as_ref().map(|p| match p.loader {
            crate::empack::parsing::ModLoader::Fabric => "fabric",
            crate::empack::parsing::ModLoader::Forge => "forge",
            crate::empack::parsing::ModLoader::Quilt => "quilt",
            crate::empack::parsing::ModLoader::NeoForge => "neoforge",
        });

        match resolver.resolve_project(&mod_query, Some("mod"), minecraft_version, mod_loader).await {
            Ok(project_info) => {
                println!("✅ Found: {} on {}", project_info.title, project_info.platform);
                println!("   Confidence: {}%", project_info.confidence);
                
                // Execute appropriate packwiz command
                let packwiz_result = match project_info.platform {
                    Platform::Modrinth => {
                        execute_packwiz_command(&["mr", "add", &project_info.project_id])
                    }
                    Platform::CurseForge => {
                        execute_packwiz_command(&["cf", "add", &project_info.project_id])
                    }
                    Platform::Forge => {
                        execute_packwiz_command(&["cf", "add", &project_info.project_id])
                    }
                };

                match packwiz_result {
                    Ok(_) => {
                        println!("   ✅ Successfully added to pack");
                        added_mods.push((mod_query, project_info));
                    }
                    Err(e) => {
                        println!("   ❌ Failed to add to pack: {}", e);
                        failed_mods.push((mod_query, format!("Packwiz error: {}", e)));
                    }
                }
            }
            Err(e) => {
                println!("❌ Failed to resolve mod: {}", e);
                failed_mods.push((mod_query, e.to_string()));
            }
        }
    }

    // Show summary
    println!("\n📊 Add Summary:");
    println!("   ✅ Successfully added: {}", added_mods.len());
    println!("   ❌ Failed: {}", failed_mods.len());

    if !failed_mods.is_empty() {
        println!("\n❌ Failed mods:");
        for (mod_name, error) in failed_mods {
            println!("   • {}: {}", mod_name, error);
        }
    }

    if !added_mods.is_empty() {
        println!("\n💡 Tip: Run 'empack sync' to update empack.yml with new dependencies");
    }

    Ok(())
}

async fn handle_remove(mods: Vec<String>, deps: bool) -> Result<()> {
    if mods.is_empty() {
        println!("❌ No mods specified to remove");
        println!("   Usage: empack remove <mod1> [mod2] [mod3]...");
        return Ok(());
    }

    let workdir = env::current_dir().context("Failed to get current directory")?;
    let manager = ModpackStateManager::new_default(workdir.clone());

    // Verify we're in a configured state
    let current_state = manager.discover_state().map_err(StateError::from)?;
    if current_state == ModpackState::Uninitialized {
        println!("❌ Not in a modpack directory");
        println!("   Run 'empack init' to set up a modpack project");
        return Ok(());
    }

    println!("➖ Removing {} mod(s) from modpack...", mods.len());

    let mut removed_mods = Vec::new();
    let mut failed_mods = Vec::new();

    for mod_name in mods {
        println!("\n🗑️  Removing mod: {}", mod_name);

        // Execute packwiz remove command
        let mut packwiz_args = vec!["remove", &mod_name];
        if deps {
            packwiz_args.push("--remove-deps");
        }

        match execute_packwiz_command(&packwiz_args) {
            Ok(_) => {
                println!("   ✅ Successfully removed from pack");
                removed_mods.push(mod_name);
            }
            Err(e) => {
                println!("   ❌ Failed to remove from pack: {}", e);
                failed_mods.push((mod_name, e.to_string()));
            }
        }
    }

    // Show summary
    println!("\n📊 Remove Summary:");
    println!("   ✅ Successfully removed: {}", removed_mods.len());
    println!("   ❌ Failed: {}", failed_mods.len());

    if !failed_mods.is_empty() {
        println!("\n❌ Failed mods:");
        for (mod_name, error) in failed_mods {
            println!("   • {}: {}", mod_name, error);
        }
    }

    if !removed_mods.is_empty() {
        println!("\n💡 Tip: Run 'empack sync' to update empack.yml after removing dependencies");
    }

    Ok(())
}

async fn handle_clean(targets: Vec<String>) -> Result<()> {
    let workdir = env::current_dir().context("Failed to get current directory")?;
    let manager = ModpackStateManager::new_default(workdir);

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

/// Execute packwiz command with arguments
fn execute_packwiz_command(args: &[&str]) -> Result<()> {
    let output = Command::new("packwiz")
        .args(args)
        .output()
        .context("Failed to execute packwiz command")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow::anyhow!("Packwiz command failed: {}", stderr));
    }

    Ok(())
}

/// Get the list of currently installed mods from packwiz
fn get_installed_mods() -> Result<HashSet<String>> {
    let output = Command::new("packwiz")
        .arg("list")
        .output()
        .context("Failed to execute packwiz list command")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow::anyhow!("Packwiz list command failed: {}", stderr));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut installed_mods = HashSet::new();

    // Parse packwiz list output - each line should contain a mod name
    // The format varies, but we're looking for .toml files or project names
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("Mods:") || line.starts_with("Total:") {
            continue;
        }

        // Extract mod name from various formats packwiz might use
        // Common formats: "- modname" or "modname.pw.toml" or just "modname"
        let mod_name = if line.starts_with("- ") {
            line.trim_start_matches("- ").trim()
        } else if line.ends_with(".pw.toml") {
            line.trim_end_matches(".pw.toml")
        } else {
            line
        };

        // Convert to a normalized key format (lowercase, replace spaces/dashes with underscores)
        let normalized_name = mod_name
            .to_lowercase()
            .replace(' ', "_")
            .replace('-', "_");

        installed_mods.insert(normalized_name);
    }

    Ok(installed_mods)
}