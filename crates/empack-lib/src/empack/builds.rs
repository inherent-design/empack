//! Build system for empack targets
//! Five-target system: mrpack, client, server, client-full, server-full

use crate::empack::PackwizInstaller;
use crate::primitives::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Build system errors
#[derive(Debug, Error)]
pub enum BuildError {
    #[error("IO error: {source}")]
    IoError {
        #[from]
        source: std::io::Error,
    },

    #[error("Command execution failed: {command}")]
    CommandFailed { command: String },

    #[error("Build target not supported: {target:?}")]
    UnsupportedTarget { target: BuildTarget },

    #[error("Missing required tool: {tool}")]
    MissingTool { tool: String },

    #[error("Build configuration error: {reason}")]
    ConfigError { reason: String },

    #[error("Build validation failed: {reason}")]
    ValidationError { reason: String },

    #[error("Pack info extraction failed: {reason}")]
    PackInfoError { reason: String },
}

/// Build orchestrator with state tracking and template processing
pub struct BuildOrchestrator<'a> {
    workdir: PathBuf,
    dist_dir: PathBuf,

    // State tracking for incremental builds
    pack_refreshed: bool,
    mrpack_extracted: bool,

    // Cached template variables
    pack_info: Option<PackInfo>,

    // Session provider for resource resolution and state management
    session: &'a dyn crate::application::session::Session,
}

impl<'a> std::fmt::Debug for BuildOrchestrator<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BuildOrchestrator")
            .field("workdir", &self.workdir)
            .field("dist_dir", &self.dist_dir)
            .field("pack_refreshed", &self.pack_refreshed)
            .field("mrpack_extracted", &self.mrpack_extracted)
            .field("pack_info", &self.pack_info)
            .field("session", &"<dyn Session>")
            .finish()
    }
}

/// Pack metadata from pack.toml for template processing
#[derive(Debug, Clone)]
pub struct PackInfo {
    pub author: String,
    pub name: String,
    pub version: String,
    pub mc_version: String,
    pub fabric_version: String,
}

/// Build configuration for a specific target (V1's register_build_target pattern)
#[derive(Debug, Clone)]
pub struct BuildConfig {
    pub target: BuildTarget,
    pub handler: String,
    pub dependencies: Vec<BuildTarget>,
    pub output_dir: PathBuf,
}

/// Build result for a specific target
#[derive(Debug, Clone)]
pub struct BuildResult {
    pub target: BuildTarget,
    pub success: bool,
    pub output_path: Option<PathBuf>,
    pub artifacts: Vec<BuildArtifact>,
    pub warnings: Vec<String>,
}

/// Individual build artifact
#[derive(Debug, Clone)]
pub struct BuildArtifact {
    pub name: String,
    pub path: PathBuf,
    pub size: u64,
}

impl<'a> BuildOrchestrator<'a> {
    pub fn new(session: &'a dyn crate::application::session::Session) -> Result<Self, BuildError> {
        let workdir = session
            .config()
            .app_config()
            .workdir
            .as_ref()
            .cloned()
            .unwrap_or_else(|| {
                session
                    .filesystem()
                    .current_dir()
                    .expect("Failed to get current directory")
            });
        let dist_dir = workdir.join("dist");

        Ok(Self {
            workdir,
            dist_dir,
            pack_refreshed: false,
            mrpack_extracted: false,
            pack_info: None,
            session,
        })
    }

