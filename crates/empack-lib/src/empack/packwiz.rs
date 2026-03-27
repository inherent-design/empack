//! Packwiz integration layer for metadata management and mod installation
//!
//! Provides two main abstractions:
//! - PackwizMetadata: OPTIONAL convenience wrapper for `packwiz modrinth/curseforge add/remove/refresh`
//! - PackwizInstaller: Wraps `java -jar packwiz-installer-bootstrap.jar` invocation (used by build system)
//!
//! Both use ProcessProvider for command execution and follow the session-based DI pattern.
//!
//! ## Usage Patterns
//!
//! PackwizMetadata is designed for complex packwiz operations that benefit from abstraction:
//! - Multi-step validation (refresh_index with stderr parsing for HashMismatch/PackFormat errors)
//! - Structured error handling (PackwizError variants vs generic anyhow errors)
//! - Cached availability checks (ensure_packwiz caches result across multiple calls)
//! - Future transactional behavior (rollback on partial failure)
//!
//! Commands MAY use either approach:
//! - **DIRECT ProcessProvider**: For simple single-command operations (add, remove)
//!   - Lower cognitive overhead
//!   - Easier debugging (visible args in code)
//!   - Matches computational desperation principle (minimal abstractions until complexity justifies)
//! - **PackwizMetadata wrapper**: For complex operations with multi-step validation
//!   - Better error context
//!   - Consistent error types
//!   - Easier to test in isolation (mock wrapper instead of ProcessProvider)
//!
//! **Current design (Phase 4):** Commands use direct ProcessProvider for simplicity.
//! Future phases may integrate wrapper IF complexity emerges (dependency resolution, transactional behavior).
//! See phase4-design-documentation report for rationale.

use crate::application::session::{FileSystemProvider, ProcessProvider, Session};
use crate::empack::state::StateError;
use crate::primitives::ProjectPlatform;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Trait for packwiz CLI operations extracted from FileSystemProvider.
///
/// Separates packwiz-specific orchestration (init, refresh, list, JAR caching)
/// from pure filesystem I/O, following the Interface Segregation Principle.
pub trait PackwizOps {
    /// Run packwiz init command to scaffold a new pack
    #[allow(clippy::too_many_arguments)]
    fn run_packwiz_init(
        &self,
        workdir: &Path,
        name: &str,
        author: &str,
        version: &str,
        modloader: &str,
        mc_version: &str,
        loader_version: &str,
    ) -> Result<(), StateError>;

    /// Run packwiz refresh command to sync index
    fn run_packwiz_refresh(&self, workdir: &Path) -> Result<(), StateError>;

    /// Get list of currently installed mods from packwiz
    fn get_installed_mods(&self, workdir: &Path) -> crate::Result<HashSet<String>>;

    /// Get the expected cache path for packwiz-installer-bootstrap.jar
    fn bootstrap_jar_cache_path(&self) -> crate::Result<PathBuf>;

    /// Get the expected cache path for packwiz-installer.jar
    fn installer_jar_cache_path(&self) -> crate::Result<PathBuf>;
}

/// Live implementation that routes all packwiz commands through ProcessProvider
pub struct LivePackwizOps<'a> {
    process: &'a dyn ProcessProvider,
    filesystem: &'a dyn FileSystemProvider,
}

impl<'a> LivePackwizOps<'a> {
    pub fn new(process: &'a dyn ProcessProvider, filesystem: &'a dyn FileSystemProvider) -> Self {
        Self {
            process,
            filesystem,
        }
    }
}

