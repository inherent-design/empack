use anyhow::Context;
use crate::empack::builds::BuildError;
use crate::empack::config::ConfigError;
use crate::primitives::*;
use std::path::{Path, PathBuf};
use thiserror::Error;

// StateProvider trait removed - now using unified FileSystemProvider
// LiveStateProvider removed - now using LiveFileSystemProvider from session.rs

// LiveStateProvider implementation removed - now using LiveFileSystemProvider from session.rs

// Pure business logic functions - zero I/O, 100% testable.
// These functions contain the core state machine logic without side effects.

/// Canonical project-local artifact root for build outputs.
/// Keeping this rooted at `workdir/dist` lets build and clean share one trusted
/// artifact boundary instead of falling back to historical `.empack/dist` paths.
pub fn artifact_root(workdir: &Path) -> PathBuf {
    workdir.join("dist")
}

fn has_config_file<P: crate::application::session::FileSystemProvider + ?Sized>(
    provider: &P,
    workdir: &Path,
) -> bool {
    provider.exists(&workdir.join("empack.yml"))
}

fn has_pack_metadata<P: crate::application::session::FileSystemProvider + ?Sized>(
    provider: &P,
    workdir: &Path,
) -> bool {
    let pack_dir = workdir.join("pack");
    provider.is_directory(&pack_dir) && provider.exists(&pack_dir.join("pack.toml"))
}

fn has_canonical_build_artifacts<P: crate::application::session::FileSystemProvider + ?Sized>(
    provider: &P,
    workdir: &Path,
) -> bool {
    let dist_dir = artifact_root(workdir);
    provider.is_directory(&dist_dir) && provider.has_build_artifacts(&dist_dir).unwrap_or(false)
}

fn is_progressive_init_state<P: crate::application::session::FileSystemProvider + ?Sized>(
    provider: &P,
    workdir: &Path,
) -> bool {
    has_config_file(provider, workdir)
        && !has_pack_metadata(provider, workdir)
        && !has_canonical_build_artifacts(provider, workdir)
}

fn validate_state_layout<P: crate::application::session::FileSystemProvider + ?Sized>(
    provider: &P,
    workdir: &Path,
    expected: PackState,
) -> bool {
    match expected {
        PackState::Uninitialized => true,
        PackState::Configured | PackState::Building => {
            has_config_file(provider, workdir) && has_pack_metadata(provider, workdir)
        }
        PackState::Built => {
            validate_state_layout(provider, workdir, PackState::Configured)
                && has_canonical_build_artifacts(provider, workdir)
        }
        PackState::Cleaning => provider.is_directory(&artifact_root(workdir)),
    }
}

/// Discover current state from filesystem structure (pure function)
pub fn discover_state<P: crate::application::session::FileSystemProvider + ?Sized>(
    provider: &P,
    workdir: &Path,
) -> Result<PackState, StateError> {
    // Check if directory exists and is valid
    if !provider.is_directory(workdir) {
        return Err(StateError::InvalidDirectory {
            path: workdir.to_path_buf(),
        });
    }

    let empack_yml = workdir.join("empack.yml");
    let pack_toml = workdir.join("pack").join("pack.toml");
    let dist_dir = artifact_root(workdir);

    // Check for built state first (most advanced)
    if provider.is_directory(&dist_dir) {
        // Check if we have any build artifacts
        if provider.has_build_artifacts(&dist_dir).unwrap_or(false) {
            return Ok(PackState::Built);
        }
    }

    // Check for configured state
    if provider.is_directory(empack_yml.parent().unwrap()) {
        let files = provider.get_file_list(workdir).unwrap_or_default();
        if files.contains(&empack_yml) || files.contains(&pack_toml) {
            return Ok(PackState::Configured);
        }
    }

    // Default to uninitialized
    Ok(PackState::Uninitialized)
}

/// Check if state transition is valid (pure function)
pub fn can_transition(from: PackState, to: PackState) -> bool {
    match (from, to) {
        // Can always clean (go backwards)
        (PackState::Built, PackState::Configured) => true,
        (PackState::Configured, PackState::Uninitialized) => true,

        // Can advance through states
        (PackState::Uninitialized, PackState::Configured) => true,
        (PackState::Configured, PackState::Built) => true,
        (PackState::Built, PackState::Building) => true,

        // Can sync within configured state
        (PackState::Configured, PackState::Configured) => true,

        // Same state is always valid
        (a, b) if a == b => true,

        // All other transitions are invalid
        _ => false,
    }
}

