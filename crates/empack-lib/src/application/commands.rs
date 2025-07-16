//! Command execution handlers
//! 
//! New session-based architecture for command execution.
//! Implements the Session-Scoped Dependency Injection Pattern.

use crate::application::{Commands, CliConfig};
use crate::application::session::{CommandSession, Session};
use crate::platform::{GoCapabilities, ArchiverCapabilities};
use crate::primitives::{ModpackState, StateTransition, ProjectType, BuildTarget};
use crate::empack::state::{StateError, ModpackStateManager};
use crate::empack::config::ConfigManager;
use crate::empack::search::{ProjectResolver, Platform};
use crate::empack::parsing::ModLoader;
use anyhow::{Result, Context};
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

/// Execute CLI commands using the new session-based architecture
pub async fn execute_command(config: CliConfig) -> Result<()> {
    // Create command session (owns all ephemeral state)
    let session = CommandSession::new(config.app_config);
    
    let command = match config.command {
        Some(cmd) => cmd,
        None => {
            session.display().status().message("empack - Minecraft modpack management");
            session.display().status().subtle("Run 'empack --help' for usage information");
            return Ok(());
        }
    };

    // Dispatch to session-aware command handlers
    execute_command_with_session(command, &session).await
}

/// Execute a specific command with a provided session (for testing)
pub async fn execute_command_with_session(command: Commands, session: &dyn Session) -> Result<()> {
    match command {
        Commands::Requirements => handle_requirements(session).await,
        Commands::Version => handle_version(session).await,
        Commands::Init { name, force } => handle_init(session, name, force).await,
        Commands::Add { mods, force, platform } => handle_add(session, mods, force, platform).await,
        Commands::Remove { mods, deps } => handle_remove(session, mods, deps).await,
        Commands::Build { targets, clean, jobs: _ } => handle_build(session, targets, clean).await,
        Commands::Clean { targets } => handle_clean(session, targets).await,
        Commands::Sync { dry_run } => handle_sync(session, dry_run).await,
    }
}