impl PackwizOps for LivePackwizOps<'_> {
    fn run_packwiz_init(
        &self,
        workdir: &Path,
        name: &str,
        author: &str,
        version: &str,
        modloader: &str,
        mc_version: &str,
        loader_version: &str,
    ) -> Result<(), StateError> {
        let pack_dir = workdir.join("pack");

        if !self.filesystem.exists(&pack_dir) {
            return Err(StateError::MissingFile {
                file: pack_dir.to_path_buf(),
            });
        }

        let mut args = vec![
            "init",
            "--name",
            name,
            "--author",
            author,
            "--version",
            version,
            "--mc-version",
            mc_version,
            "--modloader",
            modloader,
            "-y",
        ];

        match modloader {
            "neoforge" => {
                args.push("--neoforge-version");
                args.push(loader_version);
            }
            "fabric" => {
                args.push("--fabric-version");
                args.push(loader_version);
            }
            "quilt" => {
                args.push("--quilt-version");
                args.push(loader_version);
            }
            "forge" => {
                args.push("--forge-version");
                args.push(loader_version);
            }
            _ => {}
        }

        let output = self
            .process
            .execute("packwiz", &args, &pack_dir)
            .map_err(|e| StateError::CommandFailed {
                command: format!("packwiz init failed: {}", e),
            })?;

        if !output.success {
            return Err(StateError::CommandFailed {
                command: format!("packwiz init returned non-zero: {}", output.stderr),
            });
        }

        Ok(())
    }

    fn run_packwiz_refresh(&self, workdir: &Path) -> Result<(), StateError> {
        let pack_file = workdir.join("pack").join("pack.toml");

        let pack_file_str = pack_file.to_str().ok_or_else(|| StateError::IoError {
            source: anyhow::anyhow!("Invalid UTF-8 in pack.toml path"),
        })?;

        let output = self
            .process
            .execute(
                "packwiz",
                &["--pack-file", pack_file_str, "refresh"],
                workdir,
            )
            .map_err(|e| StateError::CommandFailed {
                command: format!("packwiz refresh failed: {}", e),
            })?;

        if !output.success {
            return Err(StateError::CommandFailed {
                command: format!("packwiz refresh returned non-zero: {}", output.stderr),
            });
        }

        Ok(())
    }

    fn get_installed_mods(&self, workdir: &Path) -> crate::Result<HashSet<String>> {
        let pack_dir = workdir.join("pack");
        let scan_dirs = ["mods", "resourcepacks", "shaderpacks"];

        let mut installed = HashSet::new();
        for folder in &scan_dirs {
            let dir = pack_dir.join(folder);
            if !self.filesystem.exists(&dir) {
                continue;
            }
            let file_list = self.filesystem.get_file_list(&dir)?;
            for path in &file_list {
                if let Some(file_name) = path.file_name().and_then(|f| f.to_str())
                    && let Some(slug) = file_name.strip_suffix(".pw.toml")
                    && !slug.is_empty()
                {
                    installed.insert(slug.to_string());
                }
            }
        }

        Ok(installed)
    }

    fn bootstrap_jar_cache_path(&self) -> crate::Result<PathBuf> {
        let cache_dir = crate::platform::cache::cache_root()?.join("jars");
        Ok(cache_dir.join("packwiz-installer-bootstrap.jar"))
    }

    fn installer_jar_cache_path(&self) -> crate::Result<PathBuf> {
        let cache_dir = crate::platform::cache::cache_root()?.join("jars");
        Ok(cache_dir.join("packwiz-installer.jar"))
    }
}

/// Check if packwiz is available in PATH and return version info.
///
/// Uses ProcessProvider::find_program for cross-platform program lookup.
pub fn check_packwiz_available(
    process: &dyn ProcessProvider,
    workdir: &Path,
) -> crate::Result<(bool, String)> {
    match process.find_program("packwiz") {
        Some(path) => {
            let version = get_packwiz_version(process, &path, workdir)
                .unwrap_or_else(|| "unknown".to_string());
            Ok((true, version))
        }
        None => Ok((false, "not found".to_string())),
    }
}

/// Get packwiz version using go toolchain.
///
/// Takes the packwiz binary path directly and queries version via `go version -m`.
/// The `workdir` parameter is only needed as a required arg to `process.execute`;
/// `go version -m` reads the binary at an absolute path and ignores the working directory.
pub fn get_packwiz_version(
    process: &dyn ProcessProvider,
    packwiz_path: &str,
    workdir: &Path,
) -> Option<String> {
    let go_output = process
        .execute("go", &["version", "-m", packwiz_path], workdir)
        .ok()?;
    if !go_output.success {
        return None;
    }

    for line in go_output.stdout.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("mod") {
            let fields: Vec<&str> = trimmed.split_whitespace().collect();
            if fields.len() >= 3 {
                return Some(fields[2].to_string());
            }
        }
    }

    None
}

