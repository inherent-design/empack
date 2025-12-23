use reqwest::{Client, Request, Response, StatusCode};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::{trace, warn};

use super::NetworkingError;

// Re-export ModPlatform as Platform for backward compatibility
pub use crate::primitives::ModPlatform as Platform;

/// Backoff strategy for rate limiting
#[derive(Debug, Clone)]
pub struct BackoffConfig {
    /// Initial backoff duration
    pub initial: Duration,
    /// Maximum backoff duration
    pub max: Duration,
    /// Multiplier for exponential backoff
    pub multiplier: f64,
}

impl Default for BackoffConfig {
    fn default() -> Self {
        Self {
            initial: Duration::from_secs(1),
            max: Duration::from_secs(60),
            multiplier: 2.0,
        }
    }
}

/// Rate-limited HTTP client with exponential backoff
pub struct RateLimitedClient {
    client: Client,
    platform: Platform,
    backoff_config: BackoffConfig,
    /// Current backoff state (duration for next retry)
    current_backoff: Arc<RwLock<Duration>>,
}

impl RateLimitedClient {
    /// Create a new rate-limited client for a specific platform
    pub fn new(client: Client, platform: Platform) -> Self {
        Self {
            client,
            platform,
            backoff_config: BackoffConfig::default(),
            current_backoff: Arc::new(RwLock::new(BackoffConfig::default().initial)),
        }
    }

    /// Create a new rate-limited client with custom backoff config
    pub fn with_backoff(client: Client, platform: Platform, backoff_config: BackoffConfig) -> Self {
        let initial_backoff = backoff_config.initial;
        Self {
            client,
            platform,
            backoff_config,
            current_backoff: Arc::new(RwLock::new(initial_backoff)),
        }
    }

    /// Get the platform for this client
    pub fn platform(&self) -> Platform {
        self.platform
    }

    /// Execute a request with rate limiting and backoff
    pub async fn execute(&self, request: Request) -> Result<Response, NetworkingError> {
        let mut retry_count = 0;
        const MAX_RETRIES: u32 = 5;

        loop {
            // Execute the request
            let response =
                self.client
                    .execute(request.try_clone().ok_or_else(|| {
                        NetworkingError::RateLimitError {
                            message: "Failed to clone request for retry".to_string(),
                        }
                    })?)
                    .await?;

            // Check for rate limiting (429 Too Many Requests)
            if response.status() == StatusCode::TOO_MANY_REQUESTS {
                retry_count += 1;

                if retry_count > MAX_RETRIES {
                    return Err(NetworkingError::RateLimitError {
                        message: format!(
                            "Rate limit exceeded after {} retries for platform: {:?}",
                            MAX_RETRIES, self.platform
                        ),
                    });
                }

                // Calculate backoff duration
                let backoff_duration = {
                    let mut current = self.current_backoff.write().await;
                    let duration = *current;

                    // Apply exponential backoff
                    let next = Duration::from_secs_f64(
                        duration.as_secs_f64() * self.backoff_config.multiplier,
                    );
                    *current = next.min(self.backoff_config.max);

                    duration
                };

                warn!(
                    "Rate limit hit (429) for platform {:?}, backing off for {:?} (retry {}/{})",
                    self.platform, backoff_duration, retry_count, MAX_RETRIES
                );

                // Wait before retrying
                tokio::time::sleep(backoff_duration).await;
                continue;
            }

            // Success - reset backoff
            if response.status().is_success() {
                let mut current = self.current_backoff.write().await;
                *current = self.backoff_config.initial;
                trace!("Request successful, backoff reset");
            }

            return Ok(response);
        }
    }

    /// Make a GET request with rate limiting
    pub async fn get(&self, url: &str) -> Result<Response, NetworkingError> {
        let request = self.client.get(url).build()?;
        self.execute(request).await
    }

    /// Make a POST request with rate limiting
    pub async fn post(&self, url: &str, body: Vec<u8>) -> Result<Response, NetworkingError> {
        let request = self.client.post(url).body(body).build()?;
        self.execute(request).await
    }

    /// Get the underlying HTTP client
    pub fn client(&self) -> &Client {
        &self.client
    }
}

/// Rate limiter manager for multiple platforms
pub struct RateLimiterManager {
    modrinth: RateLimitedClient,
    curseforge: RateLimitedClient,
}

impl RateLimiterManager {
    /// Create a new rate limiter manager
    pub fn new(client: Client) -> Self {
        Self {
            modrinth: RateLimitedClient::new(client.clone(), Platform::Modrinth),
            curseforge: RateLimitedClient::new(client, Platform::CurseForge),
        }
    }

    /// Create a new rate limiter manager with custom backoff config
    pub fn with_backoff(client: Client, backoff_config: BackoffConfig) -> Self {
        Self {
            modrinth: RateLimitedClient::with_backoff(
                client.clone(),
                Platform::Modrinth,
                backoff_config.clone(),
            ),
            curseforge: RateLimitedClient::with_backoff(
                client,
                Platform::CurseForge,
                backoff_config,
            ),
        }
    }

    /// Get the rate-limited client for Modrinth
    pub fn modrinth(&self) -> &RateLimitedClient {
        &self.modrinth
    }

    /// Get the rate-limited client for CurseForge
    pub fn curseforge(&self) -> &RateLimitedClient {
        &self.curseforge
    }

    /// Get the rate-limited client for a specific platform
    pub fn client_for_platform(&self, platform: Platform) -> &RateLimitedClient {
        match platform {
            Platform::Modrinth => &self.modrinth,
            Platform::CurseForge => &self.curseforge,
        }
    }
}

#[cfg(test)]
mod tests {
    include!("rate_limit.test.rs");
}
