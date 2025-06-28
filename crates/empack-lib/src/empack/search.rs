//! Project search and resolution across mod platforms
//!
//! Project matching with confidence scoring, platform
//! priority, and fuzzy string matching based on the proven bash implementation.

use percent_encoding::{CONTROLS, utf8_percent_encode};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{debug, error, trace, warn};

/// Search and resolution errors
#[derive(Debug, Error)]
pub enum SearchError {
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

    #[error("No results found for query: {query}")]
    NoResults { query: String },

    #[error("Low confidence match: {confidence}% < {threshold}%")]
    LowConfidence { confidence: u8, threshold: u8 },

    #[error("Project has extra words: '{found}' vs '{query}'")]
    ExtraWords { found: String, query: String },

    #[error("API key missing for platform: {platform}")]
    MissingApiKey { platform: String },
}

/// Platform-specific project information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectInfo {
    pub platform: Platform,
    pub project_id: String,
    pub title: String,
    pub downloads: u64,
    pub confidence: u8,
    pub project_type: String,
}

/// Platform enumeration for search results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Platform {
    Modrinth,
    CurseForge,
}

impl std::fmt::Display for Platform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Platform::Modrinth => write!(f, "modrinth"),
            Platform::CurseForge => write!(f, "curseforge"),
        }
    }
}

/// Modrinth API response structures
#[derive(Debug, Deserialize)]
struct ModrinthSearchResponse {
    hits: Vec<ModrinthProject>,
}

#[derive(Debug, Deserialize)]
struct ModrinthProject {
    project_id: String,
    title: String,
    downloads: u64,
    categories: Vec<String>,
}

/// CurseForge API response structures  
#[derive(Debug, Deserialize)]
struct CurseForgeSearchResponse {
    data: Vec<CurseForgeProject>,
}

#[derive(Debug, Deserialize)]
struct CurseForgeProject {
    id: u32,
    name: String,
    #[serde(rename = "downloadCount")]
    download_count: u64,
}

/// Configuration constants from bash implementation
const MODRINTH_CONFIDENCE_THRESHOLD: u8 = 90;
const CURSEFORGE_CONFIDENCE_THRESHOLD: u8 = 85;
const MIN_DOWNLOAD_THRESHOLD: u64 = 1000;
const EXTRA_WORDS_MAX_RATIO: u8 = 150;

/// Project search resolver with platform priority and confidence scoring
pub struct ProjectResolver {
    client: Client,
    curseforge_api_key: Option<String>,
}

impl ProjectResolver {
    /// Create new resolver with optional CurseForge API key
    pub fn new(client: Client, curseforge_api_key: Option<String>) -> Self {
        Self {
            client,
            curseforge_api_key,
        }
    }

    /// Resolve project with platform priority: Modrinth first, then CurseForge
    pub async fn resolve_project(
        &self,
        title: &str,
        project_type: Option<&str>,
        minecraft_version: Option<&str>,
        mod_loader: Option<&str>,
    ) -> Result<ProjectInfo, SearchError> {
        let project_type = project_type.unwrap_or("mod");

        debug!("Resolving project: {} ({})", title, project_type);

        // Try Modrinth first (preferred platform)
        match self
            .search_modrinth(title, project_type, minecraft_version, mod_loader)
            .await
        {
            Ok(mut project) => {
                let confidence =
                    self.calculate_confidence(title, &project.title, project.downloads);
                project.confidence = confidence;

                if !self.has_extra_words(title, &project.title)
                    && confidence >= MODRINTH_CONFIDENCE_THRESHOLD
                {
                    debug!(
                        "High confidence Modrinth match: {}% for '{}'",
                        confidence, project.title
                    );
                    return Ok(project);
                } else {
                    warn!(
                        "Modrinth match rejected: confidence {}% or extra words",
                        confidence
                    );
                }
            }
            Err(e) => {
                debug!("Modrinth search failed: {}", e);
            }
        }

        // Fallback to CurseForge with lower threshold
        match self
            .search_curseforge(title, project_type, minecraft_version, mod_loader)
            .await
        {
            Ok(mut project) => {
                let confidence =
                    self.calculate_confidence(title, &project.title, project.downloads);
                project.confidence = confidence;

                if !self.has_extra_words(title, &project.title)
                    && confidence >= CURSEFORGE_CONFIDENCE_THRESHOLD
                {
                    debug!(
                        "Acceptable CurseForge match: {}% for '{}'",
                        confidence, project.title
                    );
                    return Ok(project);
                } else {
                    warn!(
                        "CurseForge match rejected: confidence {}% or extra words",
                        confidence
                    );
                }
            }
            Err(e) => {
                debug!("CurseForge search failed: {}", e);
            }
        }

        Err(SearchError::NoResults {
            query: title.to_string(),
        })
    }

