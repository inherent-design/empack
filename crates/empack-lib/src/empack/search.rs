//! Project search and resolution across project platforms
//!
//! Project matching with confidence scoring, platform
//! priority, and fuzzy string matching based on the proven bash implementation.

use crate::networking::cache::HttpCache;
use crate::networking::rate_limit::RateLimiterManager;
use crate::primitives::ProjectPlatform;
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use thiserror::Error;
use tracing::{debug, trace, warn};

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

    #[error("'{project_title}' exists but does not support {requested_loader:?}. Supported loaders: {}", available_loaders.join(", "))]
    IncompatibleProject {
        query: String,
        project_title: String,
        project_slug: String,
        available_loaders: Vec<String>,
        available_versions: Vec<String>,
        requested_loader: Option<String>,
        requested_version: Option<String>,
        downloads: u64,
    },

    #[error("API key missing for platform: {platform}")]
    MissingApiKey { platform: String },

    #[error("{0}")]
    Other(#[from] anyhow::Error),
}

/// Trait for project resolution across project platforms
pub trait ProjectResolverTrait: Send + Sync {
    /// Resolve project with platform priority: Modrinth first, then CurseForge, then Forge
    ///
    /// When `preferred_platform` is `Some(CurseForge)`, tries CurseForge first.
    /// Otherwise keeps the default Modrinth-first order.
    fn resolve_project(
        &self,
        title: &str,
        project_type: Option<&str>,
        minecraft_version: Option<&str>,
        mod_loader: Option<&str>,
        preferred_platform: Option<ProjectPlatform>,
    ) -> Pin<Box<dyn Future<Output = Result<ProjectInfo, SearchError>> + Send + '_>>;
}

/// Platform-specific project information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectInfo {
    pub platform: ProjectPlatform,
    pub project_id: String,
    pub title: String,
    pub downloads: u64,
    pub confidence: u8,
    pub project_type: String,
}

/// Modrinth API response structures
#[derive(Debug, Deserialize)]
struct ModrinthSearchResponse {
    hits: Vec<ModrinthProject>,
}

#[derive(Debug, Deserialize)]
struct ModrinthProject {
    project_id: String,
    slug: String,
    title: String,
    downloads: u64,
    categories: Vec<String>,
    #[serde(default)]
    versions: Vec<String>,
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

use crate::empack::fuzzy;

const KNOWN_LOADERS: &[&str] = &[
    "fabric", "forge", "neoforge", "quilt", "liteloader", "rift",
];

fn extract_loaders(categories: &[String]) -> Vec<String> {
    categories
        .iter()
        .filter(|cat| KNOWN_LOADERS.contains(&cat.as_str()))
        .cloned()
        .collect()
}

/// Project search resolver with platform priority and confidence scoring
pub struct ProjectResolver {
    client: Client,
    curseforge_api_key: Option<String>,
    modrinth_base_url: String,
    curseforge_base_url: String,
    cache: Option<Arc<HttpCache>>,
    rate_limiter: Option<Arc<RateLimiterManager>>,
}

impl ProjectResolver {
    /// Create new resolver with optional CurseForge API key
    pub fn new(client: Client, curseforge_api_key: Option<String>) -> Self {
        Self {
            client,
            curseforge_api_key,
            modrinth_base_url: "https://api.modrinth.com".to_string(),
            curseforge_base_url: "https://api.curseforge.com".to_string(),
            cache: None,
            rate_limiter: None,
        }
    }

    /// Create new resolver with cache and rate limiter
    pub fn with_networking(
        client: Client,
        curseforge_api_key: Option<String>,
        cache: Arc<HttpCache>,
        rate_limiter: Arc<RateLimiterManager>,
    ) -> Self {
        Self {
            client,
            curseforge_api_key,
            modrinth_base_url: "https://api.modrinth.com".to_string(),
            curseforge_base_url: "https://api.curseforge.com".to_string(),
            cache: Some(cache),
            rate_limiter: Some(rate_limiter),
        }
    }

