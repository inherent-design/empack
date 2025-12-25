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

use crate::application::session::{ProcessProvider, Session};
use crate::primitives::ProjectPlatform;
use std::path::{Path, PathBuf};
use thiserror::Error;

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

        let (available, _version) = self
            .process_provider
            .check_packwiz()
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
    pub fn add_mod(&mut self, project_id: &str, platform: ProjectPlatform) -> Result<(), PackwizError> {
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

        let output = self.process_provider.execute(
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
        ).map_err(|e| PackwizError::ProcessFailed {
            source: std::io::Error::new(std::io::ErrorKind::Other, e),
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

        let output = self.process_provider.execute(
            "packwiz",
            &["--pack-file", pack_toml_str, "remove", mod_name, "-y"],
            &self.pack_dir,
        ).map_err(|e| PackwizError::ProcessFailed {
            source: std::io::Error::new(std::io::ErrorKind::Other, e),
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

        let output = self.process_provider.execute(
            "packwiz",
            &["--pack-file", pack_toml_str, "refresh"],
            &self.pack_dir,
        ).map_err(|e| PackwizError::ProcessFailed {
            source: std::io::Error::new(std::io::ErrorKind::Other, e),
        })?;

        if !output.success {
            // Parse stderr for specific errors
            if output.stderr.contains("Hash mismatch") {
                return Err(PackwizError::HashMismatchError(output.stderr.clone()));
            }

            if output.stderr.contains("pack format")
                && output.stderr.contains("not supported")
            {
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

        let output = self.process_provider.execute(
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
        ).map_err(|e| PackwizError::ProcessFailed {
            source: std::io::Error::new(std::io::ErrorKind::Other, e),
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
/// Wraps: `java -jar packwiz-installer-bootstrap.jar -g -s <side> --pack-folder pack`
pub struct PackwizInstaller<'a> {
    process_provider: &'a dyn ProcessProvider,
    bootstrap_jar_path: PathBuf,
}

impl<'a> PackwizInstaller<'a> {
    /// Create a new PackwizInstaller instance
    ///
    /// Requires explicit bootstrap JAR path (caller is responsible for download/caching)
    pub fn new(
        session: &'a dyn Session,
        bootstrap_jar_path: PathBuf,
    ) -> Result<Self, PackwizError> {
        Ok(Self {
            process_provider: session.process(),
            bootstrap_jar_path,
        })
    }

    /// Install projects for specified side
    ///
    /// Executes: `java -jar packwiz-installer-bootstrap.jar -g -s <side> --pack-folder pack`
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

        let jar_str = self
            .bootstrap_jar_path
            .to_str()
            .ok_or_else(|| PackwizError::InvalidPath {
                reason: "JAR path contains invalid UTF-8".to_string(),
            })?;

        let output = self.process_provider.execute(
            "java",
            &["-jar", jar_str, "-g", "-s", side, "--pack-folder", "pack"],
            working_dir,
        ).map_err(|e| PackwizError::ProcessFailed {
            source: std::io::Error::new(std::io::ErrorKind::Other, e),
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
        Ok(std::fs::metadata(&self.bootstrap_jar_path).is_ok())
    }
}

#[cfg(test)]
mod tests {
    include!("packwiz.test.rs");
}