/// Execute a state transition (pure function)
pub async fn execute_transition<P: crate::application::session::FileSystemProvider + ?Sized>(
    provider: &P,
    process: &dyn crate::application::session::ProcessProvider,
    workdir: &Path,
    transition: StateTransition<'_>,
) -> Result<PackState, StateError> {
    let current = discover_state(provider, workdir)?;

    match transition {
        StateTransition::Initialize(config) => {
            // Progressive init is only allowed from a transient config-only layout.
            // Once pack metadata or build artifacts exist, callers must use the
            // normal configured-state flows instead of re-running init.
            let can_initialize = matches!(current, PackState::Uninitialized)
                || (current == PackState::Configured
                    && is_progressive_init_state(provider, workdir));
            if !can_initialize {
                return Err(StateError::InvalidTransition {
                    from: current,
                    to: PackState::Configured,
                });
            }
            execute_initialize(
                provider,
                process,
                workdir,
                config.name,
                config.author,
                config.version,
                config.modloader,
                config.mc_version,
                config.loader_version,
            )
        }

        StateTransition::RefreshIndex => {
            if current != PackState::Configured
                || !validate_state_layout(provider, workdir, PackState::Configured)
            {
                return Err(StateError::InvalidTransition {
                    from: current,
                    to: PackState::Configured,
                });
            }
            execute_refresh_index(provider, process, workdir)
        }

        StateTransition::Build(orchestrator, targets) => {
            if !matches!(current, PackState::Configured | PackState::Built)
                || !validate_state_layout(provider, workdir, current)
            {
                return Err(StateError::InvalidTransition {
                    from: current,
                    to: PackState::Built,
                });
            }
            execute_build(orchestrator, &targets).await
        }

        StateTransition::Clean => match current {
            PackState::Built => {
                clean_build_artifacts(provider, workdir)?;
                Ok(PackState::Configured)
            }
            PackState::Configured => {
                clean_configuration(provider, workdir)?;
                Ok(PackState::Uninitialized)
            }
            PackState::Uninitialized => Ok(PackState::Uninitialized),
            PackState::Building => Err(StateError::InvalidTransition {
                from: current,
                to: PackState::Configured,
            }),
            PackState::Cleaning => Err(StateError::InvalidTransition {
                from: current,
                to: PackState::Configured,
            }),
        },

        StateTransition::Building => {
            if !matches!(current, PackState::Configured | PackState::Built) {
                return Err(StateError::InvalidTransition {
                    from: current,
                    to: PackState::Building,
                });
            }
            Ok(PackState::Building)
        }

        StateTransition::Cleaning => {
            if current != PackState::Built {
                return Err(StateError::InvalidTransition {
                    from: current,
                    to: PackState::Cleaning,
                });
            }
            Ok(PackState::Cleaning)
        }
    }
}

/// Execute initialization process (pure function)
#[allow(clippy::too_many_arguments)]
pub fn execute_initialize<P: crate::application::session::FileSystemProvider + ?Sized>(
    provider: &P,
    process: &dyn crate::application::session::ProcessProvider,
    workdir: &Path,
    name: &str,
    author: &str,
    version: &str,
    modloader: &str,
    mc_version: &str,
    loader_version: &str,
) -> Result<PackState, StateError> {
    // Create basic directory structure
    create_initial_structure(provider, workdir).inspect_err(|_| {
        let _ = clean_configuration(provider, workdir);
    })?;

    // Generate empack.yml via config.rs using session provider (only if it doesn't exist)
    let empack_yml = workdir.join("empack.yml");
    if !provider.exists(&empack_yml) {
        let config_manager = provider.config_manager(workdir.to_path_buf());
        let default_yml = config_manager
            .generate_default_empack_yml()
            .inspect_err(|_| {
                let _ = clean_configuration(provider, workdir);
            })?;

        provider
            .write_file(&empack_yml, &default_yml)
            .inspect_err(|_| {
                clean_configuration(provider, workdir).ok(); // Cleanup on failure
            })
            .context("Failed to write empack.yml")?;
    }

    // Run packwiz init
    provider
        .run_packwiz_init(
            process,
            workdir,
            name,
            author,
            version,
            modloader,
            mc_version,
            loader_version,
        )
        .inspect_err(|_| {
            let _ = clean_configuration(provider, workdir);
        })?;

    Ok(PackState::Configured)
}

