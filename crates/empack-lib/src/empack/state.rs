use crate::empack::builds::{BuildError, BuildOrchestrator};
use crate::empack::config::{ConfigError, ConfigManager};
use crate::primitives::*;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use thiserror::Error;

/// Handle trait for abstracting all I/O operations required by the state machine
/// This enables pure business logic testing without filesystem dependencies
pub trait StateProvider {
    /// Check if a path exists and is a directory
    fn is_directory(&self, path: &Path) -> Result<bool, std::io::Error>;

    /// Get list of files and directories in a path
    fn get_file_list(&self, path: &Path) -> Result<HashSet<PathBuf>, std::io::Error>;

    /// Check if directory has build artifacts (mrpack, zip, jar files or build target dirs)
    fn has_build_artifacts(&self, dist_dir: &Path) -> Result<bool, std::io::Error>;

    /// Create directory and all parent directories
    fn create_dir_all(&self, path: &Path) -> Result<(), std::io::Error>;

    /// Write content to a file
    fn write_file(&self, path: &Path, content: &str) -> Result<(), std::io::Error>;

    /// Remove a file
    fn remove_file(&self, path: &Path) -> Result<(), std::io::Error>;

    /// Remove a directory and all its contents
    fn remove_dir_all(&self, path: &Path) -> Result<(), std::io::Error>;

    /// Run packwiz init command
    fn run_packwiz_init(&self, workdir: &Path) -> Result<(), StateError>;

    /// Run packwiz refresh command
    fn run_packwiz_refresh(&self, workdir: &Path) -> Result<(), StateError>;
}

/// Production implementation of StateProvider using real filesystem and commands
pub struct LiveStateProvider;

impl StateProvider for LiveStateProvider {
    fn is_directory(&self, path: &Path) -> Result<bool, std::io::Error> {
        Ok(path.exists() && path.is_dir())
    }

    fn get_file_list(&self, path: &Path) -> Result<HashSet<PathBuf>, std::io::Error> {
        let mut files = HashSet::new();

        if !path.exists() {
            return Ok(files);
        }

        let entries = fs::read_dir(path)?;
        for entry in entries {
            let entry = entry?;
            files.insert(entry.path());
        }

        Ok(files)
    }

    fn has_build_artifacts(&self, dist_dir: &Path) -> Result<bool, std::io::Error> {
        if !dist_dir.exists() {
            return Ok(false);
        }

        let entries = fs::read_dir(dist_dir)?;
        for entry in entries {
            let entry = entry?;
            let path = entry.path();

            // Look for common build artifacts (files)
            if path.is_file() {
                if let Some(extension) = path.extension() {
                    match extension.to_str() {
                        Some("mrpack") | Some("zip") | Some("jar") => return Ok(true),
                        _ => continue,
                    }
                }
            }

            // Also consider build target directories as evidence of build state
            if path.is_dir() {
                if let Some(dir_name) = path.file_name().and_then(|n| n.to_str()) {
                    match dir_name {
                        "mrpack" | "client" | "server" | "client-full" | "server-full" => {
                            return Ok(true);
                        }
                        _ => continue,
                    }
                }
            }
        }

        Ok(false)
    }

    fn create_dir_all(&self, path: &Path) -> Result<(), std::io::Error> {
        fs::create_dir_all(path)
    }

    fn write_file(&self, path: &Path, content: &str) -> Result<(), std::io::Error> {
        fs::write(path, content)
    }

    fn remove_file(&self, path: &Path) -> Result<(), std::io::Error> {
        fs::remove_file(path)
    }

    fn remove_dir_all(&self, path: &Path) -> Result<(), std::io::Error> {
        fs::remove_dir_all(path)
    }

