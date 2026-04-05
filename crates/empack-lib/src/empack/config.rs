//! Configuration management for empack projects
//! Unified empack.yml (user intent) and pack.toml (packwiz reality) handling

use crate::empack::parsing::ModLoader;
use crate::primitives::{ProjectPlatform, ProjectType};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use thiserror::Error;

/// Configuration errors
#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("IO error: {source}")]
    IoError {
        #[from]
        source: std::io::Error,
    },

    #[error("YAML parsing error: {source}")]
    YamlError {
        #[source]
        source: serde_saphyr::Error,
    },

    #[error("YAML serialization error: {source}")]
    YamlSerError {
        #[source]
        source: serde_saphyr::ser_error::Error,
    },

    #[error("TOML parsing error: {source}")]
    TomlError {
        #[from]
        source: toml::de::Error,
    },

    #[error("Missing required field: {field}")]
    MissingField { field: String },

    #[error("Invalid project specification: {spec}")]
    InvalidProjectSpec { spec: String },

    #[error("Configuration validation error: {reason}")]
    ValidationError { reason: String },

    #[error("Ambiguous removal: '{query}' matches multiple dependencies: {matches:?}")]
    AmbiguousRemoval { query: String, matches: Vec<String> },
}

/// Top-level empack.yml configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmpackConfig {
    pub empack: EmpackProjectConfig,
}

/// Discriminator for resolved dependency entries
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DependencyStatus {
    Resolved,
}

/// A fully resolved dependency entry in empack.yml
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DependencyRecord {
    /// Status discriminator (always "resolved")
    pub status: DependencyStatus,

    /// Display name for UI (e.g., "Sodium")
    pub title: String,

    /// Canonical provider: modrinth or curseforge
    pub platform: ProjectPlatform,

    /// Canonical provider project ID (e.g., "AANobbMI" or "306612")
    pub project_id: String,

    /// Resource type
    #[serde(default = "default_project_type")]
    #[serde(rename = "type")]
    pub project_type: ProjectType,

    /// Optional pinned version ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// Hand-written search stub, resolved to DependencyRecord on sync
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DependencySearch {
    pub title: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "type")]
    pub project_type: Option<ProjectType>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub platform: Option<ProjectPlatform>,
}

/// A dependency entry that is either a resolved record or a search stub
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DependencyEntry {
    Resolved(DependencyRecord),
    Search(DependencySearch),
}

/// Common accessors for any dependency variant
pub trait Dependency {
    fn title(&self) -> &str;
    fn project_type(&self) -> Option<ProjectType>;
}

impl Dependency for DependencyEntry {
    fn title(&self) -> &str {
        match self {
            DependencyEntry::Resolved(r) => &r.title,
            DependencyEntry::Search(s) => &s.title,
        }
    }

    fn project_type(&self) -> Option<ProjectType> {
        match self {
            DependencyEntry::Resolved(r) => Some(r.project_type),
            DependencyEntry::Search(s) => s.project_type,
        }
    }
}

fn default_project_type() -> ProjectType {
    ProjectType::Mod
}

/// Project configuration within empack.yml
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EmpackProjectConfig {
    /// Dependencies keyed by slug (= packwiz .pw.toml filename stem)
    #[serde(default)]
    pub dependencies: BTreeMap<String, DependencyEntry>,

    /// Target Minecraft version (if not specified, extracted from pack.toml)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minecraft_version: Option<String>,

    /// Target mod loader (if not specified, extracted from pack.toml)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loader: Option<ModLoader>,

    /// Mod loader version (if not specified, extracted from pack.toml)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loader_version: Option<String>,

    /// Relative path from the pack root where datapacks are installed.
    ///
    /// Required for packwiz to route datapack-typed content. When set,
    /// empack writes `datapack-folder` into `pack.toml [options]`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub datapack_folder: Option<String>,

    /// Additional Minecraft versions accepted during mod resolution.
    ///
    /// Widens version matching so a 1.20.1 pack can accept mods tagged
    /// for 1.20 or 1.20.2. Written as `acceptable-game-versions` in
    /// `pack.toml [options]`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acceptable_game_versions: Option<Vec<String>>,

    /// Optional modpack metadata (if not specified, extracted from pack.toml)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// Packwiz pack.toml metadata for fallback values
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackMetadata {
    pub name: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,

    pub versions: PackVersions,
}

/// Version information from pack.toml
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackVersions {
    pub minecraft: String,
    #[serde(flatten)]
    pub loader_versions: HashMap<String, String>,
}

/// Unified project plan after empack configuration resolution
#[derive(Debug, Clone)]
pub struct ProjectPlan {
    /// Project metadata
    pub name: String,
    pub author: Option<String>,
    pub version: Option<String>,

    /// Target platform
    pub minecraft_version: String,
    pub loader: Option<ModLoader>,
    pub loader_version: String,

    /// Resolved project specifications from empack.yml
    pub dependencies: Vec<ProjectSpec>,
}

/// Project specification derived from a DependencyRecord + plan context
#[derive(Debug, Clone)]
pub struct ProjectSpec {
    /// Slug key (= packwiz filename stem)
    pub key: String,

    /// Search query (= title from record)
    pub search_query: String,

    /// Project type filter
    pub project_type: ProjectType,

    /// Target Minecraft version (defaults to plan version)
    pub minecraft_version: String,

    /// Target mod loader (defaults to plan loader; None for vanilla)
    pub loader: Option<ModLoader>,

    /// Required project ID (from DependencyRecord)
    pub project_id: String,

    /// Required project platform (from DependencyRecord)
    pub project_platform: ProjectPlatform,

    /// Optional pinned version
    pub version_pin: Option<String>,
}

/// Configuration manager bridging empack.yml and pack.toml
pub struct ConfigManager<'a> {
    workdir: PathBuf,
    fs_provider: &'a dyn crate::application::session::FileSystemProvider,
}

