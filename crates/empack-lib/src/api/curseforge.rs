//! CurseForge API client implementation
//!
//! Provides production (Live) and test (Mock) implementations of the CurseForge API client.
//! Uses NetworkingManager for HTTP requests with caching and rate limiting.

use crate::networking::NetworkingManager;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::Mutex;

/// CurseForge API errors
#[derive(Debug, Error)]
pub enum CurseForgeError {
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

    #[error("Mod not found: {mod_id}")]
    ModNotFound { mod_id: u32 },

    #[error("File not found: {file_id}")]
    FileNotFound { file_id: u32 },

    #[error("Invalid search parameters: {message}")]
    InvalidSearchParams { message: String },

    #[error("Missing API key")]
    MissingApiKey,
}

/// Search result from CurseForge API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub id: u32,
    #[serde(rename = "gameId")]
    pub game_id: u32,
    pub name: String,
    pub slug: String,
    pub summary: String,
    #[serde(rename = "downloadCount")]
    pub download_count: u64,
    #[serde(rename = "dateCreated")]
    pub date_created: String,
    #[serde(rename = "dateModified")]
    pub date_modified: String,
    #[serde(rename = "dateReleased")]
    pub date_released: String,
    pub authors: Vec<ModAuthor>,
    pub categories: Vec<Category>,
    #[serde(rename = "latestFiles")]
    pub latest_files: Vec<FileInfo>,
}

/// Mproject author information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModAuthor {
    pub id: u32,
    pub name: String,
    pub url: String,
}

/// Category information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Category {
    pub id: u32,
    pub name: String,
    pub slug: String,
    #[serde(rename = "iconUrl")]
    pub icon_url: String,
}

/// Pagination information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pagination {
    pub index: usize,
    #[serde(rename = "pageSize")]
    pub page_size: usize,
    #[serde(rename = "resultCount")]
    pub result_count: usize,
    #[serde(rename = "totalCount")]
    pub total_count: usize,
}

/// Search results response from CurseForge API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResults {
    pub data: Vec<SearchResult>,
    pub pagination: Pagination,
}

/// Dependency relationship type (numeric enum from CurseForge)
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[repr(u8)]
pub enum DependencyType {
    EmbeddedLibrary = 1,
    OptionalDependency = 2,
    RequiredDependency = 3,
    Tool = 4,
    Incompatible = 5,
    Include = 6,
}

/// File dependency
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileDependency {
    #[serde(rename = "modId")]
    pub mod_id: u32,
    #[serde(rename = "relationType")]
    pub relation_type: u8,
}

/// File hash
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileHash {
    pub value: String,
    pub algo: u8, // 1=SHA1, 2=MD5
}

/// File information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileInfo {
    pub id: u32,
    #[serde(rename = "gameId")]
    pub game_id: u32,
    #[serde(rename = "modId")]
    pub mod_id: u32,
    #[serde(rename = "isAvailable")]
    pub is_available: bool,
    #[serde(rename = "displayName")]
    pub display_name: String,
    #[serde(rename = "fileName")]
    pub file_name: String,
    #[serde(rename = "fileDate")]
    pub file_date: String,
    #[serde(rename = "fileLength")]
    pub file_length: u64,
    #[serde(rename = "downloadUrl")]
    pub download_url: String,
    #[serde(rename = "gameVersions")]
    pub game_versions: Vec<String>,
    pub dependencies: Vec<FileDependency>,
    pub hashes: Vec<FileHash>,
}

/// Mod dependencies response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModDependencies {
    pub mods: Vec<SearchResult>,
    pub files: Vec<FileInfo>,
}

/// Trait for CurseForge API operations
pub trait CurseForgeClient {
    /// Search for mods on CurseForge
    ///
    /// # Arguments
    /// * `game_id` - Game ID (432 for Minecraft)
    /// * `search_filter` - Search query string
    /// * `page_size` - Maximum results (max 50)
    /// * `index` - Skip this many results
    ///
    /// # Returns
    /// Search results with data and pagination info
    fn search(
        &self,
        game_id: u32,
        search_filter: &str,
        page_size: usize,
        index: usize,
    ) -> impl std::future::Future<Output = Result<SearchResults, CurseForgeError>> + Send;