    /// Search Modrinth API for project
    async fn search_modrinth(
        &self,
        title: &str,
        project_type: &str,
        minecraft_version: Option<&str>,
        mod_loader: Option<&str>,
    ) -> Result<ProjectInfo, SearchError> {
        let normalized_type = self.normalize_project_type(project_type);
        let mut facets = vec![format!("project_type:{}", normalized_type)];

        if let Some(version) = minecraft_version {
            facets.push(format!("versions:{}", version));
        }

        if let Some(loader) = mod_loader {
            facets.push(format!("categories:{}", loader));
        }

        let facets_json = format!(
            "[{}]",
            facets
                .iter()
                .map(|f| format!("[\"{}\"]", f))
                .collect::<Vec<_>>()
                .join(",")
        );

        let url = format!(
            "https://api.modrinth.com/v2/search?query={}&facets={}",
            utf8_percent_encode(title, CONTROLS),
            utf8_percent_encode(&facets_json, CONTROLS)
        );

        trace!("Modrinth search URL: {}", url);

        let response = self
            .client
            .get(&url)
            .header("User-Agent", "empack/0.1.0")
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(SearchError::NetworkError {
                source: crate::networking::NetworkingError::RequestFailed {
                    source: response.error_for_status().unwrap_err(),
                },
            });
        }

        let search_response: ModrinthSearchResponse = response.json().await?;

        if search_response.hits.is_empty() {
            return Err(SearchError::NoResults {
                query: title.to_string(),
            });
        }

        // If we have mod_loader filter, try to find better match
        let project = if let Some(loader) = mod_loader {
            search_response
                .hits
                .iter()
                .find(|p| {
                    p.categories
                        .iter()
                        .any(|cat| cat.to_lowercase().contains(&loader.to_lowercase()))
                })
                .unwrap_or(&search_response.hits[0])
        } else {
            &search_response.hits[0]
        };

