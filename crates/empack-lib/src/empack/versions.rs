//! Dynamic version fetching for Minecraft and mod loaders
//!
//! Fetches available versions from official APIs with disk caching.

use crate::Result;
use crate::application::session::{FileSystemProvider, NetworkProvider};
use anyhow::Context;
use semver::Version;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::warn;

/// Check if a Minecraft version is a stable release (not snapshot/pre-release)
fn is_stable_minecraft_version(mc_version: &str) -> bool {
    // Stable versions follow pattern like "1.20.1", "1.21.4", etc.
    // Snapshots are like "24w45a", pre-releases like "1.21-pre1"
    if mc_version.contains("pre") || mc_version.contains("rc") || mc_version.contains("snapshot") {
        return false;
    }

    // Check if it follows stable version pattern (X.Y or X.Y.Z with only numbers and dots)
    mc_version.chars().all(|c| c.is_ascii_digit() || c == '.')
        && mc_version
            .split('.')
            .all(|part| part.parse::<u32>().is_ok())
        && !mc_version.is_empty()
}

/// Parse a version string into a semver::Version, normalizing 2-component
/// strings like "1.20" to "1.20.0" since Minecraft uses both forms.
/// Legacy Forge 4+ component numeric versions are mapped into prerelease
/// identifiers so semver can still compare them numerically.
pub(crate) fn parse_version(s: &str) -> Option<Version> {
    let normalized = if s.matches('.').count() == 1 {
        format!("{s}.0")
    } else {
        s.to_string()
    };

    Version::parse(&normalized).ok().or_else(|| {
        let parts: Vec<&str> = s.split('.').collect();
        if parts.len() > 3
            && parts
                .iter()
                .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
        {
            Version::parse(&format!(
                "{}.{}.{}-{}",
                parts[0],
                parts[1],
                parts[2],
                parts[3..].join(".")
            ))
            .ok()
        } else {
            None
        }
    })
}

/// Sort version strings in descending order (newest first) using semver.
/// Unparseable versions sort to the end.
pub(crate) fn sort_versions_desc(versions: &mut [String]) {
    versions.sort_by(|a, b| match (parse_version(a), parse_version(b)) {
        (Some(va), Some(vb)) => vb.cmp(&va),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => a.cmp(b),
    });
}

const LEGACY_FORGE_1710_SUFFIX_START: &str = "10.13.2.1300";
const FABRIC_SUPPORT_FLOOR: &str = "1.14";
const QUILT_SUPPORT_FLOOR: &str = "1.14.4";

fn supports_loader_from_floor(mc_version: &str, floor: &str) -> bool {
    if !mc_version.starts_with("1.") {
        return true;
    }

    parse_version(mc_version).is_none_or(|version| {
        version >= parse_version(floor).expect("hardcoded support floor must parse")
    })
}

pub(crate) fn supports_fabric_loader(mc_version: &str) -> bool {
    supports_loader_from_floor(mc_version, FABRIC_SUPPORT_FLOOR)
}

pub(crate) fn supports_quilt_loader(mc_version: &str) -> bool {
    supports_loader_from_floor(mc_version, QUILT_SUPPORT_FLOOR)
}

/// NeoForge 1.20.1 was published under the legacy `net/neoforged/forge`
/// artifact family before NeoForged switched to the dedicated
/// `net/neoforged/neoforge` coordinates for 1.20.2+.
pub(crate) fn uses_forge_style_neoforge_coordinate(mc_version: &str) -> bool {
    mc_version == "1.20.1"
}

/// Canonicalize Forge loader versions for legacy 1.7.10 metadata.
///
/// Forge maven metadata switches from raw versions like `10.13.2.1291`
/// to suffixed versions like `10.13.2.1300-1.7.10`. Empack stores the raw
/// Forge loader version internally, so late legacy values are normalized
/// back to that form here.
pub(crate) fn canonicalize_forge_loader_version(mc_version: &str, loader_version: &str) -> String {
    if mc_version == "1.7.10" {
        loader_version
            .strip_suffix("-1.7.10")
            .unwrap_or(loader_version)
            .to_string()
    } else {
        loader_version.to_string()
    }
}

/// Returns true when a Forge 1.7.10 loader version requires the repeated-MC
/// legacy coordinate form introduced at 10.13.2.1300.
pub(crate) fn uses_legacy_forge_coordinate(mc_version: &str, loader_version: &str) -> bool {
    if mc_version != "1.7.10" {
        return false;
    }

    let Some(version) = parse_version(&canonicalize_forge_loader_version(
        mc_version,
        loader_version,
    )) else {
        return false;
    };

    let threshold =
        parse_version(LEGACY_FORGE_1710_SUFFIX_START).expect("legacy Forge boundary should parse");
    version >= threshold
}