    /// Get all dependencies for a mod file
    ///
    /// # Arguments
    /// * `mod_id` - Mod ID
    /// * `file_id` - File ID
    ///
    /// # Returns
    /// Mods and files that this file depends on
    fn get_dependencies(
        &self,
        mod_id: u32,
        file_id: u32,
    ) -> impl std::future::Future<Output = Result<ModDependencies, CurseForgeError>> + Send;

    /// Download a file with hash verification
    ///
    /// # Arguments
    /// * `url` - Download URL
    /// * `expected_hashes` - Expected MD5 and SHA1 hashes
    ///
    /// # Returns
    /// Downloaded file bytes
    fn download_file(
        &self,
        url: &str,
        expected_hashes: &[FileHash],
    ) -> impl std::future::Future<Output = Result<Vec<u8>, CurseForgeError>> + Send;
}

/// Live CurseForge API client (production)
pub struct LiveCurseForgeClient {
    networking: Arc<NetworkingManager>,
    base_url: String,
    api_key: String,
}

impl LiveCurseForgeClient {
    /// Create new live CurseForge client
    ///
    /// # Arguments
    /// * `networking` - Network manager for HTTP requests
    /// * `api_key` - CurseForge API key
    pub fn new(networking: Arc<NetworkingManager>, api_key: String) -> Self {
        Self {
            networking,
            base_url: "https://api.curseforge.com".to_string(),
            api_key,
        }
    }

    /// Create client with custom base URL (for staging/testing)
    pub fn with_base_url(
        networking: Arc<NetworkingManager>,
        api_key: String,
        base_url: String,
    ) -> Self {
        Self {
            networking,
            base_url,
            api_key,
        }
    }
}

impl CurseForgeClient for LiveCurseForgeClient {
    async fn search(
        &self,
        game_id: u32,
        search_filter: &str,
        page_size: usize,
        index: usize,
    ) -> Result<SearchResults, CurseForgeError> {
        if page_size > 50 {
            return Err(CurseForgeError::InvalidSearchParams {
                message: "pageSize must be <= 50".to_string(),
            });
        }

        if index + page_size > 10000 {
            return Err(CurseForgeError::InvalidSearchParams {
                message: "index + pageSize must be <= 10,000".to_string(),
            });
        }

        let url = format!(
            "{}/v1/mods/search?gameId={}&searchFilter={}&pageSize={}&index={}",
            self.base_url,
            game_id,
            urlencoding::encode(search_filter),
            page_size,
            index
        );

        let client = self.networking.client();
        let response = client
            .get(&url)
            .header("x-api-key", &self.api_key)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(CurseForgeError::RequestFailed {
                source: response.error_for_status().unwrap_err(),
            });
        }

        let bytes = response.bytes().await?;
        let results: SearchResults = serde_json::from_slice(&bytes)?;
        Ok(results)
    }

    async fn get_dependencies(
        &self,
        mod_id: u32,
        file_id: u32,
    ) -> Result<ModDependencies, CurseForgeError> {
        // First, get the file to extract dependencies
        let file_url = format!("{}/v1/mods/{}/files/{}", self.base_url, mod_id, file_id);

        let client = self.networking.client();
        let response = client
            .get(&file_url)
            .header("x-api-key", &self.api_key)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(CurseForgeError::FileNotFound { file_id });
        }

        let bytes = response.bytes().await?;

        #[derive(Deserialize)]
        struct FileResponse {
            data: FileInfo,
        }

        let file_response: FileResponse = serde_json::from_slice(&bytes)?;
        let file = file_response.data;

        // Extract mod IDs from dependencies
        let dep_mod_ids: Vec<u32> = file.dependencies.iter().map(|dep| dep.mod_id).collect();

        if dep_mod_ids.is_empty() {
            return Ok(ModDependencies {
                mods: vec![],
                files: vec![],
            });
        }

        // Fetch dependency mods using batch endpoint
        let mods_url = format!("{}/v1/mods", self.base_url);

        #[derive(Serialize)]
        struct ModsRequest {
            #[serde(rename = "modIds")]
            mod_ids: Vec<u32>,
        }

        let response = client
            .post(&mods_url)
            .header("x-api-key", &self.api_key)
            .json(&ModsRequest {
                mod_ids: dep_mod_ids,
            })
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(CurseForgeError::RequestFailed {
                source: response.error_for_status().unwrap_err(),
            });
        }

        let bytes = response.bytes().await?;

        #[derive(Deserialize)]
        struct ModsResponse {
            data: Vec<SearchResult>,
        }

        let mods_response: ModsResponse = serde_json::from_slice(&bytes)?;

        Ok(ModDependencies {
            mods: mods_response.data,
            files: vec![file],
        })
    }

    async fn download_file(
        &self,
        url: &str,
        expected_hashes: &[FileHash],
    ) -> Result<Vec<u8>, CurseForgeError> {
        // Direct download without rate limiting (CDN)
        let client = self.networking.client();
        let response = client.get(url).send().await?;

        if !response.status().is_success() {
            return Err(CurseForgeError::RequestFailed {
                source: response.error_for_status().unwrap_err(),
            });
        }

        let bytes = response.bytes().await?.to_vec();

        // Verify hashes (MD5 preferred, algo: 2)
        if let Some(md5_hash) = expected_hashes.iter().find(|h| h.algo == 2) {
            let hash = format!("{:x}", md5::compute(&bytes));

            if hash != md5_hash.value {
                return Err(CurseForgeError::InvalidSearchParams {
                    message: format!("Hash mismatch: expected {}, got {}", md5_hash.value, hash),
                });
            }
        }

        Ok(bytes)
    }
}