impl<'a> ConfigManager<'a> {
    pub fn new(
        workdir: PathBuf,
        fs_provider: &'a dyn crate::application::session::FileSystemProvider,
    ) -> Self {
        Self {
            workdir,
            fs_provider,
        }
    }

    /// Load empack.yml configuration
    pub fn load_empack_config(&self) -> Result<EmpackConfig, ConfigError> {
        let empack_path = self.workdir.join("empack.yml");

        if !self.fs_provider.exists(&empack_path) {
            return Err(ConfigError::MissingField {
                field: "empack.yml".to_string(),
            });
        }

        let content =
            self.fs_provider
                .read_to_string(&empack_path)
                .map_err(|e| ConfigError::IoError {
                    source: std::io::Error::other(e),
                })?;
        let config: EmpackConfig =
            serde_saphyr::from_str(&content).map_err(|e| ConfigError::YamlError { source: e })?;

        Ok(config)
    }

    /// Extract pack.toml metadata for fallback values
    pub fn load_pack_metadata(&self) -> Result<Option<PackMetadata>, ConfigError> {
        let pack_path = self.workdir.join("pack").join("pack.toml");

        if !self.fs_provider.exists(&pack_path) {
            return Ok(None);
        }

        let content =
            self.fs_provider
                .read_to_string(&pack_path)
                .map_err(|e| ConfigError::IoError {
                    source: std::io::Error::other(e),
                })?;
        let metadata: PackMetadata = toml::from_str(&content)?;

        Ok(Some(metadata))
    }

    /// Create unified project plan from empack.yml with pack.toml fallbacks
    pub fn create_project_plan(&self) -> Result<ProjectPlan, ConfigError> {
        let empack_config = self.load_empack_config()?;
        let pack_metadata = self.load_pack_metadata()?;

        // Resolve metadata with empack.yml taking precedence
        let name = empack_config
            .empack
            .name
            .clone()
            .or_else(|| pack_metadata.as_ref().map(|p| p.name.clone()))
            .unwrap_or_else(|| "Unnamed Modpack".to_string());

        let author = empack_config
            .empack
            .author
            .clone()
            .or_else(|| pack_metadata.as_ref().and_then(|p| p.author.clone()));

        let version = empack_config
            .empack
            .version
            .clone()
            .or_else(|| pack_metadata.as_ref().and_then(|p| p.version.clone()));

        // Resolve platform details with empack.yml taking precedence
        let minecraft_version = empack_config
            .empack
            .minecraft_version
            .clone()
            .or_else(|| pack_metadata.as_ref().map(|p| p.versions.minecraft.clone()))
            .ok_or_else(|| ConfigError::MissingField {
                field: "minecraft_version (from empack.yml or pack.toml)".to_string(),
            })?;

        let loader: Option<ModLoader> = if empack_config.empack.loader.is_some() {
            empack_config.empack.loader
        } else if let Some(pack_meta) = &pack_metadata {
            self.infer_loader_from_metadata(pack_meta).ok()
        } else {
            None
        };

        let loader_version = empack_config
            .empack
            .loader_version
            .clone()
            .or_else(|| {
                loader.and_then(|l| {
                    pack_metadata
                        .as_ref()
                        .and_then(|p| self.get_loader_version_from_metadata(p, &l).ok())
                })
            })
            .unwrap_or_default();

        // Build project specs from resolved dependency records only
        let mut dependencies = Vec::new();
        for (slug, entry) in &empack_config.empack.dependencies {
            if let DependencyEntry::Resolved(record) = entry {
                let spec =
                    self.build_project_spec_from_record(slug, record, &minecraft_version, loader);
                dependencies.push(spec);
            }
        }

        Ok(ProjectPlan {
            name,
            author,
            version,
            minecraft_version,
            loader,
            loader_version,
            dependencies,
        })
    }