/// Cached version data with timestamp
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedVersions {
    versions: Vec<String>,
    cached_at: u64,
}

impl CachedVersions {
    fn new(versions: Vec<String>) -> Self {
        let cached_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Self {
            versions,
            cached_at,
        }
    }

    fn is_expired(&self, max_age_hours: u64) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let max_age_seconds = max_age_hours * 3600;
        now - self.cached_at > max_age_seconds
    }
}

/// Response from Minecraft Launcher Meta API
#[derive(Debug, Deserialize)]
struct MinecraftVersionManifest {
    versions: Vec<MinecraftVersionInfo>,
}

#[derive(Debug, Deserialize)]
struct MinecraftVersionInfo {
    id: String,
    #[serde(rename = "type")]
    version_type: String,
}

/// Response from Fabric API
#[derive(Debug, Deserialize)]
struct FabricLoaderVersion {
    loader: FabricLoaderInfo,
}

#[derive(Debug, Deserialize)]
struct FabricLoaderInfo {
    version: String,
    stable: bool,
}

/// Response from NeoForge API - actual structure
#[derive(Debug, Deserialize)]
struct NeoForgeVersionResponse {
    #[serde(rename = "isSnapshot")]
    #[allow(dead_code)]
    is_snapshot: bool,
    versions: Vec<String>,
}

/// Response from Quilt API
/// Endpoint: GET https://meta.quiltmc.org/v3/versions/loader/{minecraft_version}
/// Returns array of loader combinations (use first element for latest loader)
#[derive(Debug, Deserialize)]
struct QuiltLoaderCombination {
    loader: QuiltLoaderVersion,
    // Other fields (hashed, intermediary, launcherMeta) exist but not needed for version resolution
}

#[derive(Debug, Deserialize)]
struct QuiltLoaderVersion {
    version: String,
    #[allow(dead_code)]
    maven: String,
    #[serde(default)]
    #[allow(dead_code)]
    separator: String,
    #[serde(default)]
    #[allow(dead_code)]
    build: u32,
}

/// Response from Forge maven-metadata.xml API
/// XML structure: <metadata><versioning><versions><version>1.21.11-61.0.3</version>...</versions></versioning></metadata>
#[derive(Debug, Deserialize)]
struct ForgeMavenMetadata {
    versioning: ForgeMavenVersioning,
}

#[derive(Debug, Deserialize)]
struct ForgeMavenVersioning {
    versions: ForgeMavenVersionsList,
}

#[derive(Debug, Deserialize)]
struct ForgeMavenVersionsList {
    version: Vec<String>,
}

/// Filter NeoForge versions by Minecraft version compatibility
///
/// NeoForge has three compatibility eras:
///
/// - MC `1.20.1` uses the old Forge-style coordinate family and is handled
///   by `fetch_neoforge_loader_versions()` before this filter is reached.
/// - MC `1.X.Y` versions at `1.20.2+` map to NeoForge `X.Y.` prefixes.
/// - Future non-`1.*` Minecraft versions use NeoForge's newer
///   `year.major.minor.x...(+prerelease)` family, which is matched by
///   `year.major.minor` prefix and prerelease suffix.
///
/// Primary-source basis:
/// - `packwiz-tx/core/versionutil.go` `fetchForNeoForge`
/// - `packwiz-tx/core/versionutil.go` `fetchOldNeoForgeStyle`
/// - `packwiz-tx/core/versionutil.go` `fetchNeoForgeStyle`
///
/// This function extracts compatible versions and filters out beta versions
/// unless no stable versions exist for that MC version.
fn filter_neoforge_versions_by_minecraft(
    all_versions: &[String],
    mc_version: &str,
) -> Result<Vec<String>> {
    let matching_versions: Vec<String> = if mc_version.starts_with("1.") {
        // NeoForge only supports MC 1.20.2+ in the 1.x era.
        if parse_version(mc_version)
            .is_none_or(|v| v < parse_version("1.20.2").expect("hardcoded version must parse"))
        {
            return Ok(vec![]);
        }

        // Extract X.Y from "1.X.Y" to create "X.Y." prefix
        // MC "1.20.2" → remove "1." → "20.2" → add "." → "20.2."
        // MC "1.21.10" → remove "1." → "21.10" → add "." → "21.10."
        // MC "1.21" → normalize to "1.21.0" → remove "1." → "21.0" → add "." → "21.0."
        let normalized_version = if mc_version.matches('.').count() == 1 {
            format!("{}.0", mc_version)
        } else {
            mc_version.to_string()
        };

        let expected_prefix = if let Some(suffix) = normalized_version.strip_prefix("1.") {
            format!("{}.", suffix)
        } else {
            return Ok(vec![]);
        };

        all_versions
            .iter()
            .filter(|v| v.starts_with(&expected_prefix))
            .cloned()
            .collect()
    } else {
        // Future non-1.* Minecraft versions switch to NeoForge's newer
        // year.major.minor.x(+prerelease) scheme. Match the year.major.minor
        // prefix and, when present, the Minecraft prerelease suffix.
        let mc_split: Vec<_> = mc_version.splitn(3, '.').collect();
        if mc_split.len() < 2 {
            return Ok(vec![]);
        }

        let year = mc_split[0];
        let mut major = mc_split[1];
        let mut minor = "0";
        let mut prerelease = "";

        if let Some(third) = mc_split.get(2) {
            if let Some((value, suffix)) = third.split_once('-') {
                minor = value;
                prerelease = suffix;
            } else {
                minor = third;
            }
        } else if let Some((value, suffix)) = major.split_once('-') {
            major = value;
            prerelease = suffix;
        }

        let required_prefix = format!("{year}.{major}.{minor}");

        all_versions
            .iter()
            .filter(|version| {
                version.starts_with(&required_prefix) && version.ends_with(prerelease)
            })
            .cloned()
            .collect()
    };

    // Separate stable and beta versions
    let stable_versions: Vec<String> = matching_versions
        .iter()
        .filter(|v| !v.contains("-beta"))
        .cloned()
        .collect();

    // Prefer stable versions, but if none exist, include betas
    let result = if !stable_versions.is_empty() {
        stable_versions
    } else {
        matching_versions
    };

    let mut result = result;
    sort_versions_desc(&mut result);
    Ok(result)
}