/// Execute packwiz refresh for an already configured project
pub fn execute_refresh_index<P: crate::application::session::FileSystemProvider + ?Sized>(
    provider: &P,
    process: &dyn crate::application::session::ProcessProvider,
    workdir: &Path,
) -> Result<PackState, StateError> {
    // Validate configuration consistency using session provider
    let config_manager = provider.config_manager(workdir.to_path_buf());
    let issues = config_manager.validate_consistency()?;

    if !issues.is_empty() {
        for issue in issues {
            eprintln!("Warning: {}", issue);
        }
    }

    // Run packwiz refresh to sync mods
    provider.run_packwiz_refresh(process, workdir)?;

    Ok(PackState::Configured)
}

/// Execute build process (pure function)
pub async fn execute_build<'a>(
    mut orchestrator: crate::empack::builds::BuildOrchestrator<'a>,
    targets: &[BuildTarget],
) -> Result<PackState, StateError> {
    orchestrator
        .execute_build_pipeline(targets)
        .await
        .map_err(|e| StateError::BuildError { source: e })?;

    Ok(PackState::Built)
}

/// Create initial modpack structure (pure function)
pub fn create_initial_structure<P: crate::application::session::FileSystemProvider + ?Sized>(
    provider: &P,
    workdir: &Path,
) -> Result<(), StateError> {
    let pack_dir = workdir.join("pack");
    provider
        .create_dir_all(&pack_dir)
        .context("Failed to create pack directory")?;

    let template_dir = workdir.join("templates");
    provider
        .create_dir_all(&template_dir)
        .context("Failed to create templates directory")?;

    let installer_dir = workdir.join("installer");
    provider
        .create_dir_all(&installer_dir)
        .context("Failed to create installer directory")?;

    Ok(())
}

/// Clean build artifacts (pure function)
pub fn clean_build_artifacts<P: crate::application::session::FileSystemProvider + ?Sized>(
    provider: &P,
    workdir: &Path,
) -> Result<(), StateError> {
    let dist_dir = artifact_root(workdir);
    if provider.is_directory(&dist_dir) {
        provider.remove_dir_all(&dist_dir).context("Failed to remove dist directory")?;
    }
    Ok(())
}

/// Clean configuration files (pure function)
pub fn clean_configuration<P: crate::application::session::FileSystemProvider + ?Sized>(
    provider: &P,
    workdir: &Path,
) -> Result<(), StateError> {
    let empack_yml = workdir.join("empack.yml");
    let files = provider.get_file_list(workdir).unwrap_or_default();
    if files.contains(&empack_yml) {
        provider
            .remove_file(&empack_yml)
            .context("Failed to remove empack.yml")?;
    }

    let pack_dir = workdir.join("pack");
    if provider.is_directory(&pack_dir) {
        provider
            .remove_dir_all(&pack_dir)
            .context("Failed to remove pack directory")?;
    }

    let empack_dir = workdir.join(".empack");
    if provider.is_directory(&empack_dir) {
        provider
            .remove_dir_all(&empack_dir)
            .context("Failed to remove .empack directory")?;
    }

    Ok(())
}

/// Filesystem state machine for modpack development
/// Folder layout determines state - now generic over FileSystemProvider for testability
pub struct PackStateManager<'a, P: crate::application::session::FileSystemProvider + ?Sized> {
    /// Working directory (where empack.yml should be)
    pub workdir: PathBuf,
    /// Provider for all I/O operations
    provider: &'a P,
}

/// State detection errors
#[derive(Debug, Error)]
pub enum StateError {
    #[error("IO error")]
    IoError {
        #[from]
        source: anyhow::Error,
    },

    #[error("Invalid modpack directory: {path}")]
    InvalidDirectory { path: PathBuf },

    #[error("State transition not allowed: {from} -> {to}")]
    InvalidTransition { from: PackState, to: PackState },

    #[error("Missing required file: {file}")]
    MissingFile { file: PathBuf },

    #[error("Configuration error: {reason}")]
    ConfigError { reason: String },

    #[error("Build error")]
    BuildError {
        #[from]
        source: BuildError,
    },

    #[error("Config management error")]
    ConfigManagementError {
        #[from]
        source: ConfigError,
    },

    #[error("Command execution failed: {command}")]
    CommandFailed { command: String },
}

impl<'a, P: crate::application::session::FileSystemProvider + ?Sized> PackStateManager<'a, P> {
    /// Create a new state manager for the given directory with dependency injection
    pub fn new(workdir: PathBuf, provider: &'a P) -> Self {
        Self { workdir, provider }
    }

    /// Discover current state from filesystem
    pub fn discover_state(&self) -> Result<PackState, StateError> {
        discover_state(self.provider, &self.workdir)
    }

