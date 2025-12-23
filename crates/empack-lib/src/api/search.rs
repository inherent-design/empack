//! Unified search abstraction across project platforms
//!
//! Provides a platform-agnostic SearchProvider trait with Modrinth and CurseForge
//! implementations. Enables unified project search across different hosting platforms.

use crate::primitives::ModPlatform;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::Mutex;

/// Search errors
#[derive(Debug, Error)]
pub enum SearchError {
    #[error("Network error: {0}")]
    Network(String),

    #[error("Invalid query: {0}")]
    InvalidQuery(String),

    #[error("Platform error: {0}")]
    Platform(String),

    #[error("Modrinth error: {source}")]
    Modrinth {
        #[from]
        source: crate::api::modrinth::ModrinthError,
    },

    #[error("CurseForge error: {source}")]
    CurseForge {
        #[from]
        source: crate::api::curseforge::CurseForgeError,
    },
}

/// Unified search result across platforms
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    /// Project identifier (slug for Modrinth, ID for CurseForge)
    pub slug: String,
    /// Display name
    pub title: String,
    /// Short description
    pub description: String,
    /// Project ID (platform-specific format)
    pub project_id: String,
    /// Total downloads
    pub downloads: u64,
    /// Source platform
    pub platform: ModPlatform,
    /// Author name
    pub author: String,
    /// Supported game versions
    pub versions: Vec<String>,
    /// Icon URL (optional)
    pub icon_url: Option<String>,
    /// Creation date (ISO-8601)
    pub date_created: String,
    /// Modification date (ISO-8601)
    pub date_modified: String,
}

/// Search provider trait for platform-agnostic project search
pub trait SearchProvider: Send + Sync {
    /// Search for mods matching query
    ///
    /// # Arguments
    /// * `query` - Search query string
    /// * `limit` - Maximum results to return
    /// * `offset` - Skip this many results (pagination)
    ///
    /// # Returns
    /// Vector of unified search results
    fn search(
        &self,
        query: &str,
        limit: u32,
        offset: u32,
    ) -> impl std::future::Future<Output = Result<Vec<SearchResult>, SearchError>> + Send;

    /// Get the platform this provider searches
    fn platform(&self) -> ModPlatform;
}

/// Modrinth search provider implementation
pub struct ModrinthSearchProvider<C>
where
    C: crate::api::modrinth::ModrinthClient + Send + Sync,
{
    client: Arc<C>,
}

impl<C> ModrinthSearchProvider<C>
where
    C: crate::api::modrinth::ModrinthClient + Send + Sync,
{
    /// Create new Modrinth search provider
    pub fn new(client: Arc<C>) -> Self {
        Self { client }
    }
}

impl<C> SearchProvider for ModrinthSearchProvider<C>
where
    C: crate::api::modrinth::ModrinthClient + Send + Sync,
{
    async fn search(
        &self,
        query: &str,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<SearchResult>, SearchError> {
        // Modrinth has max limit of 100
        if limit > 100 {
            return Err(SearchError::InvalidQuery(
                "Modrinth limit must be <= 100".to_string(),
            ));
        }

        let results = self
            .client
            .search(query, None, limit as usize, offset as usize)
            .await?;

        Ok(results
            .hits
            .into_iter()
            .map(|hit| SearchResult {
                slug: hit.slug,
                title: hit.title,
                description: hit.description,
                project_id: hit.project_id,
                downloads: hit.downloads,
                platform: ModPlatform::Modrinth,
                author: hit.author,
                versions: hit.versions,
                icon_url: hit.icon_url,
                date_created: hit.date_created,
                date_modified: hit.date_modified,
            })
            .collect())
    }

    fn platform(&self) -> ModPlatform {
        ModPlatform::Modrinth
    }
}

/// CurseForge search provider implementation
pub struct CurseForgeSearchProvider<C>
where
    C: crate::api::curseforge::CurseForgeClient + Send + Sync,
{
    client: Arc<C>,
}

impl<C> CurseForgeSearchProvider<C>
where
    C: crate::api::curseforge::CurseForgeClient + Send + Sync,
{
    /// Create new CurseForge search provider
    pub fn new(client: Arc<C>) -> Self {
        Self { client }
    }
}

impl<C> SearchProvider for CurseForgeSearchProvider<C>
where
    C: crate::api::curseforge::CurseForgeClient + Send + Sync,
{
    async fn search(
        &self,
        query: &str,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<SearchResult>, SearchError> {
        // CurseForge has max pageSize of 50
        if limit > 50 {
            return Err(SearchError::InvalidQuery(
                "CurseForge limit must be <= 50".to_string(),
            ));
        }

        // CurseForge has pagination constraint: index + pageSize <= 10,000
        if offset + limit > 10000 {
            return Err(SearchError::InvalidQuery(
                "CurseForge offset + limit must be <= 10,000".to_string(),
            ));
        }

        // Game ID 432 = Minecraft
        let results = self
            .client
            .search(432, query, limit as usize, offset as usize)
            .await?;

        Ok(results
            .data
            .into_iter()
            .map(|mod_info| {
                // Extract game versions from latest files
                let versions: Vec<String> = mod_info
                    .latest_files
                    .iter()
                    .flat_map(|file| file.game_versions.clone())
                    .collect::<std::collections::HashSet<_>>()
                    .into_iter()
                    .collect();

                // Get primary author
                let author = mod_info
                    .authors
                    .first()
                    .map(|a| a.name.clone())
                    .unwrap_or_default();

                SearchResult {
                    slug: mod_info.slug,
                    title: mod_info.name,
                    description: mod_info.summary,
                    project_id: mod_info.id.to_string(),
                    downloads: mod_info.download_count,
                    platform: ModPlatform::CurseForge,
                    author,
                    versions,
                    icon_url: None, // CurseForge uses logo field differently
                    date_created: mod_info.date_created,
                    date_modified: mod_info.date_modified,
                }
            })
            .collect())
    }

    fn platform(&self) -> ModPlatform {
        ModPlatform::CurseForge
    }
}

/// Mock search provider for testing
pub struct MockSearchProvider {
    results: Arc<Mutex<HashMap<String, Vec<SearchResult>>>>,
    platform: ModPlatform,
}

impl MockSearchProvider {
    /// Create new mock search provider
    pub fn new(platform: ModPlatform) -> Self {
        Self {
            results: Arc::new(Mutex::new(HashMap::new())),
            platform,
        }
    }

    /// Add mock search results
    pub async fn with_results(self, query: String, results: Vec<SearchResult>) -> Self {
        self.results.lock().await.insert(query, results);
        self
    }
}

impl SearchProvider for MockSearchProvider {
    async fn search(
        &self,
        query: &str,
        _limit: u32,
        _offset: u32,
    ) -> Result<Vec<SearchResult>, SearchError> {
        let results = self.results.lock().await;
        results
            .get(query)
            .cloned()
            .ok_or_else(|| SearchError::InvalidQuery(format!("No results for query: {}", query)))
    }

    fn platform(&self) -> ModPlatform {
        self.platform
    }
}

#[cfg(test)]
mod tests {
    include!("search.test.rs");
}