    /// Create new resolver with custom base URLs (for testing)
    #[cfg(feature = "test-utils")]
    pub fn new_with_base_urls(
        client: Client,
        curseforge_api_key: Option<String>,
        modrinth_base_url: Option<String>,
        curseforge_base_url: Option<String>,
    ) -> Self {
        Self {
            client,
            curseforge_api_key,
            modrinth_base_url: modrinth_base_url
                .unwrap_or_else(|| "https://api.modrinth.com".to_string()),
            curseforge_base_url: curseforge_base_url
                .unwrap_or_else(|| "https://api.curseforge.com".to_string()),
            cache: None,
            rate_limiter: None,
        }
    }

    /// Create new resolver with custom base URLs and networking (for testing)
    #[cfg(feature = "test-utils")]
    pub fn new_with_base_urls_and_networking(
        client: Client,
        curseforge_api_key: Option<String>,
        modrinth_base_url: Option<String>,
        curseforge_base_url: Option<String>,
        cache: Arc<HttpCache>,
        rate_limiter: Arc<RateLimiterManager>,
    ) -> Self {
        Self {
            client,
            curseforge_api_key,
            modrinth_base_url: modrinth_base_url
                .unwrap_or_else(|| "https://api.modrinth.com".to_string()),
            curseforge_base_url: curseforge_base_url
                .unwrap_or_else(|| "https://api.curseforge.com".to_string()),
            cache: Some(cache),
            rate_limiter: Some(rate_limiter),
        }
    }

    /// Resolve project with platform priority: Modrinth first, then CurseForge
    ///
    /// When `project_type` is `Some`, searches only that type.
    /// When `None`, tries each type in tier order (mod, resourcepack, shader, datapack)
    /// and returns the first high-confidence match.
    ///
    /// When `preferred_platform` is `Some(CurseForge)`, tries CurseForge first.
    /// Otherwise keeps the default Modrinth-first order.
    pub async fn resolve_project(
        &self,
        title: &str,
        project_type: Option<&str>,
        minecraft_version: Option<&str>,
        mod_loader: Option<&str>,
        preferred_platform: Option<ProjectPlatform>,
    ) -> Result<ProjectInfo, SearchError> {
        if let Some(pt) = project_type {
            return self
                .resolve_project_for_type(title, pt, minecraft_version, mod_loader, preferred_platform)
                .await;
        }

        // No type specified: tiered search across all types
        let type_tiers = ["mod", "resourcepack", "shader", "datapack"];

        debug!(
            "Resolving project with tiered search: {} (trying {:?})",
            title, type_tiers
        );

        for project_type in &type_tiers {
            match self
                .resolve_project_for_type(title, project_type, minecraft_version, mod_loader, preferred_platform)
                .await
            {
                Ok(result) => return Ok(result),
                Err(SearchError::NoResults { .. })
                | Err(SearchError::LowConfidence { .. })
                | Err(SearchError::ExtraWords { .. }) => {
                    debug!("No match for type '{}', trying next tier", project_type);
                    continue;
                }
                Err(e) => {
                    // Network/API errors should not be swallowed
                    return Err(e);
                }
            }
        }

        Err(SearchError::NoResults {
            query: title.to_string(),
        })
    }

