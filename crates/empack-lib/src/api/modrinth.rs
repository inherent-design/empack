//! Modrinth API client implementation
//!
//! Provides production (Live) and test (Mock) implementations of the Modrinth API client.
//! Uses NetworkingManager for HTTP requests with caching and rate limiting.

use crate::networking::{NetworkingManager, Platform};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::Mutex;

/// Modrinth API errors
#[derive(Debug, Error)]
pub enum ModrinthError {
    #[error("HTTP request failed: {source}")]
    RequestFailed {
        #[from]
        source: reqwest::Error,
    },

    #[error("Network error: {source}")]
    NetworkError {
        #[from]
        source: crate::networking::NetworkingError,
    },

    #[error("JSON parsing failed: {source}")]
    JsonError {
        #[from]
        source: serde_json::Error,
    },

    #[error("Project not found: {project_id}")]
    ProjectNotFound { project_id: String },

    #[error("Version not found: {version_id}")]
    VersionNotFound { version_id: String },

    #[error("Invalid search parameters: {message}")]
    InvalidSearchParams { message: String },
}

/// Search result hit from Modrinth API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHit {
    pub slug: String,
    pub title: String,
    pub description: String,
    pub project_id: String,
    pub project_type: String,
    pub downloads: u64,
    pub icon_url: Option<String>,
    pub author: String,
    pub versions: Vec<String>,
    pub follows: u64,
    pub date_created: String,
    pub date_modified: String,
    pub latest_version: Option<String>,
    pub license: Option<String>,
    pub categories: Vec<String>,
    pub client_side: String,
    pub server_side: String,
}

/// Search results response from Modrinth API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResults {
    pub hits: Vec<SearchHit>,
    pub offset: usize,
    pub limit: usize,
    pub total_hits: usize,
}

/// Dependency relationship type
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DependencyType {
    Required,
    Optional,
    Incompatible,
    Embedded,
}

/// Version dependency
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionDependency {
    pub version_id: Option<String>,
    pub project_id: Option<String>,
    pub file_name: Option<String>,
    pub dependency_type: DependencyType,
}

/// File hash
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileHash {
    pub sha1: String,
    pub sha512: String,
}

/// Version file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionFile {
    pub hashes: FileHash,
    pub url: String,
    pub filename: String,
    pub primary: bool,
    pub size: u64,
    pub file_type: Option<String>,
}

/// Version information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Version {
    pub id: String,
    pub project_id: String,
    pub author_id: String,
    pub name: String,
    pub version_number: String,
    pub changelog: Option<String>,
    pub dependencies: Vec<VersionDependency>,
    pub game_versions: Vec<String>,
    pub version_type: String,
    pub loaders: Vec<String>,
    pub featured: bool,
    pub status: String,
    pub date_published: String,
    pub downloads: u64,
    pub files: Vec<VersionFile>,
}

/// Dependencies response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectDependencies {
    pub projects: Vec<SearchHit>,
    pub versions: Vec<Version>,
}

/// Trait for Modrinth API operations
pub trait ModrinthClient {
    /// Search for projects on Modrinth
    ///
    /// # Arguments
    /// * `query` - Search query string
    /// * `facets` - JSON facets for filtering
    /// * `limit` - Maximum results (max 100)
    /// * `offset` - Skip this many results
    ///
    /// # Returns
    /// Search results with hits and pagination info
    fn search(
        &self,
        query: &str,
        facets: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> impl std::future::Future<Output = Result<SearchResults, ModrinthError>> + Send;

    /// Get all dependencies for a project
    ///
    /// # Arguments
    /// * `project_id` - Project ID or slug
    ///
    /// # Returns
    /// Projects and versions that this project depends on
    fn get_dependencies(
        &self,
        project_id: &str,
    ) -> impl std::future::Future<Output = Result<ProjectDependencies, ModrinthError>> + Send;

    /// Download a file with hash verification
    ///
    /// # Arguments
    /// * `url` - Download URL
    /// * `expected_hashes` - Expected SHA-1 and SHA-512 hashes
    ///
    /// # Returns
    /// Downloaded file bytes
    fn download_file(
        &self,
        url: &str,
        expected_hashes: &FileHash,
    ) -> impl std::future::Future<Output = Result<Vec<u8>, ModrinthError>> + Send;
}

/// Live Modrinth API client (production)
pub struct LiveModrinthClient {
    networking: Arc<NetworkingManager>,
    base_url: String,
}

impl LiveModrinthClient {
    /// Create new live Modrinth client
    pub fn new(networking: Arc<NetworkingManager>) -> Self {
        Self {
            networking,
            base_url: "https://api.modrinth.com/v2".to_string(),
        }
    }