    /// Load pack info from pack.toml (V1's load_pack_info implementation)
    fn load_pack_info(&mut self) -> Result<&PackInfo, BuildError> {
        if self.pack_info.is_some() {
            return Ok(self.pack_info.as_ref().unwrap());
        }

        let pack_toml = self.workdir.join("pack").join("pack.toml");
        let filesystem = self.session.filesystem();
        if !filesystem.exists(&pack_toml) {
            return Err(BuildError::PackInfoError {
                reason: "pack.toml not found".to_string(),
            });
        }

        let content =
            filesystem
                .read_to_string(&pack_toml)
                .map_err(|e| BuildError::ConfigError {
                    reason: e.to_string(),
                })?;
        let toml_value: toml::Value =
            toml::from_str(&content).map_err(|e| BuildError::PackInfoError {
                reason: format!("TOML parse error: {}", e),
            })?;

        let author = toml_value
            .get("author")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown")
            .to_string();

        let name = toml_value
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown")
            .to_string();

        let version = toml_value
            .get("version")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown")
            .to_string();

        let mc_version = toml_value
            .get("versions")
            .and_then(|v| v.get("minecraft"))
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown")
            .to_string();

        let fabric_version = toml_value
            .get("versions")
            .and_then(|v| v.get("fabric"))
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown")
            .to_string();

        self.pack_info = Some(PackInfo {
            author,
            name,
            version,
            mc_version,
            fabric_version,
        });

        Ok(self.pack_info.as_ref().unwrap())
    }

    /// Register build targets (V1's register_all_build_targets pattern)
    fn create_build_registry() -> HashMap<BuildTarget, BuildConfig> {
        let mut registry = HashMap::new();

        // V1 pattern: register_build_target "mrpack" "build_mrpack_impl" ""
        registry.insert(
            BuildTarget::Mrpack,
            BuildConfig {
                target: BuildTarget::Mrpack,
                handler: "build_mrpack_impl".to_string(),
                dependencies: vec![],
                output_dir: PathBuf::new(), // Will be set later
            },
        );

        // V1 pattern: register_build_target "client" "build_client_impl" "mrpack"
        registry.insert(
            BuildTarget::Client,
            BuildConfig {
                target: BuildTarget::Client,
                handler: "build_client_impl".to_string(),
                dependencies: vec![BuildTarget::Mrpack],
                output_dir: PathBuf::new(),
            },
        );

        // V1 pattern: register_build_target "server" "build_server_impl" "mrpack"
        registry.insert(
            BuildTarget::Server,
            BuildConfig {
                target: BuildTarget::Server,
                handler: "build_server_impl".to_string(),
                dependencies: vec![BuildTarget::Mrpack],
                output_dir: PathBuf::new(),
            },
        );

        // V1 pattern: register_build_target "client-full" "build_client_full_impl" ""
        registry.insert(
            BuildTarget::ClientFull,
            BuildConfig {
                target: BuildTarget::ClientFull,
                handler: "build_client_full_impl".to_string(),
                dependencies: vec![],
                output_dir: PathBuf::new(),
            },
        );

        // V1 pattern: register_build_target "server-full" "build_server_full_impl" ""
        registry.insert(
            BuildTarget::ServerFull,
            BuildConfig {
                target: BuildTarget::ServerFull,
                handler: "build_server_full_impl".to_string(),
                dependencies: vec![],
                output_dir: PathBuf::new(),
            },
        );

        registry
    }

    /// Refresh pack files using packwiz (V1's refresh_pack implementation)
    fn refresh_pack(&mut self) -> Result<(), BuildError> {
        if self.pack_refreshed {
            return Ok(());
        }

        let pack_file = self.workdir.join("pack").join("pack.toml");
        if !self.session.filesystem().exists(&pack_file) {
            return Err(BuildError::ConfigError {
                reason: format!("Pack file not found: {}", pack_file.display()),
            });
        }

        let output = self
            .session
            .process()
            .execute(
                "packwiz",
                &["--pack-file", pack_file.to_str().unwrap(), "refresh"],
                &self.workdir,
            )
            .map_err(|e| BuildError::CommandFailed {
                command: format!("packwiz refresh: {}", e),
            })?;

        if !output.success {
            return Err(BuildError::CommandFailed {
                command: format!("packwiz refresh: {}", output.stderr),
            });
        }

        self.pack_refreshed = true;
        Ok(())
    }