        Ok(ProjectInfo {
            platform: Platform::Modrinth,
            project_id: project.project_id.clone(),
            title: project.title.clone(),
            downloads: project.downloads,
            confidence: 0, // Will be calculated by caller
            project_type: normalized_type,
        })
    }

    /// Search CurseForge API for project
    async fn search_curseforge(
        &self,
        title: &str,
        project_type: &str,
        minecraft_version: Option<&str>,
        mod_loader: Option<&str>,
    ) -> Result<ProjectInfo, SearchError> {
        let api_key =
            self.curseforge_api_key
                .as_ref()
                .ok_or_else(|| SearchError::MissingApiKey {
                    platform: "CurseForge".to_string(),
                })?;

        let normalized_type = self.normalize_project_type(project_type);
        let class_id = self.curseforge_class_id(&normalized_type);

        let mut params = vec![
            ("gameId", "432".to_string()),
            ("classId", class_id.to_string()),
            ("searchFilter", title.to_string()),
            ("sortField", "6".to_string()), // Downloads
            ("sortOrder", "desc".to_string()),
        ];

        if let Some(version) = minecraft_version {
            params.push(("gameVersion", version.to_string()));
        }

        if let Some(loader) = mod_loader {
            if let Some(loader_id) = self.curseforge_loader_id(loader) {
                params.push(("modLoaderType", loader_id.to_string()));
            }
        }

        let query_string = params
            .iter()
            .map(|(k, v)| format!("{}={}", k, utf8_percent_encode(v, CONTROLS)))
            .collect::<Vec<_>>()
            .join("&");

        let url = format!("https://api.curseforge.com/v1/mods/search?{}", query_string);

        trace!("CurseForge search URL: {}", url);

        let response = self
            .client
            .get(&url)
            .header("x-api-key", api_key)
            .header("User-Agent", "empack/0.1.0")
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(SearchError::NetworkError {
                source: crate::networking::NetworkingError::RequestFailed {
                    source: response.error_for_status().unwrap_err(),
                },
            });
        }

        let search_response: CurseForgeSearchResponse = response.json().await?;

        if search_response.data.is_empty() {
            return Err(SearchError::NoResults {
                query: title.to_string(),
            });
        }

        let project = &search_response.data[0];

        Ok(ProjectInfo {
            platform: Platform::CurseForge,
            project_id: project.id.to_string(),
            title: project.name.clone(),
            downloads: project.download_count,
            confidence: 0, // Will be calculated by caller
            project_type: normalized_type,
        })
    }

    /// Calculate confidence score based on title match and download count
    fn calculate_confidence(&self, query: &str, found_title: &str, downloads: u64) -> u8 {
        // Fuzzy string matching using Levenshtein distance
        let query_lower = query.to_lowercase();
        let found_lower = found_title.to_lowercase();

        // Exact match gets 100%
        if query_lower == found_lower {
            return 100;
        }

        // Contains match gets high score
        if found_lower.contains(&query_lower) || query_lower.contains(&found_lower) {
            let base_score = 85;
            // Boost for popular projects
            let download_boost = if downloads >= MIN_DOWNLOAD_THRESHOLD {
                5
            } else {
                0
            };
            return std::cmp::min(100, base_score + download_boost);
        }

        // Basic Levenshtein distance calculation
        let distance = self.levenshtein_distance(&query_lower, &found_lower);
        let max_len = std::cmp::max(query.len(), found_title.len());

        if max_len == 0 {
            return 0;
        }

        let similarity = 100 - ((distance * 100) / max_len);
        let download_boost = if downloads >= MIN_DOWNLOAD_THRESHOLD {
            5
        } else {
            0
        };

        std::cmp::min(100, similarity + download_boost) as u8
    }

    /// Check if found title has too many extra words compared to query
    fn has_extra_words(&self, query: &str, found_title: &str) -> bool {
        // Normalize: lowercase, remove spaces, dashes, underscores, dots
        let norm_query = query
            .to_lowercase()
            .chars()
            .filter(|&c| c != ' ' && c != '-' && c != '_' && c != '.')
            .collect::<String>();
        let norm_found = found_title
            .to_lowercase()
            .chars()
            .filter(|&c| c != ' ' && c != '-' && c != '_' && c != '.')
            .collect::<String>();

        if norm_query.is_empty() {
            return false;
        }

        let ratio = (norm_found.len() * 100) / norm_query.len();
        ratio > EXTRA_WORDS_MAX_RATIO as usize
    }

    /// Simple Levenshtein distance implementation
    fn levenshtein_distance(&self, s1: &str, s2: &str) -> usize {
        let len1 = s1.len();
        let len2 = s2.len();
        let mut matrix = vec![vec![0; len2 + 1]; len1 + 1];

        for i in 0..=len1 {
            matrix[i][0] = i;
        }
        for j in 0..=len2 {
            matrix[0][j] = j;
        }

        for (i, c1) in s1.chars().enumerate() {
            for (j, c2) in s2.chars().enumerate() {
                let cost = if c1 == c2 { 0 } else { 1 };
                matrix[i + 1][j + 1] = std::cmp::min(
                    std::cmp::min(
                        matrix[i][j + 1] + 1, // deletion
                        matrix[i + 1][j] + 1, // insertion
                    ),
                    matrix[i][j] + cost, // substitution
                );
            }
        }

        matrix[len1][len2]
    }

    /// Normalize project type names across platforms
    fn normalize_project_type(&self, project_type: &str) -> String {
        match project_type {
            "texture-pack" | "texturepack" => "resourcepack".to_string(),
            "data-pack" => "datapack".to_string(),
            _ => project_type.to_string(),
        }
    }

    /// Get CurseForge class ID for project type
    fn curseforge_class_id(&self, project_type: &str) -> u32 {
        match project_type {
            "mod" => 6,
            "resourcepack" => 12,
            "datapack" => 17,
            _ => 6, // Default to mod
        }
    }

    /// Get CurseForge mod loader ID
    fn curseforge_loader_id(&self, mod_loader: &str) -> Option<u32> {
        match mod_loader.to_lowercase().as_str() {
            "forge" => Some(1),
            "fabric" => Some(4),
            "quilt" => Some(5),
            "neoforge" => Some(6),
            _ => None,
        }
    }
}