fn sanitize_neoforge_loader_versions(versions: &[String], mc_version: &str) -> Result<Vec<String>> {
    if uses_forge_style_neoforge_coordinate(mc_version) {
        let mut filtered: Vec<String> = versions
            .iter()
            .filter(|v| v.starts_with("47.1."))
            .cloned()
            .collect();
        sort_versions_desc(&mut filtered);
        return Ok(filtered);
    }

    filter_neoforge_versions_by_minecraft(versions, mc_version)
}

/// Parse Forge maven-metadata.xml and extract version strings
///
/// XML structure example:
/// ```xml
/// <metadata>
///   <versioning>
///     <versions>
///       <version>1.21.11-61.0.3</version>
///       <version>1.21.11-61.0.2</version>
///       ...
///     </versions>
///   </versioning>
/// </metadata>
/// ```
fn parse_forge_maven_metadata(xml_content: &str) -> Result<Vec<String>> {
    let metadata: ForgeMavenMetadata =
        quick_xml::de::from_str(xml_content).context("Failed to parse Forge maven-metadata.xml")?;

    Ok(metadata.versioning.versions.version)
}

/// Filter Forge versions by Minecraft version from maven-metadata.xml
///
/// Forge maven-metadata.xml structure:
/// - Version format: "{mc_version}-{forge_version}"
/// - Examples: "1.21.11-61.0.3", "1.20.1-47.4.13", "1.7.10-10.13.4.1614"
///
/// Strategy:
/// - Extract all versions matching "{mc_version}-" prefix
/// - Return sorted newest first (semantic versioning on forge version component)
///
/// Artifact URL construction (legacy vs modern):
/// - Late MC 1.7.10 Forge (starting at 10.13.2.1300): `forge-{mc}-{forge}-{mc}-installer.jar`
/// - Earlier MC 1.7.10 Forge and newer MC versions: `forge-{mc}-{forge}-installer.jar`
///
/// Examples:
/// - MC "1.20.1" → All versions starting with "1.20.1-" (e.g., "47.4.13", "47.4.10", ...)
/// - MC "1.16.4" → All versions starting with "1.16.4-" (e.g., "35.1.37", "35.1.36", ..., "35.0.0")
/// - MC "1.7.10" → Raw versions until `10.13.2.1291`, then suffixed metadata entries
///   starting at `10.13.2.1300-1.7.10` which are normalized back to raw versions here
fn filter_forge_versions_by_minecraft(
    all_versions: &[String],
    mc_version: &str,
) -> Result<Vec<String>> {
    // Forge supports very old MC versions (back to 1.1), no minimum check needed

    // Normalize MC version (handle "1.21" → "1.21.0" for consistency)
    let normalized_version = if mc_version.matches('.').count() == 1 {
        format!("{}.0", mc_version)
    } else {
        mc_version.to_string()
    };

    // Extract forge version component from full version string
    // Input: "1.20.1-47.4.13" → Output: "47.4.13"
    let extract_forge_version = |full_version: &str, prefix: &str| -> Option<String> {
        full_version.strip_prefix(prefix).map(|v| v.to_string())
    };

    // Collect all matching versions (try both normalized and original)
    let mut matching_versions: Vec<String> = Vec::new();
    let normalized_prefix = format!("{}-", normalized_version);
    let original_prefix = format!("{}-", mc_version);

    for version in all_versions {
        // Try normalized prefix (e.g., "1.21.0-")
        if let Some(forge_ver) = extract_forge_version(version, &normalized_prefix) {
            let forge_ver = canonicalize_forge_loader_version(mc_version, &forge_ver);
            if !matching_versions.contains(&forge_ver) {
                matching_versions.push(forge_ver);
            }
        }

        // Also try original prefix if different (e.g., "1.21-")
        if mc_version != normalized_version
            && let Some(forge_ver) = extract_forge_version(version, &original_prefix)
        {
            let forge_ver = canonicalize_forge_loader_version(mc_version, &forge_ver);
            if !matching_versions.contains(&forge_ver) {
                matching_versions.push(forge_ver);
            }
        }
    }

    sort_versions_desc(&mut matching_versions);

    Ok(matching_versions)
}