    /// Extract mrpack for distribution builds (V1's extract_mrpack implementation)
    fn extract_mrpack(&mut self) -> Result<(), BuildError> {
        if self.mrpack_extracted {
            return Ok(());
        }

        let pack_info = self.load_pack_info()?.clone();
        let mrpack_file = self
            .dist_dir
            .join(format!("{}-v{}.mrpack", pack_info.name, pack_info.version));

        if !self.session.filesystem().exists(&mrpack_file) {
            // Build mrpack first (V1 pattern)
            self.build_mrpack_impl()?;
        }

        let temp_extract_dir = self.dist_dir.join("temp-mrpack-extract");
        if self.session.filesystem().exists(&temp_extract_dir) {
            self.session
                .filesystem()
                .remove_dir_all(&temp_extract_dir)
                .map_err(|e| BuildError::ConfigError {
                    reason: e.to_string(),
                })?;
        }
        self.session
            .filesystem()
            .create_dir_all(&temp_extract_dir)
            .map_err(|e| BuildError::ConfigError {
                reason: e.to_string(),
            })?;

        // V1 uses generic extract_archive - we'll use unzip for now
        let output = self
            .session
            .process()
            .execute(
                "unzip",
                &[
                    "-q",
                    mrpack_file.to_str().unwrap(),
                    "-d",
                    temp_extract_dir.to_str().unwrap(),
                ],
                &self.workdir,
            )
            .map_err(|e| BuildError::CommandFailed {
                command: format!("unzip mrpack: {}", e),
            })?;

        if !output.success {
            return Err(BuildError::CommandFailed {
                command: format!("unzip mrpack: {}", output.stderr),
            });
        }

        self.mrpack_extracted = true;
        Ok(())
    }

    /// Create distribution zip file (V1's zip_distribution implementation)
    fn zip_distribution(&self, target: BuildTarget) -> Result<PathBuf, BuildError> {
        let pack_info = self
            .pack_info
            .as_ref()
            .ok_or_else(|| BuildError::PackInfoError {
                reason: "Pack info not loaded".to_string(),
            })?;

        let dist_dir = self.dist_dir.join(target.to_string());
        let filename = format!(
            "{}-v{}-{}.zip",
            pack_info.name,
            pack_info.version,
            target
        );
        let zip_path = self.dist_dir.join(&filename);

        // Remove existing zip file
        if self.session.filesystem().exists(&zip_path) {
            self.session
                .filesystem()
                .remove_file(&zip_path)
                .map_err(|e| BuildError::ConfigError {
                    reason: e.to_string(),
                })?;
        }

        // Check if directory has content (V1's pattern)
        let has_content = self
            .session
            .filesystem()
            .get_file_list(&dist_dir)
            .map(|entries| !entries.is_empty())
            .unwrap_or(false);

        if !has_content {
            return Err(BuildError::ValidationError {
                reason: format!("No files to zip in '{}'", dist_dir.display()),
            });
        }

        // Create zip file (V1 pattern: cd dist_dir && zip -r0 "../filename" ./)
        let output = self
            .session
            .process()
            .execute("zip", &["-r0", zip_path.to_str().unwrap(), "./"], &dist_dir)
            .map_err(|e| BuildError::CommandFailed {
                command: format!("zip {}: {}", filename, e),
            })?;

        if !output.success {
            return Err(BuildError::CommandFailed {
                command: format!("zip {}: {}", filename, output.stderr),
            });
        }

        Ok(zip_path)
    }

