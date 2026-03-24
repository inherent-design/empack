//! Command execution handlers
//!
//! New session-based architecture for command execution.
//! Implements the Session-Scoped Dependency Injection Pattern.

use crate::Result;
use crate::application::cli::SearchPlatform;
use crate::application::session::{CommandSession, FileSystemProvider, Session};
use crate::application::sync::{
    AddContractError, AddResolution, SyncExecutionAction, SyncPlanAction, build_sync_plan,
    loader_arg, project_type_arg, resolve_add_contract, resolve_sync_action,
};
use crate::empack::config::{DependencyEntry, DependencyRecord, DependencyStatus};
use crate::application::{CliConfig, Commands};
use crate::empack::parsing::ModLoader;
use crate::platform::{ArchiverCapabilities, GoCapabilities};
use crate::primitives::{BuildTarget, PackState, ProjectPlatform, ProjectType, StateTransition};
use anyhow::Context;
use std::collections::{BTreeMap, HashSet};

/// Build an empack.yml string via serde serialization (injection-safe).
fn format_empack_yml(
    name: &str,
    author: &str,
    version: &str,
    minecraft_version: &str,
    loader: &str,
    loader_version: &str,
) -> String {
    let loader_enum = ModLoader::parse(loader).ok();

    // Dedicated struct for init output: includes loader_version (which
    // EmpackProjectConfig doesn't carry) and always emits an empty
    // dependencies map.
    #[derive(serde::Serialize)]
    struct InitEmpackYml<'a> {
        empack: InitFields<'a>,
    }

    #[derive(serde::Serialize)]
    struct InitFields<'a> {
        name: &'a str,
        author: &'a str,
        version: &'a str,
        minecraft_version: &'a str,
        #[serde(skip_serializing_if = "Option::is_none")]
        loader: Option<ModLoader>,
        loader_version: &'a str,
        dependencies: BTreeMap<String, DependencyEntry>,
    }

    let config = InitEmpackYml {
        empack: InitFields {
            name,
            author,
            version,
            minecraft_version,
            loader: loader_enum,
            loader_version,
            dependencies: BTreeMap::new(),
        },
    };

    serde_saphyr::to_string(&config).expect("serializing init config should never fail")
}