/// Supported mod loaders in priority order
#[derive(Debug, Clone, PartialEq)]
pub enum ModLoader {
    NeoForge,
    Fabric,
    Forge,
    Quilt,
}

impl ModLoader {
    pub fn as_str(&self) -> &'static str {
        match self {
            ModLoader::NeoForge => "neoforge",
            ModLoader::Fabric => "fabric",
            ModLoader::Forge => "forge",
            ModLoader::Quilt => "quilt",
        }
    }
}

impl From<crate::empack::parsing::ModLoader> for ModLoader {
    fn from(loader: crate::empack::parsing::ModLoader) -> Self {
        use crate::empack::parsing::ModLoader as P;
        match loader {
            P::NeoForge => Self::NeoForge,
            P::Fabric => Self::Fabric,
            P::Quilt => Self::Quilt,
            P::Forge => Self::Forge,
        }
    }
}

/// Dynamic version fetcher with caching
pub struct VersionFetcher<'a> {
    network: &'a dyn NetworkProvider,
    filesystem: &'a dyn FileSystemProvider,
    cache_dir: PathBuf,
}

impl<'a> VersionFetcher<'a> {
    /// Create a new version fetcher using session providers
    pub fn new(
        network: &'a dyn NetworkProvider,
        filesystem: &'a dyn FileSystemProvider,
    ) -> Result<Self> {
        let cache_dir = crate::platform::cache::versions_cache_dir()
            .unwrap_or_else(|_| std::env::temp_dir().join("empack-cache").join("versions"));

        Ok(Self {
            network,
            filesystem,
            cache_dir,
        })
    }

    /// Find compatible mod loaders for a specific Minecraft version
    /// Uses real API calls to determine compatibility, following session-based DI pattern
    pub async fn fetch_compatible_loaders(&self, mc_version: &str) -> Result<Vec<ModLoader>> {
        let all_loaders = [
            ModLoader::NeoForge,
            ModLoader::Fabric,
            ModLoader::Forge,
            ModLoader::Quilt,
        ];

        let mut compatible_loaders = Vec::new();

        // Check each loader for compatibility by attempting to fetch versions
        // This is API-driven compatibility checking, not hardcoded assumptions
        for loader in &all_loaders {
            let has_versions = match loader {
                ModLoader::NeoForge => {
                    // NeoForge has strict compatibility rules - only check for known versions
                    match self.fetch_neoforge_loader_versions(mc_version).await {
                        Ok(versions) => !versions.is_empty(),
                        Err(_) => false, // API error or compatibility check failed
                    }
                }
                ModLoader::Fabric => {
                    // Fabric usually supports versions quickly, but don't assume on API failure
                    match self.fetch_fabric_loader_versions(mc_version).await {
                        Ok(versions) => !versions.is_empty(),
                        Err(_) => {
                            // For stable releases, assume Fabric works on API failure
                            // For snapshots/bleeding edge, be conservative
                            is_stable_minecraft_version(mc_version)
                                && supports_fabric_loader(mc_version)
                        }
                    }
                }
                ModLoader::Forge => {
                    // Forge has specific version mappings - don't assume compatibility
                    match self.fetch_forge_loader_versions(mc_version).await {
                        Ok(versions) => !versions.is_empty(),
                        Err(_) => {
                            // Only assume compatibility for well-known stable versions
                            is_stable_minecraft_version(mc_version)
                        }
                    }
                }
                ModLoader::Quilt => {
                    // Quilt is Fabric-compatible but may lag behind
                    match self.fetch_quilt_loader_versions(mc_version).await {
                        Ok(versions) => !versions.is_empty(),
                        Err(_) => {
                            // Conservative approach for Quilt
                            is_stable_minecraft_version(mc_version)
                                && supports_quilt_loader(mc_version)
                        }
                    }
                }
            };

            if has_versions {
                compatible_loaders.push(loader.clone());
            }
        }

        // Never add fallback loaders for unknown versions; empty result is
        // the correct answer for snapshots and bleeding-edge releases.

        Ok(compatible_loaders)
    }

