//! Command execution handlers
//!
//! New session-based architecture for command execution.
//! Implements the Session-Scoped Dependency Injection Pattern.

use crate::Result;
use crate::application::cli::{CliProjectType, SearchPlatform};
use crate::application::session::{CommandSession, FileSystemProvider, Session};
use crate::application::sync::{
    AddContractError, AddResolution, SyncExecutionAction, SyncPlanAction, build_sync_plan,
    loader_arg, project_type_arg, resolve_add_contract, resolve_sync_action,
};
use crate::application::{CliConfig, Commands};
use crate::empack::config::{DependencyEntry, DependencyRecord, DependencyStatus};
use crate::empack::content::{JarResolver, UrlKind};
use crate::empack::import::{
    ImportConfig, ModpackManifest, SourceKind, execute_import, parse_curseforge_zip,
    parse_modrinth_mrpack, resolve_manifest,
};
use crate::empack::parsing::ModLoader;
use crate::empack::search::SearchError;
use crate::primitives::{BuildTarget, PackState, ProjectPlatform, ProjectType, StateTransition};
use anyhow::Context;
use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

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
        #[serde(skip_serializing_if = "str::is_empty")]
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
            dir,
            force,
            modloader,
            mc_version,
            author,
            pack_name,
            loader_version,
            pack_version,
            from_source,
        } => {
            handle_init(
                session,
                dir,
                pack_name,
                force,
                modloader,
                mc_version,
                author,
                loader_version,
                pack_version,
                from_source,
            )
            .await
        }
        Commands::Add {
            mods,
            force,
            platform,
            project_type,
            version_id,
            file_id,
        } => handle_add(session, mods, force, platform, project_type, version_id, file_id).await,
        Commands::Remove { mods, deps } => handle_remove(session, mods, deps).await,
        Commands::Build {
            targets,
            clean,
            format,
        } => handle_build(session, targets, clean, format.to_archive_format()).await,
        Commands::Clean { targets } => handle_clean(session, targets).await,
        Commands::Sync {} => handle_sync(session).await,
    }
}