    /// Infer mod loader from pack metadata
    pub(crate) fn infer_loader_from_metadata(
        &self,
        pack_metadata: &PackMetadata,
    ) -> Result<ModLoader, ConfigError> {
        // Check for known loader keys in versions
        if pack_metadata
            .versions
            .loader_versions
            .contains_key("fabric")
        {
            Ok(ModLoader::Fabric)
        } else if pack_metadata.versions.loader_versions.contains_key("forge") {
            Ok(ModLoader::Forge)
        } else if pack_metadata.versions.loader_versions.contains_key("quilt") {
            Ok(ModLoader::Quilt)
        } else if pack_metadata
            .versions
            .loader_versions
            .contains_key("neoforge")
        {
            Ok(ModLoader::NeoForge)
        } else {
            Err(ConfigError::ValidationError {
                reason: "Cannot infer mod loader from pack.toml versions".to_string(),
            })
        }
    }

    /// Get loader version from pack metadata
    fn get_loader_version_from_metadata(
        &self,
        pack_metadata: &PackMetadata,
        loader: &ModLoader,
    ) -> Result<String, ConfigError> {
        let loader_key = match loader {
            ModLoader::Fabric => "fabric",
            ModLoader::Forge => "forge",
            ModLoader::Quilt => "quilt",
            ModLoader::NeoForge => "neoforge",
        };

        pack_metadata
            .versions
            .loader_versions
            .get(loader_key)
            .cloned()
            .ok_or_else(|| ConfigError::MissingField {
                field: format!("versions.{} in pack.toml", loader_key),
            })
    }

    fn build_project_spec_from_record(
        &self,
        slug: &str,
        record: &DependencyRecord,
        default_minecraft: &str,
        default_loader: Option<ModLoader>,
    ) -> ProjectSpec {
        ProjectSpec {
            key: slug.to_string(),
            search_query: record.title.clone(),
            project_type: record.project_type,
            minecraft_version: default_minecraft.to_string(),
            loader: default_loader,
            project_id: record.project_id.clone(),
            project_platform: record.platform,
            version_pin: record.version.clone(),
        }
    }

    /// Generate default empack.yml content based on available metadata
    pub fn generate_default_empack_yml(&self) -> Result<String, ConfigError> {
        let pack_metadata = self.load_pack_metadata()?;

        let (minecraft_version, loader) = if let Some(metadata) = &pack_metadata {
            let loader = self
                .infer_loader_from_metadata(metadata)
                .unwrap_or(ModLoader::Fabric);
            (Some(metadata.versions.minecraft.clone()), Some(loader))
        } else {
            (None, None)
        };

        let mut deps = BTreeMap::new();
        if loader == Some(ModLoader::Fabric) || loader == Some(ModLoader::Quilt) {
            deps.insert(
                "sodium".to_string(),
                DependencyEntry::Resolved(DependencyRecord {
                    status: DependencyStatus::Resolved,
                    title: "Sodium".to_string(),
                    platform: ProjectPlatform::Modrinth,
                    project_id: "AANobbMI".to_string(),
                    project_type: ProjectType::Mod,
                    version: None,
                }),
            );
            deps.insert(
                "lithium".to_string(),
                DependencyEntry::Resolved(DependencyRecord {
                    status: DependencyStatus::Resolved,
                    title: "Lithium".to_string(),
                    platform: ProjectPlatform::Modrinth,
                    project_id: "gvQqBUqZ".to_string(),
                    project_type: ProjectType::Mod,
                    version: None,
                }),
            );
            if loader == Some(ModLoader::Fabric) {
                deps.insert(
                    "fabric-api".to_string(),
                    DependencyEntry::Resolved(DependencyRecord {
                        status: DependencyStatus::Resolved,
                        title: "Fabric API".to_string(),
                        platform: ProjectPlatform::Modrinth,
                        project_id: "P7dR8mSH".to_string(),
                        project_type: ProjectType::Mod,
                        version: None,
                    }),
                );
            }
        }

        let loader_version = pack_metadata
            .as_ref()
            .and_then(|p| loader.and_then(|l| self.get_loader_version_from_metadata(p, &l).ok()));

        let config = EmpackConfig {
            empack: EmpackProjectConfig {
                dependencies: deps,
                minecraft_version,
                loader,
                loader_version,
                datapack_folder: None,
                acceptable_game_versions: None,
                name: pack_metadata.as_ref().map(|m| m.name.clone()),
                author: pack_metadata.as_ref().and_then(|m| m.author.clone()),
                version: pack_metadata.as_ref().and_then(|m| m.version.clone()),
            },
        };

        serde_saphyr::to_string(&config).map_err(|e| ConfigError::YamlSerError { source: e })
    }