    /// Build mrpack implementation (V1's build_mrpack_impl)
    fn build_mrpack_impl(&mut self) -> Result<BuildResult, BuildError> {
        self.refresh_pack()?;

        let pack_info = self.load_pack_info()?.clone();
        let pack_file = self.workdir.join("pack").join("pack.toml");
        let output_file = self
            .dist_dir
            .join(format!("{}-v{}.mrpack", pack_info.name, pack_info.version));

        // Ensure dist directory exists
        self.session
            .filesystem()
            .create_dir_all(&self.dist_dir)
            .map_err(|e| BuildError::ConfigError {
                reason: e.to_string(),
            })?;

        // Remove existing mrpack file
        if self.session.filesystem().exists(&output_file) {
            self.session
                .filesystem()
                .remove_file(&output_file)
                .map_err(|e| BuildError::ConfigError {
                    reason: e.to_string(),
                })?;
        }

        // Build mrpack using packwiz (V1 pattern)
        let output = self
            .session
            .process()
            .execute(
                "packwiz",
                &[
                    "--pack-file",
                    pack_file.to_str().unwrap(),
                    "mr",
                    "export",
                    "-o",
                    output_file.to_str().unwrap(),
                ],
                &self.workdir,
            )
            .map_err(|e| BuildError::CommandFailed {
                command: format!("packwiz mr export: {}", e),
            })?;

        if !output.success {
            return Ok(BuildResult {
                target: BuildTarget::Mrpack,
                success: false,
                output_path: None,
                artifacts: vec![],
                warnings: vec!["packwiz mr export failed".to_string()],
            });
        }

        let artifact = self.create_artifact(&output_file)?;

        Ok(BuildResult {
            target: BuildTarget::Mrpack,
            success: true,
            output_path: Some(output_file),
            artifacts: vec![artifact],
            warnings: vec![],
        })
    }

    /// Build client implementation (V1's build_client_impl)
    fn build_client_impl(&mut self, bootstrap_jar_path: &Path) -> Result<BuildResult, BuildError> {
        // Clean first (V1 pattern)
        self.clean_target(BuildTarget::Client)?;

        self.refresh_pack()?;

        let dist_dir = self.dist_dir.join("client");
        self.session
            .filesystem()
            .create_dir_all(&dist_dir)
            .map_err(|e| BuildError::ConfigError {
                reason: e.to_string(),
            })?;

        // V1 pattern: process_build_templates "templates/client" "$dist_dir"
        self.process_build_templates("templates/client", &dist_dir)?;

        // Set up client structure (V1 pattern)
        let minecraft_dir = dist_dir.join(".minecraft");
        self.session
            .filesystem()
            .create_dir_all(&minecraft_dir)
            .map_err(|e| BuildError::ConfigError {
                reason: e.to_string(),
            })?;

        // Copy packwiz installer
        let bootstrap_content = self
            .session
            .filesystem()
            .read_to_string(bootstrap_jar_path)
            .map_err(|e| BuildError::ConfigError {
                reason: e.to_string(),
            })?;
        self.session
            .filesystem()
            .write_file(
                &minecraft_dir.join("packwiz-installer-bootstrap.jar"),
                &bootstrap_content,
            )
            .map_err(|e| BuildError::ConfigError {
                reason: e.to_string(),
            })?;

        // Copy pack files (V1 pattern)
        let pack_dir = self.workdir.join("pack");
        self.copy_dir_contents(&pack_dir, &minecraft_dir.join("pack"))?;

        // Extract mrpack overrides (V1 pattern)
        self.extract_mrpack()?;
        let temp_extract_dir = self.dist_dir.join("temp-mrpack-extract");
        let overrides_dir = temp_extract_dir.join("overrides");
        if self.session.filesystem().exists(&overrides_dir) {
            self.copy_dir_contents(&overrides_dir, &minecraft_dir)?;
        }

        // Create distribution (V1 pattern)
        let zip_path = self.zip_distribution(BuildTarget::Client)?;
        let artifact = self.create_artifact(&zip_path)?;

        Ok(BuildResult {
            target: BuildTarget::Client,
            success: true,
            output_path: Some(zip_path),
            artifacts: vec![artifact],
            warnings: vec![],
        })
    }