/// Resolve a single project using the modern resolver
pub async fn resolve_modrinth_mod(client: Client, mod_slug: String) -> Result<String, SearchError> {
    trace!("Resolving Modrinth mod: {}", mod_slug);

    let url = format!("https://api.modrinth.com/v2/project/{}", mod_slug);

    let response = client
        .get(&url)
        .header("User-Agent", "empack/0.1.0")
        .send()
        .await?;

    if !response.status().is_success() {
        error!("Modrinth API request failed: {}", response.status());
        return Err(SearchError::NetworkError {
            source: crate::networking::NetworkingError::RequestFailed {
                source: response.error_for_status().unwrap_err(),
            },
        });
    }

    let mod_data = response.text().await?;
    trace!("Successfully resolved Modrinth mod: {}", mod_slug);

    Ok(mod_data)
}

/// Resolve CurseForge project by ID
pub async fn resolve_curseforge_mod(
    client: Client,
    project_id: String,
    api_key: &str,
) -> Result<String, SearchError> {
    trace!("Resolving CurseForge mod: {}", project_id);

    let url = format!("https://api.curseforge.com/v1/mods/{}", project_id);

    let response = client
        .get(&url)
        .header("x-api-key", api_key)
        .header("User-Agent", "empack/0.1.0")
        .send()
        .await?;

    if !response.status().is_success() {
        error!("CurseForge API request failed: {}", response.status());
        return Err(SearchError::NetworkError {
            source: crate::networking::NetworkingError::RequestFailed {
                source: response.error_for_status().unwrap_err(),
            },
        });
    }

    let mod_data = response.text().await?;
    trace!("Successfully resolved CurseForge mod: {}", project_id);

    Ok(mod_data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_levenshtein_distance() {
        let resolver = ProjectResolver::new(Client::new(), None);

        assert_eq!(resolver.levenshtein_distance("kitten", "sitting"), 3);
        assert_eq!(resolver.levenshtein_distance("test", "test"), 0);
        assert_eq!(resolver.levenshtein_distance("", "test"), 4);
        assert_eq!(resolver.levenshtein_distance("test", ""), 4);
    }

    #[test]
    fn test_confidence_calculation() {
        let resolver = ProjectResolver::new(Client::new(), None);

        // Exact match
        assert_eq!(resolver.calculate_confidence("JEI", "JEI", 1000), 100);

        // Contains match with high downloads
        assert_eq!(
            resolver.calculate_confidence("JEI", "Just Enough Items (JEI)", 5000),
            90
        );

        // Contains match with low downloads
        assert_eq!(resolver.calculate_confidence("test", "test mod", 500), 85);
    }

    #[test]
    fn test_has_extra_words() {
        let resolver = ProjectResolver::new(Client::new(), None);

        // Cases that should trigger extra words detection (like bash implementation)
        assert!(resolver.has_extra_words("JEI", "Just Enough Items"));
        assert!(resolver.has_extra_words("Apotheosis", "Apotheosis Ascended"));

        // Too many extra words
        assert!(resolver.has_extra_words("test", "test mod with lots of extra words here"));

        // Normal cases that should NOT trigger extra words detection
        assert!(!resolver.has_extra_words("Create", "Create Mod"));
        assert!(!resolver.has_extra_words("Iron Chests", "Iron Chests"));

        // Edge case - empty query
        assert!(!resolver.has_extra_words("", "anything"));
    }

    #[test]
    fn test_normalize_project_type() {
        let resolver = ProjectResolver::new(Client::new(), None);

        assert_eq!(
            resolver.normalize_project_type("texture-pack"),
            "resourcepack"
        );
        assert_eq!(resolver.normalize_project_type("data-pack"), "datapack");
        assert_eq!(resolver.normalize_project_type("mod"), "mod");
    }

    #[test]
    fn test_curseforge_class_id() {
        let resolver = ProjectResolver::new(Client::new(), None);

        assert_eq!(resolver.curseforge_class_id("mod"), 6);
        assert_eq!(resolver.curseforge_class_id("resourcepack"), 12);
        assert_eq!(resolver.curseforge_class_id("datapack"), 17);
        assert_eq!(resolver.curseforge_class_id("unknown"), 6);
    }

    #[test]
    fn test_curseforge_loader_id() {
        let resolver = ProjectResolver::new(Client::new(), None);

        assert_eq!(resolver.curseforge_loader_id("forge"), Some(1));
        assert_eq!(resolver.curseforge_loader_id("fabric"), Some(4));
        assert_eq!(resolver.curseforge_loader_id("quilt"), Some(5));
        assert_eq!(resolver.curseforge_loader_id("neoforge"), Some(6));
        assert_eq!(resolver.curseforge_loader_id("unknown"), None);
    }
}