    /// Fetch available Minecraft versions (stable releases only)
    pub async fn fetch_minecraft_versions(&self) -> Result<Vec<String>> {
        self.fetch_cached_or_network(
            "minecraft_versions.json",
            4, // 4 hour cache
            || async {
                let client = self.network.http_client()?;
                let url = "https://launchermeta.mojang.com/mc/game/version_manifest.json";

                let response = client
                    .get(url)
                    .send()
                    .await
                    .context("Failed to fetch Minecraft version manifest")?;

                if !response.status().is_success() {
                    return Err(anyhow::anyhow!(
                        "Failed to fetch Minecraft versions: HTTP {}",
                        response.status()
                    ));
                }

                let manifest: MinecraftVersionManifest = response
                    .json()
                    .await
                    .context("Failed to parse Minecraft version manifest")?;

                // Filter to stable releases only and sort newest first
                let mut versions: Vec<String> = manifest
                    .versions
                    .into_iter()
                    .filter(|v| v.version_type == "release")
                    .map(|v| v.id)
                    .collect();

                sort_versions_desc(&mut versions);

                Ok(versions)
            },
        )
        .await
    }

    /// Fetch available Fabric loader versions for a specific Minecraft version
    pub async fn fetch_fabric_loader_versions(&self, mc_version: &str) -> Result<Vec<String>> {
        if !supports_fabric_loader(mc_version) {
            return Ok(vec![]);
        }

        let cache_key = format!("fabric_loader_{}.json", mc_version);

        self.fetch_cached_or_network(
            &cache_key,
            6, // 6 hour cache
            || async {
                let client = self.network.http_client()?;
                let url = format!(
                    "https://meta.fabricmc.net/v2/versions/loader/{}",
                    mc_version
                );

                let response = client
                    .get(&url)
                    .send()
                    .await
                    .context("Failed to fetch Fabric loader versions")?;

                // Handle HTTP 400 - unsupported MC version
                // Return empty vec to hide Fabric from loader selection (matches NeoForge/Forge pattern)
                if response.status() == 400 {
                    return Ok(vec![]);
                }

                if !response.status().is_success() {
                    return Err(anyhow::anyhow!(
                        "Failed to fetch Fabric versions: HTTP {}",
                        response.status()
                    ));
                }

                let versions: Vec<FabricLoaderVersion> = response
                    .json()
                    .await
                    .context("Failed to parse Fabric loader versions")?;

                // Prefer stable versions, sort newest first
                let mut stable_versions: Vec<String> = versions
                    .iter()
                    .filter(|v| v.loader.stable)
                    .map(|v| v.loader.version.clone())
                    .collect();

                let mut beta_versions: Vec<String> = versions
                    .iter()
                    .filter(|v| !v.loader.stable)
                    .map(|v| v.loader.version.clone())
                    .collect();

                sort_versions_desc(&mut stable_versions);
                sort_versions_desc(&mut beta_versions);

                // Combine with stable first
                stable_versions.append(&mut beta_versions);
                Ok(stable_versions)
            },
        )
        .await
    }

