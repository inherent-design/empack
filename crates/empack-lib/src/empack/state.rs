use crate::primitives::*;
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Filesystem state machine for modpack development
/// Folder layout determines state
pub struct ModpackStateManager {
    /// Working directory (where empack.yml should be)
    pub workdir: PathBuf,
}

/// State detection errors
#[derive(Debug, Error)]
pub enum StateError {
    #[error("IO error: {source}")]
    IoError {
        #[from]
        source: std::io::Error,
    },

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
}

impl ModpackStateManager {
    /// Create a new state manager for the given directory
    pub fn new(workdir: PathBuf) -> Self {
        Self { workdir }
    }

    /// Discover current state from filesystem
    pub fn discover_state(&self) -> Result<ModpackState, StateError> {
        // Check if directory exists
        if !self.workdir.exists() {
            return Err(StateError::InvalidDirectory {
                path: self.workdir.display().to_string(),
            });
        }

        if !self.workdir.is_dir() {
            return Err(StateError::InvalidDirectory {
                path: self.workdir.display().to_string(),
            });
        }

        // State detection based on file existence
        let empack_yml = self.workdir.join("empack.yml");
        let pack_toml = self.workdir.join("pack").join("pack.toml");
        let dist_dir = self.workdir.join(".empack").join("dist");

        // Check for built state first (most advanced)
        if dist_dir.exists() && dist_dir.is_dir() {
            // Check if we have any build artifacts
            if self.has_build_artifacts(&dist_dir)? {
                return Ok(ModpackState::Built);
            }
        }

        // Check for configured state
        if empack_yml.exists() || pack_toml.exists() {
            return Ok(ModpackState::Configured);
        }

        // Default to uninitialized
        Ok(ModpackState::Uninitialized)
    }

    /// Check if directory has build artifacts
    fn has_build_artifacts(&self, dist_dir: &Path) -> Result<bool, StateError> {
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
                files.push(self.workdir.join(".empack").join("dist"));
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
                Ok(pack_dir.exists() && pack_dir.is_dir())
            }
            ModpackState::Built => {
                let dist_dir = self.workdir.join(".empack").join("dist");
                Ok(dist_dir.exists() && self.has_build_artifacts(&dist_dir)?)
            }
        }
    }

    /// Check if state transition is valid
    pub fn can_transition(&self, from: ModpackState, to: ModpackState) -> bool {
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

    /// Execute a state transition
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
                self.create_initial_structure()?;
                Ok(ModpackState::Configured)
            }

            StateTransition::Synchronize => {
                if current != ModpackState::Configured {
                    return Err(StateError::InvalidTransition {
                        from: current,
                        to: ModpackState::Configured,
                    });
                }
                // Sync operation doesn't change state, just reconciles files
                Ok(ModpackState::Configured)
            }

            StateTransition::Build(targets) => {
                if !self.can_transition(current, ModpackState::Built) {
                    return Err(StateError::InvalidTransition {
                        from: current,
                        to: ModpackState::Built,
                    });
                }
                self.create_build_structure(&targets)?;
                Ok(ModpackState::Built)
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

    /// Create initial modpack structure
    fn create_initial_structure(&self) -> Result<(), StateError> {
        // Create pack directory
        let pack_dir = self.workdir.join("pack");
        fs::create_dir_all(&pack_dir)?;

        // Create .empack directory
        let empack_dir = self.workdir.join(".empack");
        fs::create_dir_all(&empack_dir)?;

        // Create initial empack.yml if it doesn't exist
        let empack_yml = self.workdir.join("empack.yml");
        if !empack_yml.exists() {
            fs::write(&empack_yml, self.default_empack_yml())?;
        }

        Ok(())
    }

    /// Create build directory structure
    fn create_build_structure(&self, _targets: &[BuildTarget]) -> Result<(), StateError> {
        let dist_dir = self.workdir.join(".empack").join("dist");
        fs::create_dir_all(&dist_dir)?;

        // Create target-specific directories
        for target in [
            BuildTarget::Mrpack,
            BuildTarget::Client,
            BuildTarget::Server,
            BuildTarget::ClientFull,
            BuildTarget::ServerFull,
        ] {
            let target_dir = dist_dir.join(target.to_string());
            fs::create_dir_all(&target_dir)?;
        }

        Ok(())
    }

    /// Clean build artifacts
    fn clean_build_artifacts(&self) -> Result<(), StateError> {
        let dist_dir = self.workdir.join(".empack").join("dist");
        if dist_dir.exists() {
            fs::remove_dir_all(&dist_dir)?;
        }
        Ok(())
    }

    /// Clean configuration files
    fn clean_configuration(&self) -> Result<(), StateError> {
        let empack_yml = self.workdir.join("empack.yml");
        if empack_yml.exists() {
            fs::remove_file(&empack_yml)?;
        }

        let pack_dir = self.workdir.join("pack");
        if pack_dir.exists() {
            fs::remove_dir_all(&pack_dir)?;
        }

        let empack_dir = self.workdir.join(".empack");
        if empack_dir.exists() {
            fs::remove_dir_all(&empack_dir)?;
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
            empack_dir: self.workdir.join(".empack"),
            dist_dir: self.workdir.join(".empack").join("dist"),
        }
    }
}