/// Mock CurseForge API client (testing)
pub struct MockCurseForgeClient {
    search_responses: Arc<Mutex<HashMap<String, Result<SearchResults, String>>>>,
    dependency_responses: Arc<Mutex<HashMap<String, Result<ModDependencies, String>>>>,
    download_responses: Arc<Mutex<HashMap<String, Result<Vec<u8>, String>>>>,
}

impl MockCurseForgeClient {
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
        search_filter: String,
        result: Result<SearchResults, String>,
    ) -> Self {
        self.search_responses
            .lock()
            .await
            .insert(search_filter, result);
        self
    }

    /// Add mock dependency response
    pub async fn with_dependency_result(
        self,
        key: String,
        result: Result<ModDependencies, String>,
    ) -> Self {
        self.dependency_responses.lock().await.insert(key, result);
        self
    }

    /// Add mock download response
    pub async fn with_download_result(self, url: String, result: Result<Vec<u8>, String>) -> Self {
        self.download_responses.lock().await.insert(url, result);
        self
    }
}

impl CurseForgeClient for MockCurseForgeClient {
    async fn search(
        &self,
        _game_id: u32,
        search_filter: &str,
        _page_size: usize,
        _index: usize,
    ) -> Result<SearchResults, CurseForgeError> {
        let responses = self.search_responses.lock().await;

        match responses.get(search_filter) {
            Some(Ok(results)) => Ok(results.clone()),
            Some(Err(err)) => Err(CurseForgeError::InvalidSearchParams {
                message: err.clone(),
            }),
            None => Err(CurseForgeError::InvalidSearchParams {
                message: format!("No mock response for search: {}", search_filter),
            }),
        }
    }

    async fn get_dependencies(
        &self,
        mod_id: u32,
        file_id: u32,
    ) -> Result<ModDependencies, CurseForgeError> {
        let key = format!("{}:{}", mod_id, file_id);
        let responses = self.dependency_responses.lock().await;

        match responses.get(&key) {
            Some(Ok(deps)) => Ok(deps.clone()),
            Some(Err(err)) => Err(CurseForgeError::ModNotFound {
                mod_id: err.parse().unwrap_or(0),
            }),
            None => Err(CurseForgeError::ModNotFound { mod_id }),
        }
    }

    async fn download_file(
        &self,
        url: &str,
        _expected_hashes: &[FileHash],
    ) -> Result<Vec<u8>, CurseForgeError> {
        let responses = self.download_responses.lock().await;

        match responses.get(url) {
            Some(Ok(bytes)) => Ok(bytes.clone()),
            Some(Err(err)) => Err(CurseForgeError::InvalidSearchParams {
                message: err.clone(),
            }),
            None => Err(CurseForgeError::InvalidSearchParams {
                message: format!("No mock response for URL: {}", url),
            }),
        }
    }
}

impl Default for MockCurseForgeClient {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    include!("curseforge.test.rs");
}