    /// Fetch available NeoForge versions for a specific Minecraft version
    pub async fn fetch_neoforge_loader_versions(&self, mc_version: &str) -> Result<Vec<String>> {
        let cache_key = format!("neoforge_loader_{}.json", mc_version);
        let versions = self.fetch_cached_or_network(
            &cache_key,
            6, // 6 hour cache
            || async {
                let client = self.network.http_client()?;

                if uses_forge_style_neoforge_coordinate(mc_version) {
                    let legacy_url =
                        "https://maven.neoforged.net/api/maven/versions/releases/net/neoforged/forge";

                    let response = client.get(legacy_url).send().await;

                    match response {
                        Ok(resp) if resp.status().is_success() => {
                            if let Ok(version_data) = resp.json::<NeoForgeVersionResponse>().await {
                                let filtered_versions = filter_forge_versions_by_minecraft(
                                    &version_data.versions,
                                    mc_version,
                                )?;

                                if !filtered_versions.is_empty() {
                                    return Ok(filtered_versions);
                                }
                            }
                        }
                        _ => {
                            // API failed, use fallback
                        }
                    }

                    return Ok(Self::get_fallback_loader_versions("neoforge", mc_version));
                }

                // Try NeoForge API first for 1.20.2+ and year-style releases.
                let url =
                    "https://maven.neoforged.net/api/maven/versions/releases/net/neoforged/neoforge";

                let response = client.get(url).send().await;

                match response {
                    Ok(resp) if resp.status().is_success() => {
                        if let Ok(version_data) = resp.json::<NeoForgeVersionResponse>().await {
                            let filtered_versions = filter_neoforge_versions_by_minecraft(
                                &version_data.versions,
                                mc_version,
                            )?;

                            if !filtered_versions.is_empty() {
                                return Ok(filtered_versions);
                            }
                        }
                    }
                    _ => {
                        // API failed, use fallback
                    }
                }

                Ok(Self::get_fallback_loader_versions("neoforge", mc_version))
            }
        ).await?;

        let sanitized = sanitize_neoforge_loader_versions(&versions, mc_version)?;
        if sanitized != versions {
            warn!(
                "Discarding incompatible cached NeoForge loader versions for {}",
                mc_version
            );

            let repaired_versions = if sanitized.is_empty() {
                Self::get_fallback_loader_versions("neoforge", mc_version)
            } else {
                sanitized
            };
            let cache_path = self.cache_dir.join(&cache_key);
            if let Err(e) = self.save_to_cache(&cache_path, &repaired_versions) {
                warn!("Failed to repair NeoForge loader cache: {}", e);
            }
            return Ok(repaired_versions);
        }

        Ok(sanitized)
    }

    /// Fetch Forge versions with proper MC version compatibility
    ///
    /// Uses maven-metadata.xml for complete version enumeration (not promotions_slim.json).
    /// This provides ALL Forge versions for a given MC version, not just latest/recommended.
    ///
    /// Smart filtering strategy based on context:
    /// - Interactive mode (no --modloader flag): Return ALL versions (dialoguer handles pagination)
    /// - CLI mode with @latest/@recommended: Filter to those specific versions (future implementation)
    /// - CLI mode with @version_id: Search full list for specific version (future implementation)
    pub async fn fetch_forge_loader_versions(&self, mc_version: &str) -> Result<Vec<String>> {
        let cache_key = format!("forge_loader_{}.json", mc_version);

        self.fetch_cached_or_network(
            &cache_key,
            6, // 6 hour cache
            || async {
                let client = self.network.http_client()?;

                // Fetch maven-metadata.xml (contains ALL Forge versions)
                // NOTE: Must use maven.minecraftforge.net, NOT files.minecraftforge.net (404)
                let url =
                    "https://maven.minecraftforge.net/net/minecraftforge/forge/maven-metadata.xml";

                let response = client.get(url).send().await;

                match response {
                    Ok(resp) if resp.status().is_success() => {
                        if let Ok(xml_content) = resp.text().await {
                            // Parse XML to extract version list
                            let all_versions = parse_forge_maven_metadata(&xml_content)?;

                            // Filter versions by MC version compatibility
                            let filtered_versions =
                                filter_forge_versions_by_minecraft(&all_versions, mc_version)?;

                            // Return all matching versions (dialoguer handles pagination via .max_length(6))
                            if !filtered_versions.is_empty() {
                                return Ok(filtered_versions);
                            }
                            // Otherwise fall through to fallback
                        }
                    }
                    _ => {
                        // API failed, use fallback
                    }
                }

                // Fallback for network failures or unknown MC versions
                let fallback_versions = match mc_version {
                    "1.20.1" => vec!["47.4.13".to_string(), "47.4.10".to_string()],
                    "1.16.5" => vec!["36.2.42".to_string(), "36.2.34".to_string()],
                    "1.21.1" => vec!["52.1.8".to_string(), "52.1.0".to_string()],
                    _ => {
                        // For unknown versions, return empty to hide Forge option
                        vec![]
                    }
                };

                Ok(fallback_versions)
            },
        )
        .await
    }