// Session-based command handlers using dependency injection pattern
async fn handle_requirements(session: &dyn Session) -> Result<()> {
    session.display().status().section("🔧 Checking tool dependencies");
    
    // Check packwiz
    let packwiz = session.process().check_packwiz();
    match packwiz {
        Ok((true, version)) => {
            session.display().status().success("packwiz", &version);
        },
        _ => {
            session.display().status().error("packwiz", "not found");
            session.display().status().subtle("   Install from: https://packwiz.infra.link/installation/");
            if GoCapabilities::detect().available {
                session.display().status().subtle("   Or via Go: go install github.com/packwiz/packwiz@latest");
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
        session.display().status().success("archive creation", &format!("{} available", available.join(", ")));
    } else {
        session.display().status().error("archive creation", "no tools found");
    }
    
    if can_extract {
        let available: Vec<String> = extract_caps.iter().filter(|p| p.available).map(|p| p.name.clone()).collect();
        session.display().status().success("archive extraction", &format!("{} available", available.join(", ")));
    } else {
        session.display().status().error("archive extraction", "no tools found");
    }

    Ok(())
}

async fn handle_version(session: &dyn Session) -> Result<()> {
    session.display().status().emphasis(&format!("empack {}", env!("CARGO_PKG_VERSION")));
    session.display().status().message("A Minecraft modpack development and distribution tool");
    session.display().status().message("");
    
    let build_info = [
        ("Built from commit", option_env!("GIT_HASH").unwrap_or("unknown")),
        ("Build date", option_env!("BUILD_DATE").unwrap_or("unknown")),
        ("Target", std::env::consts::ARCH),
    ];
    
    session.display().table().properties(&build_info);
    
    Ok(())
}

async fn handle_init(
    session: &dyn Session,
    name: Option<String>,
    force: bool,
) -> Result<()> {
    let manager = session.state();

    // Check if already initialized
    let current_state = manager.discover_state().map_err(StateError::from)?;
    if current_state != ModpackState::Uninitialized && !force {
        session.display().status().error("Directory already contains a modpack project", "");
        session.display().status().subtle("   Use --force to overwrite existing files");
        return Ok(());
    }

    session.display().status().section("🚀 Initializing modpack project");
    
    if let Some(modpack_name) = name {
        session.display().status().info(&format!("Name: {}", modpack_name));
    }

    // Execute initialization
    let result = manager.execute_transition(StateTransition::Initialize).await
        .context("Failed to initialize modpack project")?;

    match result {
        ModpackState::Configured => {
            session.display().status().complete("Modpack project initialized successfully");
            
            let next_steps = [
                "📝 Edit empack.yml to configure your dependencies",
                "🔧 Run 'empack sync' to sync with packwiz", 
                "🏗️  Run 'empack build all' to build distribution packages"
            ];
            session.display().status().list(&next_steps);
        },
        _ => return Err(anyhow::anyhow!("Unexpected state after initialization: {:?}", result)),
    }

    Ok(())
}

async fn handle_add(
    session: &dyn Session,
    mods: Vec<String>,
    force: bool,
    platform: Option<crate::application::cli::SearchPlatform>,
) -> Result<()> {
    // Migrate from legacy handle_add - using session providers
    
    if mods.is_empty() {
        session.display().status().error("No mods specified to add", "");
        session.display().status().subtle("   Usage: empack add <mod1> [mod2] [mod3]...");
        return Ok(());
    }

    let manager = session.state();

    // Verify we're in a configured state
    let current_state = manager.discover_state().map_err(|e| anyhow::anyhow!("State error: {:?}", e))?;
    if current_state == crate::primitives::ModpackState::Uninitialized {
        session.display().status().error("Not in a modpack directory", "");
        session.display().status().subtle("   Run 'empack init' to set up a modpack project");
        return Ok(());
    }

    // Create config manager
    let workdir = session.filesystem().current_dir()?;
    let config_manager = session.filesystem().config_manager(workdir.clone());

    // Try to load existing project plan to get context
    let project_plan = match config_manager.create_project_plan() {
        Ok(plan) => Some(plan),
        Err(_) => {
            session.display().status().warning("No empack.yml found, using defaults");
            None
        }
    };

    // Create HTTP client for API requests
    let client = session.network().http_client()?;

    // Get CurseForge API key from app configuration
    let curseforge_api_key = session.config().app_config().curseforge_api_client_key.clone();

    // Create project resolver
    let resolver = session.network().project_resolver(client, curseforge_api_key);

    session.display().status().section(&format!("➕ Adding {} mod(s) to modpack", mods.len()));

    let mut added_mods = Vec::new();
    let mut failed_mods = Vec::new();

    for mod_query in mods {
        session.display().status().checking(&format!("Resolving mod: {}", mod_query));

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
                session.display().status().success("Found", &format!("{} on {}", project_info.title, project_info.platform));
                session.display().status().info(&format!("Confidence: {}%", project_info.confidence));
                
                // Execute appropriate packwiz command
                let packwiz_result = match project_info.platform {
                    crate::empack::search::Platform::Modrinth => {
                        session.process().execute("packwiz", &["mr", "add", &project_info.project_id], &workdir)
                    }
                    crate::empack::search::Platform::CurseForge => {
                        session.process().execute("packwiz", &["cf", "add", &project_info.project_id], &workdir)
                    }
                    crate::empack::search::Platform::Forge => {
                        session.process().execute("packwiz", &["cf", "add", &project_info.project_id], &workdir)
                    }
                };
                
                // Check if command was successful
                let packwiz_result = packwiz_result.and_then(|output| {
                    if output.success {
                        Ok(())
                    } else {
                        Err(anyhow::anyhow!("Packwiz command failed: {}", output.stderr))
                    }
                });

                match packwiz_result {
                    Ok(_) => {
                        session.display().status().success("Successfully added to pack", "");
                        added_mods.push((mod_query, project_info));
                    }
                    Err(e) => {
                        session.display().status().error("Failed to add to pack", &e.to_string());
                        failed_mods.push((mod_query, format!("Packwiz error: {}", e)));
                    }
                }
            }
            Err(e) => {
                session.display().status().error("Failed to resolve mod", &e.to_string());
                failed_mods.push((mod_query, e.to_string()));
            }
        }
    }

    // Show summary
    session.display().status().section("📊 Add Summary");
    session.display().status().success("Successfully added", &added_mods.len().to_string());
    session.display().status().info(&format!("Failed: {}", failed_mods.len()));

    if !failed_mods.is_empty() {
        session.display().status().section("Failed mods");
        for (mod_name, error) in failed_mods {
            session.display().status().error(&mod_name, &error);
        }
    }

    if !added_mods.is_empty() {
        session.display().status().subtle("💡 Tip: Run 'empack sync' to update empack.yml with new dependencies");
    }

    Ok(())
}