    fn run_packwiz_init(&self, workdir: &Path) -> Result<(), StateError> {
        #[cfg(test)]
        {
            // Mock packwiz init - create expected files
            let pack_dir = workdir.join("pack");
            self.create_dir_all(&pack_dir)?;

            let pack_file = pack_dir.join("pack.toml");
            let default_pack_toml = r#"name = "Test Modpack"
author = "Test Author"
version = "1.0.0"
pack-format = "packwiz:1.1.0"

[index]
file = "index.toml"
hash-format = "sha256"
hash = ""

[versions]
minecraft = "1.20.1"
fabric = "0.14.21"
"#;
            self.write_file(&pack_file, default_pack_toml)?;

            // Also create index.toml
            let index_file = pack_dir.join("index.toml");
            let default_index = r#"hash-format = "sha256"

[[files]]
file = "pack.toml"
hash = ""
"#;
            self.write_file(&index_file, default_index)?;
            return Ok(());
        }

        #[cfg(not(test))]
        {
            let pack_file = workdir.join("pack").join("pack.toml");

            let status = Command::new("packwiz")
                .args(&["init", "--pack-file", pack_file.to_str().unwrap()])
                .current_dir(workdir)
                .status()
                .map_err(|e| StateError::CommandFailed {
                    command: format!("packwiz init failed: {}", e),
                })?;

            if !status.success() {
                return Err(StateError::CommandFailed {
                    command: "packwiz init returned non-zero".to_string(),
                });
            }

            Ok(())
        }
    }

    fn run_packwiz_refresh(&self, workdir: &Path) -> Result<(), StateError> {
        #[cfg(test)]
        {
            // Mock packwiz refresh - verify pack.toml exists
            let pack_file = workdir.join("pack").join("pack.toml");
            if !pack_file.exists() {
                return Err(StateError::MissingFile {
                    file: "pack.toml".to_string(),
                });
            }
            return Ok(());
        }

        #[cfg(not(test))]
        {
            let pack_file = workdir.join("pack").join("pack.toml");

            let status = Command::new("packwiz")
                .args(&["--pack-file", pack_file.to_str().unwrap(), "refresh"])
                .current_dir(workdir)
                .status()
                .map_err(|e| StateError::CommandFailed {
                    command: format!("packwiz refresh failed: {}", e),
                })?;

            if !status.success() {
                return Err(StateError::CommandFailed {
                    command: "packwiz refresh returned non-zero".to_string(),
                });
            }

            Ok(())
        }
    }
}

/// Pure business logic functions - zero I/O, 100% testable
/// These functions contain the core state machine logic without side effects

/// Discover current state from filesystem structure (pure function)
pub fn discover_state<P: StateProvider>(
    provider: &P,
    workdir: &Path,
) -> Result<ModpackState, StateError> {
    // Check if directory exists and is valid
    if !provider.is_directory(workdir)? {
        return Err(StateError::InvalidDirectory {
            path: workdir.display().to_string(),
        });
    }

    let empack_yml = workdir.join("empack.yml");
    let pack_toml = workdir.join("pack").join("pack.toml");
    let dist_dir = workdir.join("dist");

    // Check for built state first (most advanced)
    if provider.is_directory(&dist_dir)? {
        // Check if we have any build artifacts
        if provider.has_build_artifacts(&dist_dir)? {
            return Ok(ModpackState::Built);
        }
    }

    // Check for configured state
    if provider.is_directory(&empack_yml.parent().unwrap())? {
        let files = provider.get_file_list(workdir)?;
        if files.contains(&empack_yml) || files.contains(&pack_toml) {
            return Ok(ModpackState::Configured);
        }
    }

    // Default to uninitialized
    Ok(ModpackState::Uninitialized)
}

/// Check if state transition is valid (pure function)
pub fn can_transition(from: ModpackState, to: ModpackState) -> bool {
    match (from, to) {
        // Can always clean (go backwards)
        (ModpackState::Built, ModpackState::Configured) => true,
        (ModpackState::Configured, ModpackState::Uninitialized) => true,

        // Can advance through states
        (ModpackState::Uninitialized, ModpackState::Configured) => true,
        (ModpackState::Configured, ModpackState::Built) => true,

        // Can sync within configured state
        (ModpackState::Configured, ModpackState::Configured) => true,

        // Same state is always valid
        (a, b) if a == b => true,

        // All other transitions are invalid
        _ => false,
    }
}