    /// Build server implementation (V1's build_server_impl)
    fn build_server_impl(&mut self, bootstrap_jar_path: &Path) -> Result<BuildResult, BuildError> {
        // Step 1: Clean the dist/server/ directory
        self.clean_target(BuildTarget::Server)?;

        // Step 2: Refresh the pack using packwiz refresh
        self.refresh_pack()?;

        let dist_dir = self.dist_dir.join("server");
        self.session
            .filesystem()
            .create_dir_all(&dist_dir)
            .map_err(|e| BuildError::ConfigError {
                reason: e.to_string(),
            })?;

        // Step 3: Process templates from templates/server/ into dist/server/
        self.process_build_templates("templates/server", &dist_dir)?;

        // Step 4: Copy the entire pack/ directory into dist/server/
        let pack_dir = self.workdir.join("pack");
        self.copy_dir_contents(&pack_dir, &dist_dir.join("pack"))?;

        // Step 5: Copy the packwiz-installer-bootstrap.jar into dist/server/
        let bootstrap_content = self
            .session
            .filesystem()
            .read_to_string(bootstrap_jar_path)
            .map_err(|e| BuildError::ConfigError {
                reason: e.to_string(),
            })?;
        self.session
            .filesystem()
            .write_file(
                &dist_dir.join("packwiz-installer-bootstrap.jar"),
                &bootstrap_content,
            )
            .map_err(|e| BuildError::ConfigError {
                reason: e.to_string(),
            })?;

        // Step 6: Execute mrpack-install to download the appropriate Minecraft server JAR
        let pack_info = self.load_pack_info()?.clone();
        let server_type = match pack_info.fabric_version.as_str() {
            "" => "vanilla",
            _ => "fabric",
        };
        let result = self.session.process().execute(
            "mrpack-install",
            &["server", server_type, "--server-file", "srv.jar"],
            &dist_dir,
        );

        match result {
            Ok(output) if output.success => {
                // Server JAR downloaded successfully
            }
            Ok(output) => {
                return Ok(BuildResult {
                    target: BuildTarget::Server,
                    success: false,
                    output_path: None,
                    artifacts: vec![],
                    warnings: vec![format!(
                        "mrpack-install command failed to download server JAR: {}",
                        output.stderr
                    )],
                });
            }
            Err(e) => {
                return Ok(BuildResult {
                    target: BuildTarget::Server,
                    success: false,
                    output_path: None,
                    artifacts: vec![],
                    warnings: vec![
                        "mrpack-install command not found - ensure it's installed and in PATH"
                            .to_string(),
                    ],
                });
            }
        }

        // Step 7: Extract the .mrpack file (building it first if necessary)
        self.extract_mrpack()?;

        // Step 8: Copy the overrides/ from the extracted mrpack into dist/server/
        let temp_extract_dir = self.dist_dir.join("temp-mrpack-extract");
        let overrides_dir = temp_extract_dir.join("overrides");
        if self.session.filesystem().exists(&overrides_dir) {
            self.copy_dir_contents(&overrides_dir, &dist_dir)?;
        }

        // Step 9: Create a final zip archive of the dist/server/ directory
        let zip_path = self.zip_distribution(BuildTarget::Server)?;
        let artifact = self.create_artifact(&zip_path)?;

        Ok(BuildResult {
            target: BuildTarget::Server,
            success: true,
            output_path: Some(zip_path),
            artifacts: vec![artifact],
            warnings: vec![],
        })
    }

    /// Build client-full implementation (V1's build_client_full_impl)
    fn build_client_full_impl(
        &mut self,
        bootstrap_jar_path: &Path,
    ) -> Result<BuildResult, BuildError> {
        // Step 1: Clean the dist/client-full/ directory
        self.clean_target(BuildTarget::ClientFull)?;

        // Step 2: Refresh the pack using packwiz refresh
        self.refresh_pack()?;

        let dist_dir = self.dist_dir.join("client-full");
        self.session
            .filesystem()
            .create_dir_all(&dist_dir)
            .map_err(|e| BuildError::ConfigError {
                reason: e.to_string(),
            })?;

        // Step 3: Execute packwiz-installer-bootstrap.jar with -g (no-GUI) and -s both flags

        // Copy pack files to client-full directory for installer to use
        let pack_dir = self.workdir.join("pack");
        self.copy_dir_contents(&pack_dir, &dist_dir.join("pack"))?;

        // Execute installer with PackwizInstaller abstraction
        let installer = PackwizInstaller::new(self.session, bootstrap_jar_path.to_owned())
            .map_err(|e| BuildError::CommandFailed {
                command: format!("PackwizInstaller initialization: {}", e),
            })?;

        installer
            .install_mods("both", &dist_dir)
            .map_err(|e| BuildError::CommandFailed {
                command: format!("packwiz-installer-bootstrap.jar: {}", e),
            })?;

        // Step 4: Create a final zip archive of the dist/client-full/ directory
        let zip_path = self.zip_distribution(BuildTarget::ClientFull)?;
        let artifact = self.create_artifact(&zip_path)?;

        Ok(BuildResult {
            target: BuildTarget::ClientFull,
            success: true,
            output_path: Some(zip_path),
            artifacts: vec![artifact],
            warnings: vec![],
        })
    }