async fn handle_remove(
    session: &dyn Session,
    mods: Vec<String>,
    deps: bool,
) -> Result<()> {
    if mods.is_empty() {
        session.display().status().error("No mods specified to remove", "");
        session.display().status().subtle("   Usage: empack remove <mod1> [mod2] [mod3]...");
        return Ok(());
    }

    let manager = session.state();

    // Verify we're in a configured state
    let current_state = manager.discover_state().map_err(StateError::from)?;
    if current_state == ModpackState::Uninitialized {
        session.display().status().error("Not in a modpack directory", "");
        session.display().status().subtle("   Run 'empack init' to set up a modpack project");
        return Ok(());
    }

    session.display().status().section(&format!("➖ Removing {} mod(s) from modpack", mods.len()));

    let workdir = session.filesystem().current_dir()?;
    let mut removed_mods = Vec::new();
    let mut failed_mods = Vec::new();

    for mod_name in mods {
        session.display().status().checking(&format!("Removing mod: {}", mod_name));

        // Execute packwiz remove command
        let mut packwiz_args = vec!["remove", &mod_name];
        if deps {
            packwiz_args.push("--remove-deps");
        }

        let result = session.process().execute("packwiz", &packwiz_args, &workdir)
            .and_then(|output| {
                if output.success {
                    Ok(())
                } else {
                    Err(anyhow::anyhow!("Packwiz command failed: {}", output.stderr))
                }
            });

        match result {
            Ok(_) => {
                session.display().status().success("Successfully removed from pack", "");
                removed_mods.push(mod_name);
            }
            Err(e) => {
                session.display().status().error("Failed to remove from pack", &e.to_string());
                failed_mods.push((mod_name, e.to_string()));
            }
        }
    }

    // Show summary
    session.display().status().section("📊 Remove Summary");
    session.display().status().success("Successfully removed", &removed_mods.len().to_string());
    session.display().status().info(&format!("Failed: {}", failed_mods.len()));

    if !failed_mods.is_empty() {
        session.display().status().section("Failed mods");
        for (mod_name, error) in failed_mods {
            session.display().status().error(&mod_name, &error);
        }
    }

    if !removed_mods.is_empty() {
        session.display().status().subtle("💡 Tip: Run 'empack sync' to update empack.yml after removing dependencies");
    }

    Ok(())
}

async fn handle_build(
    session: &dyn Session,
    targets: Vec<String>,
    clean: bool,
) -> Result<()> {
    let manager = session.state();

    // Verify we're in a configured state
    let current_state = manager.discover_state().map_err(StateError::from)?;
    if current_state == ModpackState::Uninitialized {
        session.display().status().error("Not in a modpack directory", "");
        session.display().status().subtle("   Run 'empack init' to set up a modpack project");
        return Ok(());
    }

    // Clean if requested
    if clean {
        session.display().status().checking("Cleaning build artifacts");
        manager.execute_transition(StateTransition::Clean).await
            .context("Failed to clean build artifacts")?;
    }

    // Parse build targets
    let build_targets = parse_build_targets(targets)?;
    
    session.display().status().section(&format!("🏗️  Building targets: {:?}", build_targets));

    // Ensure packwiz-installer-bootstrap.jar is available for builds that need it
    let bootstrap_jar_path = session.filesystem().get_bootstrap_jar_cache_path()?;
    let needs_bootstrap_jar = build_targets.iter().any(|target| {
        matches!(target, BuildTarget::Client | BuildTarget::Server | BuildTarget::ClientFull | BuildTarget::ServerFull)
    });

    if needs_bootstrap_jar && !session.filesystem().exists(&bootstrap_jar_path) {
        session.display().status().info("Downloading required component: packwiz-installer-bootstrap.jar...");

        // Create cache directory if it doesn't exist
        if let Some(parent) = bootstrap_jar_path.parent() {
            session.filesystem().create_dir_all(parent)?;
        }

        // Use the NetworkProvider to download the file
        let client = session.network().http_client()?;
        let url = "https://github.com/packwiz/packwiz-installer-bootstrap/releases/latest/download/packwiz-installer-bootstrap.jar";
        let response = client.get(url).send().await
            .context("Failed to download packwiz-installer-bootstrap.jar")?;
        
        if !response.status().is_success() {
            return Err(anyhow::anyhow!(
                "Failed to download packwiz-installer-bootstrap.jar: HTTP {}",
                response.status()
            ));
        }

        let bytes = response.bytes().await
            .context("Failed to read response bytes")?;

        // Use the FileSystemProvider to save the file
        std::fs::write(&bootstrap_jar_path, bytes)
            .context("Failed to write JAR file to cache")?;

        session.display().status().complete("Downloaded packwiz-installer-bootstrap.jar");
    }

    // Create BuildOrchestrator with session
    let mut build_orchestrator = crate::empack::builds::BuildOrchestrator::new(session)
        .context("Failed to create build orchestrator")?;
    
    // Execute build pipeline with state management
    let results = build_orchestrator.execute_build_pipeline(&build_targets).await
        .context("Failed to execute build pipeline")?;

    session.display().status().complete("Build completed successfully");
    session.display().status().subtle("   📦 Check dist/ directory for build artifacts");

    Ok(())
}