/// Execute CLI commands using the new session-based architecture
pub async fn execute_command(config: CliConfig) -> Result<()> {
    // Create command session (owns all ephemeral state)
    let session = CommandSession::new(config.app_config);

    let command = match config.command {
        Some(cmd) => cmd,
        None => {
            session
                .display()
                .status()
                .message("empack - Minecraft modpack management");
            session
                .display()
                .status()
                .subtle("Run 'empack --help' for usage information");
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
        Commands::Init {
            name,
            force,
            modloader,
            mc_version,
            author,
            pack_name,
            loader_version,
            pack_version,
        } => {
            handle_init(
                session, name, pack_name, force, modloader, mc_version, author, loader_version,
                pack_version,
            )
            .await
        }
        Commands::Add {
            mods,
            force,
            platform,
        } => handle_add(session, mods, force, platform).await,
        Commands::Remove { mods, deps } => handle_remove(session, mods, deps).await,
        Commands::Build {
            targets,
            clean,
            jobs: _,
        } => handle_build(session, targets, clean).await,
        Commands::Clean { targets } => handle_clean(session, targets).await,
        Commands::Sync {} => handle_sync(session).await,
    }
}

// Session-based command handlers using dependency injection pattern
async fn handle_requirements(session: &dyn Session) -> Result<()> {
    session
        .display()
        .status()
        .section("Checking tool dependencies");

    // Check packwiz
    let workdir = session.filesystem().current_dir().unwrap_or_default();
    let packwiz = crate::empack::packwiz::check_packwiz_available(session.process(), &workdir);
    match packwiz {
        Ok((true, version)) => {
            session.display().status().success("packwiz", &version);
        }
        _ => {
            session.display().status().error("packwiz", "not found");
            session
                .display()
                .status()
                .subtle("   Install from: https://packwiz.infra.link/installation/");
            if GoCapabilities::detect().available {
                session
                    .display()
                    .status()
                    .subtle("   Or via Go: go install github.com/packwiz/packwiz@latest");
            }
        }
    }

    // Check archiving capabilities
    let create_caps = ArchiverCapabilities::detect_creation();
    let extract_caps = ArchiverCapabilities::detect_extraction();

    let can_create = create_caps.iter().any(|p| p.available);
    let can_extract = extract_caps.iter().any(|p| p.available);

    if can_create {
        let available: Vec<String> = create_caps
            .iter()
            .filter(|p| p.available)
            .map(|p| p.name.clone())
            .collect();
        session.display().status().success(
            "archive creation",
            &format!("{} available", available.join(", ")),
        );
    } else {
        session
            .display()
            .status()
            .error("archive creation", "no tools found");
    }

    if can_extract {
        let available: Vec<String> = extract_caps
            .iter()
            .filter(|p| p.available)
            .map(|p| p.name.clone())
            .collect();
        session.display().status().success(
            "archive extraction",
            &format!("{} available", available.join(", ")),
        );
    } else {
        session
            .display()
            .status()
            .error("archive extraction", "no tools found");
    }

    Ok(())
}

async fn handle_version(session: &dyn Session) -> Result<()> {
    session
        .display()
        .status()
        .emphasis(&format!("empack {}", env!("CARGO_PKG_VERSION")));
    session
        .display()
        .status()
        .message("A Minecraft modpack development and distribution tool");
    session.display().status().message("");

    let build_info = [
        (
            "Built from commit",
            option_env!("GIT_HASH").unwrap_or("unknown"),
        ),
        ("Build date", option_env!("BUILD_DATE").unwrap_or("unknown")),
        ("Target", std::env::consts::ARCH),
    ];

    session.display().table().properties(&build_info);

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn handle_init(
    session: &dyn Session,
    positional_name: Option<String>,
    cli_pack_name: Option<String>,
    force: bool,
    cli_modloader: Option<String>,
    cli_mc_version: Option<String>,
    cli_author: Option<String>,
    cli_loader_version: Option<String>,
    cli_pack_version: Option<String>,
) -> Result<()> {
    // Handle directory creation case: `empack init <name>` where <name> is a directory
    // Precedence: positional arg > --name flag
    let name = positional_name.or(cli_pack_name.clone());
    let (mut target_dir, initial_name, mut needs_mkdir) = if let Some(name) = name {
        let potential_dir = session
            .config()
            .app_config()
            .workdir
            .as_ref()
            .unwrap_or(&session.filesystem().current_dir()?)
            .join(&name);

        let needs_mkdir = !session.filesystem().exists(&potential_dir);
        (potential_dir, Some(name), needs_mkdir)
    } else {
        let workdir = match session.config().app_config().workdir.as_ref().cloned() {
            Some(w) => w,
            None => session
                .filesystem()
                .current_dir()
                .context("Failed to get current directory")?,
        };
        (workdir, None, false)
    };

    // Track whether the target directory already contains a project.
    // When true and initial_name is None, the user wants in-place reinit,
    // so we should NOT retarget to a subdirectory after the interactive prompt.
    let mut existing_project_in_cwd = false;

    // Check state only if the directory already exists
    if !needs_mkdir {
        let manager =
            crate::empack::state::PackStateManager::new(target_dir.clone(), session.filesystem());

        let mut current_state = manager.discover_state()?;
        if current_state != PackState::Uninitialized {
            existing_project_in_cwd = initial_name.is_none();
            if !force {
                session
                    .display()
                    .status()
                    .error("Directory already contains a modpack project", "");
                session
                    .display()
                    .status()
                    .subtle("   Use --force to overwrite existing files");
                return Ok(());
            }

            session
                .display()
                .status()
                .checking("Resetting existing project state for --force init");

            while current_state != PackState::Uninitialized {
                let result = manager
                    .execute_transition(
                        session.process(),
                        &*session.packwiz(),
                        StateTransition::Clean,
                    )
                    .await
                    .context("Failed to reset existing project before initialization")?;
                for w in &result.warnings {
                    session.display().status().warning(w);
                }
                current_state = result.state;
            }
        }
    }

    session
        .display()
        .status()
        .section("Initializing modpack project");

    // Get default name from directory or command line
    let default_name = initial_name
        .as_deref()
        .unwrap_or(
            target_dir
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("Pack"),
        )
        .to_string();

    // Interactive prompt for modpack configuration (or use CLI flag)
    let modpack_name = if let Some(name) = cli_pack_name.clone() {
        session
            .display()
            .status()
            .info(&format!("Using name: {}", name));
        name
    } else {
        session
            .interactive()
            .text_input("Modpack name", default_name)?
    };

    // When no name was provided via CLI and the cwd is not already a project,
    // the interactively-entered name determines the target subdirectory
    // (like `empack init <name>` would).
    if initial_name.is_none() && !existing_project_in_cwd {
        let new_target = target_dir.join(&modpack_name);
        needs_mkdir = !session.filesystem().exists(&new_target);

        // If the new target exists and already has a modpack, check state
        if !needs_mkdir {
            let new_manager =
                crate::empack::state::PackStateManager::new(new_target.clone(), session.filesystem());
            let new_state = new_manager.discover_state()?;
            if new_state != PackState::Uninitialized {
                if !force {
                    session.display().status().error(
                        "Directory already contains a modpack project",
                        &new_target.display().to_string(),
                    );
                    session
                        .display()
                        .status()
                        .subtle("   Use --force to overwrite existing files");
                    return Ok(());
                }
                // Force path: clean existing state
                session
                    .display()
                    .status()
                    .checking("Resetting existing project state for --force init");
                let mut current_state = new_state;
                while current_state != PackState::Uninitialized {
                    let result = new_manager
                        .execute_transition(
                            session.process(),
                            &*session.packwiz(),
                            StateTransition::Clean,
                        )
                        .await
                        .context("Failed to reset existing project before initialization")?;
                    for w in &result.warnings {
                        session.display().status().warning(w);
                    }
                    current_state = result.state;
                }
            }
        }

        target_dir = new_target;
    }

    // Try to get git user.name as smart default.
    // Use parent dir (or target_dir itself) as cwd because target_dir
    // may not exist yet when needs_mkdir is true.
    let git_cwd = if needs_mkdir {
        target_dir.parent().unwrap_or(&target_dir).to_path_buf()
    } else {
        target_dir.clone()
    };
    let default_author = session
        .process()
        .execute("git", &["config", "user.name"], &git_cwd)
        .ok()
        .and_then(|output| {
            if output.success {
                Some(output.stdout)
            } else {
                None
            }
        })
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Unknown Author".to_string());

    let author = if let Some(author) = cli_author {
        session
            .display()
            .status()
            .info(&format!("Using author: {}", author));
        author
    } else {
        session.interactive().text_input("Author", default_author)?
    };

    let version = if let Some(v) = cli_pack_version {
        session
            .display()
            .status()
            .info(&format!("Using pack version: {}", v));
        v
    } else {
        session
            .interactive()
            .text_input("Version", "1.0.0".to_string())?
    };

    // Create version fetcher for dynamic version discovery
    let version_fetcher =
        crate::empack::versions::VersionFetcher::new(session.network(), session.filesystem())?;

    // Fetch Minecraft versions with network status
    session
        .display()
        .status()
        .info("Fetching Minecraft versions...");
    let minecraft_versions = match version_fetcher.fetch_minecraft_versions().await {
        Ok(versions) => {
            session
                .display()
                .status()
                .success("Found", &format!("{} Minecraft versions", versions.len()));
            versions
        }
        Err(e) => {
            session
                .display()
                .status()
                .warning(&format!("Network fetch failed: {}", e));
            session.display().status().info("Using fallback versions");
            crate::empack::versions::VersionFetcher::get_fallback_minecraft_versions()
        }
    };

    // Minecraft version selection with FuzzySelect (pagination enabled, 6 items per page)
    let minecraft_version = if let Some(mc_ver) = cli_mc_version {
        session
            .display()
            .status()
            .info(&format!("Using Minecraft version: {}", mc_ver));
        if !minecraft_versions.iter().any(|v| v.eq_ignore_ascii_case(&mc_ver)) {
            anyhow::bail!(
                "Minecraft version '{}' not found. Available versions include: {}",
                mc_ver,
                minecraft_versions.iter().take(5).cloned().collect::<Vec<_>>().join(", ")
            );
        }
        mc_ver
    } else {
        let mc_version_index = session
            .interactive()
            .fuzzy_select("Minecraft version", &minecraft_versions)?
            .ok_or_else(|| anyhow::anyhow!("Minecraft version selection cancelled"))?;
        minecraft_versions[mc_version_index].clone()
    };

    // Step 3: Dynamic, Filtered Mod Loader Prompt
    session
        .display()
        .status()
        .info("Finding compatible mod loaders...");
    let compatible_loaders = match version_fetcher
        .fetch_compatible_loaders(&minecraft_version)
        .await
    {
        Ok(loaders) => {
            session
                .display()
                .status()
                .success("Found", &format!("{} compatible loaders", loaders.len()));
            // Debug: Show which loaders were found compatible
            let loader_names: Vec<String> =
                loaders.iter().map(|l| l.as_str().to_string()).collect();
            session
                .display()
                .status()
                .subtle(&format!("Compatible: {}", loader_names.join(", ")));
            loaders
        }
        Err(e) => {
            session
                .display()
                .status()
                .warning(&format!("Compatibility check failed: {}", e));
            session
                .display()
                .status()
                .info("Using all loaders as fallback");
            vec![
                crate::empack::versions::ModLoader::NeoForge,
                crate::empack::versions::ModLoader::Fabric,
                crate::empack::versions::ModLoader::Forge,
                crate::empack::versions::ModLoader::Quilt,
            ]
        }
    };

    // Loader compatibility is already filtered by fetch_compatible_loaders above.
    // Loaders that 404 for a given MC version are excluded from the list.

    // If no compatible loaders found, inform user and exit gracefully
    if compatible_loaders.is_empty() {
        session.display().status().error(
            "No compatible mod loaders found",
            &format!("for Minecraft {}", minecraft_version),
        );
        session
            .display()
            .status()
            .subtle("   Try selecting a different Minecraft version");
        return Ok(());
    }

    // Present filtered loader list with intelligent priority
    let (selected_loader, loader_str) = if let Some(loader_str) = cli_modloader {
        session
            .display()
            .status()
            .info(&format!("Using loader: {}", loader_str));
        let parsed_loader = ModLoader::parse(&loader_str)
            .with_context(|| format!("Invalid mod loader: {}", loader_str))?;

        let versions_loader: crate::empack::versions::ModLoader = parsed_loader.into();

        if !compatible_loaders.contains(&versions_loader) {
            let available: Vec<&str> = compatible_loaders.iter().map(|l| l.as_str()).collect();
            anyhow::bail!(
                "Loader '{}' is not compatible with Minecraft {}. Compatible loaders: {}",
                loader_str,
                minecraft_version,
                available.join(", ")
            );
        }

        let loader_str = versions_loader.as_str().to_string();
        (versions_loader, loader_str)
    } else {
        let loader_names: Vec<String> = compatible_loaders
            .iter()
            .map(|l| l.as_str().to_string())
            .collect();
        let loader_name_refs: Vec<&str> = loader_names.iter().map(|s| s.as_str()).collect();
        let loader_index = session
            .interactive()
            .select("Mod loader", &loader_name_refs)?;
        let selected_loader = &compatible_loaders[loader_index];
        (
            selected_loader.clone(),
            selected_loader.as_str().to_string(),
        )
    };

    // Step 4: Dynamic, Searchable Loader Version Prompt
    session.display().status().info(&format!(
        "Fetching {} versions for Minecraft {}...",
        loader_str, minecraft_version
    ));
    let loader_versions = match &selected_loader {
        crate::empack::versions::ModLoader::Fabric => {
            match version_fetcher
                .fetch_fabric_loader_versions(&minecraft_version)
                .await
            {
                Ok(versions) => {
                    session
                        .display()
                        .status()
                        .success("Found", &format!("{} Fabric versions", versions.len()));
                    versions
                }
                Err(e) => {
                    session
                        .display()
                        .status()
                        .warning(&format!("Network fetch failed: {}", e));
                    session.display().status().info("Using fallback versions");
                    crate::empack::versions::VersionFetcher::get_fallback_loader_versions(
                        "fabric",
                        &minecraft_version,
                    )
                }
            }
        }
        crate::empack::versions::ModLoader::NeoForge => {
            match version_fetcher
                .fetch_neoforge_loader_versions(&minecraft_version)
                .await
            {
                Ok(versions) => {
                    session
                        .display()
                        .status()
                        .success("Found", &format!("{} NeoForge versions", versions.len()));
                    versions
                }
                Err(e) => {
                    session
                        .display()
                        .status()
                        .warning(&format!("Network fetch failed: {}", e));
                    session.display().status().info("Using fallback versions");
                    crate::empack::versions::VersionFetcher::get_fallback_loader_versions(
                        "neoforge",
                        &minecraft_version,
                    )
                }
            }
        }
        crate::empack::versions::ModLoader::Forge => {
            match version_fetcher
                .fetch_forge_loader_versions(&minecraft_version)
                .await
            {
                Ok(versions) => {
                    session
                        .display()
                        .status()
                        .success("Found", &format!("{} Forge versions", versions.len()));
                    versions
                }
                Err(e) => {
                    session
                        .display()
                        .status()
                        .warning(&format!("Network fetch failed: {}", e));
                    session.display().status().info("Using fallback versions");
                    crate::empack::versions::VersionFetcher::get_fallback_loader_versions(
                        "forge",
                        &minecraft_version,
                    )
                }
            }
        }
        crate::empack::versions::ModLoader::Quilt => {
            match version_fetcher
                .fetch_quilt_loader_versions(&minecraft_version)
                .await
            {
                Ok(versions) => {
                    session
                        .display()
                        .status()
                        .success("Found", &format!("{} Quilt versions", versions.len()));
                    versions
                }
                Err(e) => {
                    session
                        .display()
                        .status()
                        .warning(&format!("Network fetch failed: {}", e));
                    session.display().status().info("Using fallback versions");
                    crate::empack::versions::VersionFetcher::get_fallback_loader_versions(
                        "quilt",
                        &minecraft_version,
                    )
                }
            }
        }
    };

    // Each loader's fetch function already filters by MC version.
    // Empty results are caught by the is_empty() check below.

    // Loader version selection with FuzzySelect (pagination enabled, 6 items per page)
    let loader_version = if loader_versions.is_empty() {
        return Err(anyhow::anyhow!(
            "No {} versions available for Minecraft {}",
            loader_str,
            minecraft_version
        ));
    } else if let Some(lv) = cli_loader_version {
        session
            .display()
            .status()
            .info(&format!("Using {} version: {}", loader_str, lv));
        if !loader_versions.iter().any(|v| v == &lv) {
            anyhow::bail!(
                "Loader version '{}' not found for {} on Minecraft {}. Available versions include: {}",
                lv,
                loader_str,
                minecraft_version,
                loader_versions.iter().take(5).cloned().collect::<Vec<_>>().join(", ")
            );
        }
        lv
    } else {
        let loader_version_index = session
            .interactive()
            .fuzzy_select(&format!("{} version", loader_str), &loader_versions)?
            .ok_or_else(|| anyhow::anyhow!("Loader version selection cancelled"))?;
        loader_versions[loader_version_index].clone()
    };

    // Step 5: Final Confirmation and Execution
    session.display().status().info("Configuration Summary:");
    session
        .display()
        .status()
        .info(&format!("   Name: {}", modpack_name));
    session
        .display()
        .status()
        .info(&format!("   Author: {}", author));
    session
        .display()
        .status()
        .info(&format!("   Version: {}", version));
    session
        .display()
        .status()
        .info(&format!("   Minecraft: {}", minecraft_version));
    session
        .display()
        .status()
        .info(&format!("   Loader: {} v{}", loader_str, loader_version));

    // Final confirmation
    let confirmed = session
        .interactive()
        .confirm("Create modpack with these settings?", true)?;

    if !confirmed {
        session
            .display()
            .status()
            .info("Modpack initialization cancelled");
        return Ok(());
    }

    validate_init_inputs(
        &minecraft_version,
        &minecraft_versions,
        &loader_str,
        &compatible_loaders,
        &loader_version,
        &loader_versions,
    )?;

    // === Execute phase: all filesystem mutations happen below this line ===

    let created_dir = needs_mkdir;

    // Create directory if needed (deferred from path resolution)
    if needs_mkdir {
        let dir_name = target_dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("target");
        session
            .display()
            .status()
            .info(&format!("Creating directory: {}", dir_name));
        session.filesystem().create_dir_all(&target_dir)?;
    }

    let init_config = crate::primitives::InitializationConfig {
        name: &modpack_name,
        author: &author,
        version: &version,
        modloader: &loader_str,
        mc_version: &minecraft_version,
        loader_version: &loader_version,
    };

    let result = execute_init_phase(session, &target_dir, &init_config).await;

    if let Err(ref e) = result
        && created_dir
        && session.filesystem().is_directory(&target_dir)
    {
        let _ = session.filesystem().remove_dir_all(&target_dir);
        session
            .display()
            .status()
            .warning(&format!("Cleaned up directory after init failure: {}", e));
    }

    result
}

async fn execute_init_phase(
    session: &dyn Session,
    target_dir: &std::path::Path,
    config: &crate::primitives::InitializationConfig<'_>,
) -> Result<()> {
    let manager =
        crate::empack::state::PackStateManager::new(target_dir.to_path_buf(), session.filesystem());

    let empack_yml_content = format_empack_yml(
        config.name,
        config.author,
        config.version,
        config.mc_version,
        config.modloader,
        config.loader_version,
    );

    session
        .filesystem()
        .write_file(&target_dir.join("empack.yml"), &empack_yml_content)?;

    let transition_result = manager
        .execute_transition(session.process(), &*session.packwiz(), StateTransition::Initialize(*config))
        .await
        .context("Failed to initialize modpack project")?;
    for w in &transition_result.warnings {
        session.display().status().warning(w);
    }

    match transition_result.state {
        PackState::Configured => {
            session
                .display()
                .status()
                .complete("Modpack project initialized successfully");

            let next_steps = [
                "Run 'empack add <mod>' to add mods interactively",
                "Or edit empack.yml directly for bulk dependency configuration",
                "Run 'empack sync' to resolve and download dependencies",
                "Run 'empack build all' to build distribution packages",
            ];
            session.display().status().list(&next_steps);
        }
        _ => {
            return Err(anyhow::anyhow!(
                "Unexpected state after initialization: {:?}",
                transition_result.state
            ));
        }
    }

    Ok(())
}

/// Validate that all init inputs are consistent with the fetched version lists.
/// Called as a final checkpoint before executing packwiz init.
fn validate_init_inputs(
    mc_version: &str,
    minecraft_versions: &[String],
    loader_str: &str,
    compatible_loaders: &[crate::empack::versions::ModLoader],
    loader_version: &str,
    loader_versions: &[String],
) -> Result<()> {
    if !minecraft_versions.iter().any(|v| v.eq_ignore_ascii_case(mc_version)) {
        anyhow::bail!(
            "Minecraft version '{}' not found in available versions",
            mc_version
        );
    }

    let parsed_loader = ModLoader::parse(loader_str)
        .with_context(|| format!("Invalid mod loader: {}", loader_str))?;
    let versions_loader: crate::empack::versions::ModLoader = parsed_loader.into();
    if !compatible_loaders.contains(&versions_loader) {
        let available: Vec<&str> = compatible_loaders.iter().map(|l| l.as_str()).collect();
        anyhow::bail!(
            "Loader '{}' is not compatible with Minecraft {}. Compatible: {}",
            loader_str,
            mc_version,
            available.join(", ")
        );
    }

    if !loader_versions.iter().any(|v| v == loader_version) {
        anyhow::bail!(
            "Loader version '{}' not found for {} on Minecraft {}",
            loader_version,
            loader_str,
            mc_version
        );
    }

    Ok(())
}

/// Handle `empack add` command - search, resolve, and install projects
///
/// ## Packwiz Integration Strategy
///
/// This function uses **direct ProcessProvider.execute()** to invoke packwiz CLI commands
/// rather than the PackwizMetadata wrapper (defined in empack/packwiz.rs).
///
/// **Design Rationale:**
/// - Single-command operations: Each mod is added via one packwiz invocation
/// - Simplicity advantage: Direct CLI is ~17 lines vs wrapper's ~35 lines
/// - No state management: Commands don't need cached availability checks
/// - Computational desperation: Minimal abstractions until complexity justifies them
/// - Resolution and command-planning errors stay typed at the shared seam; packwiz execution remains local here
///
/// **When to use PackwizMetadata wrapper instead:**
/// PackwizMetadata should be integrated IF future commands need:
/// - Cached availability checks across multiple invocations
/// - Structured error parsing (HashMismatch, PackFormat detection)
/// - Transactional behavior with rollback on failure
/// - Complex multi-step validation (refresh_index, export_mrpack)
/// - Better test isolation (mock wrapper instead of ProcessProvider)
///
/// **See also:** packwiz.rs module documentation for usage patterns
async fn handle_add(
    session: &dyn Session,
    mods: Vec<String>,
    force: bool,
    platform: Option<SearchPlatform>,
) -> Result<()> {
    // Migrate from legacy handle_add - using session providers

    if mods.is_empty() {
        session
            .display()
            .status()
            .error("No mods specified to add", "");
        session
            .display()
            .status()
            .subtle("   Usage: empack add <mod1> [mod2] [mod3]...");
        return Ok(());
    }

    let manager = session.state()?;

    // Verify we're in a configured state
    let current_state = manager
        .discover_state()
        .map_err(|e| anyhow::anyhow!("State error: {:?}", e))?;
    if current_state == crate::primitives::PackState::Uninitialized {
        session
            .display()
            .status()
            .error("Not in a modpack directory", "");
        session
            .display()
            .status()
            .subtle("   Run 'empack init' to set up a modpack project");
        return Ok(());
    }
    if current_state == PackState::Configured && !manager.validate_state(PackState::Configured)? {
        session
            .display()
            .status()
            .error("Project initialization is incomplete", "");
        session.display().status().subtle(
            "   Re-run 'empack init --force' to restore empack.yml and pack/ metadata before adding dependencies",
        );
        return Ok(());
    }

    let workdir = manager.workdir.clone();
    let config_manager = session.filesystem().config_manager(workdir.clone());

    // Build dependency graph from existing mods to detect duplicates and cycles
    let mut dep_graph = crate::api::dependency_graph::DependencyGraph::new();
    let mods_dir = workdir.join("pack").join("mods");

    if mods_dir.exists()
        && let Err(e) = dep_graph.build_from_directory(&mods_dir)
    {
        session
            .display()
            .status()
            .warning(&format!("Failed to build dependency graph: {}", e));
    }

    // Try to load existing project plan to get context
    let project_plan = match config_manager.create_project_plan() {
        Ok(plan) => Some(plan),
        Err(_) => {
            session
                .display()
                .status()
                .warning("No empack.yml found, using defaults");
            None
        }
    };

    // Create HTTP client for API requests
    let client = session.network().http_client()?;

    // Get CurseForge API key from app configuration
    let curseforge_api_key = session
        .config()
        .app_config()
        .curseforge_api_client_key
        .clone();

    // Create project resolver
    let resolver = session
        .network()
        .project_resolver(client, curseforge_api_key);

    session
        .display()
        .status()
        .section(&format!("Adding {} mod(s) to modpack", mods.len()));

    let mut added_mods = Vec::new();
    let mut failed_mods: Vec<(String, String)> = Vec::new();

    // === Gather phase: resolve all mods, no side effects ===
    let mut resolved_mods: Vec<ResolvedMod> = Vec::new();
    let mut batch_project_ids: std::collections::HashSet<String> =
        std::collections::HashSet::new();

    for mod_query in mods {
        let resolution_intent = AddResolutionIntent::from_cli_input(&mod_query, platform.clone());
        session
            .display()
            .status()
            .checking(&format!("Resolving mod: {}", mod_query));

        // Use project plan context if available
        let minecraft_version = project_plan.as_ref().map(|p| p.minecraft_version.as_str());
        let mod_loader = project_plan.as_ref().map(|p| p.loader);

        // For CLI add, we resolve via search (empty project_id triggers search path)
        let direct_project_id = resolution_intent.direct_project_id.as_deref().unwrap_or("");
        let direct_platform = resolution_intent.direct_platform.unwrap_or(ProjectPlatform::Modrinth);

        match resolve_add_contract(
            &resolution_intent.search_query,
            crate::primitives::ProjectType::Mod,
            minecraft_version,
            mod_loader,
            direct_project_id,
            direct_platform,
            None,
            resolution_intent.preferred_platform,
            resolver.as_ref(),
        )
        .await
        {
            Ok(resolution) => {
                let status_label = if resolution_intent.direct_project_id.is_some() {
                    "Using direct project ID"
                } else {
                    "Found"
                };
                session.display().status().success(
                    status_label,
                    &format!("{} on {}", resolution.title, resolution.resolved_platform),
                );
                if let Some(confidence) = resolution.confidence {
                    session
                        .display()
                        .status()
                        .info(&format!("Confidence: {}%", confidence));
                }

                // Check for duplicate mod in existing dep graph (unless --force flag is set)
                if !force && dep_graph.contains(&resolution.resolved_project_id) {
                    session.display().status().warning(&format!(
                        "Mod already installed: {} (use --force to reinstall)",
                        resolution.title
                    ));
                    continue; // Skip this mod
                }

                // Check for duplicate within this batch
                if !force && batch_project_ids.contains(&resolution.resolved_project_id) {
                    session.display().status().warning(&format!(
                        "Duplicate in batch: {} (already queued for addition)",
                        resolution.title
                    ));
                    continue;
                }

                batch_project_ids.insert(resolution.resolved_project_id.clone());
                // Use the mod query lowercased as slug key
                let dep_key = mod_query.to_lowercase().replace(' ', "-");
                resolved_mods.push(ResolvedMod {
                    query: mod_query,
                    resolution,
                    dep_key,
                });
            }
            Err(e) => {
                let rendered = render_add_contract_error(&e);
                session
                    .display()
                    .status()
                    .error(&rendered.item, &rendered.details);
                failed_mods.push((mod_query, rendered.details));
            }
        }
    }

    if session.config().app_config().dry_run {
        session.display().status().section("Planned Actions");
        for resolved in &resolved_mods {
            session.display().status().info(&format!(
                "Would add: {} ({} on {})",
                resolved.resolution.title,
                resolved.resolution.resolved_project_id,
                resolved.resolution.resolved_platform,
            ));
        }
        session
            .display()
            .status()
            .complete("Dry run complete - no changes applied");
        return Ok(());
    }

    // === Execute phase: all side effects happen below this line ===
    for resolved in resolved_mods {
        // Snapshot .pw.toml slugs before packwiz add so we can diff after
        let before_slugs = scan_pw_toml_slugs(session.filesystem(), &mods_dir);

        let mut packwiz_result: std::result::Result<(), ()> = Ok(());
        let mut last_error = None;
        for command in &resolved.resolution.commands {
            match session.process().execute(
                "packwiz",
                &command.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
                &workdir.join("pack"),
            ) {
                Ok(output) if output.success => {
                    packwiz_result = Ok(());
                    last_error = None;
                    break;
                }
                Ok(output) => {
                    packwiz_result = Err(());
                    last_error =
                        Some(anyhow::anyhow!("Packwiz command failed: {}", output.stderr));
                }
                Err(error) => {
                    packwiz_result = Err(());
                    last_error = Some(anyhow::anyhow!(error));
                }
            }
        }

        match packwiz_result {
            Ok(_) => {
                session
                    .display()
                    .status()
                    .success("Successfully added to pack", "");

                // Derive dep_key from the actual .pw.toml file that packwiz created,
                // rather than from user input which may diverge from the registry slug.
                let dep_key = discover_dep_key(
                    session.filesystem(),
                    &mods_dir,
                    &before_slugs,
                    &resolved.dep_key,
                    session.display(),
                );

                // Update dependency graph with newly added mod
                // Rebuild from directory to capture new .pw.toml files
                let mut updated_graph =
                    crate::api::dependency_graph::DependencyGraph::new();
                if let Err(e) = updated_graph.build_from_directory(&mods_dir) {
                    session
                        .display()
                        .status()
                        .warning(&format!("Failed to update dependency graph: {}", e));
                } else {
                    // Check for cycles introduced by new mod
                    if updated_graph.has_cycles()
                        && let Some(cycle) = updated_graph.detect_cycle()
                    {
                        let p = crate::primitives::terminal::primitives();
                        let arrow_sep = format!(" {} ", p.arrow);
                        session
                            .display()
                            .status()
                            .error("Circular dependency detected", &cycle.join(&arrow_sep));
                        session
                            .display()
                            .status()
                            .warning("Installation may fail - consider removing conflicting mods");
                    }
                }

                // Atomically update empack.yml with the new dependency
                let record = DependencyRecord {
                    status: DependencyStatus::Resolved,
                    title: resolved.resolution.title.clone(),
                    platform: resolved.resolution.resolved_platform,
                    project_id: resolved.resolution.resolved_project_id.clone(),
                    project_type: crate::primitives::ProjectType::Mod,
                    version: None,
                };
                if let Err(e) = config_manager.add_dependency(
                    &dep_key,
                    record,
                ) {
                    session
                        .display()
                        .status()
                        .warning(&format!("Failed to update empack.yml: {}", e));
                }

                added_mods.push((resolved.query, resolved.resolution));
            }
            Err(_) => {
                let e = last_error
                    .unwrap_or_else(|| anyhow::anyhow!("Unknown packwiz add failure"));
                session
                    .display()
                    .status()
                    .error("Failed to add to pack", &e.to_string());
                failed_mods.push((resolved.query, format!("Packwiz error: {}", e)));
            }
        }
    }

    // Show summary
    session.display().status().section("Add Summary");
    session
        .display()
        .status()
        .success("Successfully added", &added_mods.len().to_string());
    session
        .display()
        .status()
        .info(&format!("Failed: {}", failed_mods.len()));

    if !failed_mods.is_empty() {
        session.display().status().section("Failed mods");
        for (mod_name, error) in failed_mods {
            session.display().status().error(&mod_name, &error);
        }
    }

    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RenderedStatusError {
    item: String,
    details: String,
}

// Keep command-specific rendering local so the shared add/sync seam can return
// typed failures without forcing one user-facing message contract on both commands.
fn render_add_contract_error(error: &AddContractError) -> RenderedStatusError {
    let item = match error {
        AddContractError::ResolveProject { .. } => "Failed to resolve mod",
        AddContractError::PlanPackwizAdd { .. } => "Failed to prepare add command",
    };

    RenderedStatusError {
        item: item.to_string(),
        details: render_add_contract_error_details(error),
    }
}

fn render_add_contract_error_details(error: &AddContractError) -> String {
    match error {
        AddContractError::ResolveProject { query, source } => format!("{query}: {source}"),
        AddContractError::PlanPackwizAdd {
            project_id,
            platform,
            source,
        } => format!("{platform} project {project_id}: {source}"),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AddResolutionIntent {
    search_query: String,
    direct_project_id: Option<String>,
    direct_platform: Option<ProjectPlatform>,
    preferred_platform: Option<ProjectPlatform>,
}

impl AddResolutionIntent {
    fn from_cli_input(mod_query: &str, platform: Option<SearchPlatform>) -> Self {
        let preferred_platform = match platform {
            Some(SearchPlatform::Modrinth) => Some(ProjectPlatform::Modrinth),
            Some(SearchPlatform::Curseforge) => Some(ProjectPlatform::CurseForge),
            Some(SearchPlatform::Both) | None => None,
        };

        // Auto-detect only CurseForge numeric IDs (unambiguous: all-digit strings).
        // Modrinth IDs (8-char base62) are too easily confused with mod names
        // (e.g. "faithful", "optifine"). Users must pass --platform modrinth
        // for direct Modrinth ID lookup.
        let (direct_project_id, direct_platform) = match preferred_platform {
            Some(ProjectPlatform::CurseForge) if is_curseforge_project_id(mod_query) => {
                (Some(mod_query.to_string()), Some(ProjectPlatform::CurseForge))
            }
            None if is_curseforge_project_id(mod_query) => {
                (Some(mod_query.to_string()), Some(ProjectPlatform::CurseForge))
            }
            _ => (None, None),
        };

        Self {
            search_query: mod_query.to_string(),
            direct_project_id,
            direct_platform,
            preferred_platform,
        }
    }
}

/// Scan a directory for `.pw.toml` files and extract their slugs.
///
/// Replicates the slug-extraction logic from `PackwizOps::get_installed_mods`
/// but operates directly on the filesystem provider so we can snapshot before/after
/// a packwiz command without requiring stateful mocks.
fn scan_pw_toml_slugs(
    filesystem: &dyn FileSystemProvider,
    mods_dir: &std::path::Path,
) -> HashSet<String> {
    if !filesystem.exists(mods_dir) {
        return HashSet::new();
    }
    let file_list = match filesystem.get_file_list(mods_dir) {
        Ok(list) => list,
        Err(_) => return HashSet::new(),
    };
    let mut slugs = HashSet::new();
    for path in &file_list {
        if path.extension().and_then(|e| e.to_str()) == Some("toml")
            && let Some(stem) = path.file_stem().and_then(|s| s.to_str())
        {
            let slug = stem.strip_suffix(".pw").unwrap_or(stem);
            slugs.insert(slug.to_string());
        }
    }
    slugs
}

/// After a successful `packwiz add`, discover the dep_key by diffing the `.pw.toml`
/// files before and after the command. Falls back to the input-derived key if the
/// diff is empty or ambiguous.
fn discover_dep_key(
    filesystem: &dyn FileSystemProvider,
    mods_dir: &std::path::Path,
    before_slugs: &HashSet<String>,
    fallback_key: &str,
    display: &dyn crate::display::DisplayProvider,
) -> String {
    let after_slugs = scan_pw_toml_slugs(filesystem, mods_dir);
    let new_slugs: Vec<&String> = after_slugs.difference(before_slugs).collect();
    match new_slugs.len() {
        1 => new_slugs[0].clone(),
        0 => {
            // No new file detected — packwiz may have updated an existing file
            display.status().subtle(&format!(
                "Could not detect new .pw.toml file; using '{}' as dependency key",
                fallback_key
            ));
            fallback_key.to_string()
        }
        _ => {
            // Multiple new files — ambiguous, use fallback
            display.status().subtle(&format!(
                "Multiple new .pw.toml files detected; using '{}' as dependency key",
                fallback_key
            ));
            fallback_key.to_string()
        }
    }
}

/// Holds a fully-resolved mod ready for the execute phase of handle_add.
/// Separating resolution (network + user interaction) from execution (file writes)
/// ensures that a Ctrl+C during the gather phase leaves no files modified.
struct ResolvedMod {
    query: String,
    resolution: AddResolution,
    dep_key: String,
}

fn is_curseforge_project_id(value: &str) -> bool {
    !value.is_empty() && value.chars().all(|c| c.is_ascii_digit())
}

async fn handle_remove(session: &dyn Session, mods: Vec<String>, deps: bool) -> Result<()> {
    if mods.is_empty() {
        session
            .display()
            .status()
            .error("No mods specified to remove", "");
        session
            .display()
            .status()
            .subtle("   Usage: empack remove <mod1> [mod2] [mod3]...");
        return Ok(());
    }

    let manager = session.state()?;

    // Verify we're in a configured state
    let current_state = manager.discover_state()?;
    if current_state == PackState::Uninitialized {
        session
            .display()
            .status()
            .error("Not in a modpack directory", "");
        session
            .display()
            .status()
            .subtle("   Run 'empack init' to set up a modpack project");
        return Ok(());
    }
    if current_state == PackState::Configured && !manager.validate_state(PackState::Configured)? {
        session
            .display()
            .status()
            .error("Project initialization is incomplete", "");
        session.display().status().subtle(
            "   Re-run 'empack init --force' to restore empack.yml and pack/ metadata before removing dependencies",
        );
        return Ok(());
    }

    session
        .display()
        .status()
        .section(&format!("Removing {} mod(s) from modpack", mods.len()));

    let workdir = manager.workdir.clone();
    let mods_dir = workdir.join("pack").join("mods");
    let config_manager = session.filesystem().config_manager(workdir.clone());
    let mut removed_mods = Vec::new();
    let mut failed_mods = Vec::new();

    // === Gather phase: validate mod names, no side effects ===
    let validated_mods: Vec<String> = mods
        .into_iter()
        .filter(|name| {
            if name.trim().is_empty() {
                session
                    .display()
                    .status()
                    .warning("Skipping empty mod name");
                false
            } else {
                true
            }
        })
        .collect();

    if session.config().app_config().dry_run {
        session.display().status().section("Planned Actions");
        for mod_name in &validated_mods {
            session
                .display()
                .status()
                .info(&format!("Would remove: {}", mod_name));
        }
        session
            .display()
            .status()
            .complete("Dry run complete - no changes applied");
        return Ok(());
    }

    // === Execute phase: all side effects happen below this line ===
    for mod_name in validated_mods {
        session
            .display()
            .status()
            .checking(&format!("Removing mod: {}", mod_name));

        // Execute packwiz remove command
        // Note: packwiz does not support --remove-deps flag
        // Orphan detection must be implemented using DependencyGraph
        let packwiz_args = vec!["remove", "-y", &mod_name];

        let result = session
            .process()
            .execute("packwiz", &packwiz_args, &workdir.join("pack"))
            .and_then(|output| {
                if output.success {
                    Ok(())
                } else {
                    Err(anyhow::anyhow!("Packwiz command failed: {}", output.stderr))
                }
            });

        match result {
            Ok(_) => {
                // Atomically remove from empack.yml
                if let Err(e) = config_manager.remove_dependency(&mod_name) {
                    session
                        .display()
                        .status()
                        .warning(&format!("Failed to update empack.yml: {}", e));
                }
                session
                    .display()
                    .status()
                    .success("Successfully removed from pack", "");
                removed_mods.push(mod_name);
            }
            Err(e) => {
                session
                    .display()
                    .status()
                    .error("Failed to remove from pack", &e.to_string());
                failed_mods.push((mod_name, e.to_string()));
            }
        }
    }

    // Orphan detection: Find mods with no dependents (if --deps flag is set)
    let mut removed_orphans = Vec::new();
    if deps && !removed_mods.is_empty() && mods_dir.exists() {
        session
            .display()
            .status()
            .section("Detecting orphaned dependencies");

        // Rebuild dependency graph after removals
        let mut dep_graph = crate::api::dependency_graph::DependencyGraph::new();
        if let Err(e) = dep_graph.build_from_directory(&mods_dir) {
            session
                .display()
                .status()
                .warning(&format!("Failed to build dependency graph: {}", e));
        } else {
            // Load empack.yml to get top-level mods
            let top_level_mods: std::collections::HashSet<String> =
                match config_manager.create_project_plan() {
                    Ok(plan) => {
                        // Use the dependency key as the mod identifier
                        plan.dependencies
                            .iter()
                            .map(|dep| dep.key.clone())
                            .collect()
                    }
                    Err(_) => std::collections::HashSet::new(),
                };

            // Find orphans: mods not in top-level AND no dependents
            let mut orphans = Vec::new();
            for node in dep_graph.all_nodes() {
                // Skip if mod is explicitly declared in empack.yml
                if top_level_mods.contains(&node.mod_id) {
                    continue;
                }

                // Check if any mods depend on this one
                let has_dependents = dep_graph
                    .get_dependents(&node.mod_id)
                    .map(|deps| !deps.is_empty())
                    .unwrap_or(false);

                if !has_dependents {
                    orphans.push(node.mod_id.clone());
                }
            }

            if !orphans.is_empty() {
                session
                    .display()
                    .status()
                    .info(&format!("Found {} orphaned dependencies:", orphans.len()));
                for orphan in &orphans {
                    session
                        .display()
                        .status()
                        .subtle(&format!("  - {}", orphan));
                }

                // Prompt user to remove orphans
                let should_remove = session
                    .interactive()
                    .text_input("Remove orphaned dependencies? [y/N]", "N".to_string())?
                    .to_lowercase();

                if should_remove == "y" || should_remove == "yes" {
                    session.display().status().section("Removing orphans");

                    for orphan in orphans {
                        let result = session
                            .process()
                            .execute("packwiz", &["remove", "-y", &orphan], &workdir.join("pack"))
                            .and_then(|output| {
                                if output.success {
                                    Ok(())
                                } else {
                                    Err(anyhow::anyhow!(
                                        "Packwiz command failed: {}",
                                        output.stderr
                                    ))
                                }
                            });

                        match result {
                            Ok(_) => {
                                // Atomically remove from empack.yml
                                if let Err(e) = config_manager.remove_dependency(&orphan) {
                                    session
                                        .display()
                                        .status()
                                        .warning(&format!("Failed to update empack.yml: {}", e));
                                }
                                session
                                    .display()
                                    .status()
                                    .success(&format!("Removed orphan: {}", orphan), "");
                                removed_orphans.push(orphan);
                            }
                            Err(e) => {
                                session.display().status().error(
                                    &format!("Failed to remove orphan: {}", orphan),
                                    &e.to_string(),
                                );
                            }
                        }
                    }
                } else {
                    session.display().status().info("Orphans not removed");
                }
            } else {
                session
                    .display()
                    .status()
                    .info("No orphaned dependencies found");
            }
        }
    }

    // Show summary
    session.display().status().section("Remove Summary");
    session
        .display()
        .status()
        .success("Successfully removed", &removed_mods.len().to_string());
    if !removed_orphans.is_empty() {
        session
            .display()
            .status()
            .success("Orphans removed", &removed_orphans.len().to_string());
    }
    session
        .display()
        .status()
        .info(&format!("Failed: {}", failed_mods.len()));

    if !failed_mods.is_empty() {
        session.display().status().section("Failed mods");
        for (mod_name, error) in failed_mods {
            session.display().status().error(&mod_name, &error);
        }
    }

    Ok(())
}

async fn handle_build(session: &dyn Session, targets: Vec<String>, clean: bool) -> Result<()> {
    let manager = session.state()?;

    // Verify we're in a configured state
    let current_state = manager.discover_state()?;
    if current_state == PackState::Uninitialized {
        session
            .display()
            .status()
            .error("Not in a modpack directory", "");
        session
            .display()
            .status()
            .subtle("   Run 'empack init' to set up a modpack project");
        return Ok(());
    }
    if current_state == PackState::Configured && !manager.validate_state(PackState::Configured)? {
        session
            .display()
            .status()
            .error("Project initialization is incomplete", "");
        session.display().status().subtle(
            "   Re-run 'empack init --force' to restore empack.yml and pack/ metadata before building",
        );
        return Ok(());
    }

    // Clean if requested
    if clean {
        session
            .display()
            .status()
            .checking("Cleaning build artifacts");
        crate::empack::state::clean_build_artifacts(session.filesystem(), &manager.workdir)
            .context("Failed to clean build artifacts")?;
    }

    // Parse build targets
    let build_targets = parse_build_targets(targets)?;

    session
        .display()
        .status()
        .section(&format!("Building targets: {:?}", build_targets));

    if session.config().app_config().dry_run {
        session.display().status().section("Planned Actions");
        for target in &build_targets {
            session
                .display()
                .status()
                .info(&format!("Would build: {}", target));
        }
        session
            .display()
            .status()
            .complete("Dry run complete - no changes applied");
        return Ok(());
    }

    // Ensure packwiz-installer-bootstrap.jar is available for builds that need it
    let bootstrap_jar_path = session.packwiz().bootstrap_jar_cache_path()?;
    let needs_bootstrap_jar = build_targets.iter().any(|target| {
        matches!(
            target,
            BuildTarget::Client
                | BuildTarget::Server
                | BuildTarget::ClientFull
                | BuildTarget::ServerFull
        )
    });

    if needs_bootstrap_jar && !session.filesystem().exists(&bootstrap_jar_path) {
        session
            .display()
            .status()
            .info("Downloading required component: packwiz-installer-bootstrap.jar...");

        // Create cache directory if it doesn't exist
        if let Some(parent) = bootstrap_jar_path.parent() {
            session.filesystem().create_dir_all(parent)?;
        }

        // Use the NetworkProvider to download the file
        let client = session.network().http_client()?;
        let url = "https://github.com/packwiz/packwiz-installer-bootstrap/releases/latest/download/packwiz-installer-bootstrap.jar";
        let response = client
            .get(url)
            .send()
            .await
            .context("Failed to download packwiz-installer-bootstrap.jar")?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!(
                "Failed to download packwiz-installer-bootstrap.jar: HTTP {}",
                response.status()
            ));
        }

        let bytes = response
            .bytes()
            .await
            .context("Failed to read response bytes")?;

        // Use the FileSystemProvider to save the file
        session
            .filesystem()
            .write_bytes(&bootstrap_jar_path, bytes.as_ref())
            .context("Failed to write JAR file to cache")?;

        session
            .display()
            .status()
            .complete("Downloaded packwiz-installer-bootstrap.jar");
    }

    // Ensure packwiz-installer.jar is available for builds that need it
    let installer_jar_path = session.packwiz().installer_jar_cache_path()?;
    let needs_installer_jar = build_targets
        .iter()
        .any(|target| matches!(target, BuildTarget::ClientFull | BuildTarget::ServerFull));

    if needs_installer_jar && !session.filesystem().exists(&installer_jar_path) {
        session
            .display()
            .status()
            .info("Downloading required component: packwiz-installer.jar...");

        // Create cache directory if it doesn't exist
        if let Some(parent) = installer_jar_path.parent() {
            session.filesystem().create_dir_all(parent)?;
        }

        // Use the NetworkProvider to download the file
        let client = session.network().http_client()?;
        let url = "https://github.com/packwiz/packwiz-installer/releases/latest/download/packwiz-installer.jar";
        let response = client
            .get(url)
            .send()
            .await
            .context("Failed to download packwiz-installer.jar")?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!(
                "Failed to download packwiz-installer.jar: HTTP {}",
                response.status()
            ));
        }

        let bytes = response
            .bytes()
            .await
            .context("Failed to read response bytes")?;

        // Use the FileSystemProvider to save the file
        session
            .filesystem()
            .write_bytes(&installer_jar_path, bytes.as_ref())
            .context("Failed to write JAR file to cache")?;

        session
            .display()
            .status()
            .complete("Downloaded packwiz-installer.jar");
    }

    // Create BuildOrchestrator with session
    let mut build_orchestrator = crate::empack::builds::BuildOrchestrator::new(session)
        .context("Failed to create build orchestrator")?;

    // Execute build pipeline with state management
    build_orchestrator
        .execute_build_pipeline(&build_targets)
        .await
        .inspect_err(|_| {
            session
                .display()
                .status()
                .info("If the build left partial artifacts, run 'empack clean --builds' to reset");
        })
        .context("Failed to execute build pipeline")?;

    session
        .display()
        .status()
        .complete("Build completed successfully");
    session
        .display()
        .status()
        .subtle("   Check dist/ directory for build artifacts");

    Ok(())
}

async fn handle_clean(session: &dyn Session, targets: Vec<String>) -> Result<()> {
    let manager = session.state()?;

    if session.config().app_config().dry_run {
        session.display().status().section("Planned Actions");
        if targets.is_empty()
            || targets.contains(&"builds".to_string())
            || targets.contains(&"all".to_string())
        {
            session
                .display()
                .status()
                .info("Would clean build artifacts in dist/");
        }
        if targets.contains(&"cache".to_string()) || targets.contains(&"all".to_string()) {
            session
                .display()
                .status()
                .info("Would clean cached data");
        }
        session
            .display()
            .status()
            .complete("Dry run complete - no changes applied");
        return Ok(());
    }

    if targets.is_empty()
        || targets.contains(&"builds".to_string())
        || targets.contains(&"all".to_string())
    {
        session
            .display()
            .status()
            .checking("Cleaning build artifacts");

        let current_state = manager.discover_state()?;
        let dist_dir = crate::empack::state::artifact_root(&manager.workdir);
        let has_dist = session.filesystem().is_directory(&dist_dir);

        if current_state == PackState::Built {
            let result = manager
                .execute_transition(session.process(), &*session.packwiz(), StateTransition::Clean)
                .await
                .context("Failed to clean build artifacts")?;
            for w in &result.warnings {
                session.display().status().warning(w);
            }
            session
                .display()
                .status()
                .complete("Build artifacts cleaned");
        } else if has_dist {
            crate::empack::state::clean_build_artifacts(session.filesystem(), &manager.workdir)
                .context("Failed to clean build artifacts")?;
            session
                .display()
                .status()
                .complete("Build artifacts cleaned");
        } else {
            session
                .display()
                .status()
                .info("No build artifacts to clean");
        }
    }

    if targets.contains(&"cache".to_string()) || targets.contains(&"all".to_string()) {
        session.display().status().checking("Cleaning cache");
        session
            .display()
            .status()
            .subtle("(Cache cleaning not yet implemented)");
    }

    Ok(())
}

async fn handle_sync(session: &dyn Session) -> Result<()> {
    let manager = session.state()?;

    // Verify we're in a configured state
    let current_state = manager.discover_state()?;
    if current_state == PackState::Uninitialized {
        session
            .display()
            .status()
            .error("Not in a modpack directory", "");
        session
            .display()
            .status()
            .subtle("   Run 'empack init' to set up a modpack project");
        return Ok(());
    }
    if current_state == PackState::Configured && !manager.validate_state(PackState::Configured)? {
        session
            .display()
            .status()
            .error("Project initialization is incomplete", "");
        session.display().status().subtle(
            "   Re-run 'empack init --force' to restore empack.yml and pack/ metadata before synchronizing",
        );
        return Ok(());
    }

    let workdir = manager.workdir.clone();
    let config_manager = session.filesystem().config_manager(workdir.clone());

    // Create HTTP client for API requests
    let client = session.network().http_client()?;

    // Get CurseForge API key from app configuration
    let curseforge_api_key = session
        .config()
        .app_config()
        .curseforge_api_client_key
        .clone();

    // Create project resolver
    let resolver = session
        .network()
        .project_resolver(client, curseforge_api_key);

    // Phase 1: Resolve any Search entries before building the project plan
    let empack_config = config_manager
        .load_empack_config()
        .context("Failed to load empack.yml configuration")?;

    let search_entries: Vec<(String, crate::empack::config::DependencySearch)> = empack_config
        .empack
        .dependencies
        .iter()
        .filter_map(|(slug, entry)| {
            if let DependencyEntry::Search(search) = entry {
                Some((slug.clone(), search.clone()))
            } else {
                None
            }
        })
        .collect();

    // Track slugs whose Search resolution fails so we can protect them from removal
    let mut unresolved_slugs: HashSet<String> = HashSet::new();

    if !search_entries.is_empty() {
        session
            .display()
            .status()
            .section("Resolving search entries");

        let pack_metadata = config_manager
            .load_pack_metadata()
            .context("Failed to load pack metadata")?;

        let minecraft_version_opt = empack_config
            .empack
            .minecraft_version
            .as_deref()
            .or(pack_metadata.as_ref().map(|p| p.versions.minecraft.as_str()));

        let mod_loader_opt = empack_config.empack.loader.or_else(|| {
            pack_metadata
                .as_ref()
                .and_then(|p| config_manager.infer_loader_from_metadata(p).ok())
        });

        for (slug, search) in &search_entries {
            session
                .display()
                .status()
                .checking(&format!("Resolving: {}", search.title));

            let pt_str = search.project_type.map(project_type_arg);
            let loader_str = mod_loader_opt.map(loader_arg);

            match resolver
                .resolve_project(
                    &search.title,
                    pt_str,
                    minecraft_version_opt,
                    loader_str,
                    search.platform,
                )
                .await
            {
                Ok(project_info) => {
                    let resolved_project_type = match project_info.project_type.as_str() {
                        "resourcepack" => ProjectType::ResourcePack,
                        "shader" => ProjectType::Shader,
                        "datapack" => ProjectType::Datapack,
                        _ => ProjectType::Mod,
                    };

                    let record = DependencyRecord {
                        status: DependencyStatus::Resolved,
                        title: project_info.title.clone(),
                        platform: project_info.platform,
                        project_id: project_info.project_id.clone(),
                        project_type: search.project_type.unwrap_or(resolved_project_type),
                        version: None,
                    };

                    // Remove old search entry if slug differs from resolved slug
                    let resolved_slug = slug.clone();
                    if let Err(e) = config_manager.add_dependency(&resolved_slug, record) {
                        session.display().status().warning(&format!(
                            "Failed to update empack.yml for '{}': {}",
                            search.title, e
                        ));
                        unresolved_slugs.insert(slug.clone());
                        continue;
                    }

                    session.display().status().success(
                        "Resolved",
                        &format!(
                            "{} -> {} on {}",
                            search.title, project_info.title, project_info.platform
                        ),
                    );
                }
                Err(e) => {
                    session.display().status().warning(&format!(
                        "Could not resolve '{}': {}",
                        search.title, e
                    ));
                    unresolved_slugs.insert(slug.clone());
                }
            }
        }
    }

    // Phase 2: Build project plan (now all resolvable entries are Resolved)
    let project_plan = config_manager
        .create_project_plan()
        .context("Failed to load empack.yml configuration")?;

    session
        .display()
        .status()
        .section("Synchronizing empack.yml with packwiz");
    session.display().status().info(&format!(
        "Target: {} v{}",
        project_plan.minecraft_version, project_plan.loader_version
    ));

    // Get currently installed mods
    let installed_mods = match session.packwiz().get_installed_mods(&workdir) {
        Ok(mods) => {
            session
                .display()
                .status()
                .info(&format!("Found {} currently installed mods", mods.len()));
            mods
        }
        Err(e) => {
            session
                .display()
                .status()
                .warning(&format!("Could not read installed mods: {}", e));
            session
                .display()
                .status()
                .info("Assuming empty pack (add-only mode)");
            HashSet::new()
        }
    };

    let sync_plan = build_sync_plan(&project_plan, &installed_mods);

    // Protect installed mods whose Search entries failed resolution from removal
    let protected_actions: Vec<_> = sync_plan
        .actions
        .into_iter()
        .filter(|action| match action {
            SyncPlanAction::Remove { key, .. } => !unresolved_slugs.contains(key),
            _ => true,
        })
        .collect();

    let mut planned_actions = Vec::new();

    let already_installed: Vec<_> = project_plan
        .dependencies
        .iter()
        .filter(|dep| installed_mods.contains(&dep.key))
        .collect();
    let total_steps = already_installed.len() + protected_actions.len();
    let mut step = 0;

    for dep_spec in &already_installed {
        step += 1;
        session
            .display()
            .status()
            .step(step, total_steps, &format!("Processing dependency: {}", dep_spec.key));
        session
            .display()
            .status()
            .success("Already installed", &dep_spec.key);
    }

    let mut planning_failure_count: usize = 0;

    for action in &protected_actions {
        step += 1;
        let action_label = match action {
            SyncPlanAction::Add(dep) => dep.search_query.as_str(),
            SyncPlanAction::Remove { key, .. } => key.as_str(),
        };
        session
            .display()
            .status()
            .step(step, total_steps, &format!("Processing dependency: {}", action_label));

        match resolve_sync_action(action, resolver.as_ref()).await {
            Ok(resolved) => {
                if let SyncExecutionAction::Add {
                    title,
                    resolved_platform,
                    ..
                } = &resolved
                {
                    session
                        .display()
                        .status()
                        .success("Resolved", &format!("{} on {}", title, resolved_platform));
                }
                planned_actions.push(resolved);
            }
            Err(e) => {
                planning_failure_count += 1;
                session.display().status().error(
                    &format!("Failed to plan {action_label}"),
                    &render_add_contract_error_details(&e),
                );
            }
        }
    }

    // Show planned actions
    if planned_actions.is_empty() {
        if planning_failure_count > 0 {
            anyhow::bail!(
                "All {} planned action(s) failed during resolution. Check warnings above.",
                planning_failure_count
            );
        } else if !unresolved_slugs.is_empty() {
            session.display().status().warning(&format!(
                "{} search {} could not be resolved. Run sync again to retry.",
                unresolved_slugs.len(),
                if unresolved_slugs.len() == 1 { "entry" } else { "entries" }
            ));
        } else {
            session
                .display()
                .status()
                .complete("No changes needed - empack.yml already in sync");
        }
        return Ok(());
    }

    if planning_failure_count > 0 {
        session.display().status().warning(&format!(
            "{} action(s) failed during resolution and will be skipped. Proceeding with {} resolved action(s).",
            planning_failure_count, planned_actions.len()
        ));
    }

    session.display().status().section("Planned Actions");
    for action in &planned_actions {
        match action {
            SyncExecutionAction::Add {
                key,
                title,
                commands,
                resolved_project_id: _,
                resolved_platform: _,
            } => {
                session
                    .display()
                    .status()
                    .info(&format!("Add: {} ({})", title, key));
                if session.config().app_config().dry_run {
                    for command in commands {
                        session
                            .display()
                            .status()
                            .subtle(&format!("      Command: packwiz {}", command.join(" ")));
                    }
                }
            }
            SyncExecutionAction::Remove { key, title } => {
                session
                    .display()
                    .status()
                    .info(&format!("Remove: {} ({})", title, key));
                if session.config().app_config().dry_run {
                    session
                        .display()
                        .status()
                        .subtle(&format!("      Command: packwiz remove {}", key));
                }
            }
        }
    }

    if session.config().app_config().dry_run {
        session
            .display()
            .status()
            .complete("Dry run complete - no changes applied");
        return Ok(());
    }

    // Execute planned actions
    session
        .display()
        .status()
        .section("Executing sync actions");
    let mut success_count = 0;
    let mut failure_count = 0;

    for action in planned_actions {
        match action {
            SyncExecutionAction::Add {
                key: _,
                title,
                commands,
                resolved_project_id: _,
                resolved_platform: _,
            } => {
                session
                    .display()
                    .status()
                    .checking(&format!("Adding: {}", title));
                let mut last_error = None;
                let mut result = Ok(());
                for command in &commands {
                    match session.process().execute(
                        "packwiz",
                        &command.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
                        &workdir.join("pack"),
                    ) {
                        Ok(output) if output.success => {
                            result = Ok(());
                            last_error = None;
                            break;
                        }
                        Ok(output) => {
                            result = Err(());
                            last_error =
                                Some(anyhow::anyhow!("Packwiz command failed: {}", output.stderr));
                        }
                        Err(error) => {
                            result = Err(());
                            last_error = Some(anyhow::anyhow!(error));
                        }
                    }
                }
                match result {
                    Ok(_) => {
                        session.display().status().success("Added", "successfully");
                        success_count += 1;
                    }
                    Err(_) => {
                        let e = last_error
                            .unwrap_or_else(|| anyhow::anyhow!("Unknown packwiz add failure"));
                        session.display().status().error("Failed", &e.to_string());
                        failure_count += 1;
                    }
                }
            }
            SyncExecutionAction::Remove { key, title: _ } => {
                session
                    .display()
                    .status()
                    .checking(&format!("Removing: {}", key));
                let result = session
                    .process()
                    .execute("packwiz", &["remove", "-y", &key], &workdir.join("pack"))
                    .and_then(|output| {
                        if output.success {
                            Ok(())
                        } else {
                            Err(anyhow::anyhow!("Packwiz command failed: {}", output.stderr))
                        }
                    });
                match result {
                    Ok(_) => {
                        session
                            .display()
                            .status()
                            .success("Removed", "successfully");
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
    session.display().status().section("Sync Summary");
    session
        .display()
        .status()
        .success("Successful actions", &success_count.to_string());
    session
        .display()
        .status()
        .info(&format!("Failed actions: {}", failure_count));

    if failure_count == 0 {
        session
            .display()
            .status()
            .complete("empack.yml synchronized successfully with packwiz");
        Ok(())
    } else {
        session
            .display()
            .status()
            .warning(&format!("Sync completed with {} failures", failure_count));
        anyhow::bail!(
            "Sync completed with {} failed action(s); review warnings above",
            failure_count
        )
    }
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
            }
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