    /// Build server-full implementation (V1's build_server_full_impl)
    fn build_server_full_impl(
        &mut self,
        bootstrap_jar_path: &Path,
    ) -> Result<BuildResult, BuildError> {
        // Step 1: Clean the dist/server-full/ directory
        self.clean_target(BuildTarget::ServerFull)?;

        // Step 2: Refresh the pack using packwiz refresh
        self.refresh_pack()?;

        let dist_dir = self.dist_dir.join("server-full");
        self.session
            .filesystem()
            .create_dir_all(&dist_dir)
            .map_err(|e| BuildError::ConfigError {
                reason: e.to_string(),
            })?;

        // Step 3: Process templates from templates/server/ into dist/server-full/
        self.process_build_templates("templates/server", &dist_dir)?;

        // Step 4: Execute mrpack-install to download the Minecraft server JAR
        let pack_info = self.load_pack_info()?.clone();
        let server_type = match pack_info.fabric_version.as_str() {
            "" => "vanilla",
            _ => "fabric",
        };
        let result = self.session.process().execute(
            "mrpack-install",
            &["server", server_type, "--server-file", "srv.jar"],
            &dist_dir,
        );

        match result {
            Ok(output) if output.success => {
                // Server JAR downloaded successfully
            }
            Ok(output) => {
                return Ok(BuildResult {
                    target: BuildTarget::ServerFull,
                    success: false,
                    output_path: None,
                    artifacts: vec![],
                    warnings: vec![format!(
                        "mrpack-install command failed to download server JAR: {}",
                        output.stderr
                    )],
                });
            }
            Err(e) => {
                return Ok(BuildResult {
                    target: BuildTarget::ServerFull,
                    success: false,
                    output_path: None,
                    artifacts: vec![],
                    warnings: vec![format!(
                        "mrpack-install command not found - ensure it's installed and in PATH: {}",
                        e
                    )],
                });
            }
        }

        // Step 5: Execute packwiz-installer-bootstrap.jar with -g and -s server flags

        // Copy pack files to server-full directory for installer to use
        let pack_dir = self.workdir.join("pack");
        self.copy_dir_contents(&pack_dir, &dist_dir.join("pack"))?;

        // Execute installer with PackwizInstaller abstraction
        let installer = PackwizInstaller::new(self.session, bootstrap_jar_path.to_owned())
            .map_err(|e| BuildError::CommandFailed {
                command: format!("PackwizInstaller initialization: {}", e),
            })?;

        installer
            .install_mods("server", &dist_dir)
            .map_err(|e| BuildError::CommandFailed {
                command: format!("packwiz-installer-bootstrap.jar: {}", e),
            })?;

        // Step 6: Create a final zip archive of the dist/server-full/ directory
        let zip_path = self.zip_distribution(BuildTarget::ServerFull)?;
        let artifact = self.create_artifact(&zip_path)?;

        Ok(BuildResult {
            target: BuildTarget::ServerFull,
            success: true,
            output_path: Some(zip_path),
            artifacts: vec![artifact],
            warnings: vec![],
        })
    }

