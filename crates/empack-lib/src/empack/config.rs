//! Configuration management for empack projects
//! Unified empack.yml (user intent) and pack.toml (packwiz reality) handling

use crate::empack::parsing::ModLoader;
use crate::primitives::ProjectType;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
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
        #[from]
        source: serde_yaml::Error,
    },

    #[error("TOML parsing error: {source}")]
    TomlError {
        #[from]
        source: toml::de::Error,
    },

    #[error("Anyhow error: {source}")]
    AnyhowError {
        #[from]
        source: anyhow::Error,
    },

    #[error("Missing required field: {field}")]
    MissingField { field: String },

    #[error("Invalid project specification: {spec}")]
    InvalidProjectSpec { spec: String },

    #[error("Configuration validation error: {reason}")]
    ValidationError { reason: String },
}

/// Top-level empack.yml configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmpackConfig {
    pub empack: EmpackProjectConfig,
}

/// Project configuration within empack.yml
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmpackProjectConfig {
    /// User-defined dependencies with search specifications
    pub dependencies: Vec<String>,

    /// Optional project ID mappings for performance
    #[serde(default)]
    pub project_ids: HashMap<String, String>,

    /// Optional version overrides
    #[serde(default)]
    pub version_overrides: HashMap<String, VersionOverride>,

    /// Target Minecraft version (if not specified, extracted from pack.toml)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minecraft_version: Option<String>,

    /// Target mod loader (if not specified, extracted from pack.toml)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loader: Option<ModLoader>,

    /// Optional modpack metadata (if not specified, extracted from pack.toml)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// Version override can be single version or list of compatible versions
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum VersionOverride {
    Single(String),
    Multiple(Vec<String>),
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
    pub loader: ModLoader,
    pub loader_version: String,

    /// Resolved project specifications from empack.yml
    pub dependencies: Vec<ProjectSpec>,
}

/// Project specification parsed from dependency string
#[derive(Debug, Clone)]
pub struct ProjectSpec {
    /// Internal reference key
    pub key: String,

    /// Search query for Modrinth
    pub search_query: String,

    /// Project type filter
    pub project_type: ProjectType,

    /// Target Minecraft version (defaults to plan version)
    pub minecraft_version: String,

    /// Target mod loader (defaults to plan loader)
    pub loader: ModLoader,

    /// Optional project ID for direct lookup
    pub project_id: Option<String>,

    /// Optional version override
    pub version_override: Option<VersionOverride>,
}

/// Configuration manager bridging empack.yml and pack.toml
pub struct ConfigManager<'a> {
    workdir: PathBuf,
    fs_provider: &'a dyn crate::application::session::FileSystemProvider,
}

impl<'a> ConfigManager<'a> {
    pub fn new(workdir: PathBuf, fs_provider: &'a dyn crate::application::session::FileSystemProvider) -> Self {
        Self { workdir, fs_provider }
    }

    /// Load empack.yml configuration
    pub fn load_empack_config(&self) -> Result<EmpackConfig, ConfigError> {
        let empack_path = self.workdir.join("empack.yml");

        if !self.fs_provider.exists(&empack_path) {
            return Err(ConfigError::MissingField {
                field: "empack.yml".to_string(),
            });
        }

        let content = self.fs_provider.read_to_string(&empack_path)?;
        let config: EmpackConfig = serde_yaml::from_str(&content)?;

        Ok(config)
    }