    /// Resolve project for a specific type with platform priority
    ///
    /// Default order: Modrinth first, then CurseForge.
    /// When `preferred_platform` is `Some(CurseForge)`, tries CurseForge first.
    async fn resolve_project_for_type(
        &self,
        title: &str,
        project_type: &str,
        minecraft_version: Option<&str>,
        mod_loader: Option<&str>,
        preferred_platform: Option<ProjectPlatform>,
    ) -> Result<ProjectInfo, SearchError> {
        debug!("Resolving project: {} ({})", title, project_type);

        if preferred_platform == Some(ProjectPlatform::CurseForge) {
            // CurseForge-first order when explicitly preferred
            if let Some(result) = self
                .try_platform_search(
                    title, project_type, minecraft_version, mod_loader,
                    ProjectPlatform::CurseForge,
                )
                .await?
            {
                return Ok(result);
            }
            if let Some(result) = self
                .try_platform_search(
                    title, project_type, minecraft_version, mod_loader,
                    ProjectPlatform::Modrinth,
                )
                .await?
            {
                return Ok(result);
            }
        } else {
            // Default: Modrinth first, CurseForge fallback
            if let Some(result) = self
                .try_platform_search(
                    title, project_type, minecraft_version, mod_loader,
                    ProjectPlatform::Modrinth,
                )
                .await?
            {
                return Ok(result);
            }
            if let Some(result) = self
                .try_platform_search(
                    title, project_type, minecraft_version, mod_loader,
                    ProjectPlatform::CurseForge,
                )
                .await?
            {
                return Ok(result);
            }
        }

        Err(SearchError::NoResults {
            query: title.to_string(),
        })
    }

    /// Try searching a single platform, returning Some on high-confidence match.
    ///
    /// Scores all results from the platform and returns the highest-confidence one
    /// that passes the threshold. Returns `Ok(None)` for low-confidence or no-results
    /// (try next platform). Returns `Err` for network/API failures (should propagate).
    async fn try_platform_search(
        &self,
        title: &str,
        project_type: &str,
        minecraft_version: Option<&str>,
        mod_loader: Option<&str>,
        platform: ProjectPlatform,
    ) -> Result<Option<ProjectInfo>, SearchError> {
        let (search_result, threshold, label) = match platform {
            ProjectPlatform::Modrinth => (
                self.search_modrinth(title, project_type, minecraft_version, mod_loader).await,
                fuzzy::MODRINTH_CONFIDENCE_THRESHOLD,
                "Modrinth",
            ),
            ProjectPlatform::CurseForge => (
                self.search_curseforge(title, project_type, minecraft_version, mod_loader).await,
                fuzzy::CURSEFORGE_CONFIDENCE_THRESHOLD,
                "CurseForge",
            ),
        };

        match search_result {
            Ok(projects) => {
                let scored = Self::score_results(title, projects);

                let best = scored
                    .into_iter()
                    .find(|p| !fuzzy::has_extra_words(title, &p.title) && p.confidence >= threshold);

                if let Some(project) = best {
                    debug!(
                        "High confidence {} match: {}% for '{}'",
                        label, project.confidence, project.title
                    );
                    Ok(Some(project))
                } else {
                    warn!(
                        "{} match rejected: no result above threshold {}%",
                        label, threshold
                    );
                    Ok(None)
                }
            }
            Err(e) => match &e {
                SearchError::NoResults { .. }
                | SearchError::LowConfidence { .. }
                | SearchError::ExtraWords { .. }
                | SearchError::MissingApiKey { .. } => {
                    debug!("{} search: {}", label, e);
                    Ok(None)
                }
                _ => {
                    debug!("{} search failed: {}", label, e);
                    Err(e)
                }
            },
        }
    }

    /// Score and rank a list of ProjectInfo results by confidence (descending).
    fn score_results(query: &str, mut projects: Vec<ProjectInfo>) -> Vec<ProjectInfo> {
        for project in &mut projects {
            project.confidence =
                fuzzy::calculate_confidence(query, &project.title, project.downloads);
        }
        projects.sort_by(|a, b| b.confidence.cmp(&a.confidence));
        projects
    }