/// Filesystem state machine for modpack development
/// Folder layout determines state - now generic over StateProvider for testability
pub struct ModpackStateManager<'a, P: StateProvider> {
    /// Working directory (where empack.yml should be)
    pub workdir: PathBuf,
    /// Provider for all I/O operations
    provider: &'a P,
}

/// State detection errors
#[derive(Debug, Error, Clone)]
pub enum StateError {
    #[error("IO error: {message}")]
    IoError { message: String },

    #[error("Invalid modpack directory: {path}")]
    InvalidDirectory { path: String },

    #[error("State transition not allowed: {from} -> {to}")]
    InvalidTransition {
        from: ModpackState,
        to: ModpackState,
    },

    #[error("Missing required file: {file}")]
    MissingFile { file: String },

    #[error("Configuration error: {reason}")]
    ConfigError { reason: String },

    #[error("Build error: {message}")]
    BuildError { message: String },

    #[error("Config management error: {message}")]
    ConfigManagementError { message: String },

    #[error("Command execution failed: {command}")]
    CommandFailed { command: String },
}

// Implement From traits for error conversion
impl From<std::io::Error> for StateError {
    fn from(err: std::io::Error) -> Self {
        StateError::IoError {
            message: err.to_string(),
        }
    }
}

impl From<BuildError> for StateError {
    fn from(err: BuildError) -> Self {
        StateError::BuildError {
            message: err.to_string(),
        }
    }
}

impl From<ConfigError> for StateError {
    fn from(err: ConfigError) -> Self {
        StateError::ConfigManagementError {
            message: err.to_string(),
        }
    }
}

impl<'a, P: StateProvider> ModpackStateManager<'a, P> {
    /// Create a new state manager for the given directory with dependency injection
    pub fn new(workdir: PathBuf, provider: &'a P) -> Self {
        Self { workdir, provider }
    }

    /// Discover current state from filesystem
    pub fn discover_state(&self) -> Result<ModpackState, StateError> {
        discover_state(self.provider, &self.workdir)
    }

    /// Get expected files for each state
    pub fn get_state_files(&self, state: ModpackState) -> Vec<PathBuf> {
        match state {
            ModpackState::Uninitialized => vec![],
            ModpackState::Configured => vec![
                self.workdir.join("empack.yml"),
                self.workdir.join("pack").join("pack.toml"),
                self.workdir.join("pack").join("index.toml"),
            ],
            ModpackState::Built => {
                let mut files = self.get_state_files(ModpackState::Configured);
                files.push(self.workdir.join("dist"));
                files
            }
        }
    }

    /// Validate that current filesystem matches expected state
    pub fn validate_state(&self, expected: ModpackState) -> Result<bool, StateError> {
        let current = self.discover_state()?;

        if current != expected {
            return Ok(false);
        }

        // Additional validation for configured/built states
        match expected {
            ModpackState::Uninitialized => Ok(true),
            ModpackState::Configured => {
                let pack_dir = self.workdir.join("pack");
                Ok(self.provider.is_directory(&pack_dir)?)
            }
            ModpackState::Built => {
                let dist_dir = self.workdir.join(".empack").join("dist");
                Ok(self.provider.is_directory(&dist_dir)?
                    && self.provider.has_build_artifacts(&dist_dir)?)
            }
        }
    }

    /// Check if state transition is valid
    pub fn can_transition(&self, from: ModpackState, to: ModpackState) -> bool {
        can_transition(from, to)
    }

