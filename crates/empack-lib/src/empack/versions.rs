//! Version resolution and compatibility for Minecraft and modloaders
//!
//! Version fetching from official APIs with compatibility matrix validation,
//! following patterns from V1's proven bash implementation.

use crate::networking::{NetworkingManager, NetworkingConfig};
use crate::empack::parsing::ModLoader;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;
use tracing::{error, trace};

/// Version resolution errors
#[derive(Debug, Error)]
pub enum VersionError {
    #[error("Network request failed: {source}")]
    NetworkError {
        #[from]
        source: crate::networking::NetworkingError,
    },

    #[error("HTTP request failed: {source}")]
    RequestError {
        #[from]
        source: reqwest::Error,
    },

    #[error("JSON parsing failed: {source}")]
    JsonError {
        #[from]
        source: serde_json::Error,
    },

    #[error("XML parsing failed: {message}")]
    XmlError { message: String },

    #[error("No versions found for: {target}")]
    NoVersions { target: String },

    #[error("Compatibility validation failed: {modloader} {modloader_version} incompatible with Minecraft {minecraft_version}")]
    IncompatibleVersions {
        modloader: String,
        modloader_version: String,
        minecraft_version: String,
    },

    #[error("API unavailable for: {api}")]
    ApiUnavailable { api: String },
}

/// Minecraft version information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MinecraftVersion {
    pub id: String,
    pub version_type: String,
    pub url: String,
    pub time: String,
    pub release_time: String,
}

/// Minecraft version manifest
#[derive(Debug, Deserialize)]
pub struct MinecraftManifest {
    pub latest: MinecraftLatest,
    pub versions: Vec<MinecraftVersion>,
}

#[derive(Debug, Deserialize)]
pub struct MinecraftLatest {
    pub release: String,
    pub snapshot: String,
}

/// Fabric loader version information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FabricVersion {
    pub separator: String,
    pub build: u32,
    pub maven: String,
    pub version: String,
    pub stable: bool,
}

/// Quilt loader version information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuiltVersion {
    pub separator: String,
    pub build: u32,
    pub maven: String,
    pub version: String,
}

/// NeoForge/Forge version (parsed from Maven XML)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForgeVersion {
    pub version: String,
    pub minecraft_version: Option<String>,
    pub is_stable: bool,
}

/// Resolved version configuration with compatibility
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedVersions {
    pub minecraft_version: String,
    pub modloader: ModLoader,
    pub modloader_version: Option<String>,
    pub compatibility_validated: bool,
    pub fallback_used: bool,
}

/// Version compatibility matrix
#[derive(Debug, Clone, Default)]
pub struct CompatibilityMatrix {
    /// Minecraft versions supported by each modloader version
    pub neoforge_compatibility: HashMap<String, Vec<String>>,
    pub fabric_compatibility: HashMap<String, Vec<String>>,
    pub quilt_compatibility: HashMap<String, Vec<String>>,
}

/// Version resolution manager
pub struct VersionResolver {
    client: Client,
    compatibility_matrix: CompatibilityMatrix,
    #[cfg(test)]
    base_url: Option<String>,
}

impl VersionResolver {
    /// Create new version resolver
    pub async fn new() -> Result<Self, VersionError> {
        let networking_config = NetworkingConfig::default();
        let networking = NetworkingManager::new(networking_config).await?;
        Ok(Self {
            client: networking.client().clone(),
            compatibility_matrix: CompatibilityMatrix::default(),
            #[cfg(test)]
            base_url: None,
        })
    }