    /// Search both platforms and return ranked candidates when confidence is ambiguous.
    ///
    /// Returns all results with confidence >= `min_confidence`, sorted by confidence
    /// descending. Used by the UI layer to present a selection list when the top
    /// result isn't a clear auto-select (confidence 70-89%).
    pub async fn search_candidates(
        &self,
        title: &str,
        project_type: &str,
        minecraft_version: Option<&str>,
        mod_loader: Option<&str>,
        min_confidence: u8,
        preferred_platform: Option<ProjectPlatform>,
    ) -> Result<Vec<ProjectInfo>, SearchError> {
        let mut all_results = Vec::new();

        let platforms = if preferred_platform == Some(ProjectPlatform::CurseForge) {
            vec![ProjectPlatform::CurseForge, ProjectPlatform::Modrinth]
        } else {
            vec![ProjectPlatform::Modrinth, ProjectPlatform::CurseForge]
        };

        for platform in platforms {
            let results = match platform {
                ProjectPlatform::Modrinth => {
                    self.search_modrinth(title, project_type, minecraft_version, mod_loader)
                        .await
                }
                ProjectPlatform::CurseForge => {
                    self.search_curseforge(title, project_type, minecraft_version, mod_loader)
                        .await
                }
            };

            match results {
                Ok(projects) => all_results.extend(projects),
                Err(SearchError::NoResults { .. })
                | Err(SearchError::MissingApiKey { .. }) => {
                    debug!("No results from {:?}, continuing", platform);
                }
                Err(e) => return Err(e),
            }
        }

        if all_results.is_empty() {
            return Err(SearchError::NoResults {
                query: title.to_string(),
            });
        }

        let scored = Self::score_results(title, all_results);
        let best_confidence = scored.first().map(|p| p.confidence).unwrap_or(0);

        let candidates: Vec<ProjectInfo> = scored
            .into_iter()
            .filter(|p| p.confidence >= min_confidence && !fuzzy::has_extra_words(title, &p.title))
            .collect();

        if candidates.is_empty() {
            return Err(SearchError::LowConfidence {
                confidence: best_confidence,
                threshold: min_confidence,
            });
        }

        Ok(candidates)
    }

    /// Perform a cached, rate-limited GET request, returning (status_code, body_bytes).
    ///
    /// If a cache is configured, checks for a cached response first. On cache miss
    /// (or no cache), the request goes through the rate limiter (if configured) with
    /// the provided headers. Successful responses are stored back in the cache.
    async fn cached_get(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        platform: ProjectPlatform,
    ) -> Result<(u16, Vec<u8>), SearchError> {
        use crate::networking::cache::CachedResponse;
        use std::time::SystemTime;

        // Check cache first
        if let Some(cache) = &self.cache
            && let Some(cached) = cache.get(url).await
            && !cached.is_expired()
        {
            trace!("Cache hit for search URL: {}", url);
            return Ok((cached.status, cached.data));
        }

        // Send through rate limiter or directly
        let response = if let Some(rate_limiter) = &self.rate_limiter {
            let rl_client = rate_limiter.client_for_platform(platform);
            let mut req_builder = self.client.get(url);
            for (key, value) in headers {
                req_builder = req_builder.header(*key, *value);
            }
            let request = req_builder.build()?;
            rl_client.execute(request).await?
        } else {
            let mut req_builder = self.client.get(url);
            for (key, value) in headers {
                req_builder = req_builder.header(*key, *value);
            }
            req_builder.send().await?
        };

        let status = response.status().as_u16();
        let etag = response
            .headers()
            .get("etag")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        let bytes = response.bytes().await?.to_vec();

        // Cache successful responses
        if let Some(cache) = &self.cache
            && (200..300).contains(&status)
        {
            let cached_response = CachedResponse {
                data: bytes.clone(),
                etag,
                expires: SystemTime::now() + cache.default_ttl(),
                status,
            };
            cache.put(url.to_string(), cached_response).await;
            trace!("Cached search response for URL: {}", url);
        }

        Ok((status, bytes))
    }