/// Mock implementation for unit testing.
///
/// Replicates the mock behavior previously in MockFileSystemProvider:
/// - `run_packwiz_init` creates pack/pack.toml and pack/index.toml in-memory
/// - `run_packwiz_refresh` verifies pack.toml exists
/// - `get_installed_mods` returns a configured mock mod set
/// - JAR cache paths return test-appropriate paths
#[cfg(feature = "test-utils")]
pub struct MockPackwizOps {
    pub installed_mods: HashSet<String>,
    pub filesystem: std::sync::Arc<std::sync::Mutex<std::collections::HashMap<PathBuf, String>>>,
    pub current_dir: PathBuf,
    pub fail_init: bool,
}

#[cfg(feature = "test-utils")]
impl MockPackwizOps {
    pub fn new() -> Self {
        Self {
            installed_mods: HashSet::new(),
            filesystem: std::sync::Arc::new(
                std::sync::Mutex::new(std::collections::HashMap::new()),
            ),
            current_dir: crate::application::session_mocks::mock_root().join("workdir"),
            fail_init: false,
        }
    }

    pub fn with_failing_init(mut self) -> Self {
        self.fail_init = true;
        self
    }

    pub fn with_installed_mods(mut self, mods: HashSet<String>) -> Self {
        self.installed_mods = mods;
        self
    }

    pub fn with_current_dir(mut self, dir: PathBuf) -> Self {
        self.current_dir = dir;
        self
    }

    /// Connect to a MockFileSystemProvider's in-memory files for init/refresh side effects
    pub fn with_filesystem(
        mut self,
        files: std::sync::Arc<std::sync::Mutex<std::collections::HashMap<PathBuf, String>>>,
    ) -> Self {
        self.filesystem = files;
        self
    }
}

#[cfg(feature = "test-utils")]
impl Default for MockPackwizOps {
    fn default() -> Self {
        Self::new()
    }
}

/// Default index.toml template for packwiz mock init
#[cfg(feature = "test-utils")]
const MOCK_DEFAULT_INDEX_TOML: &str = r#"hash-format = "sha256"

[[files]]
file = "pack.toml"
hash = ""
"#;

#[cfg(feature = "test-utils")]
impl PackwizOps for MockPackwizOps {
    fn run_packwiz_init(
        &self,
        workdir: &Path,
        name: &str,
        author: &str,
        version: &str,
        modloader: &str,
        mc_version: &str,
        loader_version: &str,
    ) -> Result<(), StateError> {
        if self.fail_init {
            return Err(StateError::IoError {
                source: anyhow::anyhow!("mock packwiz init failure"),
            });
        }
        let pack_dir = workdir.join("pack");
        let pack_file = pack_dir.join("pack.toml");
        let loader_line = if modloader == "none" {
            String::new()
        } else {
            format!("{} = \"{}\"\n", modloader, loader_version)
        };
        let default_pack_toml = format!(
            r#"name = "{}"
author = "{}"
version = "{}"
pack-format = "packwiz:1.1.0"

[index]
file = "index.toml"
hash-format = "sha256"
hash = ""

[versions]
minecraft = "{}"
{}"#,
            name, author, version, mc_version, loader_line
        );
        self.filesystem
            .lock()
            .unwrap()
            .insert(pack_file, default_pack_toml);

        let index_file = pack_dir.join("index.toml");
        self.filesystem
            .lock()
            .unwrap()
            .insert(index_file, MOCK_DEFAULT_INDEX_TOML.to_string());

        Ok(())
    }