    /// Extract pack.toml metadata for fallback values
    pub fn load_pack_metadata(&self) -> Result<Option<PackMetadata>, ConfigError> {
        let pack_path = self.workdir.join("pack").join("pack.toml");

        if !self.fs_provider.exists(&pack_path) {
            return Ok(None);
        }

        let content = self.fs_provider.read_to_string(&pack_path)?;
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

        let loader = if let Some(empack_loader) = empack_config.empack.loader.clone() {
            empack_loader
        } else if let Some(pack_meta) = &pack_metadata {
            self.infer_loader_from_metadata(pack_meta)?
        } else {
            return Err(ConfigError::MissingField {
                field: "loader (from empack.yml or pack.toml)".to_string(),
            });
        };

        let loader_version = if let Some(pack_meta) = &pack_metadata {
            self.get_loader_version_from_metadata(pack_meta, &loader)?
        } else {
            "latest".to_string() // Fallback when no pack.toml
        };

        // Parse dependency specifications
        let mut dependencies = Vec::new();
        for dep_string in &empack_config.empack.dependencies {
            let spec = self.parse_dependency_spec(
                dep_string,
                &minecraft_version,
                &loader,
                &empack_config.empack,
            )?;
            dependencies.push(spec);
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
    fn infer_loader_from_metadata(
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

    /// Parse dependency specification string
    /// Format: "key: search_query|project_type|minecraft_version|loader"
    fn parse_dependency_spec(
        &self,
        dep_string: &str,
        default_minecraft: &str,
        default_loader: &ModLoader,
        empack_config: &EmpackProjectConfig,
    ) -> Result<ProjectSpec, ConfigError> {
        // Handle YAML array format: "- key: value"
        let clean_string = dep_string.trim_start_matches('-').trim();

        // Split on colon to get key and value
        let parts: Vec<&str> = clean_string.splitn(2, ':').collect();
        if parts.len() != 2 {
            return Err(ConfigError::InvalidProjectSpec {
                spec: dep_string.to_string(),
            });
        }

        let key = parts[0].trim().to_string();
        let value = parts[1].trim().trim_matches('"');

        // Parse value components separated by pipes
        let components: Vec<&str> = value.split('|').collect();
        if components.is_empty() {
            return Err(ConfigError::InvalidProjectSpec {
                spec: dep_string.to_string(),
            });
        }

        let search_query = components[0].trim().to_string();

        // Parse project type (default to mod)
        let project_type = if components.len() > 1 {
            match components[1].trim().to_lowercase().as_str() {
                "mod" => ProjectType::Mod,
                "datapack" => ProjectType::Datapack,
                "resourcepack" | "resource_pack" => ProjectType::ResourcePack,
                "shader" => ProjectType::Shader,
                _ => ProjectType::Mod, // Default fallback
            }
        } else {
            ProjectType::Mod
        };

        // Parse Minecraft version (default to plan version)
        let minecraft_version = if components.len() > 2 && !components[2].trim().is_empty() {
            components[2].trim().to_string()
        } else {
            default_minecraft.to_string()
        };

        // Parse loader (default to plan loader)
        let loader = if components.len() > 3 && !components[3].trim().is_empty() {
            match components[3].trim().to_lowercase().as_str() {
                "fabric" => ModLoader::Fabric,
                "forge" => ModLoader::Forge,
                "quilt" => ModLoader::Quilt,
                "neoforge" => ModLoader::NeoForge,
                _ => *default_loader,
            }
        } else {
            *default_loader
        };

        // Look up project ID mapping
        let project_id = empack_config.project_ids.get(&key).cloned();

        // Look up version override
        let version_override = empack_config.version_overrides.get(&key).cloned();

        Ok(ProjectSpec {
            key,
            search_query,
            project_type,
            minecraft_version,
            loader,
            project_id,
            version_override,
        })
    }

    /// Generate default empack.yml content based on available metadata
    pub fn generate_default_empack_yml(&self) -> Result<String, ConfigError> {
        let pack_metadata = self.load_pack_metadata()?;

        let (minecraft_version, loader) = if let Some(metadata) = &pack_metadata {
            let loader = self
                .infer_loader_from_metadata(metadata)
                .unwrap_or(ModLoader::Fabric); // Default to Fabric if unclear
            (Some(metadata.versions.minecraft.clone()), Some(loader))
        } else {
            (None, None) // Let user specify
        };

        let config = EmpackConfig {
            empack: EmpackProjectConfig {
                dependencies: vec![
                    "fabric_api: \"Fabric API|mod\"".to_string(),
                    "sodium: \"Sodium|mod\"".to_string(),
                    "lithium: \"Lithium|mod\"".to_string(),
                    "appleskin: \"AppleSkin|mod\"".to_string(),
                    "jade: \"Jade|mod\"".to_string(),
                ],
                project_ids: HashMap::new(),
                version_overrides: HashMap::new(),
                minecraft_version,
                loader,
                name: pack_metadata.as_ref().map(|m| m.name.clone()),
                author: pack_metadata.as_ref().and_then(|m| m.author.clone()),
                version: pack_metadata.as_ref().and_then(|m| m.version.clone()),
            },
        };

        Ok(serde_yaml::to_string(&config)?)
    }

    /// Validate empack.yml consistency (pack.toml is optional)
    pub fn validate_consistency(&self) -> Result<Vec<String>, ConfigError> {
        let mut issues = Vec::new();

        let empack_config = self.load_empack_config()?;

        // Only validate consistency if pack.toml exists
        if let Ok(Some(pack_metadata)) = self.load_pack_metadata() {
            // Check Minecraft version consistency
            if let Some(empack_mc) = &empack_config.empack.minecraft_version {
                if empack_mc != &pack_metadata.versions.minecraft {
                    issues.push(format!(
                        "Minecraft version mismatch: empack.yml has '{}', pack.toml has '{}'",
                        empack_mc, pack_metadata.versions.minecraft
                    ));
                }
            }

            // Check loader consistency
            if let Some(empack_loader) = &empack_config.empack.loader {
                if let Ok(pack_loader) = self.infer_loader_from_metadata(&pack_metadata) {
                    if empack_loader != &pack_loader {
                        issues.push(format!(
                            "Loader mismatch: empack.yml has '{:?}', pack.toml infers '{:?}'",
                            empack_loader, pack_loader
                        ));
                    }
                }
            }
        }

        Ok(issues)
    }
}

#[cfg(test)]
mod tests {
    include!("config.test.rs");
}