    /// Search Modrinth API for projects (returns multiple results)
    async fn search_modrinth(
        &self,
        title: &str,
        project_type: &str,
        minecraft_version: Option<&str>,
        mod_loader: Option<&str>,
    ) -> Result<Vec<ProjectInfo>, SearchError> {
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
            "{}/v2/search?query={}&facets={}",
            self.modrinth_base_url,
            utf8_percent_encode(title, NON_ALPHANUMERIC),
            utf8_percent_encode(&facets_json, NON_ALPHANUMERIC)
        );

        trace!("Modrinth search URL: {}", url);

        let headers = vec![("User-Agent", "empack/0.1.0")];
        let (status, body) = self
            .cached_get(&url, &headers, ProjectPlatform::Modrinth)
            .await?;

        if !(200..300).contains(&status) {
            return Err(SearchError::NetworkError {
                source: crate::networking::NetworkingError::CacheError {
                    message: format!("Modrinth API returned status {}", status),
                },
            });
        }

        let search_response: ModrinthSearchResponse = serde_json::from_slice(&body)?;

        if search_response.hits.is_empty() {
            if (minecraft_version.is_some() || mod_loader.is_some())
                && let Some(incompatible) = self
                    .detect_incompatible_project(
                        title,
                        &normalized_type,
                        mod_loader,
                        minecraft_version,
                    )
                    .await?
            {
                return Err(incompatible);
            }

            return Err(SearchError::NoResults {
                query: title.to_string(),
            });
        }

        // If we have mod_loader filter, sort loader-matching results first
        let mut hits = search_response.hits;
        if let Some(loader) = mod_loader {
            let loader_lower = loader.to_lowercase();
            hits.sort_by_key(|p| {
                let matches_loader = p
                    .categories
                    .iter()
                    .any(|cat| cat.to_lowercase().contains(&loader_lower));
                if matches_loader { 0 } else { 1 }
            });
        }

        Ok(hits
            .iter()
            .map(|project| ProjectInfo {
                platform: ProjectPlatform::Modrinth,
                project_id: project.project_id.clone(),
                title: project.title.clone(),
                downloads: project.downloads,
                confidence: 0, // Will be calculated by caller
                project_type: normalized_type.clone(),
            })
            .collect())
    }

    /// Search CurseForge API for projects (returns multiple results)
    async fn search_curseforge(
        &self,
        title: &str,
        project_type: &str,
        minecraft_version: Option<&str>,
        mod_loader: Option<&str>,
    ) -> Result<Vec<ProjectInfo>, SearchError> {
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

        if let Some(loader) = mod_loader
            && let Some(loader_id) = self.curseforge_loader_id(loader)
        {
            params.push(("modLoaderType", loader_id.to_string()));
        }

        let query_string = params
            .iter()
            .map(|(k, v)| format!("{}={}", k, utf8_percent_encode(v, NON_ALPHANUMERIC)))
            .collect::<Vec<_>>()
            .join("&");

        let url = format!(
            "{}/v1/mods/search?{}",
            self.curseforge_base_url, query_string
        );

        trace!("CurseForge search URL: {}", url);

        let headers = vec![
            ("x-api-key", api_key.as_str()),
            ("User-Agent", "empack/0.1.0"),
        ];
        let (status, body) = self
            .cached_get(&url, &headers, ProjectPlatform::CurseForge)
            .await?;

        if !(200..300).contains(&status) {
            return Err(SearchError::NetworkError {
                source: crate::networking::NetworkingError::CacheError {
                    message: format!("CurseForge API returned status {}", status),
                },
            });
        }

        let search_response: CurseForgeSearchResponse = serde_json::from_slice(&body)?;

        if search_response.data.is_empty() {
            return Err(SearchError::NoResults {
                query: title.to_string(),
            });
        }

        Ok(search_response
            .data
            .iter()
            .map(|project| ProjectInfo {
                platform: ProjectPlatform::CurseForge,
                project_id: project.id.to_string(),
                title: project.name.clone(),
                downloads: project.download_count,
                confidence: 0, // Will be calculated by caller
                project_type: normalized_type.clone(),
            })
            .collect())
    }