    fn run_packwiz_refresh(&self, workdir: &Path) -> Result<(), StateError> {
        let pack_file = workdir.join("pack").join("pack.toml");
        if !self.filesystem.lock().unwrap().contains_key(&pack_file) {
            return Err(StateError::MissingFile {
                file: pack_file.to_path_buf(),
            });
        }
        Ok(())
    }

    fn get_installed_mods(&self, _workdir: &Path) -> crate::Result<HashSet<String>> {
        Ok(self.installed_mods.clone())
    }

    fn bootstrap_jar_cache_path(&self) -> crate::Result<PathBuf> {
        Ok(self
            .current_dir
            .join("cache")
            .join("packwiz-installer-bootstrap.jar"))
    }

    fn installer_jar_cache_path(&self) -> crate::Result<PathBuf> {
        Ok(self.current_dir.join("cache").join("packwiz-installer.jar"))
    }
}

/// Errors from packwiz operations
#[derive(Debug, Error)]
pub enum PackwizError {
    #[error("Packwiz not available: {0}")]
    NotAvailable(String),

    #[error("Command failed: {command}\n{stderr}")]
    CommandFailed { command: String, stderr: String },

    #[error("Pack format error: {0}")]
    PackFormatError(String),

    #[error("Hash mismatch: {0}")]
    HashMismatchError(String),

    #[error("Invalid path: {reason}")]
    InvalidPath { reason: String },

    #[error("Process execution failed: {source}")]
    ProcessFailed {
        #[from]
        source: std::io::Error,
    },
}

/// Packwiz CLI wrapper for metadata management (.pw.toml files)
///
/// Wraps commands: `packwiz modrinth add`, `packwiz curseforge add`,
/// `packwiz remove`, `packwiz refresh`, `packwiz modrinth export`
pub struct PackwizMetadata<'a> {
    process_provider: &'a dyn ProcessProvider,
    pack_dir: PathBuf,
    packwiz_available: Option<bool>,
}

impl<'a> PackwizMetadata<'a> {
    /// Create a new PackwizMetadata instance from a session
    ///
    /// Extracts workdir from session config and constructs pack_dir path.
    /// Uses standalone construction pattern (not factory) to avoid lifetime issues.
    pub fn new(session: &'a dyn Session) -> Result<Self, PackwizError> {
        let workdir = match session.config().app_config().workdir.as_ref().cloned() {
            Some(w) => w,
            None => session
                .filesystem()
                .current_dir()
                .map_err(|e| PackwizError::InvalidPath {
                    reason: format!("Failed to get current directory: {e}"),
                })?,
        };

        let pack_dir = workdir.join("pack");

        Ok(Self {
            process_provider: session.process(),
            pack_dir,
            packwiz_available: None,
        })
    }

    /// Check if packwiz is available (cached check)
    fn ensure_packwiz(&mut self) -> Result<(), PackwizError> {
        if let Some(true) = self.packwiz_available {
            return Ok(());
        }

        let (available, _version) = check_packwiz_available(self.process_provider, &self.pack_dir)
            .map_err(|e| PackwizError::NotAvailable(e.to_string()))?;

        if !available {
            return Err(PackwizError::NotAvailable(
                "packwiz CLI not found in PATH. Install from: https://packwiz.infra.link/installation/".to_string(),
            ));
        }

        self.packwiz_available = Some(true);
        Ok(())
    }

    /// Add a project from Modrinth
    ///
    /// Executes: `packwiz modrinth add --project-id <id> -y`
    /// Creates: pack/mods/<mod-name>.pw.toml
    /// Updates: pack/index.toml
    pub fn add_mod(
        &mut self,
        project_id: &str,
        platform: ProjectPlatform,
    ) -> Result<(), PackwizError> {
        self.ensure_packwiz()?;

        let pack_toml = self.pack_dir.join("pack.toml");
        let pack_toml_str = pack_toml
            .to_str()
            .ok_or_else(|| PackwizError::InvalidPath {
                reason: "pack.toml path contains invalid UTF-8".to_string(),
            })?;

        let (platform_cmd, id_flag) = match platform {
            ProjectPlatform::Modrinth => ("modrinth", "--project-id"),
            ProjectPlatform::CurseForge => ("curseforge", "--addon-id"),
        };

        let output = self
            .process_provider
            .execute(
                "packwiz",
                &[
                    "--pack-file",
                    pack_toml_str,
                    platform_cmd,
                    "add",
                    id_flag,
                    project_id,
                    "-y",
                ],
                &self.pack_dir,
            )
            .map_err(|e| PackwizError::ProcessFailed {
                source: std::io::Error::other(e),
            })?;

        if !output.success {
            return Err(PackwizError::CommandFailed {
                command: format!("packwiz {} add {} {}", platform_cmd, id_flag, project_id),
                stderr: output.stderr,
            });
        }

        Ok(())
    }