    /// Create client with custom base URL (for staging/testing)
    pub fn with_base_url(networking: Arc<NetworkingManager>, base_url: String) -> Self {
        Self {
            networking,
            base_url,
        }
    }
}

impl ModrinthClient for LiveModrinthClient {
    async fn search(
        &self,
        query: &str,
        facets: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> Result<SearchResults, ModrinthError> {
        if limit > 100 {
            return Err(ModrinthError::InvalidSearchParams {
                message: "limit must be <= 100".to_string(),
            });
        }

        let mut url = format!(
            "{}/search?query={}&limit={}&offset={}",
            self.base_url,
            urlencoding::encode(query),
            limit,
            offset
        );

        if let Some(facets_str) = facets {
            url.push_str(&format!("&facets={}", urlencoding::encode(facets_str)));
        }

        let data = self
            .networking
            .get_with_cache_and_rate_limit(&url, Platform::Modrinth)
            .await?;

        let results: SearchResults = serde_json::from_slice(&data)?;
        Ok(results)
    }

    async fn get_dependencies(
        &self,
        project_id: &str,
    ) -> Result<ProjectDependencies, ModrinthError> {
        let url = format!("{}/project/{}/dependencies", self.base_url, project_id);

        let data = self
            .networking
            .get_with_cache_and_rate_limit(&url, Platform::Modrinth)
            .await?;

        let deps: ProjectDependencies = serde_json::from_slice(&data)?;
        Ok(deps)
    }

    async fn download_file(
        &self,
        url: &str,
        expected_hashes: &FileHash,
    ) -> Result<Vec<u8>, ModrinthError> {
        // Direct download without rate limiting (CDN)
        let client = self.networking.client();
        let response = client.get(url).send().await?;

        if !response.status().is_success() {
            return Err(ModrinthError::RequestFailed {
                source: response.error_for_status().unwrap_err(),
            });
        }

        let bytes = response.bytes().await?.to_vec();

        // Verify SHA-512 hash
        use sha2::{Digest, Sha512};
        let mut hasher = Sha512::new();
        hasher.update(&bytes);
        let hash = format!("{:x}", hasher.finalize());

        if hash != expected_hashes.sha512 {
            return Err(ModrinthError::InvalidSearchParams {
                message: format!(
                    "Hash mismatch: expected {}, got {}",
                    expected_hashes.sha512, hash
                ),
            });
        }

        Ok(bytes)
    }
}

/// Mock Modrinth API client (testing)
pub struct MockModrinthClient {
    search_responses: Arc<Mutex<HashMap<String, Result<SearchResults, String>>>>,
    dependency_responses: Arc<Mutex<HashMap<String, Result<ProjectDependencies, String>>>>,
    download_responses: Arc<Mutex<HashMap<String, Result<Vec<u8>, String>>>>,
}

impl MockModrinthClient {
    /// Create new mock client
    pub fn new() -> Self {
        Self {
            search_responses: Arc::new(Mutex::new(HashMap::new())),
            dependency_responses: Arc::new(Mutex::new(HashMap::new())),
            download_responses: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Add mock search response
    pub async fn with_search_result(
        self,
        query: String,
        result: Result<SearchResults, String>,
    ) -> Self {
        self.search_responses.lock().await.insert(query, result);
        self
    }

    /// Add mock dependency response
    pub async fn with_dependency_result(
        self,
        project_id: String,
        result: Result<ProjectDependencies, String>,
    ) -> Self {
        self.dependency_responses
            .lock()
            .await
            .insert(project_id, result);
        self
    }

    /// Add mock download response
    pub async fn with_download_result(self, url: String, result: Result<Vec<u8>, String>) -> Self {
        self.download_responses.lock().await.insert(url, result);
        self
    }
}

impl ModrinthClient for MockModrinthClient {
    async fn search(
        &self,
        query: &str,
        _facets: Option<&str>,
        _limit: usize,
        _offset: usize,
    ) -> Result<SearchResults, ModrinthError> {
        let responses = self.search_responses.lock().await;

        match responses.get(query) {
            Some(Ok(results)) => Ok(results.clone()),
            Some(Err(err)) => Err(ModrinthError::InvalidSearchParams {
                message: err.clone(),
            }),
            None => Err(ModrinthError::InvalidSearchParams {
                message: format!("No mock response for query: {}", query),
            }),
        }
    }

    async fn get_dependencies(
        &self,
        project_id: &str,
    ) -> Result<ProjectDependencies, ModrinthError> {
        let responses = self.dependency_responses.lock().await;

        match responses.get(project_id) {
            Some(Ok(deps)) => Ok(deps.clone()),
            Some(Err(err)) => Err(ModrinthError::ProjectNotFound {
                project_id: err.clone(),
            }),
            None => Err(ModrinthError::ProjectNotFound {
                project_id: project_id.to_string(),
            }),
        }
    }

    async fn download_file(
        &self,
        url: &str,
        _expected_hashes: &FileHash,
    ) -> Result<Vec<u8>, ModrinthError> {
        let responses = self.download_responses.lock().await;

        match responses.get(url) {
            Some(Ok(bytes)) => Ok(bytes.clone()),
            Some(Err(err)) => Err(ModrinthError::InvalidSearchParams {
                message: err.clone(),
            }),
            None => Err(ModrinthError::InvalidSearchParams {
                message: format!("No mock response for URL: {}", url),
            }),
        }
    }
}

impl Default for MockModrinthClient {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    include!("modrinth.test.rs");
}