    async fn detect_incompatible_project(
        &self,
        title: &str,
        project_type: &str,
        requested_loader: Option<&str>,
        requested_version: Option<&str>,
    ) -> Result<Option<SearchError>, SearchError> {
        let facets_json = format!("[[\"project_type:{}\"]]", project_type);
        let url = format!(
            "{}/v2/search?query={}&facets={}",
            self.modrinth_base_url,
            utf8_percent_encode(title, NON_ALPHANUMERIC),
            utf8_percent_encode(&facets_json, NON_ALPHANUMERIC)
        );

        let headers = vec![("User-Agent", "empack/0.1.0")];
        let (status, body) = self
            .cached_get(&url, &headers, ProjectPlatform::Modrinth)
            .await?;

        if !(200..300).contains(&status) {
            return Ok(None);
        }

        let search_response: ModrinthSearchResponse = match serde_json::from_slice(&body) {
            Ok(r) => r,
            Err(_) => return Ok(None),
        };

        if search_response.hits.is_empty() {
            return Ok(None);
        }

        let best = search_response
            .hits
            .iter()
            .filter(|p| !fuzzy::has_extra_words(title, &p.title))
            .max_by_key(|p| fuzzy::calculate_confidence(title, &p.title, p.downloads));

        let project = match best {
            Some(p)
                if fuzzy::calculate_confidence(title, &p.title, p.downloads)
                    >= fuzzy::MODRINTH_CONFIDENCE_THRESHOLD =>
            {
                p
            }
            _ => return Ok(None),
        };

        let available_loaders = extract_loaders(&project.categories);

        if available_loaders.is_empty() {
            return Ok(None);
        }

        Ok(Some(SearchError::IncompatibleProject {
            query: title.to_string(),
            project_title: project.title.clone(),
            project_slug: project.slug.clone(),
            available_loaders,
            available_versions: project.versions.clone(),
            requested_loader: requested_loader.map(|s| s.to_string()),
            requested_version: requested_version.map(|s| s.to_string()),
            downloads: project.downloads,
        }))
    }

    /// Normalize project type names across platforms
    fn normalize_project_type(&self, project_type: &str) -> String {
        match project_type {
            "texture-pack" | "texturepack" => "resourcepack".to_string(),
            "data-pack" => "datapack".to_string(),
            _ => project_type.to_string(),
        }
    }

    /// Get CurseForge class ID for project type.
    ///
    /// Falls back to classId 6 (Mods) for unmapped types. This is intentional:
    /// most CurseForge shader packs (e.g. Iris Shaders, Complementary) are
    /// distributed as mods under classId 6. The CurseForge API does not expose
    /// a confirmed shader-specific class ID via /v1/categories.
    fn curseforge_class_id(&self, project_type: &str) -> u32 {
        match project_type {
            "mod" => 6,
            "resourcepack" => 12,
            "datapack" => 17,
            other => {
                debug!("No dedicated CurseForge classId for '{}', falling back to 6 (Mods)", other);
                6
            }
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

impl ProjectResolverTrait for ProjectResolver {
    fn resolve_project(
        &self,
        title: &str,
        project_type: Option<&str>,
        minecraft_version: Option<&str>,
        mod_loader: Option<&str>,
        preferred_platform: Option<ProjectPlatform>,
    ) -> Pin<Box<dyn Future<Output = Result<ProjectInfo, SearchError>> + Send + '_>> {
        let title = title.to_string();
        let project_type = project_type.map(|s| s.to_string());
        let minecraft_version = minecraft_version.map(|s| s.to_string());
        let mod_loader = mod_loader.map(|s| s.to_string());

        Box::pin(async move {
            self.resolve_project(
                &title,
                project_type.as_deref(),
                minecraft_version.as_deref(),
                mod_loader.as_deref(),
                preferred_platform,
            )
            .await
        })
    }
}

#[cfg(test)]
mod tests {
    include!("search.test.rs");
}
