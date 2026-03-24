//! Build system for empack targets
//! Five-target system: mrpack, client, server, client-full, server-full

use crate::empack::PackwizInstaller;
use crate::primitives::*;
#[cfg(test)]
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use thiserror::Error;

use serde::Deserialize;
use sha1::{Digest, Sha1};

#[derive(Deserialize)]
struct MojangVersionManifest {
    versions: Vec<MojangVersionEntry>,
}

#[derive(Deserialize)]
struct MojangVersionEntry {
    id: String,
    url: String,
}

#[derive(Deserialize)]
struct MojangVersionMeta {
    downloads: MojangDownloads,
}

#[derive(Deserialize)]
struct MojangDownloads {
    server: MojangDownloadInfo,
}

#[derive(Deserialize)]
struct MojangDownloadInfo {
    url: String,
    sha1: String,
}

#[derive(Deserialize)]
struct FabricLoaderEntry {
    loader: FabricLoaderInfo,
}

#[derive(Deserialize)]
struct FabricLoaderInfo {
    version: String,
    stable: bool,
}

#[derive(Deserialize)]
struct FabricInstallerEntry {
    version: String,
    stable: bool,
}

/// Quilt Maven metadata for resolving the latest installer version.
///
/// XML structure:
/// ```xml
/// <metadata>
///   <versioning>
///     <release>0.12.0</release>
///   </versioning>
/// </metadata>
/// ```
#[derive(Deserialize)]
struct QuiltMavenMetadata {
    versioning: QuiltMavenVersioning,
}