/// Common paths for modpack operations
#[derive(Debug, Clone)]
pub struct ModpackPaths {
    pub workdir: PathBuf,
    pub empack_yml: PathBuf,
    pub pack_dir: PathBuf,
    pub pack_toml: PathBuf,
    pub empack_dir: PathBuf,
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
    use super::*;

    use tempfile::TempDir;

    fn create_test_manager() -> (TempDir, ModpackStateManager) {
        let temp_dir = TempDir::new().unwrap();
        let manager = ModpackStateManager::new(temp_dir.path().to_path_buf());
        (temp_dir, manager)
    }

    #[test]
    fn test_initial_state_is_uninitialized() {
        let (_temp, manager) = create_test_manager();
        let state = manager.discover_state().unwrap();
        assert_eq!(state, ModpackState::Uninitialized);
    }

    #[test]
    fn test_transition_to_configured() {
        let (_temp, manager) = create_test_manager();

        let result = manager
            .execute_transition(StateTransition::Initialize)
            .unwrap();
        assert_eq!(result, ModpackState::Configured);

        // Verify files were created
        let paths = manager.paths();
        assert!(paths.empack_yml.exists());
        assert!(paths.pack_dir.exists());
        assert!(paths.empack_dir.exists());
    }

    #[test]
    fn test_transition_to_built() {
        let (_temp, manager) = create_test_manager();

        // Initialize first
        manager
            .execute_transition(StateTransition::Initialize)
            .unwrap();

        // Then build
        let targets = vec![BuildTarget::Mrpack, BuildTarget::Client];
        let result = manager
            .execute_transition(StateTransition::Build(targets))
            .unwrap();
        assert_eq!(result, ModpackState::Built);

        // Verify dist directory was created
        let paths = manager.paths();
        assert!(paths.dist_dir.exists());
        assert!(paths.build_output(BuildTarget::Mrpack).exists());
        assert!(paths.build_output(BuildTarget::Client).exists());
    }

    #[test]
    fn test_clean_transitions() {
        let (_temp, manager) = create_test_manager();

        // Build up to built state
        manager
            .execute_transition(StateTransition::Initialize)
            .unwrap();

        manager
            .execute_transition(StateTransition::Build(vec![BuildTarget::Mrpack]))
            .unwrap();

        // Clean back to configured
        let result = manager.execute_transition(StateTransition::Clean).unwrap();
        assert_eq!(result, ModpackState::Configured);
        assert!(!manager.paths().dist_dir.exists());

        // Clean back to uninitialized
        let result = manager.execute_transition(StateTransition::Clean).unwrap();
        assert_eq!(result, ModpackState::Uninitialized);
        assert!(!manager.paths().empack_yml.exists());
        assert!(!manager.paths().pack_dir.exists());
    }

    #[test]
    fn test_invalid_transitions() {
        let (_temp, manager) = create_test_manager();

        // Can't build from uninitialized
        let result = manager.execute_transition(StateTransition::Build(vec![BuildTarget::Mrpack]));
        assert!(result.is_err());

        // Can't sync from uninitialized
        let result = manager.execute_transition(StateTransition::Synchronize);
        assert!(result.is_err());
    }

    #[test]
    fn test_state_validation() {
        let (_temp, manager) = create_test_manager();

        // Uninitialized should validate correctly
        assert!(manager.validate_state(ModpackState::Uninitialized).unwrap());
        assert!(!manager.validate_state(ModpackState::Configured).unwrap());

        // After initialization, configured should validate
        manager
            .execute_transition(StateTransition::Initialize)
            .unwrap();
        assert!(manager.validate_state(ModpackState::Configured).unwrap());
        assert!(!manager.validate_state(ModpackState::Uninitialized).unwrap());
    }

    #[test]
    fn test_paths_helper() {
        let (_temp, manager) = create_test_manager();
        let paths = manager.paths();

        assert_eq!(paths.empack_yml, manager.workdir.join("empack.yml"));
        assert_eq!(
            paths.pack_toml,
            manager.workdir.join("pack").join("pack.toml")
        );
        assert_eq!(
            paths.build_output(BuildTarget::Mrpack),
            manager.workdir.join(".empack").join("dist").join("mrpack")
        );
    }
}