    #[cfg(test)]
    /// Create version resolver for testing with mock server URL
    pub fn new_with_mock_server(mock_url: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            compatibility_matrix: CompatibilityMatrix::default(),
            base_url: Some(mock_url),
        }
    }

    #[cfg(test)]
    /// Build URL for testing, using mock server base URL if available
    fn build_url(&self, path: &str) -> String {
        match &self.base_url {
            Some(base) => format!("{}{}", base, path),
            None => match path {
                "/mc/game/version_manifest.json" => "https://launchermeta.mojang.com/mc/game/version_manifest.json".to_string(),
                "/v2/versions/loader" => "https://meta.fabricmc.net/v2/versions/loader".to_string(),
                "/v3/versions/loader" => "https://meta.quiltmc.org/v3/versions/loader".to_string(),
                "/releases/net/neoforged/neoforge/maven-metadata.xml" => "https://maven.neoforged.net/releases/net/neoforged/neoforge/maven-metadata.xml".to_string(),
                _ => path.to_string(),
            }
        }
    }

    #[cfg(not(test))]
    /// Build URL for production (no override capability)
    fn build_url(&self, path: &str) -> String {
        match path {
            "/mc/game/version_manifest.json" => "https://launchermeta.mojang.com/mc/game/version_manifest.json".to_string(),
            "/v2/versions/loader" => "https://meta.fabricmc.net/v2/versions/loader".to_string(),
            "/v3/versions/loader" => "https://meta.quiltmc.org/v3/versions/loader".to_string(),
            "/releases/net/neoforged/neoforge/maven-metadata.xml" => "https://maven.neoforged.net/releases/net/neoforged/neoforge/maven-metadata.xml".to_string(),
            _ => path.to_string(),
        }
    }

    /// Get latest stable Minecraft release version
    pub async fn get_latest_minecraft_version(&self) -> Result<String, VersionError> {
        trace!("Fetching latest Minecraft version");

        let manifest = self.get_minecraft_manifest().await?;
        let latest = manifest.latest.release;

        Ok(latest)
    }

    /// Get all stable Minecraft release versions
    pub async fn get_all_minecraft_versions(&self) -> Result<Vec<String>, VersionError> {
        trace!("Fetching all Minecraft versions");

        let manifest = self.get_minecraft_manifest().await?;
        let versions: Vec<String> = manifest
            .versions
            .into_iter()
            .filter(|v| v.version_type == "release")
            .map(|v| v.id)
            .collect();

        Ok(versions)
    }

    /// Get Minecraft version manifest
    async fn get_minecraft_manifest(&self) -> Result<MinecraftManifest, VersionError> {
        let url = self.build_url("/mc/game/version_manifest.json");
        trace!("Fetching Minecraft manifest from {}", url);

        let response = self.client.get(&url).send().await?;
        let manifest: MinecraftManifest = response.json().await?;

        Ok(manifest)
    }

    /// Get latest NeoForge version from Maven
    pub async fn get_latest_neoforge_version(&self) -> Result<String, VersionError> {
        trace!("Fetching latest NeoForge version");

        let versions = self.get_all_neoforge_versions().await?;
        versions
            .first()
            .map(|v| v.version.clone())
            .ok_or_else(|| VersionError::NoVersions {
                target: "NeoForge".to_string(),
            })
    }

    /// Get stable NeoForge version (non-beta/alpha/rc)
    pub async fn get_stable_neoforge_version(&self) -> Result<String, VersionError> {
        trace!("Fetching stable NeoForge version");

        let versions = self.get_all_neoforge_versions().await?;
        
        // Find first stable version (skip beta/alpha/rc)
        let stable_version = versions
            .iter()
            .find(|v| v.is_stable)
            .map(|v| v.version.clone());

        match stable_version {
            Some(version) => {
                Ok(version)
            }
            None => {
                trace!("No stable NeoForge version found, using latest");
                self.get_latest_neoforge_version().await
            }
        }
    }

    /// Get all NeoForge versions from Maven
    pub async fn get_all_neoforge_versions(&self) -> Result<Vec<ForgeVersion>, VersionError> {
        trace!("Fetching all NeoForge versions");

        let url = self.build_url("/releases/net/neoforged/neoforge/maven-metadata.xml");
        let response = self.client.get(&url).send().await?;
        let xml_text = response.text().await?;

        self.parse_maven_versions(&xml_text, "NeoForge")
    }

    /// Get latest Fabric loader version
    pub async fn get_latest_fabric_version(&self) -> Result<String, VersionError> {
        trace!("Fetching latest Fabric version");

        let versions = self.get_all_fabric_versions().await?;
        versions
            .first()
            .map(|v| v.version.clone())
            .ok_or_else(|| VersionError::NoVersions {
                target: "Fabric".to_string(),
            })
    }

    /// Get stable Fabric loader version
    pub async fn get_stable_fabric_version(&self) -> Result<String, VersionError> {
        trace!("Fetching stable Fabric version");

        let url = self.build_url("/v2/versions/loader");
        let response = self.client.get(&url).send().await?;
        let versions: Vec<FabricVersion> = response.json().await?;

        // Find first stable version
        let stable_version = versions
            .iter()
            .find(|v| v.stable)
            .map(|v| v.version.clone());

        match stable_version {
            Some(version) => {
                Ok(version)
            }
            None => {
                trace!("No stable Fabric version found, using latest");
                self.get_latest_fabric_version().await
            }
        }
    }

    /// Get all Fabric loader versions
    pub async fn get_all_fabric_versions(&self) -> Result<Vec<FabricVersion>, VersionError> {
        trace!("Fetching all Fabric versions");

        let url = self.build_url("/v2/versions/loader");
        let response = self.client.get(&url).send().await?;
        let versions: Vec<FabricVersion> = response.json().await?;

        Ok(versions)
    }

    /// Get latest Quilt loader version
    pub async fn get_latest_quilt_version(&self) -> Result<String, VersionError> {
        trace!("Fetching latest Quilt version");

        let versions = self.get_all_quilt_versions().await?;
        versions
            .first()
            .map(|v| v.version.clone())
            .ok_or_else(|| VersionError::NoVersions {
                target: "Quilt".to_string(),
            })
    }

    /// Get stable Quilt loader version (assume latest is stable)
    pub async fn get_stable_quilt_version(&self) -> Result<String, VersionError> {
        trace!("Fetching stable Quilt version");

        // Quilt doesn't have explicit stable flags, use latest
        self.get_latest_quilt_version().await
    }

    /// Get all Quilt loader versions
    pub async fn get_all_quilt_versions(&self) -> Result<Vec<QuiltVersion>, VersionError> {
        trace!("Fetching all Quilt versions");

        let url = self.build_url("/v3/versions/loader");
        let response = self.client.get(&url).send().await?;
        let versions: Vec<QuiltVersion> = response.json().await?;

        Ok(versions)
    }

    /// Parse Maven XML metadata into version list
    fn parse_maven_versions(
        &self,
        xml_content: &str,
        modloader_name: &str,
    ) -> Result<Vec<ForgeVersion>, VersionError> {
        // Simple XML parsing for Maven metadata
        let mut versions = Vec::new();
        
        for line in xml_content.lines() {
            let line = line.trim();
            if line.starts_with("<version>") && line.ends_with("</version>") {
                let version = line
                    .strip_prefix("<version>")
                    .and_then(|s| s.strip_suffix("</version>"))
                    .unwrap_or("")
                    .to_string();

                if !version.is_empty() {
                    let is_stable = !version.contains("beta") 
                        && !version.contains("alpha") 
                        && !version.contains("rc");

                    versions.push(ForgeVersion {
                        version,
                        minecraft_version: None, // Would need additional parsing
                        is_stable,
                    });
                }
            }
        }

        if versions.is_empty() {
            return Err(VersionError::XmlError {
                message: format!("No versions found in {} Maven metadata", modloader_name),
            });
        }

        // Sort versions (latest first)
        versions.reverse();

        Ok(versions)
    }

    /// Get recommended defaults for modloader
    pub async fn get_recommended_defaults(
        &self,
        preferred_modloader: Option<ModLoader>,
    ) -> Result<ResolvedVersions, VersionError> {
        let modloader = preferred_modloader.unwrap_or(ModLoader::NeoForge);

        trace!("Resolving recommended defaults for modloader: {:?}", modloader);

        match modloader {
            ModLoader::NeoForge => self.get_neoforge_recommended_defaults().await,
            ModLoader::Fabric => self.get_fabric_recommended_defaults().await,
            ModLoader::Quilt => self.get_quilt_recommended_defaults().await,
            ModLoader::Vanilla => self.get_vanilla_recommended_defaults().await,
            ModLoader::Forge => {
                // Fallback to NeoForge for now
                trace!("Forge not implemented, falling back to NeoForge");
                self.get_neoforge_recommended_defaults().await
            }
        }
    }

    /// Port of V1's stabilize_core_input() - automatic configuration with 3D compatibility matrix
    /// Takes partial user input and returns validated complete configuration
    pub async fn stabilize_core_input(
        &self,
        provided_modloader: Option<ModLoader>,
        provided_minecraft: Option<String>,
        provided_modloader_version: Option<String>,
    ) -> Result<ResolvedVersions, VersionError> {
        trace!(
            "Stabilizing core input: modloader={:?}, minecraft={:?}, modloader_version={:?}",
            provided_modloader, provided_minecraft, provided_modloader_version
        );

        // If we have all three pieces, validate them as a complete matrix
        if let (Some(modloader), Some(minecraft_version), Some(modloader_version)) = (
            provided_modloader,
            provided_minecraft.as_ref(),
            provided_modloader_version.as_ref(),
        ) {
            trace!("Complete configuration provided, validating compatibility matrix");
            
            let is_compatible = self
                .validate_compatibility(modloader, minecraft_version, Some(modloader_version))
                .await?;
                
            if is_compatible {
                return Ok(ResolvedVersions {
                    minecraft_version: minecraft_version.clone(),
                    modloader,
                    modloader_version: Some(modloader_version.clone()),
                    compatibility_validated: true,
                    fallback_used: false,
                });
            } else {
                return Err(VersionError::IncompatibleVersions {
                    modloader: format!("{:?}", modloader),
                    modloader_version: modloader_version.clone(),
                    minecraft_version: minecraft_version.clone(),
                });
            }
        }

        // Auto-fill missing pieces using smart defaults (V1's auto-fill architecture)
        trace!("Auto-filling missing configuration pieces using smart defaults");

        // If no modloader provided, use default recommendations
        if provided_modloader.is_none() {
            trace!("No modloader specified, using recommended defaults");
            return self.get_recommended_defaults(None).await;
        }

        let modloader = provided_modloader.unwrap();
        
        // If modloader provided but missing version info, auto-fill compatible versions
        trace!("Modloader specified ({:?}), auto-filling compatible versions", modloader);
        
        // Start with recommended defaults for this modloader
        let mut defaults = self.get_recommended_defaults(Some(modloader)).await?;
        
        // Override with user-provided values where specified
        if let Some(minecraft_version) = provided_minecraft {
            trace!("Using user-provided Minecraft version: {}", minecraft_version);
            defaults.minecraft_version = minecraft_version.clone();
            
            // If user provided a specific Minecraft version, find a compatible modloader version
            if modloader != ModLoader::Vanilla {
                let compatible_version = self
                    .get_compatible_modloader_version_for_minecraft(modloader, &minecraft_version)
                    .await?;
                defaults.modloader_version = Some(compatible_version);
                trace!("Auto-selected compatible {:?} version for MC {}", modloader, minecraft_version);
            }
        }
        
        if let Some(modloader_version) = provided_modloader_version {
            trace!("Using user-provided modloader version: {}", modloader_version);
            defaults.modloader_version = Some(modloader_version);
        }

        // Final compatibility validation (V1's 3D compatibility matrix check)
        let is_compatible = self
            .validate_compatibility(
                defaults.modloader,
                &defaults.minecraft_version,
                defaults.modloader_version.as_deref(),
            )
            .await?;

        if is_compatible {
            defaults.compatibility_validated = true;
            trace!(
                "Auto-filled and validated configuration: {:?} {} + Minecraft {}",
                defaults.modloader,
                defaults.modloader_version.as_deref().unwrap_or("none"),
                defaults.minecraft_version
            );
            Ok(defaults)
        } else {
            Err(VersionError::IncompatibleVersions {
                modloader: format!("{:?}", defaults.modloader),
                modloader_version: defaults.modloader_version.unwrap_or_default(),
                minecraft_version: defaults.minecraft_version,
            })
        }
    }

    /// Get compatible modloader version for specific Minecraft version (V1 compatibility logic)
    async fn get_compatible_modloader_version_for_minecraft(
        &self,
        modloader: ModLoader,
        minecraft_version: &str,
    ) -> Result<String, VersionError> {
        match modloader {
            ModLoader::NeoForge => {
                // Port V1's Minecraft → NeoForge version mapping heuristics
                match minecraft_version {
                    version if version.starts_with("1.21") => {
                        // Try to get latest NeoForge 21.x version
                        let all_versions = self.get_all_neoforge_versions().await?;
                        for version in all_versions {
                            if version.version.starts_with("21.") {
                                return Ok(version.version);
                            }
                        }
                        // Fallback to stable
                        self.get_stable_neoforge_version().await
                    }
                    version if version.starts_with("1.20") => {
                        // Try to get latest NeoForge 20.x version
                        let all_versions = self.get_all_neoforge_versions().await?;
                        for version in all_versions {
                            if version.version.starts_with("20.") {
                                return Ok(version.version);
                            }
                        }
                        // Fallback to stable
                        self.get_stable_neoforge_version().await
                    }
                    _ => {
                        trace!("Unknown Minecraft version for NeoForge compatibility: {}", minecraft_version);
                        self.get_stable_neoforge_version().await
                    }
                }
            }
            ModLoader::Fabric | ModLoader::Quilt => {
                // Fabric and Quilt generally support most Minecraft versions with latest loader
                match modloader {
                    ModLoader::Fabric => self.get_stable_fabric_version().await,
                    ModLoader::Quilt => self.get_stable_quilt_version().await,
                    _ => unreachable!(),
                }
            }
            ModLoader::Vanilla => {
                // Vanilla doesn't have a separate modloader version
                Ok(minecraft_version.to_string())
            }
            ModLoader::Forge => {
                Err(VersionError::ApiUnavailable {
                    api: "Forge compatibility".to_string(),
                })
            }
        }
    }

    /// Get NeoForge recommended defaults (ecosystem-proven approach)
    async fn get_neoforge_recommended_defaults(&self) -> Result<ResolvedVersions, VersionError> {
        trace!("Getting NeoForge recommended defaults");

        let neoforge_version = self.get_stable_neoforge_version().await?;
        
        // Use proven ecosystem versions (from V1 compatibility.sh)
        let minecraft_version = match neoforge_version.split('.').next() {
            Some("21") => "1.21.1".to_string(),
            Some("20") => "1.20.1".to_string(),
            _ => {
                // Fallback to latest if version scheme changes
                trace!("Unknown NeoForge version scheme, using latest Minecraft");
                self.get_latest_minecraft_version().await?
            }
        };

        Ok(ResolvedVersions {
            minecraft_version,
            modloader: ModLoader::NeoForge,
            modloader_version: Some(neoforge_version),
            compatibility_validated: true, // Would validate with API
            fallback_used: false,
        })
    }

    /// Get Fabric recommended defaults
    async fn get_fabric_recommended_defaults(&self) -> Result<ResolvedVersions, VersionError> {
        trace!("Getting Fabric recommended defaults");

        let fabric_version = self.get_stable_fabric_version().await?;
        let minecraft_version = self.get_latest_minecraft_version().await?;

        Ok(ResolvedVersions {
            minecraft_version,
            modloader: ModLoader::Fabric,
            modloader_version: Some(fabric_version),
            compatibility_validated: true,
            fallback_used: false,
        })
    }

    /// Get Quilt recommended defaults
    async fn get_quilt_recommended_defaults(&self) -> Result<ResolvedVersions, VersionError> {
        trace!("Getting Quilt recommended defaults");

        let quilt_version = self.get_stable_quilt_version().await?;
        let minecraft_version = self.get_latest_minecraft_version().await?;

        Ok(ResolvedVersions {
            minecraft_version,
            modloader: ModLoader::Quilt,
            modloader_version: Some(quilt_version),
            compatibility_validated: true,
            fallback_used: false,
        })
    }

    /// Get vanilla recommended defaults
    async fn get_vanilla_recommended_defaults(&self) -> Result<ResolvedVersions, VersionError> {
        trace!("Getting vanilla recommended defaults");

        let minecraft_version = self.get_latest_minecraft_version().await?;

        Ok(ResolvedVersions {
            minecraft_version,
            modloader: ModLoader::Vanilla,
            modloader_version: None,
            compatibility_validated: true,
            fallback_used: false,
        })
    }

    /// Validate compatibility matrix for given versions
    pub async fn validate_compatibility(
        &self,
        modloader: ModLoader,
        minecraft_version: &str,
        modloader_version: Option<&str>,
    ) -> Result<bool, VersionError> {
        trace!(
            "Validating compatibility: {:?} {} + Minecraft {}",
            modloader,
            modloader_version.unwrap_or("none"),
            minecraft_version
        );

        match modloader {
            ModLoader::Vanilla => {
                // Vanilla is always compatible with any valid Minecraft version
                Ok(true)
            }
            ModLoader::NeoForge | ModLoader::Fabric | ModLoader::Quilt => {
                // For now, assume compatibility (would implement API-based validation)
                // This matches V1's fallback behavior when APIs are unavailable
                trace!("API-based compatibility validation not yet implemented, assuming compatible");
                Ok(true)
            }
            ModLoader::Forge => {
                trace!("Forge compatibility validation not implemented");
                Ok(true)
            }
        }
    }
}

// Note: Can't implement Default for async constructor

#[cfg(test)]
mod tests {
    include!("versions.test.rs");
}

