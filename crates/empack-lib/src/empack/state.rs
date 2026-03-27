use crate::empack::builds::BuildError;
use crate::empack::config::ConfigError;
use crate::empack::packwiz::PackwizOps;
use crate::primitives::*;
use anyhow::Context;
use std::path::{Path, PathBuf};
use thiserror::Error;

// StateProvider trait removed - now using unified FileSystemProvider
// LiveStateProvider removed - now using LiveFileSystemProvider from session.rs

// LiveStateProvider implementation removed - now using LiveFileSystemProvider from session.rs

/// Marker file written to workdir during Building/Cleaning transitions.
/// If this file exists on next discovery, we know the previous operation was interrupted.
pub(crate) const STATE_MARKER_FILE: &str = ".empack-state";

// Pure business logic functions - zero I/O, 100% testable.
// These functions contain the core state machine logic without side effects.

/// Result of a state transition, carrying the new state and any warnings
/// that callers should surface through DisplayProvider.
#[derive(Debug, Clone)]
pub struct StateTransitionResult {
    pub state: PackState,
    pub warnings: Vec<String>,
}

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
    expected: &PackState,
) -> bool {
    match expected {
        PackState::Uninitialized => true,
        PackState::Configured | PackState::Building => {
            has_config_file(provider, workdir) && has_pack_metadata(provider, workdir)
        }
        PackState::Built => {
            validate_state_layout(provider, workdir, &PackState::Configured)
                && has_canonical_build_artifacts(provider, workdir)
        }
        PackState::Cleaning => provider.is_directory(&artifact_root(workdir)),
        PackState::Interrupted { was } => {
            let mut inner = was.as_ref();
            while let PackState::Interrupted { was: nested } = inner {
                inner = nested;
            }
            validate_state_layout(provider, workdir, inner)
        }
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

    // Check for interrupted state first (marker file presence)
    let marker_path = workdir.join(STATE_MARKER_FILE);
    if provider.exists(&marker_path) {
        let content = provider
            .read_to_string(&marker_path)
            .map_err(|e| StateError::IoError {
                source: anyhow::anyhow!("Failed to read state marker file: {}", e),
            })?;
        let inner = match content.trim() {
            "building" => PackState::Building,
            "cleaning" => PackState::Cleaning,
            other => {
                return Err(StateError::IoError {
                    source: anyhow::anyhow!("Unknown state marker content: '{}'", other),
                });
            }
        };
        return Ok(PackState::Interrupted {
            was: Box::new(inner),
        });
    }

    let empack_yml = workdir.join("empack.yml");
    let pack_toml = workdir.join("pack").join("pack.toml");
    let dist_dir = artifact_root(workdir);

    // Check for built state first (most advanced)
    if provider.is_directory(&dist_dir) && provider.has_build_artifacts(&dist_dir).unwrap_or(false)
    {
        return Ok(PackState::Built);
    }

    // Check for configured state via direct exists() calls
    if provider.exists(&empack_yml) || provider.exists(&pack_toml) {
        return Ok(PackState::Configured);
    }

    // Default to uninitialized
    Ok(PackState::Uninitialized)
}

/// Check if an orchestrated state transition is valid (pure whitelist, no layout check).
/// Use for tests, UI queries ("can I show a build button?"), and advisory checks.
pub fn can_transition(from: &PackState, kind: TransitionKind) -> bool {
    match (from, kind) {
        // Initialize: from Uninitialized always, from Configured needs layout (handled by _with_layout)
        (PackState::Uninitialized, TransitionKind::Initialize) => true,
        (PackState::Configured, TransitionKind::Initialize) => true,

        // RefreshIndex: must be Configured, or recovering from interrupted build
        (PackState::Configured, TransitionKind::RefreshIndex) => true,
        (PackState::Interrupted { was }, TransitionKind::RefreshIndex)
            if matches!(was.as_ref(), PackState::Building) =>
        {
            true
        }

        // Build (full): Configured, Built, or retry after interrupted build
        (PackState::Configured | PackState::Built, TransitionKind::Build) => true,
        (PackState::Interrupted { was }, TransitionKind::Build)
            if matches!(was.as_ref(), PackState::Building) =>
        {
            true
        }

        // Clean: from Built, Configured, Uninitialized (idempotent), or Interrupted (recovery)
        (PackState::Built, TransitionKind::Clean) => true,
        (PackState::Configured, TransitionKind::Clean) => true,
        (PackState::Uninitialized, TransitionKind::Clean) => true,
        (PackState::Interrupted { .. }, TransitionKind::Clean) => true,

        _ => false,
    }
}

/// Check if an orchestrated state transition is valid with filesystem layout validation.
/// Use in production code paths where disk state must be verified.
pub fn can_transition_with_layout(
    from: &PackState,
    kind: TransitionKind,
    layout_ok: &dyn Fn(&PackState) -> bool,
) -> bool {
    // Must pass the pure whitelist first
    if !can_transition(from, kind) {
        return false;
    }
    // Then check layout for transitions that require it
    match (from, kind) {
        // Initialize from Uninitialized is always valid (fresh init, no layout to check)
        (PackState::Uninitialized, TransitionKind::Initialize) => true,
        // Progressive re-init from Configured needs layout validation
        (_, TransitionKind::Initialize) => layout_ok(from),
        (_, TransitionKind::RefreshIndex) => layout_ok(from),
        (_, TransitionKind::Build) => layout_ok(from),
        // Clean transitions skip layout validation
        (_, TransitionKind::Clean) => true,
    }
}

/// Check if a marker transition is valid. Layout validation is always required --
/// marker transitions are internal and must verify disk state independently.
pub(crate) fn can_enter_marker(
    from: &PackState,
    marker: MarkerKind,
    layout_ok: &dyn Fn(&PackState) -> bool,
) -> bool {
    match (from, marker) {
        // Building: same states as Build, but layout always checked
        (PackState::Configured | PackState::Built, MarkerKind::Building) => layout_ok(from),
        // Building retry: allow re-entering Building after an interrupted build
        (PackState::Interrupted { was }, MarkerKind::Building)
            if matches!(was.as_ref(), PackState::Building) =>
        {
            layout_ok(from)
        }
        // Cleaning: only from Built, layout must confirm built state
        (PackState::Built, MarkerKind::Cleaning) => layout_ok(from),
        _ => false,
    }
}

/// Write the state marker file to indicate an intermediate operation is in progress.
fn write_state_marker<P: crate::application::session::FileSystemProvider + ?Sized>(
    provider: &P,
    workdir: &Path,
    state_label: &str,
) -> Result<(), StateError> {
    provider
        .write_file(&workdir.join(STATE_MARKER_FILE), state_label)
        .context("Failed to write state marker file")?;
    Ok(())
}

/// Remove the state marker file after a successful transition.
/// Treats "not found" as success to avoid TOCTOU races.
fn remove_state_marker<P: crate::application::session::FileSystemProvider + ?Sized>(
    provider: &P,
    workdir: &Path,
) -> Result<(), StateError> {
    let marker_path = workdir.join(STATE_MARKER_FILE);
    match provider.remove_file(&marker_path) {
        Ok(()) => Ok(()),
        Err(e) => {
            // If the file was already gone, that's fine -- the goal is absence.
            if let Some(io_err) = e.downcast_ref::<std::io::Error>()
                && io_err.kind() == std::io::ErrorKind::NotFound
            {
                return Ok(());
            }
            Err(StateError::IoError {
                source: e.context("Failed to remove state marker file"),
            })
        }
    }
}

/// RAII guard that writes a state marker file on creation.
/// Call `complete()` after successful operations to remove the marker explicitly.
/// If the guard is dropped without calling `complete()` (e.g., due to error or panic),
/// it leaves the marker in place so `discover_state()` correctly reports `Interrupted`.
#[must_use = "dropping the guard without complete() leaves the marker, signalling interruption"]
pub(crate) struct StateMarkerGuard<'a, P: crate::application::session::FileSystemProvider + ?Sized>
{
    provider: &'a P,
    workdir: PathBuf,
    active: bool,
}

impl<'a, P: crate::application::session::FileSystemProvider + ?Sized> StateMarkerGuard<'a, P> {
    /// Create a new guard, writing the state marker immediately.
    pub(crate) fn new(
        provider: &'a P,
        workdir: PathBuf,
        state_label: &str,
    ) -> Result<Self, StateError> {
        write_state_marker(provider, &workdir, state_label)?;
        Ok(Self {
            provider,
            workdir,
            active: true,
        })
    }

    /// Complete the guarded operation successfully: remove the marker and disarm.
    pub(crate) fn complete(mut self) -> Result<(), StateError> {
        remove_state_marker(self.provider, &self.workdir)?;
        self.active = false;
        Ok(())
    }
}

impl<P: crate::application::session::FileSystemProvider + ?Sized> Drop for StateMarkerGuard<'_, P> {
    fn drop(&mut self) {
        if self.active {
            // Marker left in place intentionally: the operation did not complete
            // successfully, so discover_state() should report Interrupted.
            tracing::warn!(
                "State marker left in place at {:?} -- operation did not complete",
                self.workdir.join(STATE_MARKER_FILE)
            );
        }
    }
}