    /// Fetch Quilt versions from official API
    /// Uses MC-version-specific endpoint: /v3/versions/loader/{mc_version}
    /// Returns HTTP 404 for unsupported MC versions (returns empty vec, not error)
    pub async fn fetch_quilt_loader_versions(&self, mc_version: &str) -> Result<Vec<String>> {
        if !supports_quilt_loader(mc_version) {
            return Ok(vec![]);
        }

        let cache_key = format!("quilt_loader_{}.json", mc_version);

        self.fetch_cached_or_network(
            &cache_key,
            6, // 6 hour cache (matches Fabric/NeoForge/Forge)
            || async {
                let client = self.network.http_client()?;
                // Use MC-version-specific endpoint (server-side filtering)
                let url = format!("https://meta.quiltmc.org/v3/versions/loader/{}", mc_version);

                match client.get(&url).send().await {
                    Ok(response) if response.status().is_success() => {
                        // Parse array response - return all loader versions for this MC version
                        if let Ok(combinations) =
                            response.json::<Vec<QuiltLoaderCombination>>().await
                            && !combinations.is_empty()
                        {
                            return Ok(combinations
                                .iter()
                                .map(|c| c.loader.version.clone())
                                .collect());
                        }
                        // Empty array or parse failure - return empty vec
                        Ok(vec![])
                    }
                    Ok(response) if response.status() == 404 => {
                        // HTTP 404 = MC version not supported by Quilt
                        // Return empty vec to silently exclude from loader selection
                        // (Matches Fabric 400 handling pattern from Phase 1)
                        Ok(vec![])
                    }
                    _ => {
                        // Network error or other HTTP error - return fallback
                        Ok(Self::get_fallback_loader_versions("quilt", mc_version))
                    }
                }
            },
        )
        .await
    }

    /// Generic cached fetch with network fallback
    async fn fetch_cached_or_network<F, Fut>(
        &self,
        cache_filename: &str,
        max_age_hours: u64,
        network_fetch: F,
    ) -> Result<Vec<String>>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<Vec<String>>>,
    {
        let cache_path = self.cache_dir.join(cache_filename);

        // Try to load from cache first
        if let Ok(cached_data) = self.load_from_cache(&cache_path)
            && !cached_data.is_expired(max_age_hours)
        {
            return Ok(cached_data.versions);
        }

        // Cache miss or expired - fetch from network
        match network_fetch().await {
            Ok(versions) => {
                // Save to cache for next time
                if let Err(e) = self.save_to_cache(&cache_path, &versions) {
                    warn!("Failed to save to cache: {}", e);
                }
                Ok(versions)
            }
            Err(network_error) => {
                // If network fails, try to use expired cache as fallback
                if let Ok(cached_data) = self.load_from_cache(&cache_path) {
                    warn!(
                        "Network fetch failed, using cached data (may be outdated): {}",
                        network_error
                    );
                    Ok(cached_data.versions)
                } else {
                    // Final fallback: use hardcoded defaults
                    warn!(
                        "Network and cache failed, using fallback defaults: {}",
                        network_error
                    );
                    Ok(Self::get_fallback_versions_for_cache_key(cache_filename))
                }
            }
        }
    }

    /// Load cached data from disk using session filesystem provider
    fn load_from_cache(&self, cache_path: &Path) -> Result<CachedVersions> {
        if let Ok(cached) = self.load_cached_versions(cache_path) {
            return Ok(cached);
        }

        let legacy_path = crate::platform::cache::legacy_versions_cache_file(
            cache_path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| anyhow::anyhow!("Invalid cache filename"))?,
        )?;

        if legacy_path != cache_path {
            let cached = self.load_cached_versions(&legacy_path)?;
            if let Err(error) = self.write_cached_versions(cache_path, &cached) {
                warn!(
                    error = %error,
                    legacy = %legacy_path.display(),
                    current = %cache_path.display(),
                    "failed to migrate legacy versions cache file"
                );
            }
            return Ok(cached);
        }

