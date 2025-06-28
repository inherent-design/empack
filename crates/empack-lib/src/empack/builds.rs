//! Build system for empack targets
//! Five-target system: mrpack, client, server, client-full, server-full

use crate::primitives::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
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
pub struct BuildOrchestrator {
    workdir: PathBuf,
    dist_dir: PathBuf,

    // State tracking for incremental builds
    pack_refreshed: bool,
    mrpack_extracted: bool,

    // Cached template variables
    pack_info: Option<PackInfo>,
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

impl BuildOrchestrator {
    pub fn new(workdir: PathBuf) -> Self {
        let dist_dir = workdir.join("dist");

        Self {
            workdir,
            dist_dir,
            pack_refreshed: false,
            mrpack_extracted: false,
            pack_info: None,
        }
    }

    /// Load pack info from pack.toml (V1's load_pack_info implementation)
    fn load_pack_info(&mut self) -> Result<&PackInfo, BuildError> {
        if self.pack_info.is_some() {
            return Ok(self.pack_info.as_ref().unwrap());
        }

        let pack_toml = self.workdir.join("pack").join("pack.toml");
        if !pack_toml.exists() {
            return Err(BuildError::PackInfoError {
                reason: "pack.toml not found".to_string(),
            });
        }

        let content = std::fs::read_to_string(&pack_toml)?;
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
        if !pack_file.exists() {
            return Err(BuildError::ConfigError {
                reason: format!("Pack file not found: {}", pack_file.display()),
            });
        }

        let status = Command::new("packwiz")
            .args(&["--pack-file", pack_file.to_str().unwrap(), "refresh"])
            .current_dir(&self.workdir)
            .status()?;

        if !status.success() {
            return Err(BuildError::CommandFailed {
                command: "packwiz refresh".to_string(),
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

        if !mrpack_file.exists() {
            // Build mrpack first (V1 pattern)
            self.build_mrpack_impl()?;
        }

        let temp_extract_dir = self.dist_dir.join("temp-mrpack-extract");
        if temp_extract_dir.exists() {
            std::fs::remove_dir_all(&temp_extract_dir)?;
        }
        std::fs::create_dir_all(&temp_extract_dir)?;

        // V1 uses generic extract_archive - we'll use unzip for now
        let status = Command::new("unzip")
            .args(&[
                "-q",
                mrpack_file.to_str().unwrap(),
                "-d",
                temp_extract_dir.to_str().unwrap(),
            ])
            .status()?;

        if !status.success() {
            return Err(BuildError::CommandFailed {
                command: "unzip mrpack".to_string(),
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
            target.to_string()
        );
        let zip_path = self.dist_dir.join(&filename);

        // Remove existing zip file
        if zip_path.exists() {
            std::fs::remove_file(&zip_path)?;
        }

        // Check if directory has content (V1's pattern)
        let has_content = std::fs::read_dir(&dist_dir)
            .map(|entries| entries.count() > 0)
            .unwrap_or(false);

        if !has_content {
            return Err(BuildError::ValidationError {
                reason: format!("No files to zip in '{}'", dist_dir.display()),
            });
        }

        // Create zip file (V1 pattern: cd dist_dir && zip -r0 "../filename" ./)
        let status = Command::new("zip")
            .args(&["-r0", zip_path.to_str().unwrap(), "./"])
            .current_dir(&dist_dir)
            .status()?;

        if !status.success() {
            return Err(BuildError::CommandFailed {
                command: format!("zip {}", filename),
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
        std::fs::create_dir_all(&self.dist_dir)?;

        // Remove existing mrpack file
        if output_file.exists() {
            std::fs::remove_file(&output_file)?;
        }

        // Build mrpack using packwiz (V1 pattern)
        let status = Command::new("packwiz")
            .args(&[
                "--pack-file",
                pack_file.to_str().unwrap(),
                "mr",
                "export",
                "-o",
                output_file.to_str().unwrap(),
            ])
            .current_dir(&self.workdir)
            .status()?;

        if !status.success() {
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
    fn build_client_impl(&mut self) -> Result<BuildResult, BuildError> {
        // Clean first (V1 pattern)
        self.clean_target(BuildTarget::Client)?;

        self.refresh_pack()?;

        let dist_dir = self.dist_dir.join("client");
        std::fs::create_dir_all(&dist_dir)?;

        // V1 pattern: process_build_templates "templates/client" "$dist_dir"
        self.process_build_templates("templates/client", &dist_dir)?;

        // Set up client structure (V1 pattern)
        let minecraft_dir = dist_dir.join(".minecraft");
        std::fs::create_dir_all(&minecraft_dir)?;

        // Copy packwiz installer (V1 pattern)
        let installer_jar = self
            .workdir
            .join("installer")
            .join("packwiz-installer-bootstrap.jar");
        if installer_jar.exists() {
            std::fs::copy(
                &installer_jar,
                minecraft_dir.join("packwiz-installer-bootstrap.jar"),
            )?;
        } else {
            return Ok(BuildResult {
                target: BuildTarget::Client,
                success: false,
                output_path: None,
                artifacts: vec![],
                warnings: vec![
                    "packwiz-installer-bootstrap.jar not found in installer/".to_string(),
                ],
            });
        }

        // Copy pack files (V1 pattern)
        let pack_dir = self.workdir.join("pack");
        self.copy_dir_contents(&pack_dir, &minecraft_dir.join("pack"))?;

        // Extract mrpack overrides (V1 pattern)
        self.extract_mrpack()?;
        let temp_extract_dir = self.dist_dir.join("temp-mrpack-extract");
        let overrides_dir = temp_extract_dir.join("overrides");
        if overrides_dir.exists() {
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

    /// Execute V1's proven 5-target build pipeline
    pub async fn execute_build_pipeline(
        &mut self,
        targets: &[BuildTarget],
    ) -> Result<Vec<BuildResult>, BuildError> {
        self.prepare_build_environment()?;

        let mut results = Vec::new();

        for target in targets {
            let result = match target {
                BuildTarget::Mrpack => self.build_mrpack_impl()?,
                BuildTarget::Client => self.build_client_impl()?,
                _ => {
                    // TODO: Implement remaining targets following V1 patterns
                    BuildResult {
                        target: *target,
                        success: false,
                        output_path: None,
                        artifacts: vec![],
                        warnings: vec![format!("{:?} build not yet implemented", target)],
                    }
                }
            };
            results.push(result);
        }

        Ok(results)
    }

    /// Prepare build environment (V1's pattern checking)
    fn prepare_build_environment(&self) -> Result<(), BuildError> {
        // Ensure pack directory exists
        let pack_dir = self.workdir.join("pack");
        if !pack_dir.exists() {
            return Err(BuildError::ConfigError {
                reason: "pack/ directory not found - run empack init first".to_string(),
            });
        }

        // Ensure dist directory exists
        std::fs::create_dir_all(&self.dist_dir)?;

        // Validate packwiz is available (V1 pattern)
        let status = Command::new("packwiz").args(&["--version"]).status();

        match status {
            Ok(status) if status.success() => Ok(()),
            Ok(_) => Err(BuildError::MissingTool {
                tool: "packwiz (command failed)".to_string(),
            }),
            Err(_) => Err(BuildError::MissingTool {
                tool: "packwiz (not found in PATH)".to_string(),
            }),
        }
    }

    /// Clean build target (V1's clean_target implementation)
    fn clean_target(&self, target: BuildTarget) -> Result<(), BuildError> {
        let pack_info = self.pack_info.as_ref();

        let dist_dir = self.dist_dir.join(target.to_string());

        // Clean directory contents (V1 pattern with .gitkeep preservation)
        if dist_dir.exists() {
            for entry in std::fs::read_dir(&dist_dir)? {
                let entry = entry?;
                let file_name = entry.file_name();
                if file_name != ".gitkeep" {
                    let path = entry.path();
                    if path.is_dir() {
                        std::fs::remove_dir_all(&path)?;
                    } else {
                        std::fs::remove_file(&path)?;
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
                target.to_string()
            ));
            if zip_file.exists() {
                std::fs::remove_file(&zip_file)?;
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
        if !template_path.exists() {
            // Not an error - templates are optional
            return Ok(());
        }

        let pack_info = self.load_pack_info()?.clone();

        for entry in std::fs::read_dir(&template_path)? {
            let entry = entry?;
            let path = entry.path();
            if !path.is_file() {
                continue;
            }

            let filename = path.file_name().unwrap().to_str().unwrap();
            let target_file = if filename.ends_with(".template") {
                target_dir.join(&filename[..filename.len() - 9]) // Remove .template suffix
            } else {
                target_dir.join(filename)
            };

            let content = std::fs::read_to_string(&path)?;

            // V1's template variable processing
            let processed = content
                .replace("{{VERSION}}", &pack_info.version)
                .replace("{{NAME}}", &pack_info.name)
                .replace("{{AUTHOR}}", &pack_info.author)
                .replace("{{MC_VERSION}}", &pack_info.mc_version)
                .replace("{{FABRIC_VERSION}}", &pack_info.fabric_version);

            std::fs::write(&target_file, processed)?;
        }

        Ok(())
    }

    /// Helper: Copy directory contents recursively
    fn copy_dir_contents(&self, src: &Path, dst: &Path) -> Result<(), BuildError> {
        std::fs::create_dir_all(dst)?;

        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            let src_path = entry.path();
            let dst_path = dst.join(entry.file_name());

            if src_path.is_dir() {
                self.copy_dir_contents(&src_path, &dst_path)?;
            } else {
                std::fs::copy(&src_path, &dst_path)?;
            }
        }

        Ok(())
    }

    /// Helper: Create build artifact metadata
    fn create_artifact(&self, path: &Path) -> Result<BuildArtifact, BuildError> {
        let metadata = std::fs::metadata(path)?;
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

        Ok(BuildArtifact {
            name,
            path: path.to_path_buf(),
            size: metadata.len(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_orchestrator() -> (TempDir, BuildOrchestrator) {
        let temp_dir = TempDir::new().unwrap();
        let orchestrator = BuildOrchestrator::new(temp_dir.path().to_path_buf());
        (temp_dir, orchestrator)
    }

    #[test]
    fn test_build_registry() {
        let registry = BuildOrchestrator::create_build_registry();
        assert_eq!(registry.len(), 5);
        assert!(registry.contains_key(&BuildTarget::Mrpack));
        assert!(registry.contains_key(&BuildTarget::Client));

        // Test dependencies (V1 pattern)
        let client_config = &registry[&BuildTarget::Client];
        assert_eq!(client_config.dependencies, vec![BuildTarget::Mrpack]);
    }

    #[test]
    fn test_prepare_build_environment() {
        let (_temp, orchestrator) = create_test_orchestrator();

        // Should fail without pack directory
        let result = orchestrator.prepare_build_environment();
        assert!(result.is_err());

        // Create pack directory
        std::fs::create_dir_all(orchestrator.workdir.join("pack")).unwrap();

        // May still fail if packwiz not available, but structure should be validated
        let _result = orchestrator.prepare_build_environment();
    }
}