    /// Remove a project by name
    ///
    /// Executes: `packwiz remove <name> -y`
    /// Deletes: pack/mods/<name>.pw.toml
    /// Updates: pack/index.toml
    pub fn remove_mod(&mut self, mod_name: &str) -> Result<(), PackwizError> {
        self.ensure_packwiz()?;

        let pack_toml = self.pack_dir.join("pack.toml");
        let pack_toml_str = pack_toml
            .to_str()
            .ok_or_else(|| PackwizError::InvalidPath {
                reason: "pack.toml path contains invalid UTF-8".to_string(),
            })?;

        let output = self
            .process_provider
            .execute(
                "packwiz",
                &["--pack-file", pack_toml_str, "remove", mod_name, "-y"],
                &self.pack_dir,
            )
            .map_err(|e| PackwizError::ProcessFailed {
                source: std::io::Error::other(e),
            })?;

        if !output.success {
            return Err(PackwizError::CommandFailed {
                command: format!("packwiz remove {}", mod_name),
                stderr: output.stderr,
            });
        }

        Ok(())
    }

    /// Refresh index file
    ///
    /// Executes: `packwiz refresh`
    /// Updates: pack/index.toml with latest hashes
    pub fn refresh_index(&mut self) -> Result<(), PackwizError> {
        self.ensure_packwiz()?;

        let pack_toml = self.pack_dir.join("pack.toml");
        let pack_toml_str = pack_toml
            .to_str()
            .ok_or_else(|| PackwizError::InvalidPath {
                reason: "pack.toml path contains invalid UTF-8".to_string(),
            })?;

        let output = self
            .process_provider
            .execute(
                "packwiz",
                &["--pack-file", pack_toml_str, "refresh"],
                &self.pack_dir,
            )
            .map_err(|e| PackwizError::ProcessFailed {
                source: std::io::Error::other(e),
            })?;

        if !output.success {
            // Parse stderr for specific errors
            if output.stderr.contains("Hash mismatch") {
                return Err(PackwizError::HashMismatchError(output.stderr.clone()));
            }

            if output.stderr.contains("pack format") && output.stderr.contains("not supported") {
                return Err(PackwizError::PackFormatError(output.stderr.clone()));
            }

            return Err(PackwizError::CommandFailed {
                command: "packwiz refresh".to_string(),
                stderr: output.stderr,
            });
        }

        Ok(())
    }

    /// Export to Modrinth pack format
    ///
    /// Executes: `packwiz modrinth export -o <output>`
    /// Creates: .mrpack ZIP archive
    pub fn export_mrpack(&mut self, output_path: &Path) -> Result<(), PackwizError> {
        self.ensure_packwiz()?;

        let pack_toml = self.pack_dir.join("pack.toml");
        let pack_toml_str = pack_toml
            .to_str()
            .ok_or_else(|| PackwizError::InvalidPath {
                reason: "pack.toml path contains invalid UTF-8".to_string(),
            })?;

        let output_str = output_path
            .to_str()
            .ok_or_else(|| PackwizError::InvalidPath {
                reason: "output path contains invalid UTF-8".to_string(),
            })?;

        let output = self
            .process_provider
            .execute(
                "packwiz",
                &[
                    "--pack-file",
                    pack_toml_str,
                    "modrinth",
                    "export",
                    "-o",
                    output_str,
                ],
                &self.pack_dir,
            )
            .map_err(|e| PackwizError::ProcessFailed {
                source: std::io::Error::other(e),
            })?;

        if !output.success {
            return Err(PackwizError::CommandFailed {
                command: format!("packwiz modrinth export -o {}", output_path.display()),
                stderr: output.stderr,
            });
        }

        Ok(())
    }
}