/// Execute a state transition (pure function)
pub async fn execute_transition<P: crate::application::session::FileSystemProvider + ?Sized>(
    provider: &P,
    _process: &dyn crate::application::session::ProcessProvider,
    packwiz: &dyn PackwizOps,
    workdir: &Path,
    transition: StateTransition<'_>,
) -> Result<StateTransitionResult, StateError> {
    let current = discover_state(provider, workdir)?;

    let no_warnings = |state| {
        Ok(StateTransitionResult {
            state,
            warnings: vec![],
        })
    };

    match transition {
        StateTransition::Initialize(config) => {
            // Progressive init needs a layout check beyond the whitelist:
            // only allow re-init when no pack metadata or build artifacts exist yet.
            let layout_ok = |_state: &PackState| is_progressive_init_state(provider, workdir);
            if !can_transition_with_layout(&current, TransitionKind::Initialize, &layout_ok) {
                return Err(StateError::InvalidTransition {
                    from: current,
                    to: PackState::Configured,
                });
            }
            execute_initialize(
                provider,
                packwiz,
                workdir,
                config.name,
                config.author,
                config.version,
                config.modloader,
                config.mc_version,
                config.loader_version,
            )
            .map(|state| StateTransitionResult {
                state,
                warnings: vec![],
            })
        }

        StateTransition::RefreshIndex => {
            let layout_ok = |state: &PackState| validate_state_layout(provider, workdir, state);
            if !can_transition_with_layout(&current, TransitionKind::RefreshIndex, &layout_ok) {
                return Err(StateError::InvalidTransition {
                    from: current,
                    to: PackState::Configured,
                });
            }
            execute_refresh_index(provider, packwiz, workdir)
        }

        StateTransition::Build(orchestrator, targets) => {
            let layout_ok = |state: &PackState| validate_state_layout(provider, workdir, state);
            if !can_transition_with_layout(&current, TransitionKind::Build, &layout_ok) {
                return Err(StateError::InvalidTransition {
                    from: current,
                    to: PackState::Built,
                });
            }
            // Marker writing is handled by BuildOrchestrator::execute_build_pipeline
            // via begin_state_transition/complete_state_transition
            execute_build(orchestrator, &targets)
                .await
                .map(|state| StateTransitionResult {
                    state,
                    warnings: vec![],
                })
        }

        StateTransition::Clean => {
            if !can_transition(&current, TransitionKind::Clean) {
                return Err(StateError::InvalidTransition {
                    from: current,
                    to: PackState::Configured,
                });
            }
            match current {
                PackState::Built => {
                    write_state_marker(provider, workdir, "cleaning")?;
                    clean_build_artifacts(provider, workdir)?;
                    remove_state_marker(provider, workdir)?;
                    no_warnings(PackState::Configured)
                }
                PackState::Configured => {
                    clean_configuration(provider, workdir)?;
                    remove_state_marker(provider, workdir)?;
                    no_warnings(PackState::Uninitialized)
                }
                PackState::Uninitialized => no_warnings(PackState::Uninitialized),
                PackState::Interrupted { .. } => {
                    // Recovery: remove the marker and clean from the underlying state
                    remove_state_marker(provider, workdir)?;
                    // After removing the marker, re-discover the actual filesystem state
                    let recovered = discover_state(provider, workdir)?;
                    match recovered {
                        PackState::Built => {
                            clean_build_artifacts(provider, workdir)?;
                            no_warnings(PackState::Configured)
                        }
                        PackState::Configured => {
                            clean_configuration(provider, workdir)?;
                            no_warnings(PackState::Uninitialized)
                        }
                        PackState::Uninitialized => no_warnings(PackState::Uninitialized),
                        _ => no_warnings(recovered),
                    }
                }
                PackState::Building | PackState::Cleaning => {
                    unreachable!("can_transition rejects Building/Cleaning for Clean")
                }
            }
        }
    }
}