        self.load_cached_versions(cache_path)
    }

    /// Save data to cache using session filesystem provider
    fn save_to_cache(&self, cache_path: &Path, versions: &[String]) -> Result<()> {
        self.write_cached_versions(cache_path, &CachedVersions::new(versions.to_vec()))
    }

    fn load_cached_versions(&self, cache_path: &Path) -> Result<CachedVersions> {
        let content = self
            .filesystem
            .read_to_string(cache_path)
            .context("Failed to read cache file")?;

        serde_json::from_str(&content).context("Failed to parse cache file")
    }

    fn write_cached_versions(&self, cache_path: &Path, cached: &CachedVersions) -> Result<()> {
        if let Some(parent) = cache_path.parent() {
            self.filesystem
                .create_dir_all(parent)
                .context("Failed to create cache directory")?;
        }

        let content =
            serde_json::to_string_pretty(cached).context("Failed to serialize cache data")?;

        self.filesystem
            .write_file(cache_path, &content)
            .context("Failed to write cache file")?;

        Ok(())
    }

    /// Get fallback versions when both network and cache fail
    fn get_fallback_versions_for_cache_key(cache_filename: &str) -> Vec<String> {
        let loader_cache_mc_version = |prefix: &str| {
            cache_filename
                .strip_prefix(prefix)
                .and_then(|name| name.strip_suffix(".json"))
                .unwrap_or("")
        };

        if cache_filename == "minecraft_versions.json" {
            Self::get_fallback_minecraft_versions()
        } else if cache_filename.starts_with("fabric_loader_") {
            Self::get_fallback_loader_versions("fabric", loader_cache_mc_version("fabric_loader_"))
        } else if cache_filename.starts_with("neoforge_loader_") {
            Self::get_fallback_loader_versions(
                "neoforge",
                loader_cache_mc_version("neoforge_loader_"),
            )
        } else if cache_filename.starts_with("quilt_loader_") {
            Self::get_fallback_loader_versions("quilt", loader_cache_mc_version("quilt_loader_"))
        } else if cache_filename.starts_with("forge_loader_") {
            Self::get_fallback_loader_versions("forge", loader_cache_mc_version("forge_loader_"))
        } else {
            vec!["latest".to_string()]
        }
    }

    /// Get fallback versions when network is unavailable
    pub fn get_fallback_minecraft_versions() -> Vec<String> {
        vec![
            "1.21.4".to_string(),
            "1.21.1".to_string(),
            "1.20.1".to_string(),
            "1.19.2".to_string(),
            "1.18.2".to_string(),
        ]
    }

    /// Get fallback loader versions for a specific modloader and MC version
    pub fn get_fallback_loader_versions(modloader: &str, mc_version: &str) -> Vec<String> {
        match modloader {
            "fabric"
                if is_stable_minecraft_version(mc_version)
                    && supports_fabric_loader(mc_version) =>
            {
                vec![
                    "0.15.0".to_string(),
                    "0.14.21".to_string(),
                    "0.14.20".to_string(),
                ]
            }
            "neoforge" => Self::get_neoforge_fallback_loader_versions(mc_version),
            "forge" if is_stable_minecraft_version(mc_version) => vec![
                "47.3.0".to_string(),
                "47.2.20".to_string(),
                "47.2.0".to_string(),
            ],
            "quilt"
                if is_stable_minecraft_version(mc_version) && supports_quilt_loader(mc_version) =>
            {
                vec![
                    "0.20.0".to_string(),
                    "0.19.2".to_string(),
                    "0.19.1".to_string(),
                ]
            }
            "fabric" | "forge" | "quilt" => vec![],
            _ => vec!["latest".to_string()],
        }
    }

    fn get_neoforge_fallback_loader_versions(mc_version: &str) -> Vec<String> {
        match mc_version {
            // Snapshot of the official NeoForged Maven families as of 2026-04-09.
            "1.20.1" => vec![
                "47.1.106".to_string(),
                "47.1.105".to_string(),
                "47.1.104".to_string(),
            ],
            "1.20.2" => vec![
                "20.2.93".to_string(),
                "20.2.92".to_string(),
                "20.2.91".to_string(),
            ],
            "1.20.3" | "1.20.4" => vec![
                "20.4.251".to_string(),
                "20.4.250".to_string(),
                "20.4.249".to_string(),
            ],
            "1.20.5" | "1.20.6" => vec![
                "20.6.139".to_string(),
                "20.6.138".to_string(),
                "20.6.137".to_string(),
            ],
            "1.21" => vec![
                "21.0.167".to_string(),
                "21.0.166".to_string(),
                "21.0.165".to_string(),
            ],
            "1.21.1" => vec![
                "21.1.224".to_string(),
                "21.1.223".to_string(),
                "21.1.222".to_string(),
            ],
            "1.21.2" | "1.21.3" => vec![
                "21.3.96".to_string(),
                "21.3.95".to_string(),
                "21.3.94".to_string(),
            ],
            "1.21.4" => vec![
                "21.4.157".to_string(),
                "21.4.156".to_string(),
                "21.4.155".to_string(),
            ],
            "1.21.5" => vec![
                "21.5.97".to_string(),
                "21.5.96".to_string(),
                "21.5.95".to_string(),
            ],
            "1.21.6" => vec![
                "21.6.20-beta".to_string(),
                "21.6.19-beta".to_string(),
                "21.6.18-beta".to_string(),
            ],
            "1.21.7" => vec![
                "21.7.25-beta".to_string(),
                "21.7.24-beta".to_string(),
                "21.7.23-beta".to_string(),
            ],
            "1.21.8" => vec![
                "21.8.53".to_string(),
                "21.8.52".to_string(),
                "21.8.51".to_string(),
            ],
            "1.21.9" => vec![
                "21.9.16-beta".to_string(),
                "21.9.15-beta".to_string(),
                "21.9.14-beta".to_string(),
            ],
            "1.21.10" => vec![
                "21.10.64".to_string(),
                "21.10.63".to_string(),
                "21.10.62-beta".to_string(),
            ],
            "26.1-snapshot-6" => vec![
                "26.1.0.0-alpha.10+snapshot-6".to_string(),
                "26.1.0.0-alpha.9+snapshot-6".to_string(),
            ],
            _ => vec![],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    include!("versions.test.rs");
}