    /// Execute V1's proven 5-target build pipeline with state management
    pub async fn execute_build_pipeline(
        &mut self,
        targets: &[BuildTarget],
    ) -> Result<Vec<BuildResult>, BuildError> {
        // Begin state transition
        self.session
            .state()
            .begin_state_transition(crate::primitives::StateTransition::Building)
            .map_err(|e| BuildError::ConfigError {
                reason: format!("Failed to begin build transition: {:?}", e),
            })?;

        self.prepare_build_environment()?;

        // Get bootstrap JAR path from session
        let bootstrap_jar_path = self
            .session
            .filesystem()
            .get_bootstrap_jar_cache_path()
            .map_err(|e| BuildError::ConfigError {
                reason: format!("Failed to get bootstrap JAR path: {}", e),
            })?;

        let mut results = Vec::new();

        for target in targets {
            let result = match target {
                BuildTarget::Mrpack => self.build_mrpack_impl()?,
                BuildTarget::Client => self.build_client_impl(&bootstrap_jar_path)?,
                BuildTarget::Server => self.build_server_impl(&bootstrap_jar_path)?,
                BuildTarget::ClientFull => self.build_client_full_impl(&bootstrap_jar_path)?,
                BuildTarget::ServerFull => self.build_server_full_impl(&bootstrap_jar_path)?,
            };
            results.push(result);
        }

        // Complete state transition on success
        self.session
            .state()
            .complete_state_transition()
            .map_err(|e| BuildError::ConfigError {
                reason: format!("Failed to complete build transition: {:?}", e),
            })?;

        Ok(results)
    }

    /// Execute clean pipeline with state management
    pub async fn execute_clean_pipeline(
        &mut self,
        targets: &[BuildTarget],
    ) -> Result<(), BuildError> {
        // Begin state transition
        self.session
            .state()
            .begin_state_transition(crate::primitives::StateTransition::Cleaning)
            .map_err(|e| BuildError::ConfigError {
                reason: format!("Failed to begin clean transition: {:?}", e),
            })?;

        // Clean each target
        for target in targets {
            self.clean_target(*target)?;
        }

        // Complete state transition on success
        self.session
            .state()
            .complete_state_transition()
            .map_err(|e| BuildError::ConfigError {
                reason: format!("Failed to complete clean transition: {:?}", e),
            })?;

        Ok(())
    }

    /// Prepare build environment (V1's pattern checking)
    fn prepare_build_environment(&self) -> Result<(), BuildError> {
        // Ensure pack directory exists
        let pack_dir = self.workdir.join("pack");
        if !self.session.filesystem().exists(&pack_dir) {
            return Err(BuildError::ConfigError {
                reason: "pack/ directory not found - run empack init first".to_string(),
            });
        }

        // Ensure dist directory exists
        self.session
            .filesystem()
            .create_dir_all(&self.dist_dir)
            .map_err(|e| BuildError::ConfigError {
                reason: e.to_string(),
            })?;

        // Note: Tool availability checking is now handled by the ProcessProvider
        // and the requirements command, which is the correct architectural separation
        Ok(())
    }

    /// Clean build target (V1's clean_target implementation)
    fn clean_target(&self, target: BuildTarget) -> Result<(), BuildError> {
        let pack_info = self.pack_info.as_ref();

        let dist_dir = self.dist_dir.join(target.to_string());

        // Clean directory contents (V1 pattern with .gitkeep preservation)
        if self.session.filesystem().exists(&dist_dir) {
            let files = self
                .session
                .filesystem()
                .get_file_list(&dist_dir)
                .map_err(|e| BuildError::ConfigError {
                    reason: e.to_string(),
                })?;
            for file_path in files {
                if let Some(file_name) = file_path.file_name() {
                    if file_name != ".gitkeep" {
                        if self.session.filesystem().is_directory(&file_path) {
                            self.session
                                .filesystem()
                                .remove_dir_all(&file_path)
                                .map_err(|e| BuildError::ConfigError {
                                    reason: e.to_string(),
                                })?;
                        } else {
                            self.session
                                .filesystem()
                                .remove_file(&file_path)
                                .map_err(|e| BuildError::ConfigError {
                                    reason: e.to_string(),
                                })?;
                        }
                    }
                }
            }
        }

        // Clean zip file (V1 pattern)
        if let Some(info) = pack_info {
            let zip_file = self.dist_dir.join(format!(
                "{}-v{}-{}.zip",
                info.name,
                info.version,
                target
            ));
            if self.session.filesystem().exists(&zip_file) {
                self.session
                    .filesystem()
                    .remove_file(&zip_file)
                    .map_err(|e| BuildError::ConfigError {
                        reason: e.to_string(),
                    })?;
            }
        }

        Ok(())
    }

