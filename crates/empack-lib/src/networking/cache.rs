use reqwest::{Client, Response, StatusCode};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::RwLock;
use tracing::{debug, trace, warn};

use super::NetworkingError;

/// Default cache TTL (Time To Live) - 5 minutes
const DEFAULT_CACHE_TTL_SECS: u64 = 300;

/// Cached HTTP response with ETag support
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedResponse {
    pub data: Vec<u8>,
    pub etag: Option<String>,
    pub expires: SystemTime,
    pub status: u16,
}

impl CachedResponse {
    /// Check if the cached response is expired
    pub fn is_expired(&self) -> bool {
        SystemTime::now() > self.expires
    }

    /// Extend the TTL of this cached response
    pub fn extend_ttl(&mut self, ttl: Duration) {
        self.expires = SystemTime::now() + ttl;
    }
}

/// HTTP cache with ETag support and disk persistence
pub struct HttpCache {
    cache: Arc<RwLock<HashMap<String, CachedResponse>>>,
    cache_dir: PathBuf,
    default_ttl: Duration,
}

impl HttpCache {
    /// Create a new HTTP cache
    pub fn new(cache_dir: PathBuf) -> Self {
        Self {
            cache: Arc::new(RwLock::new(HashMap::new())),
            cache_dir,
            default_ttl: Duration::from_secs(DEFAULT_CACHE_TTL_SECS),
        }
    }

    /// Create a new HTTP cache with custom TTL
    pub fn with_ttl(cache_dir: PathBuf, ttl: Duration) -> Self {
        Self {
            cache: Arc::new(RwLock::new(HashMap::new())),
            cache_dir,
            default_ttl: ttl,
        }
    }

    /// Get cache directory path
    pub fn cache_dir(&self) -> &PathBuf {
        &self.cache_dir
    }

    pub fn default_ttl(&self) -> Duration {
        self.default_ttl
    }

    /// Load cache from disk
    pub async fn load_from_disk(&self) -> Result<(), NetworkingError> {
        // Ensure cache directory exists
        tokio::fs::create_dir_all(&self.cache_dir)
            .await
            .map_err(|e| NetworkingError::CacheError {
                message: format!("Failed to create cache directory: {}", e),
            })?;

        let cache_file = self.cache_dir.join("http_cache.json");
        if !cache_file.exists() {
            trace!("No cache file found, starting with empty cache");
            return Ok(());
        }

        let data = tokio::fs::read_to_string(&cache_file).await.map_err(|e| {
            NetworkingError::CacheError {
                message: format!("Failed to read cache file: {}", e),
            }
        })?;

        let loaded_cache: HashMap<String, CachedResponse> =
            serde_json::from_str(&data).map_err(|e| NetworkingError::CacheError {
                message: format!("Failed to parse cache file: {}", e),
            })?;

        // Load into memory, filtering out expired entries
        let mut cache = self.cache.write().await;
        let now = SystemTime::now();
        let mut valid_count = 0;
        let mut expired_count = 0;

        for (url, entry) in loaded_cache {
            if entry.expires > now {
                cache.insert(url, entry);
                valid_count += 1;
            } else {
                expired_count += 1;
            }
        }

        debug!(
            "Loaded cache from disk: {} valid entries, {} expired entries removed",
            valid_count, expired_count
        );

        Ok(())
    }

    /// Save cache to disk
    pub async fn save_to_disk(&self) -> Result<(), NetworkingError> {
        let cache = self.cache.read().await;

        // Ensure cache directory exists
        tokio::fs::create_dir_all(&self.cache_dir)
            .await
            .map_err(|e| NetworkingError::CacheError {
                message: format!("Failed to create cache directory: {}", e),
            })?;

        let cache_file = self.cache_dir.join("http_cache.json");
        let data =
            serde_json::to_string_pretty(&*cache).map_err(|e| NetworkingError::CacheError {
                message: format!("Failed to serialize cache: {}", e),
            })?;

        tokio::fs::write(&cache_file, data)
            .await
            .map_err(|e| NetworkingError::CacheError {
                message: format!("Failed to write cache file: {}", e),
            })?;

        debug!("Saved {} cache entries to disk", cache.len());
        Ok(())
    }

    /// Get cached response for a URL
    pub async fn get(&self, url: &str) -> Option<CachedResponse> {
        let cache = self.cache.read().await;
        cache.get(url).cloned()
    }

    /// Store a response in the cache
    pub async fn put(&self, url: String, response: CachedResponse) {
        {
            let mut cache = self.cache.write().await;
            cache.insert(url, response);
        }
        self.persist_best_effort("put").await;
    }

    /// Remove a cached entry
    pub async fn remove(&self, url: &str) {
        {
            let mut cache = self.cache.write().await;
            cache.remove(url);
        }
        self.persist_best_effort("remove").await;
    }

    /// Clear all cached entries
    pub async fn clear(&self) {
        {
            let mut cache = self.cache.write().await;
            cache.clear();
        }
        self.persist_best_effort("clear").await;
    }

    /// Get the number of cached entries
    pub async fn len(&self) -> usize {
        let cache = self.cache.read().await;
        cache.len()
    }

    /// Check if the cache is empty
    pub async fn is_empty(&self) -> bool {
        let cache = self.cache.read().await;
        cache.is_empty()
    }

    /// Make a cached GET request with ETag revalidation
    pub async fn get_with_etag(
        &self,
        client: &Client,
        url: &str,
    ) -> Result<CachedResponse, NetworkingError> {
        // Check if we have a cached entry
        if let Some(cached) = self.get(url).await {
            if !cached.is_expired() {
                return Ok(cached);
            }

            if let Some(ref etag) = cached.etag {
                trace!("Cache expired, attempting ETag revalidation for: {}", url);

                let response = client.get(url).header("If-None-Match", etag).send().await?;

                if response.status() == StatusCode::NOT_MODIFIED {
                    trace!("ETag revalidation successful (304), extending TTL");
                    let mut updated = cached.clone();
                    updated.extend_ttl(self.default_ttl);
                    self.put(url.to_string(), updated.clone()).await;
                    return Ok(updated);
                }

                // ETag changed or server doesn't support conditional requests
                // Fall through to normal request handling
                trace!("ETag changed or conditional request not supported");
                return self.process_and_cache_response(url, response).await;
            }
        }

        // Cache miss or no ETag - make normal request
        trace!("Cache miss for URL: {}", url);
        let response = client.get(url).send().await?;
        self.process_and_cache_response(url, response).await
    }

    /// Process a response and cache it
    async fn process_and_cache_response(
        &self,
        url: &str,
        response: Response,
    ) -> Result<CachedResponse, NetworkingError> {
        let status = response.status().as_u16();
        let etag = response
            .headers()
            .get("etag")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        let data = response.bytes().await?.to_vec();

        let cached_response = CachedResponse {
            data,
            etag,
            expires: SystemTime::now() + self.default_ttl,
            status,
        };

        if (200..300).contains(&status) {
            self.put(url.to_string(), cached_response.clone()).await;
            trace!("Cached response for URL: {}", url);
        } else {
            warn!(
                "Not caching non-success response (status {}): {}",
                status, url
            );
        }

        Ok(cached_response)
    }

    async fn persist_best_effort(&self, action: &str) {
        if let Err(error) = self.save_to_disk().await {
            warn!(
                error = %error,
                action,
                cache_dir = %self.cache_dir.display(),
                "failed to persist HTTP cache update"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    include!("cache.test.rs");
}