    /// Execute a state transition with full business logic integration
    pub fn execute_transition(
        &self,
        transition: StateTransition,
    ) -> Result<ModpackState, StateError> {
        let current = self.discover_state()?;

        match transition {
            StateTransition::Initialize => {
                if !self.can_transition(current, ModpackState::Configured) {
                    return Err(StateError::InvalidTransition {
                        from: current,
                        to: ModpackState::Configured,
                    });
                }

                // Execute with cleanup on failure
                self.execute_initialize_with_cleanup()
            }

            StateTransition::Synchronize => {
                if current != ModpackState::Configured {
                    return Err(StateError::InvalidTransition {
                        from: current,
                        to: ModpackState::Configured,
                    });
                }

                // Execute config reconciliation
                self.execute_synchronize()
            }

            StateTransition::Build(targets) => {
                if !self.can_transition(current, ModpackState::Built) {
                    return Err(StateError::InvalidTransition {
                        from: current,
                        to: ModpackState::Built,
                    });
                }

                // Execute with cleanup on failure
                self.execute_build_with_cleanup(&targets)
            }

            StateTransition::Clean => match current {
                ModpackState::Built => {
                    self.clean_build_artifacts()?;
                    Ok(ModpackState::Configured)
                }
                ModpackState::Configured => {
                    self.clean_configuration()?;
                    Ok(ModpackState::Uninitialized)
                }
                ModpackState::Uninitialized => Ok(ModpackState::Uninitialized),
            },
        }
    }

    /// Execute initialization with error cleanup
    fn execute_initialize_with_cleanup(&self) -> Result<ModpackState, StateError> {
        // 1. Create basic directory structure
        self.create_initial_structure()?;

        // 2. Generate empack.yml via config.rs
        let config_manager = ConfigManager::new(self.workdir.clone());
        let default_yml = config_manager.generate_default_empack_yml().map_err(|e| {
            self.clean_configuration().ok(); // Cleanup on failure
            e
        })?;

        let empack_yml = self.workdir.join("empack.yml");
        self.provider
            .write_file(&empack_yml, &default_yml)
            .map_err(|e| {
                self.clean_configuration().ok(); // Cleanup on failure
                StateError::IoError { message: e.to_string() }
            })?;

        // 3. Run packwiz init
        self.run_packwiz_init().map_err(|e| {
            self.clean_configuration().ok(); // Cleanup on failure
            e
        })?;

        Ok(ModpackState::Configured)
    }

    /// Execute synchronization (config reconciliation)
    fn execute_synchronize(&self) -> Result<ModpackState, StateError> {
        // Validate configuration consistency
        let config_manager = ConfigManager::new(self.workdir.clone());
        let issues = config_manager.validate_consistency()?;

        if !issues.is_empty() {
            for issue in issues {
                eprintln!("Warning: {}", issue);
            }
        }

        // Run packwiz refresh to sync mods
        self.run_packwiz_refresh()?;

        Ok(ModpackState::Configured)
    }