/// Execute initialization process (pure function)
#[allow(clippy::too_many_arguments)]
pub fn execute_initialize<P: crate::application::session::FileSystemProvider + ?Sized>(
    provider: &P,
    packwiz: &dyn PackwizOps,
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
    packwiz
        .run_packwiz_init(
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
    packwiz: &dyn PackwizOps,
    workdir: &Path,
) -> Result<StateTransitionResult, StateError> {
    // Validate configuration consistency using session provider
    let config_manager = provider.config_manager(workdir.to_path_buf());
    let issues = config_manager.validate_consistency()?;

    let warnings: Vec<String> = issues
        .into_iter()
        .map(|issue| format!("Warning: {}", issue))
        .collect();

    // Run packwiz refresh to sync mods
    packwiz.run_packwiz_refresh(workdir)?;

    Ok(StateTransitionResult {
        state: PackState::Configured,
        warnings,
    })
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

    Ok(())
}

/// Clean build artifacts (pure function)
pub fn clean_build_artifacts<P: crate::application::session::FileSystemProvider + ?Sized>(
    provider: &P,
    workdir: &Path,
) -> Result<(), StateError> {
    let dist_dir = artifact_root(workdir);
    if provider.is_directory(&dist_dir) {
        provider
            .remove_dir_all(&dist_dir)
            .context("Failed to remove dist directory")?;
    }
    Ok(())
}

/// Clean configuration files (pure function)
pub fn clean_configuration<P: crate::application::session::FileSystemProvider + ?Sized>(
    provider: &P,
    workdir: &Path,
) -> Result<(), StateError> {
    let empack_yml = workdir.join("empack.yml");
    if provider.exists(&empack_yml) {
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
            PackState::Interrupted { was } => {
                let mut inner = *was;
                while let PackState::Interrupted { was: nested } = inner {
                    inner = *nested;
                }
                self.get_state_files(inner)
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
        Ok(validate_state_layout(
            self.provider,
            &self.workdir,
            &expected,
        ))
    }

    /// Execute a state transition with full business logic integration
    pub async fn execute_transition(
        &self,
        process: &dyn crate::application::session::ProcessProvider,
        packwiz: &dyn PackwizOps,
        transition: StateTransition<'_>,
    ) -> Result<StateTransitionResult, StateError> {
        execute_transition(self.provider, process, packwiz, &self.workdir, transition).await
    }

    /// Validate a marker transition and return the state label string for marker files.
    /// Delegates state-validity checks to `can_enter_marker`.
    fn validate_transition(&self, marker: MarkerKind) -> Result<&'static str, StateError> {
        let current = self.discover_state()?;

        let (label, target_state) = match marker {
            MarkerKind::Building => ("building", PackState::Building),
            MarkerKind::Cleaning => ("cleaning", PackState::Cleaning),
        };

        let layout_ok =
            |state: &PackState| validate_state_layout(self.provider, &self.workdir, state);
        if !can_enter_marker(&current, marker, &layout_ok) {
            return Err(StateError::InvalidTransition {
                from: current,
                to: target_state,
            });
        }

        Ok(label)
    }

    /// Begin a marker transition without RAII guard. Only used in tests --
    /// production code should use `guarded_transition` for automatic cleanup.
    #[cfg(test)]
    pub(crate) fn begin_state_transition(&self, marker: MarkerKind) -> Result<(), StateError> {
        let state_label = self.validate_transition(marker)?;
        write_state_marker(self.provider, &self.workdir, state_label)
    }

    /// Complete a state transition (removes marker file)
    pub fn complete_state_transition(&self) -> Result<(), StateError> {
        remove_state_marker(self.provider, &self.workdir)
    }

    /// Begin a marker transition with RAII guard. The returned guard leaves the
    /// marker on Drop if not explicitly completed, so `discover_state()` reports `Interrupted`.
    pub(crate) fn guarded_transition(
        &self,
        marker: MarkerKind,
    ) -> Result<StateMarkerGuard<'_, P>, StateError> {
        let state_label = self.validate_transition(marker)?;
        StateMarkerGuard::new(self.provider, self.workdir.clone(), state_label)
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