async fn handle_clean(
    session: &dyn Session,
    targets: Vec<String>,
) -> Result<()> {
    let manager = session.state();

    if targets.is_empty() || targets.contains(&"builds".to_string()) || targets.contains(&"all".to_string()) {
        session.display().status().checking("Cleaning build artifacts");
        
        let current_state = manager.discover_state().map_err(StateError::from)?;
        if current_state == ModpackState::Built {
            manager.execute_transition(StateTransition::Clean).await
                .context("Failed to clean build artifacts")?;
            session.display().status().complete("Build artifacts cleaned");
        } else {
            session.display().status().info("No build artifacts to clean");
        }
    }

    if targets.contains(&"cache".to_string()) || targets.contains(&"all".to_string()) {
        session.display().status().checking("Cleaning cache");
        session.display().status().subtle("(Cache cleaning not yet implemented)");
    }

    Ok(())
}

async fn handle_sync(
    session: &dyn Session,
    dry_run: bool,
) -> Result<()> {
    let manager = session.state();

    // Verify we're in a configured state
    let current_state = manager.discover_state().map_err(StateError::from)?;
    if current_state == ModpackState::Uninitialized {
        session.display().status().error("Not in a modpack directory", "");
        session.display().status().subtle("   Run 'empack init' to set up a modpack project");
        return Ok(());
    }

    // Create config manager
    let workdir = session.filesystem().current_dir()?;
    let config_manager = session.filesystem().config_manager(workdir.clone());

    // Load project plan from empack.yml
    let project_plan = config_manager.create_project_plan()
        .context("Failed to load empack.yml configuration")?;

    // Create HTTP client for API requests
    let client = session.network().http_client()?;

    // Get CurseForge API key from app configuration
    let curseforge_api_key = session.config().app_config().curseforge_api_client_key.clone();

    // Create project resolver
    let resolver = session.network().project_resolver(client, curseforge_api_key);

    session.display().status().section("🔄 Synchronizing empack.yml with packwiz");
    session.display().status().info(&format!("Target: {} v{}", project_plan.minecraft_version, project_plan.loader_version));

    // Get currently installed mods
    let installed_mods = match session.filesystem().get_installed_mods() {
        Ok(mods) => {
            session.display().status().info(&format!("📋 Found {} currently installed mods", mods.len()));
            mods
        }
        Err(e) => {
            session.display().status().warning(&format!("Could not read installed mods: {}", e));
            session.display().status().info("Assuming empty pack (add-only mode)");
            HashSet::new()
        }
    };

    // Collect expected mods from empack.yml
    let mut expected_mods = HashSet::new();
    let mut planned_actions = Vec::new();

    // Process each dependency in empack.yml
    for dep_spec in &project_plan.dependencies {
        session.display().status().step(1, project_plan.dependencies.len(), &format!("Processing dependency: {}", dep_spec.key));

        // Normalize the key for comparison with installed mods
        let normalized_key = dep_spec.key
            .to_lowercase()
            .replace(' ', "_")
            .replace('-', "_");
        expected_mods.insert(normalized_key.clone());

        // Check if this mod is already installed
        if installed_mods.contains(&normalized_key) {
            session.display().status().success("Already installed", &dep_spec.key);
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
                ModLoader::Fabric => "fabric",
                ModLoader::Forge => "forge",
                ModLoader::Quilt => "quilt",
                ModLoader::NeoForge => "neoforge",
            });

            match resolver.resolve_project(&dep_spec.search_query, 
                                         Some(match dep_spec.project_type {
                                             ProjectType::Mod => "mod",
                                             ProjectType::Datapack => "datapack",
                                             ProjectType::ResourcePack => "resourcepack",
                                             ProjectType::Shader => "shader",
                                         }), 
                                         minecraft_version, mod_loader).await {
                Ok(project_info) => {
                    session.display().status().success("Resolved", &format!("{} on {}", project_info.title, project_info.platform));
                    
                    // Create appropriate packwiz add command based on platform
                    let command = match project_info.platform {
                        Platform::Modrinth => vec!["mr".to_string(), "add".to_string(), project_info.project_id.clone()],
                        Platform::CurseForge => vec!["cf".to_string(), "add".to_string(), project_info.project_id.clone()],
                        Platform::Forge => vec!["cf".to_string(), "add".to_string(), project_info.project_id.clone()],
                    };
                    
                    (project_info.project_id, command)
                }
                Err(e) => {
                    session.display().status().error("Failed to resolve", &e.to_string());
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
        session.display().status().complete("No changes needed - empack.yml already in sync");
        return Ok(());
    }

    session.display().status().section("📋 Planned Actions");
    for action in &planned_actions {
        match action {
            SyncAction::Add { key, title, command } => {
                session.display().status().info(&format!("➕ Add: {} ({})", title, key));
                if dry_run {
                    session.display().status().subtle(&format!("      Command: packwiz {}", command.join(" ")));
                }
            }
            SyncAction::Remove { key, title } => {
                session.display().status().info(&format!("➖ Remove: {} ({})", title, key));
                if dry_run {
                    session.display().status().subtle(&format!("      Command: packwiz remove {}", key));
                }
            }
        }
    }

    if dry_run {
        session.display().status().complete("Dry run complete - no changes applied");
        return Ok(());
    }

    // Execute planned actions
    session.display().status().section("🚀 Executing sync actions");
    let mut success_count = 0;
    let mut failure_count = 0;

    for action in planned_actions {
        match action {
            SyncAction::Add { key, title, command } => {
                session.display().status().checking(&format!("Adding: {}", title));
                let result = session.process().execute("packwiz", &command.iter().map(|s| s.as_str()).collect::<Vec<_>>(), &workdir)
                    .and_then(|output| {
                        if output.success {
                            Ok(())
                        } else {
                            Err(anyhow::anyhow!("Packwiz command failed: {}", output.stderr))
                        }
                    });
                match result {
                    Ok(_) => {
                        session.display().status().success("Added", "successfully");
                        success_count += 1;
                    }
                    Err(e) => {
                        session.display().status().error("Failed", &e.to_string());
                        failure_count += 1;
                    }
                }
            }
            SyncAction::Remove { key, title: _ } => {
                session.display().status().checking(&format!("Removing: {}", key));
                let result = session.process().execute("packwiz", &["remove", &key], &workdir)
                    .and_then(|output| {
                        if output.success {
                            Ok(())
                        } else {
                            Err(anyhow::anyhow!("Packwiz command failed: {}", output.stderr))
                        }
                    });
                match result {
                    Ok(_) => {
                        session.display().status().success("Removed", "successfully");
                        success_count += 1;
                    }
                    Err(e) => {
                        session.display().status().error("Failed", &e.to_string());
                        failure_count += 1;
                    }
                }
            }
        }
    }

    // Show summary
    session.display().status().section("📊 Sync Summary");
    session.display().status().success("Successful actions", &success_count.to_string());
    session.display().status().info(&format!("Failed actions: {}", failure_count));

    if failure_count == 0 {
        session.display().status().complete("empack.yml synchronized successfully with packwiz");
    } else {
        session.display().status().warning(&format!("Sync completed with {} failures", failure_count));
    }

    Ok(())
}

// Helper functions

/// Parse build targets from string arguments
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

#[cfg(test)]
mod tests {
    include!("commands.test.rs");
}