    /// Validate empack.yml consistency (pack.toml is optional)
    pub fn validate_consistency(&self) -> Result<Vec<String>, ConfigError> {
        let mut issues = Vec::new();

        let empack_config = self.load_empack_config()?;

        // Only validate consistency if pack.toml exists
        if let Ok(Some(pack_metadata)) = self.load_pack_metadata() {
            // Check Minecraft version consistency
            if let Some(empack_mc) = &empack_config.empack.minecraft_version
                && empack_mc != &pack_metadata.versions.minecraft
            {
                issues.push(format!(
                    "Minecraft version mismatch: empack.yml has '{}', pack.toml has '{}'",
                    empack_mc, pack_metadata.versions.minecraft
                ));
            }

            // Check loader consistency
            if let Some(empack_loader) = &empack_config.empack.loader
                && let Ok(pack_loader) = self.infer_loader_from_metadata(&pack_metadata)
                && empack_loader != &pack_loader
            {
                issues.push(format!(
                    "Loader mismatch: empack.yml has '{:?}', pack.toml infers '{:?}'",
                    empack_loader, pack_loader
                ));
            }
        }

        Ok(issues)
    }

    /// Add a dependency to empack.yml
    ///
    /// Inserts a DependencyRecord keyed by slug. If the slug already exists,
    /// it is overwritten (upsert).
    pub fn add_dependency(&self, slug: &str, record: DependencyRecord) -> Result<(), ConfigError> {
        let empack_path = self.workdir.join("empack.yml");

        // Load existing config or create new one
        let mut config = match self.load_empack_config() {
            Ok(cfg) => cfg,
            Err(ConfigError::MissingField { .. }) => EmpackConfig {
                empack: EmpackProjectConfig::default(),
            },
            Err(e) => return Err(e),
        };

        config
            .empack
            .dependencies
            .insert(slug.to_string(), DependencyEntry::Resolved(record));

        // Serialize and write back
        let yaml_content = serde_saphyr::to_string(&config)
            .map_err(|e| ConfigError::YamlSerError { source: e })?;

        self.fs_provider
            .write_file(&empack_path, &yaml_content)
            .map_err(|e| ConfigError::IoError {
                source: std::io::Error::other(e),
            })?;

        Ok(())
    }

    /// Remove a dependency from empack.yml by slug or display title
    ///
    /// Tries direct slug lookup first (O(1)). If that misses, falls back to a
    /// case-insensitive scan over `Dependency::title()`. Returns an error when
    /// multiple titles match (ambiguous).
    pub fn remove_dependency(&self, slug_or_title: &str) -> Result<(), ConfigError> {
        let empack_path = self.workdir.join("empack.yml");

        let mut config = self.load_empack_config()?;

        // Try direct slug lookup first
        if config.empack.dependencies.remove(slug_or_title).is_none() {
            // Fallback: case-insensitive title scan
            let matches: Vec<String> = config
                .empack
                .dependencies
                .iter()
                .filter(|(_, entry)| entry.title().eq_ignore_ascii_case(slug_or_title))
                .map(|(key, _)| key.clone())
                .collect();

            match matches.len() {
                0 => return Ok(()), // Nothing to remove, same as before
                1 => {
                    config.empack.dependencies.remove(&matches[0]);
                }
                _ => {
                    return Err(ConfigError::AmbiguousRemoval {
                        query: slug_or_title.to_string(),
                        matches,
                    });
                }
            }
        }

        // Serialize and write back
        let yaml_content = serde_saphyr::to_string(&config)
            .map_err(|e| ConfigError::YamlSerError { source: e })?;

        self.fs_provider
            .write_file(&empack_path, &yaml_content)
            .map_err(|e| ConfigError::IoError {
                source: std::io::Error::other(e),
            })?;

        Ok(())
    }