    /// Execute build with error cleanup
    fn execute_build_with_cleanup(
        &self,
        targets: &[BuildTarget],
    ) -> Result<ModpackState, StateError> {
        let initial_state = self.discover_state()?;

        // 1. Load project plan via config.rs
        #[cfg(not(test))]
        {
            let config_manager = ConfigManager::new(self.workdir.clone());
            let _project_plan = config_manager.create_project_plan().map_err(|e| {
                self.revert_to_state(initial_state).ok();
                e
            })?;
        }

        // 2. Execute build pipeline via builds.rs
        #[cfg(test)]
        {
            // Mock build execution for testing
            let dist_dir = self.workdir.join("dist");
            self.provider.create_dir_all(&dist_dir)?;

            for target in targets {
                let target_dir = dist_dir.join(target.to_string().to_lowercase());
                self.provider.create_dir_all(&target_dir)?;

                // Create a dummy artifact to simulate successful build
                let artifact_name = match target {
                    BuildTarget::Mrpack => "test-v1.0.0.mrpack",
                    BuildTarget::Client => "test-v1.0.0-client.zip",
                    BuildTarget::Server => "test-v1.0.0-server.zip",
                    BuildTarget::ClientFull => "test-v1.0.0-client-full.zip",
                    BuildTarget::ServerFull => "test-v1.0.0-server-full.zip",
                };
                let artifact_path = dist_dir.join(artifact_name);
                self.provider
                    .write_file(&artifact_path, "mock build artifact")?;
            }
        }

        #[cfg(not(test))]
        {
            let mut build_orchestrator = BuildOrchestrator::new(self.workdir.clone());

            // Use tokio::runtime::Handle::current() for async execution in sync context
            let result = if let Ok(handle) = tokio::runtime::Handle::try_current() {
                // We're in an async context
                tokio::task::block_in_place(|| {
                    handle.block_on(build_orchestrator.execute_build_pipeline(targets))
                })
            } else {
                // We're not in an async context - create minimal runtime
                let rt = tokio::runtime::Runtime::new().map_err(|e| {
                    self.revert_to_state(initial_state).ok();
                    StateError::ConfigError {
                        reason: format!("Failed to create async runtime: {}", e),
                    }
                })?;
                rt.block_on(build_orchestrator.execute_build_pipeline(targets))
            };

            let build_results = result.map_err(|e| {
                self.revert_to_state(initial_state).ok();
                e
            })?;

            // Check if any builds failed
            for result in &build_results {
                if !result.success {
                    eprintln!(
                        "Build failed for target {:?}: {:?}",
                        result.target, result.warnings
                    );
                    self.revert_to_state(initial_state).ok();
                    return Err(StateError::ConfigError {
                        reason: format!("Build failed for target {:?}", result.target),
                    });
                }
            }
        }

        Ok(ModpackState::Built)
    }

    /// Run packwiz init command
    fn run_packwiz_init(&self) -> Result<(), StateError> {
        self.provider.run_packwiz_init(&self.workdir)
    }

    /// Run packwiz refresh command
    fn run_packwiz_refresh(&self) -> Result<(), StateError> {
        self.provider.run_packwiz_refresh(&self.workdir)
    }

    /// Revert to previous state on error
    fn revert_to_state(&self, target_state: ModpackState) -> Result<(), StateError> {
        match target_state {
            ModpackState::Uninitialized => {
                // Remove all configuration
                self.clean_configuration()
            }
            ModpackState::Configured => {
                // Just remove build artifacts
                self.clean_build_artifacts()
            }
            ModpackState::Built => {
                // Already in valid state
                Ok(())
            }
        }
    }

    /// Create initial modpack structure
    fn create_initial_structure(&self) -> Result<(), StateError> {
        let pack_dir = self.workdir.join("pack");
        self.provider.create_dir_all(&pack_dir)?;

        let template_dir = self.workdir.join("templates");
        self.provider.create_dir_all(&template_dir)?;

        let installer_dir = self.workdir.join("installer");
        self.provider.create_dir_all(&installer_dir)?;

        // Create initial empack.yml if it doesn't exist
        let empack_yml = self.workdir.join("empack.yml");
        if !self.provider.is_directory(&empack_yml)?
            && !self
                .provider
                .get_file_list(&self.workdir)?
                .contains(&empack_yml)
        {
            self.provider
                .write_file(&empack_yml, &self.default_empack_yml())?;
        }

        Ok(())
    }

    /// Create build directory structure
    fn create_build_structure(&self, _targets: &[BuildTarget]) -> Result<(), StateError> {
        let dist_dir = self.workdir.join(".empack").join("dist");
        self.provider.create_dir_all(&dist_dir)?;

        // Create target-specific directories
        for target in [
            BuildTarget::Mrpack,
            BuildTarget::Client,
            BuildTarget::Server,
            BuildTarget::ClientFull,
            BuildTarget::ServerFull,
        ] {
            let target_dir = dist_dir.join(target.to_string());
            self.provider.create_dir_all(&target_dir)?;
        }

        Ok(())
    }