/// Packwiz-installer wrapper for build-time JAR downloads
///
/// Wraps: `java -jar packwiz-installer-bootstrap.jar --bootstrap-main-jar packwiz-installer.jar -g -s <side> <pack_toml_path>`
pub struct PackwizInstaller<'a> {
    filesystem: &'a dyn FileSystemProvider,
    process_provider: &'a dyn ProcessProvider,
    bootstrap_jar_path: PathBuf,
    installer_jar_path: PathBuf,
}

impl<'a> PackwizInstaller<'a> {
    /// Create a new PackwizInstaller instance
    ///
    /// Requires explicit bootstrap and installer JAR paths (caller is responsible for download/caching)
    pub fn new(
        session: &'a dyn Session,
        bootstrap_jar_path: PathBuf,
        installer_jar_path: PathBuf,
    ) -> Result<Self, PackwizError> {
        Ok(Self {
            filesystem: session.filesystem(),
            process_provider: session.process(),
            bootstrap_jar_path,
            installer_jar_path,
        })
    }

    /// Install projects for specified side
    ///
    /// Executes: `java -jar packwiz-installer-bootstrap.jar --bootstrap-main-jar packwiz-installer.jar -g -s <side> <pack_toml_path>`
    /// Downloads: Mod JARs from URLs in .pw.toml files
    /// Verifies: SHA-512 hashes
    /// Side: "both" (client+server), "client" (client-only), "server" (server-only)
    pub fn install_mods(&self, side: &str, working_dir: &Path) -> Result<(), PackwizError> {
        // Validate side parameter
        if !["both", "client", "server"].contains(&side) {
            return Err(PackwizError::CommandFailed {
                command: format!("install_mods({})", side),
                stderr: format!(
                    "Invalid side: {}. Must be 'both', 'client', or 'server'",
                    side
                ),
            });
        }

        let bootstrap_str =
            self.bootstrap_jar_path
                .to_str()
                .ok_or_else(|| PackwizError::InvalidPath {
                    reason: "Bootstrap JAR path contains invalid UTF-8".to_string(),
                })?;

        let installer_str =
            self.installer_jar_path
                .to_str()
                .ok_or_else(|| PackwizError::InvalidPath {
                    reason: "Installer JAR path contains invalid UTF-8".to_string(),
                })?;

        // Use v1 pattern: --bootstrap-main-jar <installer.jar> -g -s <side> <pack.toml>
        let pack_toml_path = working_dir.join("pack").join("pack.toml");
        let pack_toml_str = pack_toml_path
            .to_str()
            .ok_or_else(|| PackwizError::InvalidPath {
                reason: "pack.toml path contains invalid UTF-8".to_string(),
            })?;

        let output = self
            .process_provider
            .execute(
                "java",
                &[
                    "-jar",
                    bootstrap_str,
                    "--bootstrap-main-jar",
                    installer_str,
                    "-g",
                    "-s",
                    side,
                    pack_toml_str,
                ],
                working_dir,
            )
            .map_err(|e| PackwizError::ProcessFailed {
                source: std::io::Error::other(e),
            })?;

        if !output.success {
            return Err(PackwizError::CommandFailed {
                command: format!("packwiz-installer-bootstrap (side={})", side),
                stderr: output.stderr,
            });
        }

        Ok(())
    }

    /// Check if packwiz-installer-bootstrap.jar is available
    pub fn check_installer_available(&self) -> Result<bool, PackwizError> {
        Ok(self.filesystem.metadata_exists(&self.bootstrap_jar_path))
    }
}

#[cfg(test)]
mod tests {
    include!("packwiz.test.rs");
}