    /// Read the `datapack_folder` value from empack.yml.
    pub fn datapack_folder(&self) -> Option<String> {
        self.load_empack_config()
            .ok()
            .and_then(|c| c.empack.datapack_folder)
    }

    /// Read the `acceptable_game_versions` value from empack.yml.
    pub fn acceptable_game_versions(&self) -> Option<Vec<String>> {
        self.load_empack_config()
            .ok()
            .and_then(|c| c.empack.acceptable_game_versions)
    }

    /// Set `datapack_folder` in empack.yml.
    ///
    /// Loads the current config, sets the field, and writes back. Creates
    /// a minimal config if empack.yml does not exist.
    pub fn set_datapack_folder(&self, folder: &str) -> Result<(), ConfigError> {
        let empack_path = self.workdir.join("empack.yml");

        let mut config = match self.load_empack_config() {
            Ok(cfg) => cfg,
            Err(ConfigError::MissingField { .. }) => EmpackConfig {
                empack: EmpackProjectConfig::default(),
            },
            Err(e) => return Err(e),
        };

        config.empack.datapack_folder = Some(folder.to_string());

        let yaml_content = serde_saphyr::to_string(&config)
            .map_err(|e| ConfigError::YamlSerError { source: e })?;

        self.fs_provider
            .write_file(&empack_path, &yaml_content)
            .map_err(|e| ConfigError::IoError {
                source: std::io::Error::other(e),
            })?;

        Ok(())
    }

    /// Set `acceptable_game_versions` in empack.yml.
    ///
    /// Loads the current config, sets the field, and writes back. Creates
    /// a minimal config if empack.yml does not exist.
    pub fn set_acceptable_game_versions(&self, versions: &[String]) -> Result<(), ConfigError> {
        let empack_path = self.workdir.join("empack.yml");

        let mut config = match self.load_empack_config() {
            Ok(cfg) => cfg,
            Err(ConfigError::MissingField { .. }) => EmpackConfig {
                empack: EmpackProjectConfig::default(),
            },
            Err(e) => return Err(e),
        };

        config.empack.acceptable_game_versions = Some(versions.to_vec());

        let yaml_content = serde_saphyr::to_string(&config)
            .map_err(|e| ConfigError::YamlSerError { source: e })?;

        self.fs_provider
            .write_file(&empack_path, &yaml_content)
            .map_err(|e| ConfigError::IoError {
                source: std::io::Error::other(e),
            })?;

        Ok(())
    }
}

/// Serialize a fresh empack.yml string for a new project.
///
/// Uses serde serialization to produce injection-safe YAML. Optional fields
/// are omitted when `None`. The `dependencies` map is always empty (populated
/// later by add/import).
#[allow(clippy::too_many_arguments)]
pub(crate) fn format_empack_yml(
    name: &str,
    author: &str,
    version: &str,
    minecraft_version: &str,
    loader: &str,
    loader_version: &str,
    datapack_folder: Option<&str>,
    acceptable_game_versions: Option<&[String]>,
) -> String {
    let loader_enum = ModLoader::parse(loader).ok();

    #[derive(serde::Serialize)]
    struct Root<'a> {
        empack: Fields<'a>,
    }

    #[derive(serde::Serialize)]
    struct Fields<'a> {
        name: &'a str,
        author: &'a str,
        version: &'a str,
        minecraft_version: &'a str,
        #[serde(skip_serializing_if = "Option::is_none")]
        loader: Option<ModLoader>,
        #[serde(skip_serializing_if = "str::is_empty")]
        loader_version: &'a str,
        #[serde(skip_serializing_if = "Option::is_none")]
        datapack_folder: Option<&'a str>,
        #[serde(skip_serializing_if = "Option::is_none")]
        acceptable_game_versions: Option<&'a [String]>,
        dependencies: BTreeMap<String, DependencyEntry>,
    }

    let config = Root {
        empack: Fields {
            name,
            author,
            version,
            minecraft_version,
            loader: loader_enum,
            loader_version,
            datapack_folder,
            acceptable_game_versions,
            dependencies: BTreeMap::new(),
        },
    };

    serde_saphyr::to_string(&config).expect("serializing empack.yml should never fail")
}

#[cfg(test)]
mod tests {
    include!("config.test.rs");
}