    /// Get expected files for each state
    pub fn get_state_files(&self, state: PackState) -> Vec<PathBuf> {
        match state {
            PackState::Uninitialized => vec![],
            PackState::Configured => vec![
                self.workdir.join("empack.yml"),
                self.workdir.join("pack").join("pack.toml"),
                self.workdir.join("pack").join("index.toml"),
            ],
            PackState::Built => {
                let mut files = self.get_state_files(PackState::Configured);
                files.push(self.workdir.join("dist"));
                files
            }
            PackState::Building => {
                // Same as Configured - intermediate state
                self.get_state_files(PackState::Configured)
            }
            PackState::Cleaning => {
                // Same as Built - intermediate state
                self.get_state_files(PackState::Built)
            }
        }
    }

    /// Validate that current filesystem matches expected state
    pub fn validate_state(&self, expected: PackState) -> Result<bool, StateError> {
        let current = self.discover_state()?;

        if current != expected {
            return Ok(false);
        }

        // Additional validation for configured/built states
        match expected {
            PackState::Uninitialized => Ok(true),
            PackState::Configured => Ok(validate_state_layout(
                self.provider,
                &self.workdir,
                PackState::Configured,
            )),
            PackState::Built => Ok(validate_state_layout(
                self.provider,
                &self.workdir,
                PackState::Built,
            )),
            PackState::Building => Ok(validate_state_layout(
                self.provider,
                &self.workdir,
                PackState::Building,
            )),
            PackState::Cleaning => Ok(validate_state_layout(
                self.provider,
                &self.workdir,
                PackState::Cleaning,
            )),
        }
    }

    /// Check if state transition is valid
    pub fn can_transition(&self, from: PackState, to: PackState) -> bool {
        can_transition(from, to)
    }

    /// Execute a state transition with full business logic integration
    pub async fn execute_transition(
        &self,
        process: &dyn crate::application::session::ProcessProvider,
        transition: StateTransition<'_>,
    ) -> Result<PackState, StateError> {
        execute_transition(self.provider, process, &self.workdir, transition).await
    }

    /// Begin a state transition (for BuildOrchestrator to use)
    pub fn begin_state_transition(
        &self,
        transition: StateTransition<'_>,
    ) -> Result<(), StateError> {
        let current = self.discover_state()?;

        // Validate that the transition is allowed
        match transition {
            StateTransition::Building => {
                if !matches!(current, PackState::Configured | PackState::Built)
                    || !validate_state_layout(self.provider, &self.workdir, current)
                {
                    return Err(StateError::InvalidTransition {
                        from: current,
                        to: PackState::Building,
                    });
                }
            }
            StateTransition::Cleaning => {
                if current != PackState::Built {
                    return Err(StateError::InvalidTransition {
                        from: current,
                        to: PackState::Cleaning,
                    });
                }
            }
            _ => {
                return Err(StateError::ConfigError {
                    reason:
                        "begin_state_transition only supports Building and Cleaning transitions"
                            .to_string(),
                });
            }
        }

        // TODO: In a full implementation, we might want to persist the intermediate state
        // For now, we just validate the transition is allowed
        Ok(())
    }

    /// Complete a state transition (for BuildOrchestrator to use)
    pub fn complete_state_transition(&self) -> Result<(), StateError> {
        // TODO: In a full implementation, we would:
        // 1. Check the current intermediate state (Building/Cleaning)
        // 2. Transition to the final state (Built/Configured)
        // 3. Persist the new state

        // For now, we just validate that the operation completed successfully
        Ok(())
    }

    /// Get paths for common modpack files
    pub fn paths(&self) -> PackPaths {
        PackPaths {
            workdir: self.workdir.clone(),
            empack_yml: self.workdir.join("empack.yml"),
            pack_dir: self.workdir.join("pack"),
            pack_toml: self.workdir.join("pack").join("pack.toml"),
            template_dir: self.workdir.join("templates"),
            dist_dir: artifact_root(&self.workdir),
        }
    }
}

// StateManager trait implementation removed - using concrete type directly

/// Common paths for modpack operations
#[derive(Debug, Clone)]
pub struct PackPaths {
    pub workdir: PathBuf,
    pub empack_yml: PathBuf,
    pub pack_dir: PathBuf,
    pub pack_toml: PathBuf,
    pub template_dir: PathBuf,
    pub dist_dir: PathBuf,
}

impl PackPaths {
    /// Get build output path for a specific target
    pub fn build_output(&self, target: BuildTarget) -> PathBuf {
        self.dist_dir.join(target.to_string())
    }
}

#[cfg(test)]
mod tests {
    include!("state.test.rs");
}