async fn handle_requirements(session: &dyn Session) -> Result<()> {
    session
        .display()
        .status()
        .section("Checking tool dependencies");

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
            if session.process().find_program("go").is_some() {
                session
                    .display()
                    .status()
                    .subtle("   Or via Go: go install github.com/packwiz/packwiz@latest");
            }
        }
    }

    match session.process().find_program("java") {
        Some(path) => {
            session.display().status().success("java", &path);
        }
        None => {
            session.display().status().error(
                "java",
                "not found (required for non-vanilla server builds: fabric, quilt, neoforge, forge)",
            );
        }
    }

    session
        .display()
        .status()
        .success("archive support", "native (zip, tar.gz, 7z)");

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
    positional_dir: Option<String>,
    cli_pack_name: Option<String>,
    force: bool,
    cli_modloader: Option<String>,
    cli_mc_version: Option<String>,
    cli_author: Option<String>,
    cli_loader_version: Option<String>,
    cli_pack_version: Option<String>,
    from_source: Option<String>,
) -> Result<()> {
    if session.config().app_config().yes && cli_modloader.is_none() && from_source.is_none() {
        return Err(anyhow::anyhow!(
            "--yes requires --modloader to be specified"
        ));
    }

    if let Some(ref source) = from_source {
        return handle_init_from_source(session, source, positional_dir, force, cli_pack_name).await;
    }

    // Phase A: Resolve target_dir (WHERE). Only the positional arg affects directory.
    let base_dir = session.config().app_config().workdir.clone().unwrap_or(
        session
            .filesystem()
            .current_dir()
            .context("Failed to get current directory")?,
    );

    let (target_dir, needs_mkdir) = if let Some(ref dir_arg) = positional_dir {
        let target = base_dir.join(dir_arg);
        let needs_mkdir = !session.filesystem().exists(&target);
        (target, needs_mkdir)
    } else {
        (base_dir, false)
    };

    // Check state only if the directory already exists
    if !needs_mkdir {
        let manager =
            crate::empack::state::PackStateManager::new(target_dir.clone(), session.filesystem());

        let mut current_state = manager.discover_state()?;
        if current_state != PackState::Uninitialized {
            if !force {
                session
                    .display()
                    .status()
                    .error("Directory already contains a modpack project", "");
                session
                    .display()
                    .status()
                    .subtle("   Use --force to overwrite existing files");
                return Err(anyhow::anyhow!(
                    "Directory already contains a modpack project. Use --force to overwrite existing files."
                ));
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

    // Phase B: Resolve pack_name (WHAT). Never affects directory.
    let dir_basename = target_dir
        .components()
        .rfind(|c| matches!(c, std::path::Component::Normal(_)))
        .and_then(|c| c.as_os_str().to_str())
        .unwrap_or("Pack")
        .to_string();

    let modpack_name = if let Some(name) = cli_pack_name.clone() {
        // --name flag is the explicit display name; highest priority
        session
            .display()
            .status()
            .info(&format!("Using name: {}", name));
        name
    } else {
        // Default: directory basename; filter "." and ".." from positional arg
        let default = positional_dir
            .as_deref()
            .filter(|s| *s != "." && *s != "..")
            .map(String::from)
            .unwrap_or(dir_basename);
        session.interactive().text_input("Modpack name", default)?
    };

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
        if !minecraft_versions
            .iter()
            .any(|v| v.eq_ignore_ascii_case(&mc_ver))
        {
            anyhow::bail!(
                "Minecraft version '{}' not found. Available versions include: {}",
                mc_ver,
                minecraft_versions
                    .iter()
                    .take(5)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
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

    // Step 3: Mod Loader Selection
    //
    // Vanilla (no modloader) is supported: pass --modloader none or select
    // "none (vanilla)" in the interactive prompt. When vanilla is chosen,
    // loader fetching, loader version fetching, and loader version prompts
    // are all skipped.

    let is_vanilla = cli_modloader
        .as_deref()
        .is_some_and(|s| s.eq_ignore_ascii_case("none"));

    let (loader_str, loader_version) = if is_vanilla {
        if cli_loader_version.is_some() {
            return Err(anyhow::anyhow!(
                "--loader-version is not allowed for vanilla packs"
            ));
        }
        session
            .display()
            .status()
            .info("Using loader: none (vanilla)");
        ("none".to_string(), String::new())
    } else {
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

        if compatible_loaders.is_empty() {
            session.display().status().error(
                "No compatible mod loaders found",
                &format!("for Minecraft {}", minecraft_version),
            );
            session
                .display()
                .status()
                .subtle("   Try selecting a different Minecraft version");
            return Err(anyhow::anyhow!(
                "No compatible mod loaders found for Minecraft {}",
                minecraft_version
            ));
        }

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
            (Some(versions_loader), loader_str)
        } else {
            let mut loader_names: Vec<String> = vec!["none (vanilla)".to_string()];
            loader_names.extend(compatible_loaders.iter().map(|l| l.as_str().to_string()));
            let loader_name_refs: Vec<&str> = loader_names.iter().map(|s| s.as_str()).collect();
            let loader_index = session
                .interactive()
                .select("Mod loader", &loader_name_refs)?;

            if loader_index == 0 {
                (None, "none".to_string())
            } else {
                let selected = &compatible_loaders[loader_index - 1];
                (Some(selected.clone()), selected.as_str().to_string())
            }
        };

        if let Some(selected_loader) = selected_loader {
            // Step 4: Dynamic, Searchable Loader Version Prompt
            session.display().status().info(&format!(
                "Fetching {} versions for Minecraft {}...",
                loader_str, minecraft_version
            ));
            let loader_versions = fetch_loader_versions(
                session,
                &version_fetcher,
                &selected_loader,
                &loader_str,
                &minecraft_version,
            )
            .await;

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
                        loader_versions
                            .iter()
                            .take(5)
                            .cloned()
                            .collect::<Vec<_>>()
                            .join(", ")
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

            (loader_str, loader_version)
        } else {
            ("none".to_string(), String::new())
        }
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
    if loader_str == "none" {
        session.display().status().info("   Loader: none (vanilla)");
    } else {
        session
            .display()
            .status()
            .info(&format!("   Loader: {} v{}", loader_str, loader_version));
    }

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

    if loader_str != "none" {
        validate_init_inputs(
            &minecraft_version,
            &minecraft_versions,
            &loader_str,
            &loader_version,
        )?;
    }

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
        match session.filesystem().remove_dir_all(&target_dir) {
            Ok(()) => session
                .display()
                .status()
                .warning(&format!("Cleaned up directory after init failure: {}", e)),
            Err(cleanup_err) => session.display().status().warning(&format!(
                "Init failed: {}. Also failed to clean up {}: {}",
                e,
                target_dir.display(),
                cleanup_err
            )),
        }
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
        .execute_transition(
            session.process(),
            &*session.packwiz(),
            StateTransition::Initialize(*config),
        )
        .await
        .context("Failed to initialize modpack project")?;
    for w in &transition_result.warnings {
        session.display().status().warning(w);
    }

    // Scaffold project structure and templates after state transition succeeds.
    // Must run AFTER transition to avoid discover_state seeing dist/ as Built.
    let mut installer = crate::empack::templates::TemplateInstaller::new(session.filesystem());
    installer.configure(
        config.name,
        config.author,
        config.mc_version,
        config.version,
    );
    if config.modloader != "none" && !config.loader_version.is_empty() {
        installer
            .engine_mut()
            .set_modloader_variables(config.modloader, config.loader_version);
    }
    if let Err(e) = installer.install_all(target_dir) {
        session
            .display()
            .status()
            .warning(&format!("Template scaffolding incomplete: {}", e));
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

/// Validate that loader inputs are consistent.
/// Skipped for vanilla (loader_str == "none"). MC version and loader
/// compatibility are validated inline before this is called.
fn validate_init_inputs(
    mc_version: &str,
    minecraft_versions: &[String],
    loader_str: &str,
    loader_version: &str,
) -> Result<()> {
    if !minecraft_versions
        .iter()
        .any(|v| v.eq_ignore_ascii_case(mc_version))
    {
        anyhow::bail!(
            "Minecraft version '{}' not found in available versions",
            mc_version
        );
    }

    ModLoader::parse(loader_str).with_context(|| format!("Invalid mod loader: {}", loader_str))?;

    if loader_version.is_empty() {
        anyhow::bail!("Loader version is required for {}", loader_str);
    }

    Ok(())
}

async fn handle_init_from_source(
    session: &dyn Session,
    source: &str,
    positional_dir: Option<String>,
    force: bool,
    cli_pack_name: Option<String>,
) -> Result<()> {
    session
        .display()
        .status()
        .section("Importing modpack from source");

    // _tmp_dir must be held alive until execute_import finishes reading the archive
    let (manifest, _tmp_dir, _archive_path) = if source.starts_with("http://")
        || source.starts_with("https://")
    {
        import_from_remote(session, source).await?
    } else {
        import_from_local(session, source)?
    };

    // Phase A: Resolve target_dir
    let base_dir = session.config().app_config().workdir.clone().unwrap_or(
        session
            .filesystem()
            .current_dir()
            .context("Failed to get current directory")?,
    );

    let target_dir = if let Some(ref dir_arg) = positional_dir {
        base_dir.join(dir_arg)
    } else {
        // Sanitize manifest name to prevent path traversal from untrusted modpack metadata
        let safe_name = manifest
            .identity
            .name
            .replace(['/', '\\', '.'], "_");
        base_dir.join(&safe_name)
    };

    // Check state
    if session.filesystem().exists(&target_dir) {
        let manager = crate::empack::state::PackStateManager::new(
            target_dir.clone(),
            session.filesystem(),
        );
        let current_state = manager.discover_state()?;
        if current_state != PackState::Uninitialized && !force {
            session
                .display()
                .status()
                .error("Directory already contains a modpack project", "");
            session
                .display()
                .status()
                .subtle("   Use --force to overwrite existing files");
            return Err(anyhow::anyhow!(
                "Directory already contains a modpack project. Use --force to overwrite."
            ));
        }
    }

    session.display().status().info(&format!(
        "Importing '{}' for Minecraft {} ({})",
        manifest.identity.name,
        manifest.target.minecraft_version,
        manifest.target.loader.as_str(),
    ));

    let pack_name = cli_pack_name.unwrap_or_else(|| manifest.identity.name.clone());
    let author = manifest
        .identity
        .author
        .clone()
        .unwrap_or_else(|| "Unknown Author".to_string());
    let version = manifest.identity.version.clone();

    // Phase B: Resolve
    let modrinth_api = session.network();
    let curseforge_api = session.network();

    let resolved = resolve_manifest(
        manifest,
        modrinth_api,
        curseforge_api,
    )
    .await?;

    for warning in &resolved.warnings {
        session.display().status().warning(warning);
    }

    // Phase C: Execute
    let config = ImportConfig {
        target_dir: target_dir.clone(),
        pack_name: pack_name.clone(),
        author: author.clone(),
        version: version.clone(),
    };

    let result = execute_import(resolved, config, session).await?;

    session.display().status().section("Import Summary");
    session.display().status().success(
        "Platform references added",
        &result.stats.platform_referenced.to_string(),
    );
    session.display().status().info(&format!(
        "Embedded files extracted: {} (unidentified)",
        result.stats.embedded_jars_unidentified
    ));
    session.display().status().info(&format!(
        "Override files copied: {}",
        result.stats.overrides_copied
    ));

    session
        .display()
        .status()
        .complete("Modpack imported successfully");
    session.display().status().subtle(&format!(
        "Project directory: {}",
        result.project_dir.display()
    ));

    Ok(())
}

fn import_from_local(
    _session: &dyn Session,
    path: &str,
) -> Result<(ModpackManifest, Option<tempfile::TempDir>, PathBuf)> {
    let source_path = PathBuf::from(path);
    let kind = crate::empack::import::detect_local_source(&source_path)?;

    match kind {
        SourceKind::CurseForgeZip => {
            let manifest = parse_curseforge_zip(&source_path)?;
            Ok((manifest, None, source_path))
        }
        SourceKind::ModrinthMrpack => {
            let manifest = parse_modrinth_mrpack(&source_path)?;
            Ok((manifest, None, source_path))
        }
        SourceKind::PackwizDirectory => {
            anyhow::bail!(
                "packwiz directory import is not yet implemented; \
                 initialize with empack init then use empack add for each mod"
            );
        }
        _ => anyhow::bail!("unsupported source kind: {:?}", kind),
    }
}

async fn import_from_remote(
    session: &dyn Session,
    source: &str,
) -> Result<(ModpackManifest, Option<tempfile::TempDir>, PathBuf)> {
    let url_kind = crate::empack::content::classify_url(source).map_err(|e| {
        crate::empack::import::ImportError::UnrecognizedSource(e.to_string())
    })?;

    match url_kind {
        UrlKind::ModrinthModpack { slug, version } => {
            let (manifest, tmp_dir, path) =
                download_modrinth_modpack(session, &slug, version.as_deref()).await?;
            Ok((manifest, Some(tmp_dir), path))
        }
        UrlKind::CurseForgeModpack { .. } => {
            Err(crate::empack::import::ImportError::RemoteCurseForgeNotSupported.into())
        }
        _ => Err(crate::empack::import::ImportError::UnrecognizedSource(
            source.to_string(),
        )
        .into()),
    }
}

async fn download_modrinth_modpack(
    session: &dyn Session,
    slug: &str,
    version_filter: Option<&str>,
) -> Result<(ModpackManifest, tempfile::TempDir, PathBuf)> {
    session
        .display()
        .status()
        .info(&format!("Fetching Modrinth modpack: {}", slug));

    let client = session.network().http_client()?;

    let version_url = format!(
        "https://api.modrinth.com/v2/project/{}/version",
        slug
    );

    let response = client
        .get(&version_url)
        .send()
        .await
        .context("failed to fetch Modrinth version list")?;

    if !response.status().is_success() {
        anyhow::bail!(
            "Modrinth API returned {} for project '{}'",
            response.status(),
            slug
        );
    }

    let versions: Vec<serde_json::Value> = response
        .json()
        .await
        .context("failed to parse Modrinth versions")?;

    let version = if let Some(ref vf) = version_filter {
        versions
            .iter()
            .find(|v| {
                v.get("version_number").and_then(|n| n.as_str()) == Some(vf)
                    || v.get("id").and_then(|n| n.as_str()) == Some(vf)
            })
            .ok_or_else(|| {
                anyhow::anyhow!("version '{}' not found for Modrinth project '{}'", vf, slug)
            })?
    } else {
        versions
            .first()
            .ok_or_else(|| anyhow::anyhow!("no versions found for Modrinth project '{}'", slug))?
    };

    let files = version
        .get("files")
        .and_then(|f| f.as_array())
        .ok_or_else(|| anyhow::anyhow!("no files in Modrinth version response"))?;

    let file_entry = files
        .iter()
        .find(|f| {
            let primary = f.get("primary").and_then(|p| p.as_bool()).unwrap_or(false);
            let name = f
                .get("filename")
                .and_then(|n| n.as_str())
                .unwrap_or("");
            primary || name.ends_with(".mrpack")
        })
        .or_else(|| files.first())
        .ok_or_else(|| anyhow::anyhow!("no downloadable file in Modrinth version"))?;

    let download_url = file_entry
        .get("url")
        .and_then(|u| u.as_str())
        .ok_or_else(|| anyhow::anyhow!("file entry missing url field in Modrinth version"))?;

    let filename = file_entry
        .get("filename")
        .and_then(|f| f.as_str())
        .unwrap_or("modpack.mrpack");

    session
        .display()
        .status()
        .info(&format!("Downloading {}...", filename));

    let tmp_dir = tempfile::tempdir().context("failed to create temp directory")?;
    let dest_path = tmp_dir.path().join(filename);

    if let Some(parent) = dest_path.parent() {
        session.filesystem().create_dir_all(parent)?;
    }

    download_file(&client, download_url, &dest_path).await?;

    let manifest = parse_modrinth_mrpack(&dest_path)?;
    Ok((manifest, tmp_dir, dest_path))
}

async fn download_file(client: &reqwest::Client, url: &str, dest: &std::path::Path) -> Result<()> {
    let response = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("failed to download from {}", url))?;

    if !response.status().is_success() {
        anyhow::bail!("HTTP {} for {}", response.status(), url);
    }

    let bytes = response
        .bytes()
        .await
        .with_context(|| format!("failed to read response body from {}", url))?;

    std::fs::write(dest, &bytes)
        .with_context(|| format!("failed to write to {}", dest.display()))?;

    Ok(())
}

async fn handle_add(
    session: &dyn Session,
    mods: Vec<String>,
    force: bool,
    platform: Option<SearchPlatform>,
    project_type: Option<CliProjectType>,
    version_id: Option<String>,
    file_id: Option<String>,
) -> Result<()> {
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

    if session.filesystem().exists(&mods_dir)
        && let Err(e) = dep_graph.build_from_directory_with(&mods_dir, session.filesystem())
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
        .project_resolver(client.clone(), curseforge_api_key.clone());

    session
        .display()
        .status()
        .section(&format!("Adding {} mod(s) to modpack", mods.len()));

    let mut added_mods: Vec<String> = Vec::new();
    let mut failed_mods: Vec<(String, String)> = Vec::new();

    let mut resolved_mods: Vec<ResolvedMod> = Vec::new();
    let mut batch_project_ids: std::collections::HashSet<String> = std::collections::HashSet::new();

    for mod_query in mods {
        let resolution_intent = AddResolutionIntent::from_cli_input(&mod_query, platform.clone());
        session
            .display()
            .status()
            .checking(&format!("Resolving mod: {}", mod_query));

        match resolution_intent.kind.clone() {
            AddIntentKind::Search => {
                let minecraft_version =
                    project_plan.as_ref().map(|p| p.minecraft_version.as_str());
                let mod_loader = project_plan.as_ref().and_then(|p| p.loader);

                let direct_project_id =
                    resolution_intent.direct_project_id.as_deref().unwrap_or("");
                let direct_platform = resolution_intent
                    .direct_platform
                    .unwrap_or(ProjectPlatform::Modrinth);

                let version_pin = derive_version_pin(
                    &version_id,
                    &file_id,
                    resolution_intent.direct_platform,
                );

                let search_project_type = project_type.as_ref().map(|pt| pt.to_project_type());
                match resolve_add_contract(
                    &resolution_intent.search_query,
                    search_project_type,
                    minecraft_version,
                    mod_loader,
                    direct_project_id,
                    direct_platform,
                    version_pin,
                    resolution_intent.preferred_platform,
                    resolver.as_ref(),
                )
                .await
                {
                    Ok(resolution) => {
                        let status_label =
                            if resolution_intent.direct_project_id.is_some() {
                                "Using direct project ID"
                            } else {
                                "Found"
                            };
                        session.display().status().success(
                            status_label,
                            &format!(
                                "{} on {}",
                                resolution.title, resolution.resolved_platform
                            ),
                        );
                        if let Some(confidence) = resolution.confidence {
                            session
                                .display()
                                .status()
                                .info(&format!("Confidence: {}%", confidence));
                        }

                        if !force && dep_graph.contains(&resolution.resolved_project_id) {
                            session.display().status().warning(&format!(
                                "Mod already installed: {} (use --force to reinstall)",
                                resolution.title
                            ));
                            continue;
                        }

                        if !force
                            && batch_project_ids
                                .contains(&resolution.resolved_project_id)
                        {
                            session.display().status().warning(&format!(
                                "Duplicate in batch: {} (already queued for addition)",
                                resolution.title
                            ));
                            continue;
                        }

                        batch_project_ids
                            .insert(resolution.resolved_project_id.clone());
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
            AddIntentKind::CurseForgeDirect { slug } => {
                match resolve_curseforge_slug(
                    &slug,
                    &client,
                    curseforge_api_key.as_deref(),
                    plan_mc_version(project_plan.as_ref()),
                    plan_loader(project_plan.as_ref()),
                    version_id.as_deref(),
                    file_id.as_deref(),
                    project_type.as_ref().map(|pt| pt.to_project_type()),
                    resolver.as_ref(),
                )
                .await
                {
                    Ok(resolution) => {
                        session.display().status().success(
                            "Found",
                            &format!(
                                "{} on {}",
                                resolution.title, resolution.resolved_platform
                            ),
                        );

                        if !force && dep_graph.contains(&resolution.resolved_project_id) {
                            session.display().status().warning(&format!(
                                "Mod already installed: {} (use --force to reinstall)",
                                resolution.title
                            ));
                            continue;
                        }

                        if !force
                            && batch_project_ids
                                .contains(&resolution.resolved_project_id)
                        {
                            session.display().status().warning(&format!(
                                "Duplicate in batch: {} (already queued for addition)",
                                resolution.title
                            ));
                            continue;
                        }

                        batch_project_ids
                            .insert(resolution.resolved_project_id.clone());
                        let dep_key = slug.to_lowercase();
                        resolved_mods.push(ResolvedMod {
                            query: mod_query,
                            resolution,
                            dep_key,
                        });
                    }
                    Err(e) => {
                        session
                            .display()
                            .status()
                            .error("Failed to resolve CurseForge slug", &e.to_string());
                        failed_mods.push((mod_query, e.to_string()));
                    }
                }
            }
            AddIntentKind::DirectDownload { ref url, ref extension } => {
                if extension != "jar" {
                    let msg = format!(
                        "Adding non-JAR files via URL is not yet supported (got .{extension}). \
                         For .jar files, the file will be identified and added automatically."
                    );
                    session.display().status().error("Unsupported file type", &msg);
                    failed_mods.push((mod_query, msg));
                    continue;
                }

                match handle_direct_download_jar(
                    session,
                    url,
                    resolver.as_ref(),
                )
                .await
                {
                    Ok(resolution) => {
                        session.display().status().success(
                            "Added",
                            &resolution.title,
                        );
                        if !resolution.local && let Some(ref pid) = resolution.project_id {
                            let record = DependencyRecord {
                                status: DependencyStatus::Resolved,
                                title: resolution.title.clone(),
                                platform: resolution.platform,
                                project_id: pid.clone(),
                                project_type: resolution.project_type,
                                version: None,
                            };
                            let dep_key = resolution
                                .title
                                .to_lowercase()
                                .replace(' ', "-");
                            if let Err(e) =
                                config_manager.add_dependency(&dep_key, record)
                            {
                                session.display().status().warning(&format!(
                                    "Failed to update empack.yml: {}",
                                    e
                                ));
                            }
                        }
                        added_mods.push(mod_query);
                    }
                    Err(e) => {
                        session
                            .display()
                            .status()
                            .error("Direct download failed", &e.to_string());
                        failed_mods.push((mod_query, e.to_string()));
                    }
                }
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

    let all_content_folders: &[&str] = &["mods", "resourcepacks", "shaderpacks", "datapacks"];
    for resolved in resolved_mods {
        let (scan_folders, before_slugs) = match resolved.resolution.resolved_project_type {
            Some(pt) => {
                let folder = content_folder_for_type(pt);
                let dir = workdir.join("pack").join(folder);
                let slugs = scan_pw_toml_slugs(session.filesystem(), &dir);
                (vec![folder], slugs)
            }
            None => {
                let mut slugs = HashSet::new();
                for folder in all_content_folders {
                    let dir = workdir.join("pack").join(folder);
                    slugs.extend(scan_pw_toml_slugs(session.filesystem(), &dir));
                }
                (all_content_folders.to_vec(), slugs)
            }
        };

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
                    last_error = Some(anyhow::anyhow!("Packwiz command failed: {}", output.stderr));
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
                let mut dep_key = resolved.dep_key.clone();
                for folder in &scan_folders {
                    let dir = workdir.join("pack").join(folder);
                    let found = discover_dep_key(
                        session.filesystem(),
                        &dir,
                        &before_slugs,
                        &resolved.dep_key,
                        session.display(),
                    );
                    if found != resolved.dep_key {
                        dep_key = found;
                        break;
                    }
                }

                // Update dependency graph with newly added mod
                // Rebuild from directory to capture new .pw.toml files
                let mut updated_graph = crate::api::dependency_graph::DependencyGraph::new();
                if let Err(e) =
                    updated_graph.build_from_directory_with(&mods_dir, session.filesystem())
                {
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

                // Phase A: detect CF-restricted mods before writing empack.yml
                let mut is_restricted = false;
                for folder in &scan_folders {
                    let dir = workdir.join("pack").join(folder);
                    let pw_toml_path = dir.join(format!("{}.pw.toml", dep_key));
                    if let Ok(content) = session.filesystem().read_to_string(&pw_toml_path)
                        && let Some(rm) = parse_restricted_pw_toml(&content)
                    {
                        let cf_url = rm.curseforge_url();
                        session.display().status().warning(&format!(
                            "\"{}\" has third-party downloads disabled on CurseForge.",
                            rm.name
                        ));
                        session
                            .display()
                            .status()
                            .warning(&format!("Download manually: {}", cf_url));
                        session.display().status().warning(&format!(
                            "Place the file in: {}",
                            packwiz_cache_import_dir().display()
                        ));

                        if let Ok(modrinth_info) = resolver
                            .resolve_project(
                                &rm.name,
                                Some("mod"),
                                project_plan.as_ref().map(|p| p.minecraft_version.as_str()),
                                project_plan.as_ref().and_then(|p| {
                                    p.loader.map(crate::application::sync::loader_arg)
                                }),
                                Some(ProjectPlatform::Modrinth),
                            )
                            .await
                            && modrinth_info.platform == ProjectPlatform::Modrinth
                        {
                            session.display().status().warning(&format!(
                                "This mod may be available on Modrinth. Try: empack remove {} && empack add \"{}\"",
                                dep_key, rm.name
                            ));
                        }

                        // Clean up the .pw.toml that packwiz just created so it
                        // doesn't cause confusing build pre-flight failures later.
                        let pack_dir = workdir.join("pack");
                        let packwiz_remove_ok = session
                            .process()
                            .execute("packwiz", &["remove", "-y", &dep_key], &pack_dir)
                            .is_ok();

                        if !packwiz_remove_ok
                            && let Err(e) = session.filesystem().remove_file(&pw_toml_path)
                        {
                            session.display().status().warning(&format!(
                                "Failed to remove {}: {}",
                                pw_toml_path.display(),
                                e
                            ));
                        }

                        is_restricted = true;
                        failed_mods.push((
                            resolved.query.clone(),
                            format!(
                                "CurseForge restricted: \"{}\" has third-party downloads disabled. Manual download required: {}",
                                rm.name, cf_url
                            ),
                        ));
                        break;
                    }
                }

                if !is_restricted {
                    let record = DependencyRecord {
                        status: DependencyStatus::Resolved,
                        title: resolved.resolution.title.clone(),
                        platform: resolved.resolution.resolved_platform,
                        project_id: resolved.resolution.resolved_project_id.clone(),
                        project_type: resolved
                            .resolution
                            .resolved_project_type
                            .unwrap_or(ProjectType::Mod),
                        version: None,
                    };
                    if let Err(e) = config_manager.add_dependency(&dep_key, record) {
                        session
                            .display()
                            .status()
                            .warning(&format!("Failed to update empack.yml: {}", e));
                    }
                    added_mods.push(resolved.query);
                }
            }
            Err(_) => {
                let e =
                    last_error.unwrap_or_else(|| anyhow::anyhow!("Unknown packwiz add failure"));
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
        for (mod_name, error) in &failed_mods {
            session.display().status().error(mod_name, error);
        }
        let summary = failed_mods
            .iter()
            .map(|(name, err)| format!("{}: {}", name, err))
            .collect::<Vec<_>>()
            .join("; ");
        return Err(anyhow::anyhow!(
            "{} mod(s) failed: {}",
            failed_mods.len(),
            summary
        ));
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
        AddContractError::ResolveProject { source, .. } => {
            if matches!(source, SearchError::IncompatibleProject { .. }) {
                "Mod found but incompatible"
            } else {
                "Failed to resolve mod"
            }
        }
        AddContractError::PlanPackwizAdd { .. } => "Failed to prepare add command",
    };

    RenderedStatusError {
        item: item.to_string(),
        details: render_add_contract_error_details(error),
    }
}

fn render_add_contract_error_details(error: &AddContractError) -> String {
    match error {
        AddContractError::ResolveProject { query, source } => {
            if let SearchError::IncompatibleProject {
                project_title,
                available_loaders,
                requested_loader,
                requested_version,
                downloads,
                ..
            } = source
            {
                let loaders_str = available_loaders.join(", ");
                let dl_str = format_downloads(*downloads);
                match (requested_loader.as_deref(), requested_version.as_deref()) {
                    (Some(loader), Some(version)) => {
                        format!(
                            "'{project_title}' ({dl_str} downloads) exists but has no version for {loader} on {version}. Supported loaders: {loaders_str}"
                        )
                    }
                    (Some(loader), None) => {
                        format!(
                            "'{project_title}' ({dl_str} downloads) exists but does not support {loader}. Supported loaders: {loaders_str}"
                        )
                    }
                    (None, Some(version)) => {
                        format!(
                            "'{project_title}' ({dl_str} downloads) exists but has no version for {version}"
                        )
                    }
                    (None, None) => format!("{query}: {source}"),
                }
            } else {
                format!("{query}: {source}")
            }
        }
        AddContractError::PlanPackwizAdd {
            project_id,
            platform,
            source,
        } => format!("{platform} project {project_id}: {source}"),
    }
}

fn format_downloads(downloads: u64) -> String {
    if downloads >= 1_000_000 {
        format!("{}M", downloads / 1_000_000)
    } else if downloads >= 1_000 {
        format!("{}K", downloads / 1_000)
    } else {
        downloads.to_string()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum AddIntentKind {
    Search,
    CurseForgeDirect { slug: String },
    DirectDownload { url: String, extension: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AddResolutionIntent {
    kind: AddIntentKind,
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

        if mod_query.starts_with("http://") || mod_query.starts_with("https://") {
            match crate::empack::content::classify_url(mod_query) {
                Ok(UrlKind::ModrinthProject { slug }) => {
                    return Self {
                        kind: AddIntentKind::Search,
                        search_query: slug.clone(),
                        direct_project_id: Some(slug),
                        direct_platform: Some(ProjectPlatform::Modrinth),
                        preferred_platform: Some(ProjectPlatform::Modrinth),
                    };
                }
                Ok(UrlKind::CurseForgeProject { slug }) => {
                    return Self {
                        kind: AddIntentKind::CurseForgeDirect { slug },
                        search_query: mod_query.to_string(),
                        direct_project_id: None,
                        direct_platform: None,
                        preferred_platform: Some(ProjectPlatform::CurseForge),
                    };
                }
                Ok(UrlKind::DirectDownload { url, extension }) => {
                    return Self {
                        kind: AddIntentKind::DirectDownload { url, extension },
                        search_query: mod_query.to_string(),
                        direct_project_id: None,
                        direct_platform: None,
                        preferred_platform: None,
                    };
                }
                _ => { /* unrecognized URL falls through to search */ }
            }
        }

        let (direct_project_id, direct_platform) = match preferred_platform {
            Some(ProjectPlatform::CurseForge) => (
                Some(mod_query.to_string()),
                Some(ProjectPlatform::CurseForge),
            ),
            Some(ProjectPlatform::Modrinth) => (
                Some(mod_query.to_string()),
                Some(ProjectPlatform::Modrinth),
            ),
            None => (None, None),
        };

        Self {
            kind: AddIntentKind::Search,
            search_query: mod_query.to_string(),
            direct_project_id,
            direct_platform,
            preferred_platform,
        }
    }
}

fn content_folder_for_type(project_type: ProjectType) -> &'static str {
    match project_type {
        ProjectType::Mod => "mods",
        ProjectType::ResourcePack => "resourcepacks",
        ProjectType::Shader => "shaderpacks",
        ProjectType::Datapack => "datapacks",
    }
}

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

struct RestrictedMod {
    name: String,
    filename: String,
    file_id: u64,
    project_id: u64,
}

impl RestrictedMod {
    fn curseforge_url(&self) -> String {
        format!(
            "https://www.curseforge.com/projects/{}/files/{}",
            self.project_id, self.file_id
        )
    }
}

fn parse_restricted_pw_toml(content: &str) -> Option<RestrictedMod> {
    let parsed: toml::Value = toml::from_str(content).ok()?;

    let download = parsed.get("download")?;
    if download.get("mode")?.as_str()? != "metadata:curseforge" {
        return None;
    }
    if download.get("url").and_then(|v| v.as_str()).is_some() {
        return None;
    }

    let name = parsed.get("name")?.as_str()?.to_string();
    let filename = parsed.get("filename")?.as_str()?.to_string();

    let update_cf = parsed.get("update")?.get("curseforge")?;
    let file_id = u64::try_from(update_cf.get("file-id")?.as_integer()?).ok()?;
    let project_id = u64::try_from(update_cf.get("project-id")?.as_integer()?).ok()?;

    Some(RestrictedMod {
        name,
        filename,
        file_id,
        project_id,
    })
}

fn scan_restricted_mods(
    filesystem: &dyn FileSystemProvider,
    pack_dir: &std::path::Path,
) -> Vec<RestrictedMod> {
    let content_dirs = ["mods", "resourcepacks", "shaderpacks", "datapacks"];
    let mut restricted = Vec::new();

    for folder in &content_dirs {
        let dir = pack_dir.join(folder);
        if !filesystem.exists(&dir) {
            continue;
        }
        let file_list = match filesystem.get_file_list(&dir) {
            Ok(list) => list,
            Err(_) => continue,
        };
        for path in &file_list {
            let ext_match = path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e == "toml")
                .unwrap_or(false);
            let stem_match = path
                .file_stem()
                .and_then(|s| s.to_str())
                .map(|s| s.ends_with(".pw"))
                .unwrap_or(false);
            if ext_match
                && stem_match
                && let Ok(content) = filesystem.read_to_string(path)
                && let Some(rm) = parse_restricted_pw_toml(&content)
            {
                restricted.push(rm);
            }
        }
    }

    restricted
}

fn packwiz_cache_import_dir() -> std::path::PathBuf {
    #[cfg(test)]
    {
        crate::application::session_mocks::mock_root()
            .join(".cache")
            .join("packwiz")
            .join("cache")
            .join("import")
    }
    #[cfg(not(test))]
    {
        // Match Go's os.UserCacheDir() behavior (which packwiz uses):
        //   Linux:   $XDG_CACHE_HOME or $HOME/.cache
        //   macOS:   $HOME/Library/Caches
        //   Windows: %LocalAppData%
        let cache_base = packwiz_user_cache_dir();
        cache_base.join("packwiz").join("cache").join("import")
    }
}

/// Replicate Go's os.UserCacheDir() to match packwiz's actual cache root.
#[cfg(not(test))]
fn packwiz_user_cache_dir() -> std::path::PathBuf {
    #[cfg(target_os = "windows")]
    {
        std::env::var("LocalAppData")
            .ok()
            .filter(|s| !s.is_empty())
            .map(std::path::PathBuf::from)
            .expect("%LocalAppData% must be set on Windows")
    }
    #[cfg(target_os = "macos")]
    {
        crate::platform::home_dir().join("Library").join("Caches")
    }
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    {
        // Go's os.UserCacheDir on Linux: $XDG_CACHE_HOME or $HOME/.cache
        std::env::var("XDG_CACHE_HOME")
            .ok()
            .filter(|s| !s.is_empty())
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| crate::platform::home_dir().join(".cache"))
    }
}

fn derive_version_pin<'a>(
    version_id: &'a Option<String>,
    file_id: &'a Option<String>,
    direct_platform: Option<ProjectPlatform>,
) -> Option<&'a str> {
    match (version_id, file_id) {
        (Some(vid), Some(fid)) => {
            if direct_platform == Some(ProjectPlatform::CurseForge) {
                Some(fid.as_str())
            } else {
                Some(vid.as_str())
            }
        }
        (Some(vid), None) => Some(vid.as_str()),
        (None, Some(fid)) => Some(fid.as_str()),
        (None, None) => None,
    }
}

fn plan_mc_version(plan: Option<&crate::empack::config::ProjectPlan>) -> Option<&str> {
    plan.map(|p| p.minecraft_version.as_str())
}

fn plan_loader(
    plan: Option<&crate::empack::config::ProjectPlan>,
) -> Option<crate::empack::parsing::ModLoader> {
    plan.and_then(|p| p.loader)
}

#[allow(clippy::too_many_arguments)]
async fn resolve_curseforge_slug(
    slug: &str,
    client: &reqwest::Client,
    curseforge_api_key: Option<&str>,
    minecraft_version: Option<&str>,
    mod_loader: Option<ModLoader>,
    version_pin_override: Option<&str>,
    file_id_override: Option<&str>,
    project_type: Option<ProjectType>,
    resolver: &dyn crate::empack::search::ProjectResolverTrait,
) -> std::result::Result<AddResolution, anyhow::Error> {
    let api_key = curseforge_api_key
        .ok_or_else(|| anyhow::anyhow!("CurseForge API key required for slug resolution"))?;

    let search_url = format!(
        "https://api.curseforge.com/v1/mods/search?gameId=432&slug={slug}"
    );

    let response = client
        .get(&search_url)
        .header("x-api-key", api_key)
        .send()
        .await?;
    if !response.status().is_success() {
        anyhow::bail!(
            "CurseForge API returned {} for slug '{}'",
            response.status(),
            slug
        );
    }

    #[derive(serde::Deserialize)]
    struct CfSearchResponse {
        data: Vec<CfModEntry>,
    }

    #[derive(serde::Deserialize)]
    struct CfModEntry {
        id: u64,
        name: String,
    }

    let body: CfSearchResponse = response.json().await?;
    let entry = body.data.into_iter().next().ok_or_else(|| {
        anyhow::anyhow!("CurseForge project not found for slug '{}'", slug)
    })?;

    let project_id = entry.id.to_string();
    let version_pin = file_id_override.or(version_pin_override);

    resolve_add_contract(
        &entry.name,
        project_type,
        minecraft_version,
        mod_loader,
        &project_id,
        ProjectPlatform::CurseForge,
        version_pin,
        Some(ProjectPlatform::CurseForge),
        resolver,
    )
    .await
    .map_err(|e| anyhow::anyhow!("{}", e))
}

async fn handle_direct_download_jar(
    session: &dyn Session,
    url: &str,
    _resolver: &dyn crate::empack::search::ProjectResolverTrait,
) -> std::result::Result<DirectDownloadResult, anyhow::Error> {
    session.display().status().info(&format!("Downloading JAR from {}", url));

    let client = session.network().http_client()?;
    let tmp_dir = tempfile::tempdir().context("failed to create temp directory")?;
    let filename = url.rsplit('/').next().unwrap_or("download.jar");
    let dest_path = tmp_dir.path().join(filename);

    download_file(&client, url, &dest_path).await?;

    let sha1 = {
        let bytes = std::fs::read(&dest_path)?;
        compute_sha1_hex_for_bytes(&bytes)
    };

    session
        .display()
        .status()
        .info(&format!("SHA-1: {}", sha1));

    let jar_resolver = crate::empack::content::ApiJarResolver {
        modrinth: session.network(),
        curseforge: session.network(),
    };
    let identify_request = crate::empack::content::JarIdentifyRequest {
        path: dest_path.clone(),
        sha1: Some(sha1),
        sha512: None,
    };

    let identity = jar_resolver.identify(identify_request).await?;
    let manager = session.state()?;
    let workdir = manager.workdir.clone();
    let mods_dir = workdir.join("pack").join("mods");

    match identity {
        crate::empack::content::JarIdentity::Modrinth {
            project_id,
            version_id,
            title,
        } => {
            session.display().status().success(
                "Identified",
                &format!("{} on Modrinth", title),
            );

            let commands = crate::application::sync::build_packwiz_add_commands(
                &project_id,
                ProjectPlatform::Modrinth,
                Some(&version_id),
            )?;

            let command = &commands[0];
            let result = session.process().execute(
                "packwiz",
                &command.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
                &workdir.join("pack"),
            );
            if let Ok(output) = result {
                if !output.success {
                    anyhow::bail!("packwiz add failed: {}", output.stderr);
                }
            } else {
                anyhow::bail!("packwiz add failed: {}", result.unwrap_err());
            }

            Ok(DirectDownloadResult {
                title: title.clone(),
                platform: ProjectPlatform::Modrinth,
                project_id: Some(project_id),
                project_type: ProjectType::Mod,
                local: false,
            })
        }
        crate::empack::content::JarIdentity::CurseForge {
            project_id,
            file_id,
            title,
        } => {
            session.display().status().success(
                "Identified",
                &format!("{} on CurseForge", title),
            );

            let pid_str = project_id.to_string();
            let fid_str = file_id.to_string();
            let commands = crate::application::sync::build_packwiz_add_commands(
                &pid_str,
                ProjectPlatform::CurseForge,
                Some(&fid_str),
            )?;

            let command = &commands[0];
            let result = session.process().execute(
                "packwiz",
                &command.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
                &workdir.join("pack"),
            );
            if let Ok(output) = result {
                if !output.success {
                    anyhow::bail!("packwiz add failed: {}", output.stderr);
                }
            } else {
                anyhow::bail!("packwiz add failed: {}", result.unwrap_err());
            }

            Ok(DirectDownloadResult {
                title: title.clone(),
                platform: ProjectPlatform::CurseForge,
                project_id: Some(pid_str),
                project_type: ProjectType::Mod,
                local: false,
            })
        }
        crate::empack::content::JarIdentity::Unidentified => {
            session
                .display()
                .status()
                .warning("Could not identify JAR via Modrinth or CurseForge");
            session
                .display()
                .status()
                .info("Copying JAR to mods/ as a local-only entry");

            session
                .filesystem()
                .create_dir_all(&mods_dir)
                .context("failed to create mods directory")?;

            let jar_filename = filename.to_string();
            let dest = mods_dir.join(&jar_filename);
            let bytes = std::fs::read(&dest_path)?;
            session
                .filesystem()
                .write_bytes(&dest, &bytes)
                .context("failed to copy JAR to mods/")?;

            session.display().status().info(&format!(
                "Copied to {} (manage updates manually)",
                dest.display()
            ));

            Ok(DirectDownloadResult {
                title: jar_filename,
                platform: ProjectPlatform::Modrinth,
                project_id: None,
                project_type: ProjectType::Mod,
                local: true,
            })
        }
    }
}

struct DirectDownloadResult {
    title: String,
    platform: ProjectPlatform,
    project_id: Option<String>,
    project_type: ProjectType,
    local: bool,
}

fn compute_sha1_hex_for_bytes(data: &[u8]) -> String {
    use sha1::Digest;
    let mut hasher = sha1::Sha1::new();
    hasher.update(data);
    let result = hasher.finalize();
    hex_encode_bytes(&result)
}

fn hex_encode_bytes(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
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
    if deps && !removed_mods.is_empty() && session.filesystem().exists(&mods_dir) {
        session
            .display()
            .status()
            .section("Detecting orphaned dependencies");

        let mut dep_graph = crate::api::dependency_graph::DependencyGraph::new();
        if let Err(e) = dep_graph.build_from_directory_with(&mods_dir, session.filesystem()) {
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

                let should_remove = session
                    .interactive()
                    .confirm("Remove orphaned dependencies?", false)?;

                if should_remove {
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
        for (mod_name, error) in &failed_mods {
            session.display().status().error(mod_name, error);
        }
        let summary = failed_mods
            .iter()
            .map(|(name, err)| format!("{}: {}", name, err))
            .collect::<Vec<_>>()
            .join("; ");
        return Err(anyhow::anyhow!(
            "{} mod(s) failed to remove: {}",
            failed_mods.len(),
            summary
        ));
    }

    Ok(())
}

async fn handle_build(
    session: &dyn Session,
    targets: Vec<String>,
    clean: bool,
    archive_format: crate::empack::archive::ArchiveFormat,
) -> Result<()> {
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
        return Err(anyhow::anyhow!("Not in a modpack directory"));
    }
    if current_state == PackState::Configured && !manager.validate_state(PackState::Configured)? {
        session
            .display()
            .status()
            .error("Project initialization is incomplete", "");
        session.display().status().subtle(
            "   Re-run 'empack init --force' to restore empack.yml and pack/ metadata before building",
        );
        return Err(anyhow::anyhow!("Project initialization is incomplete"));
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

    // Phase B: pre-flight scan for CF-restricted mods
    let pack_dir = manager.workdir.join("pack");
    let restricted_mods = scan_restricted_mods(session.filesystem(), &pack_dir);
    if !restricted_mods.is_empty() {
        let cache_dir = packwiz_cache_import_dir();
        let mut blocked: Vec<&RestrictedMod> = Vec::new();
        for rm in &restricted_mods {
            let cached_path = cache_dir.join(&rm.filename);
            if !session.filesystem().exists(&cached_path) {
                blocked.push(rm);
            }
        }

        if !blocked.is_empty() {
            session
                .display()
                .status()
                .section("CurseForge restricted mods detected");
            let mut mod_names = Vec::new();
            for rm in &blocked {
                let url = rm.curseforge_url();
                session.display().status().warning(&format!(
                    "\"{}\" has third-party downloads disabled on CurseForge.",
                    rm.name
                ));
                session
                    .display()
                    .status()
                    .warning(&format!("Download manually: {}", url));
                if session.terminal().is_tty {
                    let (cmd, prefix_args) = crate::platform::browser_open_command();
                    let mut args: Vec<&str> = prefix_args;
                    args.push(&url);
                    let _ = session.process().execute(cmd, &args, &pack_dir);
                }
                mod_names.push(rm.name.clone());
            }
            session.display().status().warning(&format!(
                "Place downloaded files in: {}",
                cache_dir.display()
            ));
            return Err(anyhow::anyhow!(
                "{} mod(s) require manual download: {}. Visit curseforge.com to download, then place files in {} and re-run this command.",
                blocked.len(),
                mod_names.join(", "),
                cache_dir.display()
            ));
        }
    }

    // Clean if requested (after dry-run check to prevent side effects during preview)
    if clean {
        session
            .display()
            .status()
            .checking("Cleaning build artifacts");
        crate::empack::state::clean_build_artifacts(session.filesystem(), &manager.workdir)
            .context("Failed to clean build artifacts")?;
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

        download_to_cache(
            session,
            "https://github.com/packwiz/packwiz-installer-bootstrap/releases/latest/download/packwiz-installer-bootstrap.jar",
            &bootstrap_jar_path,
            "packwiz-installer-bootstrap.jar",
        )
        .await?;

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

        download_to_cache(
            session,
            "https://github.com/packwiz/packwiz-installer/releases/latest/download/packwiz-installer.jar",
            &installer_jar_path,
            "packwiz-installer.jar",
        )
        .await?;

        session
            .display()
            .status()
            .complete("Downloaded packwiz-installer.jar");
    }

    // Create BuildOrchestrator with session
    let mut build_orchestrator =
        crate::empack::builds::BuildOrchestrator::new(session, archive_format)
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
            session.display().status().info("Would clean cached data");
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
                .execute_transition(
                    session.process(),
                    &*session.packwiz(),
                    StateTransition::Clean,
                )
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
        .project_resolver(client.clone(), curseforge_api_key);

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

        let minecraft_version_opt =
            empack_config
                .empack
                .minecraft_version
                .as_deref()
                .or(pack_metadata
                    .as_ref()
                    .map(|p| p.versions.minecraft.as_str()));

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
                    session
                        .display()
                        .status()
                        .warning(&format!("Could not resolve '{}': {}", search.title, e));
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
        session.display().status().step(
            step,
            total_steps,
            &format!("Processing dependency: {}", dep_spec.key),
        );
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
        session.display().status().step(
            step,
            total_steps,
            &format!("Processing dependency: {}", action_label),
        );

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
                if unresolved_slugs.len() == 1 {
                    "entry"
                } else {
                    "entries"
                }
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
    session.display().status().section("Executing sync actions");
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

async fn download_to_cache(
    session: &dyn Session,
    url: &str,
    dest: &std::path::Path,
    label: &str,
) -> Result<()> {
    if let Some(parent) = dest.parent() {
        session.filesystem().create_dir_all(parent)?;
    }

    let client = session.network().http_client()?;
    let max_attempts: u32 = 3;
    let mut last_error = None;

    for attempt in 0..max_attempts {
        if attempt > 0 {
            tokio::time::sleep(std::time::Duration::from_secs(1 << (attempt - 1))).await;
        }

        match client
            .get(url)
            .timeout(std::time::Duration::from_secs(
                session.config().app_config().net_timeout,
            ))
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => match resp.bytes().await {
                Ok(bytes) => {
                    session
                        .filesystem()
                        .write_bytes(dest, bytes.as_ref())
                        .context(format!("Failed to write {} to cache", label))?;
                    return Ok(());
                }
                Err(e) => {
                    last_error = Some(format!("Failed to read response body for {}: {}", label, e));
                    continue;
                }
            },
            Ok(resp) => {
                let status = resp.status();
                last_error = Some(format!("HTTP {} for {}", status, label));
                if status.is_client_error() {
                    break;
                }
                continue;
            }
            Err(e) => {
                last_error = Some(format!("Failed to download {}: {}", label, e));
                continue;
            }
        }
    }

    Err(anyhow::anyhow!(
        "{}",
        last_error.unwrap_or_else(|| format!(
            "Failed to download {} after {} attempts",
            label, max_attempts
        ))
    ))
}

/// Fetch loader versions with fallback on network failure.
///
/// Dispatches to the appropriate `VersionFetcher` method for the selected loader,
/// displays the result, and falls back to hardcoded versions on error.
async fn fetch_loader_versions(
    session: &dyn Session,
    version_fetcher: &crate::empack::versions::VersionFetcher<'_>,
    selected_loader: &crate::empack::versions::ModLoader,
    loader_str: &str,
    minecraft_version: &str,
) -> Vec<String> {
    let result = match selected_loader {
        crate::empack::versions::ModLoader::Fabric => {
            version_fetcher
                .fetch_fabric_loader_versions(minecraft_version)
                .await
        }
        crate::empack::versions::ModLoader::NeoForge => {
            version_fetcher
                .fetch_neoforge_loader_versions(minecraft_version)
                .await
        }
        crate::empack::versions::ModLoader::Forge => {
            version_fetcher
                .fetch_forge_loader_versions(minecraft_version)
                .await
        }
        crate::empack::versions::ModLoader::Quilt => {
            version_fetcher
                .fetch_quilt_loader_versions(minecraft_version)
                .await
        }
    };

    match result {
        Ok(versions) => {
            session.display().status().success(
                "Found",
                &format!("{} {} versions", versions.len(), loader_str),
            );
            versions
        }
        Err(e) => {
            session
                .display()
                .status()
                .warning(&format!("Network fetch failed: {}", e));
            session.display().status().info("Using fallback versions");
            crate::empack::versions::VersionFetcher::get_fallback_loader_versions(
                loader_str,
                minecraft_version,
            )
        }
    }
}

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