    /// Clean build artifacts
    fn clean_build_artifacts(&self) -> Result<(), StateError> {
        let dist_dir = self.workdir.join("dist");
        if self.provider.is_directory(&dist_dir)? {
            self.provider.remove_dir_all(&dist_dir)?;
        }
        Ok(())
    }

    /// Clean configuration files
    fn clean_configuration(&self) -> Result<(), StateError> {
        let empack_yml = self.workdir.join("empack.yml");
        let files = self.provider.get_file_list(&self.workdir)?;
        if files.contains(&empack_yml) {
            self.provider.remove_file(&empack_yml)?;
        }

        let pack_dir = self.workdir.join("pack");
        if self.provider.is_directory(&pack_dir)? {
            self.provider.remove_dir_all(&pack_dir)?;
        }

        let empack_dir = self.workdir.join(".empack");
        if self.provider.is_directory(&empack_dir)? {
            self.provider.remove_dir_all(&empack_dir)?;
        }

        Ok(())
    }

    /// Default empack.yml content
    fn default_empack_yml(&self) -> String {
        r#"empack:
  # Project dependencies - user-level definitions
  # Format: key: "search_query|project_type|minecraft_version|loader"
  # Key becomes internal reference, value defines Modrinth search
  dependencies:
    # Core Dependencies
    - fabric_api: "Fabric API|mod"
    - sodium: "Sodium|mod"

    # Quality of Life
    - appleskin: "AppleSkin|mod|1.20.1|fabric"
    - jade: "Jade|mod"

    # Performance
    - lithium: "Lithium|mod"
    - modernfix: "ModernFix|mod"

    # Datapacks
    - example_datapack: "Example Datapack|datapack"

    # Resource Packs
    - example_resourcepack: "Example Resource Pack|resourcepack"

  # User-provided project ID mappings
  # Format: key: "modrinth_project_id"
  # Keys reference the dependency keys above
  project_ids:
    # Examples - populate as needed for performance/reliability
    # fabric_api: "P7dR8mSH"
    # sodium: "AANobbMI"

  # Version overrides for specific projects
  # Format: key: "version_id" or ["version_id1", "version_id2"]
  # Keys reference the dependency keys above
  # NOTE: Use actual Modrinth/CurseForge version IDs, not strings like "latest"
  version_overrides:
    # Example with multiple version IDs for compatibility:
    # example_mod:
    #   - "JrJx24Cj"
    #   - "vWrInfg9"
    #   - "MIev1lAz"
"#
        .to_string()
    }

    /// Get paths for common modpack files
    pub fn paths(&self) -> ModpackPaths {
        ModpackPaths {
            workdir: self.workdir.clone(),
            empack_yml: self.workdir.join("empack.yml"),
            pack_dir: self.workdir.join("pack"),
            pack_toml: self.workdir.join("pack").join("pack.toml"),
            template_dir: self.workdir.join("templates"),
            dist_dir: self.workdir.join("dist"),
        }
    }
}

impl ModpackStateManager<'static, LiveStateProvider> {
    /// Create a new state manager with default LiveStateProvider for backward compatibility
    pub fn new_default(workdir: PathBuf) -> ModpackStateManager<'static, LiveStateProvider> {
        // We need a static reference to LiveStateProvider for this to work
        // This approach allows existing code to continue working
        static LIVE_PROVIDER: LiveStateProvider = LiveStateProvider;
        ModpackStateManager::new(workdir, &LIVE_PROVIDER)
    }
}

/// Common paths for modpack operations
#[derive(Debug, Clone)]
pub struct ModpackPaths {
    pub workdir: PathBuf,
    pub empack_yml: PathBuf,
    pub pack_dir: PathBuf,
    pub pack_toml: PathBuf,
    pub template_dir: PathBuf,
    pub dist_dir: PathBuf,
}

impl ModpackPaths {
    /// Get build output path for a specific target
    pub fn build_output(&self, target: BuildTarget) -> PathBuf {
        self.dist_dir.join(target.to_string())
    }
}

#[cfg(test)]
mod tests {
    include!("state.test.rs");
}