#[derive(Deserialize)]
struct QuiltMavenVersioning {
    release: String,
}

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
    pub loader_version: String,
    pub loader_type: String,
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
        let workdir = match session.config().app_config().workdir.as_ref().cloned() {
            Some(w) => w,
            None => session
                .filesystem()
                .current_dir()
                .map_err(|e| BuildError::ConfigError {
                    reason: format!("Failed to get current directory: {e}"),
                })?,
        };
        let dist_dir = crate::empack::state::artifact_root(&workdir);

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
        if self.pack_info.is_none() {
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

            let versions_table = toml_value.get("versions");
            let (loader_type, loader_version) = if let Some(versions) = versions_table {
                if let Some(v) = versions.get("fabric").and_then(|v| v.as_str()) {
                    ("fabric".to_string(), v.to_string())
                } else if let Some(v) = versions.get("neoforge").and_then(|v| v.as_str()) {
                    ("neoforge".to_string(), v.to_string())
                } else if let Some(v) = versions.get("forge").and_then(|v| v.as_str()) {
                    ("forge".to_string(), v.to_string())
                } else if let Some(v) = versions.get("quilt").and_then(|v| v.as_str()) {
                    ("quilt".to_string(), v.to_string())
                } else {
                    ("vanilla".to_string(), String::new())
                }
            } else {
                ("vanilla".to_string(), String::new())
            };

            self.pack_info = Some(PackInfo {
                author,
                name,
                version,
                mc_version,
                loader_version,
                loader_type,
            });
        }

        match self.pack_info.as_ref() {
            Some(pack_info) => Ok(pack_info),
            None => unreachable!("pack info should be cached after loading"),
        }
    }

    /// Download or install the Minecraft server JAR into `dist_dir`.
    ///
    /// Dispatches per loader type; each arm calls a dedicated provider method.
    fn download_server_jar(
        &self,
        dist_dir: &Path,
        pack_info: &PackInfo,
    ) -> Result<(), BuildError> {
        match pack_info.loader_type.as_str() {
            "vanilla" => self.install_vanilla_server(dist_dir, pack_info),
            "fabric" => self.install_fabric_server(dist_dir, pack_info),
            "quilt" => self.install_quilt_server(dist_dir, pack_info),
            "neoforge" => self.install_neoforge_server(dist_dir, pack_info),
            "forge" => self.install_forge_server(dist_dir, pack_info),
            other => Err(BuildError::ConfigError {
                reason: format!("unsupported loader type for server builds: {}", other),
            }),
        }
    }

    /// Download the vanilla Minecraft server JAR from Mojang.
    ///
    /// Resolves the version manifest, fetches per-version metadata, downloads
    /// the server JAR, and verifies its SHA1 hash.
    fn install_vanilla_server(
        &self,
        dist_dir: &Path,
        pack_info: &PackInfo,
    ) -> Result<(), BuildError> {
        let manifest_text = self.fetch_url_text(
            "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json",
        )?;
        let manifest: MojangVersionManifest =
            serde_json::from_str(&manifest_text).map_err(|e| BuildError::ConfigError {
                reason: format!("failed to parse Mojang version manifest: {}", e),
            })?;

        let entry = manifest
            .versions
            .iter()
            .find(|v| v.id == pack_info.mc_version)
            .ok_or_else(|| BuildError::ConfigError {
                reason: format!(
                    "Minecraft version {} not found in Mojang manifest",
                    pack_info.mc_version
                ),
            })?;

        let version_meta_text = self.fetch_url_text(&entry.url)?;
        let version_meta: MojangVersionMeta =
            serde_json::from_str(&version_meta_text).map_err(|e| BuildError::ConfigError {
                reason: format!("failed to parse Mojang version metadata: {}", e),
            })?;

        let jar_path = dist_dir.join("srv.jar");
        self.download_file(&version_meta.downloads.server.url, &jar_path)?;

        let jar_bytes = self
            .session
            .filesystem()
            .read_bytes(&jar_path)
            .map_err(|e| BuildError::ConfigError {
                reason: format!("failed to read downloaded server JAR: {}", e),
            })?;
        let hash = format!("{:x}", Sha1::digest(&jar_bytes));
        if hash != version_meta.downloads.server.sha1 {
            return Err(BuildError::ValidationError {
                reason: format!(
                    "SHA1 mismatch for server JAR: expected {}, got {}",
                    version_meta.downloads.server.sha1, hash
                ),
            });
        }

        Ok(())
    }

    /// Download a pre-built Fabric server JAR from the Fabric Meta API.
    ///
    /// Resolves the loader and installer versions, then downloads the merged
    /// server JAR directly (no Java required).
    fn install_fabric_server(
        &self,
        dist_dir: &Path,
        pack_info: &PackInfo,
    ) -> Result<(), BuildError> {
        let mc = &pack_info.mc_version;

        let loader_version = if pack_info.loader_version.is_empty() {
            let url = format!("https://meta.fabricmc.net/v2/versions/loader/{}", mc);
            let text = self.fetch_url_text(&url)?;
            let entries: Vec<FabricLoaderEntry> =
                serde_json::from_str(&text).map_err(|e| BuildError::ConfigError {
                    reason: format!("failed to parse Fabric loader versions: {}", e),
                })?;
            entries
                .iter()
                .find(|e| e.loader.stable)
                .map(|e| e.loader.version.clone())
                .ok_or_else(|| BuildError::ConfigError {
                    reason: "no stable Fabric loader version found".to_string(),
                })?
        } else {
            pack_info.loader_version.clone()
        };

        let installer_text =
            self.fetch_url_text("https://meta.fabricmc.net/v2/versions/installer")?;
        let installers: Vec<FabricInstallerEntry> =
            serde_json::from_str(&installer_text).map_err(|e| BuildError::ConfigError {
                reason: format!("failed to parse Fabric installer versions: {}", e),
            })?;
        let installer_version = installers
            .iter()
            .find(|e| e.stable)
            .map(|e| e.version.clone())
            .ok_or_else(|| BuildError::ConfigError {
                reason: "no stable Fabric installer version found".to_string(),
            })?;

        let jar_url = format!(
            "https://meta.fabricmc.net/v2/versions/loader/{}/{}/{}/server/jar",
            mc, loader_version, installer_version
        );
        self.download_file(&jar_url, &dist_dir.join("srv.jar"))
    }

    /// Download and run the Quilt server installer.
    ///
    /// Fetches the latest installer version from Maven, downloads the
    /// installer JAR, then invokes it with `java -jar` to install the
    /// Quilt server.
    fn install_quilt_server(
        &self,
        dist_dir: &Path,
        pack_info: &PackInfo,
    ) -> Result<(), BuildError> {
        let maven_xml = self.fetch_url_text(
            "https://maven.quiltmc.org/repository/release/org/quiltmc/quilt-installer/maven-metadata.xml",
        )?;
        let metadata: QuiltMavenMetadata =
            quick_xml::de::from_str(&maven_xml).map_err(|e| BuildError::ConfigError {
                reason: format!("failed to parse Quilt Maven metadata: {}", e),
            })?;
        let installer_version = &metadata.versioning.release;

        let installer_filename = format!("quilt-installer-{}.jar", installer_version);
        let installer_url = format!(
            "https://maven.quiltmc.org/repository/release/org/quiltmc/quilt-installer/{v}/{f}",
            v = installer_version,
            f = installer_filename
        );
        let installer_path = dist_dir.join(&installer_filename);
        self.download_file(&installer_url, &installer_path)?;

        let install_dir_flag = format!("--install-dir={}", dist_dir.to_string_lossy());
        let output = self
            .session
            .process()
            .execute(
                "java",
                &[
                    "-jar",
                    &installer_path.to_string_lossy(),
                    "install",
                    "server",
                    &pack_info.mc_version,
                    &install_dir_flag,
                    "--create-scripts",
                    "--download-server",
                ],
                dist_dir,
            )
            .map_err(|_| BuildError::MissingTool {
                tool: "java".to_string(),
            })?;

        if !output.success {
            return Err(BuildError::CommandFailed {
                command: format!("quilt installer failed: {}", output.stderr),
            });
        }

        // Quilt creates quilt-server-launch.jar and server.jar; rename
        // server.jar to srv.jar for consistency across loaders.
        let server_jar = dist_dir.join("server.jar");
        let srv_jar = dist_dir.join("srv.jar");
        if self.session.filesystem().exists(&server_jar) && !self.session.filesystem().exists(&srv_jar) {
            let bytes = self
                .session
                .filesystem()
                .read_bytes(&server_jar)
                .map_err(|e| BuildError::ConfigError {
                    reason: format!("failed to read server.jar for rename: {}", e),
                })?;
            self.session
                .filesystem()
                .write_bytes(&srv_jar, &bytes)
                .map_err(|e| BuildError::ConfigError {
                    reason: format!("failed to write srv.jar: {}", e),
                })?;
            let _ = self.session.filesystem().remove_file(&server_jar);
        }

        let _ = self.session.filesystem().remove_file(&installer_path);
        Ok(())
    }

    /// Download and run the NeoForge server installer.
    ///
    /// MC 1.20.1 uses the `forge` namespace on the NeoForged Maven as a special case.
    fn install_neoforge_server(
        &self,
        dist_dir: &Path,
        pack_info: &PackInfo,
    ) -> Result<(), BuildError> {
        let version = &pack_info.loader_version;
        let mc = &pack_info.mc_version;

        let (url, installer_filename) = if mc == "1.20.1" {
            (
                format!(
                    "https://maven.neoforged.net/releases/net/neoforged/forge/1.20.1-{v}/forge-1.20.1-{v}-installer.jar",
                    v = version
                ),
                format!("forge-1.20.1-{}-installer.jar", version),
            )
        } else {
            (
                format!(
                    "https://maven.neoforged.net/releases/net/neoforged/neoforge/{v}/neoforge-{v}-installer.jar",
                    v = version
                ),
                format!("neoforge-{}-installer.jar", version),
            )
        };

        let installer_path = dist_dir.join(&installer_filename);
        self.download_file(&url, &installer_path)?;

        let output = self
            .session
            .process()
            .execute(
                "java",
                &[
                    "-jar",
                    &installer_path.to_string_lossy(),
                    "--install-server",
                    &dist_dir.to_string_lossy(),
                ],
                dist_dir,
            )
            .map_err(|_| BuildError::MissingTool {
                tool: "java".to_string(),
            })?;

        if !output.success {
            return Err(BuildError::CommandFailed {
                command: format!("neoforge installer failed: {}", output.stderr),
            });
        }

        // Clean up installer JAR from the distribution
        let _ = self.session.filesystem().remove_file(&installer_path);
        Ok(())
    }

    /// Download and run the Forge server installer.
    fn install_forge_server(
        &self,
        dist_dir: &Path,
        pack_info: &PackInfo,
    ) -> Result<(), BuildError> {
        let mc = &pack_info.mc_version;
        let version = &pack_info.loader_version;
        let composite = format!("{}-{}", mc, version);

        let url = format!(
            "https://maven.minecraftforge.net/net/minecraftforge/forge/{c}/forge-{c}-installer.jar",
            c = composite
        );
        let installer_filename = format!("forge-{}-installer.jar", composite);
        let installer_path = dist_dir.join(&installer_filename);

        self.download_file(&url, &installer_path)?;

        let output = self
            .session
            .process()
            .execute(
                "java",
                &[
                    "-jar",
                    &installer_path.to_string_lossy(),
                    "--installServer",
                    &dist_dir.to_string_lossy(),
                ],
                dist_dir,
            )
            .map_err(|_| BuildError::MissingTool {
                tool: "java".to_string(),
            })?;

        if !output.success {
            return Err(BuildError::CommandFailed {
                command: format!("forge installer failed: {}", output.stderr),
            });
        }

        let _ = self.session.filesystem().remove_file(&installer_path);
        Ok(())
    }

    /// Download a file from `url` and write it to `dest` using reqwest.
    fn download_file(&self, url: &str, dest: &Path) -> Result<(), BuildError> {
        let bytes = self.fetch_url_bytes(url)?;
        self.session
            .filesystem()
            .write_bytes(dest, &bytes)
            .map_err(|e| BuildError::ConfigError {
                reason: format!("failed to write downloaded file to {}: {}", dest.display(), e),
            })
    }

    /// Fetch URL content as a String.
    fn fetch_url_text(&self, url: &str) -> Result<String, BuildError> {
        let bytes = self.fetch_url_bytes(url)?;
        String::from_utf8(bytes).map_err(|e| BuildError::ConfigError {
            reason: format!("response from {} is not valid UTF-8: {}", url, e),
        })
    }

    /// Fetch raw bytes from a URL using the session HTTP client.
    ///
    /// Runs the HTTP request on a dedicated thread with its own tokio runtime
    /// to avoid conflicts with the caller's async context. Works with both
    /// single-threaded and multi-threaded tokio runtimes.
    fn fetch_url_bytes(&self, url: &str) -> Result<Vec<u8>, BuildError> {
        // Gate on session network availability (respects test mocking)
        let _ = self
            .session
            .network()
            .http_client()
            .map_err(|e| BuildError::ConfigError {
                reason: format!("HTTP client unavailable: {}", e),
            })?;

        let url_owned = url.to_string();
        std::thread::scope(|s| {
            s.spawn(|| {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|e| BuildError::ConfigError {
                        reason: format!("failed to create HTTP runtime: {}", e),
                    })?;

                rt.block_on(async {
                    let client = reqwest::Client::builder()
                        .timeout(std::time::Duration::from_secs(60))
                        .build()
                        .map_err(|e| BuildError::ConfigError {
                            reason: format!("failed to create HTTP client: {}", e),
                        })?;

                    let resp = client
                        .get(&url_owned)
                        .send()
                        .await
                        .map_err(|e| BuildError::CommandFailed {
                            command: format!("HTTP GET {} failed: {}", url_owned, e),
                        })?;

                    if !resp.status().is_success() {
                        return Err(BuildError::CommandFailed {
                            command: format!(
                                "HTTP GET {} returned status {}",
                                url_owned,
                                resp.status()
                            ),
                        });
                    }

                    resp.bytes()
                        .await
                        .map(|b| b.to_vec())
                        .map_err(|e| BuildError::CommandFailed {
                            command: format!(
                                "failed to read response body from {}: {}",
                                url_owned, e
                            ),
                        })
                })
            })
            .join()
            .unwrap_or_else(|_| {
                Err(BuildError::CommandFailed {
                    command: "HTTP request thread panicked".to_string(),
                })
            })
        })
    }

    /// Register build targets (V1's register_all_build_targets pattern)
    #[cfg(test)]
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
                &["--pack-file", &pack_file.to_string_lossy(), "refresh"],
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
                    &mrpack_file.to_string_lossy(),
                    "-d",
                    &temp_extract_dir.to_string_lossy(),
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
        let filename = format!("{}-v{}-{}.zip", pack_info.name, pack_info.version, target);
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
            .execute("zip", &["-r0", &zip_path.to_string_lossy(), "./"], &dist_dir)
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
                    &pack_file.to_string_lossy(),
                    "mr",
                    "export",
                    "-o",
                    &output_file.to_string_lossy(),
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
            .read_bytes(bootstrap_jar_path)
            .map_err(|e| BuildError::ConfigError {
                reason: e.to_string(),
            })?;
        self.session
            .filesystem()
            .write_bytes(
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
            .read_bytes(bootstrap_jar_path)
            .map_err(|e| BuildError::ConfigError {
                reason: e.to_string(),
            })?;
        self.session
            .filesystem()
            .write_bytes(
                &dist_dir.join("packwiz-installer-bootstrap.jar"),
                &bootstrap_content,
            )
            .map_err(|e| BuildError::ConfigError {
                reason: e.to_string(),
            })?;

        // Step 6: Download the appropriate Minecraft server JAR
        let pack_info = self.load_pack_info()?.clone();
        if let Err(e) = self.download_server_jar(&dist_dir, &pack_info) {
            return Ok(BuildResult {
                target: BuildTarget::Server,
                success: false,
                output_path: None,
                artifacts: vec![],
                warnings: vec![format!("failed to download server JAR: {}", e)],
            });
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
        installer_jar_path: &Path,
    ) -> Result<BuildResult, BuildError> {
        // Step 1: Clean the dist/client-full/ directory
        self.clean_target(BuildTarget::ClientFull)?;

        // Step 2: Refresh the pack using packwiz refresh
        self.refresh_pack()?;
        self.load_pack_info()?;

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
        let installer = PackwizInstaller::new(
            self.session,
            bootstrap_jar_path.to_owned(),
            installer_jar_path.to_owned(),
        )
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
        installer_jar_path: &Path,
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

        // Step 4: Download the appropriate Minecraft server JAR
        let pack_info = self.load_pack_info()?.clone();
        if let Err(e) = self.download_server_jar(&dist_dir, &pack_info) {
            return Ok(BuildResult {
                target: BuildTarget::ServerFull,
                success: false,
                output_path: None,
                artifacts: vec![],
                warnings: vec![format!("failed to download server JAR: {}", e)],
            });
        }

        // Step 5: Execute packwiz-installer-bootstrap.jar with -g and -s server flags

        // Copy pack files to server-full directory for installer to use
        let pack_dir = self.workdir.join("pack");
        self.copy_dir_contents(&pack_dir, &dist_dir.join("pack"))?;

        // Execute installer with PackwizInstaller abstraction
        let installer = PackwizInstaller::new(
            self.session,
            bootstrap_jar_path.to_owned(),
            installer_jar_path.to_owned(),
        )
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

    /// Execute V1's proven 5-target build pipeline with state management.
    /// Uses an RAII guard so the state marker is removed on both success and
    /// failure (including panics) without manual cleanup.
    pub async fn execute_build_pipeline(
        &mut self,
        targets: &[BuildTarget],
    ) -> Result<Vec<BuildResult>, BuildError> {
        let state_mgr = self.session.state().map_err(|e| BuildError::ConfigError {
            reason: format!("Failed to get state manager: {}", e),
        })?;
        let guard = state_mgr
            .guarded_transition(crate::primitives::MarkerKind::Building)
            .map_err(|e| BuildError::ConfigError {
                reason: format!("Failed to begin build transition: {:?}", e),
            })?;

        let result = self.execute_build_pipeline_inner(targets)?;

        guard.complete().map_err(|e| BuildError::ConfigError {
            reason: format!("Failed to complete build transition: {:?}", e),
        })?;

        Ok(result)
    }

    /// Inner build pipeline logic, separated so the caller can guarantee
    /// state cleanup on early returns.
    fn execute_build_pipeline_inner(
        &mut self,
        targets: &[BuildTarget],
    ) -> Result<Vec<BuildResult>, BuildError> {
        self.prepare_build_environment()?;

        // Get bootstrap and installer JAR paths from session
        let bootstrap_jar_path = self
            .session
            .packwiz()
            .bootstrap_jar_cache_path()
            .map_err(|e| BuildError::ConfigError {
                reason: format!("Failed to get bootstrap JAR path: {}", e),
            })?;

        let installer_jar_path = self
            .session
            .packwiz()
            .installer_jar_cache_path()
            .map_err(|e| BuildError::ConfigError {
                reason: format!("Failed to get installer JAR path: {}", e),
            })?;

        let mut results = Vec::new();

        for target in targets {
            let result = match target {
                BuildTarget::Mrpack => self.build_mrpack_impl()?,
                BuildTarget::Client => self.build_client_impl(&bootstrap_jar_path)?,
                BuildTarget::Server => self.build_server_impl(&bootstrap_jar_path)?,
                BuildTarget::ClientFull => {
                    self.build_client_full_impl(&bootstrap_jar_path, &installer_jar_path)?
                }
                BuildTarget::ServerFull => {
                    self.build_server_full_impl(&bootstrap_jar_path, &installer_jar_path)?
                }
            };

            if !result.success {
                let details = if result.warnings.is_empty() {
                    "no additional details".to_string()
                } else {
                    result.warnings.join("; ")
                };

                return Err(BuildError::CommandFailed {
                    command: format!("Build failed for target {:?}: {}", result.target, details),
                });
            }

            results.push(result);
        }

        Ok(results)
    }

    /// Execute clean pipeline with state management.
    /// Uses an RAII guard: on success `complete()` removes the marker; on failure
    /// or panic the marker persists so `discover_state()` reports `Interrupted`.
    pub async fn execute_clean_pipeline(
        &mut self,
        targets: &[BuildTarget],
    ) -> Result<(), BuildError> {
        let state_mgr = self.session.state().map_err(|e| BuildError::ConfigError {
            reason: format!("Failed to get state manager: {}", e),
        })?;
        let guard = state_mgr
            .guarded_transition(crate::primitives::MarkerKind::Cleaning)
            .map_err(|e| BuildError::ConfigError {
                reason: format!("Failed to begin clean transition: {:?}", e),
            })?;

        self.execute_clean_pipeline_inner(targets)?;

        guard.complete().map_err(|e| BuildError::ConfigError {
            reason: format!("Failed to complete clean transition: {:?}", e),
        })?;

        Ok(())
    }

    /// Inner clean pipeline logic, separated so the caller can guarantee
    /// state cleanup on early returns.
    fn execute_clean_pipeline_inner(
        &mut self,
        targets: &[BuildTarget],
    ) -> Result<(), BuildError> {
        // Clean each target
        for target in targets {
            self.clean_target(*target)?;
        }

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
                if let Some(file_name) = file_path.file_name()
                    && file_name != ".gitkeep"
                {
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

        // Clean zip file (V1 pattern)
        if let Some(info) = pack_info {
            let zip_file = self
                .dist_dir
                .join(format!("{}-v{}-{}.zip", info.name, info.version, target));
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
            .map_err(|e| BuildError::ConfigError {
                reason: e.to_string(),
            })?;
        for path in template_files {
            if !self.session.filesystem().is_directory(&path) {
                let raw_name = path.file_name().unwrap();
                let filename = raw_name.to_string_lossy();
                let target_file = if let Some(stripped) = filename.strip_suffix(".template") {
                    target_dir.join(stripped)
                } else {
                    target_dir.join(&*filename)
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
                    .replace("{{LOADER_VERSION}}", &pack_info.loader_version);

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
        if !self.session.filesystem().exists(path) {
            return Err(BuildError::ValidationError {
                reason: format!(
                    "Build command completed without creating expected artifact: {}",
                    path.display()
                ),
            });
        }

        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

        // For mock filesystem, we'll use content length as size
        let size = self
            .session
            .filesystem()
            .read_to_string(path)
            .map(|content| content.len() as u64)
            .unwrap_or(0);

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