    /// Helper: Process build templates (V1's process_build_templates)
    fn process_build_templates(
        &mut self,
        template_dir: &str,
        target_dir: &Path,
    ) -> Result<(), BuildError> {
        let template_path = self.workdir.join(template_dir);
        if !self.session.filesystem().exists(&template_path) {
            // Not an error - templates are optional
            return Ok(());
        }

        let pack_info = self.load_pack_info()?.clone();

        let template_files = self
            .session
            .filesystem()
            .get_file_list(&template_path)
            .map_err(|e| BuildError::IoError { source: e })?;
        for path in template_files {
            if !self.session.filesystem().is_directory(&path) {
                let filename = path.file_name().unwrap().to_str().unwrap();
                let target_file = if let Some(stripped) = filename.strip_suffix(".template") {
                    target_dir.join(stripped)
                } else {
                    target_dir.join(filename)
                };

                let content = self
                    .session
                    .filesystem()
                    .read_to_string(&path)
                    .map_err(|e| BuildError::ConfigError {
                        reason: e.to_string(),
                    })?;

                // V1's template variable processing
                let processed = content
                    .replace("{{VERSION}}", &pack_info.version)
                    .replace("{{NAME}}", &pack_info.name)
                    .replace("{{AUTHOR}}", &pack_info.author)
                    .replace("{{MC_VERSION}}", &pack_info.mc_version)
                    .replace("{{FABRIC_VERSION}}", &pack_info.fabric_version);

                self.session
                    .filesystem()
                    .write_file(&target_file, &processed)
                    .map_err(|e| BuildError::ConfigError {
                        reason: e.to_string(),
                    })?;
            }
        }

        Ok(())
    }

    /// Helper: Copy directory contents recursively
    fn copy_dir_contents(&self, src: &Path, dst: &Path) -> Result<(), BuildError> {
        self.session
            .filesystem()
            .create_dir_all(dst)
            .map_err(|e| BuildError::ConfigError {
                reason: e.to_string(),
            })?;

        let src_files =
            self.session
                .filesystem()
                .get_file_list(src)
                .map_err(|e| BuildError::ConfigError {
                    reason: e.to_string(),
                })?;
        for src_path in src_files {
            let file_name = src_path.file_name().unwrap();
            let dst_path = dst.join(file_name);

            if self.session.filesystem().is_directory(&src_path) {
                self.copy_dir_contents(&src_path, &dst_path)?;
            } else {
                let content = self
                    .session
                    .filesystem()
                    .read_to_string(&src_path)
                    .map_err(|e| BuildError::ConfigError {
                        reason: e.to_string(),
                    })?;
                self.session
                    .filesystem()
                    .write_file(&dst_path, &content)
                    .map_err(|e| BuildError::ConfigError {
                        reason: e.to_string(),
                    })?;
            }
        }

        Ok(())
    }

    /// Helper: Create build artifact metadata
    fn create_artifact(&self, path: &Path) -> Result<BuildArtifact, BuildError> {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

        // For mock filesystem, we'll use content length as size
        let size = if self.session.filesystem().exists(path) {
            self.session
                .filesystem()
                .read_to_string(path)
                .map(|content| content.len() as u64)
                .unwrap_or(0)
        } else {
            0
        };

        Ok(BuildArtifact {
            name,
            path: path.to_path_buf(),
            size,
        })
    }
}

#[cfg(test)]
mod tests {
    include!("builds.test.rs");
}
