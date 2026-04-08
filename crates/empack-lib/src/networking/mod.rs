use reqwest::Client;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::Semaphore;
use tracing::trace;

use crate::platform::SystemResources;

pub mod cache;
pub mod rate_budget;
pub mod rate_limit;

/// Networking errors for project resolution and API communication
#[derive(Debug, Error)]
pub enum NetworkingError {
    #[error("HTTP request failed: {source}")]
    RequestFailed {
        #[from]
        source: reqwest::Error,
    },

    #[error("Platform resource detection failed: {source}")]
    PlatformError {
        #[from]
        source: crate::platform::PlatformError,
    },

    #[error("Task join error: {source}")]
    TaskJoinError {
        #[from]
        source: tokio::task::JoinError,
    },

    #[error("Semaphore acquire error: {source}")]
    SemaphoreError {
        #[from]
        source: tokio::sync::AcquireError,
    },

    #[error("Invalid job count: {count} (must be > 0)")]
    InvalidJobCount { count: u32 },

    #[error("No mods provided for resolution")]
    NoModsProvided,

    #[error("Cache operation failed: {message}")]
    CacheError { message: String },

    #[error("Rate limit error: {message}")]
    RateLimitError { message: String },
}

/// Resource-aware networking configuration
#[derive(Debug, Clone)]
pub struct NetworkingConfig {
    pub max_jobs: Option<u32>,
    pub timeout_seconds: u64,
    pub trace_requests: bool,
}

impl Default for NetworkingConfig {
    fn default() -> Self {
        Self {
            max_jobs: None,
            timeout_seconds: 30,
            trace_requests: false,
        }
    }
}

/// Networking manager for project resolution
pub struct NetworkingManager {
    client: Client,
    config: NetworkingConfig,
    semaphore: Arc<Semaphore>,
    optimal_jobs: u32,
}

impl NetworkingManager {
    /// Create networking manager with job calculation
    pub async fn new(config: NetworkingConfig) -> Result<Self, NetworkingError> {
        trace!("Initializing networking manager");

        let resources = SystemResources::detect()?;
        let optimal_jobs = resources.calculate_optimal_jobs(config.max_jobs);

        trace!(
            "System resources: {} CPU cores, {:.2} memory pressure, optimal jobs: {}",
            resources.cpu_cores, resources.memory_pressure, optimal_jobs
        );

        if optimal_jobs == 0 {
            return Err(NetworkingError::InvalidJobCount {
                count: optimal_jobs,
            });
        }

        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(config.timeout_seconds))
            .build()?;

        let semaphore = Arc::new(Semaphore::new(optimal_jobs as usize));

        trace!(
            "Networking manager initialized with {} concurrent jobs",
            optimal_jobs
        );

        Ok(Self {
            client,
            config,
            semaphore,
            optimal_jobs,
        })
    }

    /// Get optimal job count calculated from system resources
    pub fn optimal_jobs(&self) -> u32 {
        self.optimal_jobs
    }

    /// Resolve multiple mods concurrently with resource-aware parallelism
    pub async fn resolve_mods<T, F, Fut>(
        &self,
        mod_identifiers: Vec<T>,
        resolver_fn: F,
    ) -> Result<Vec<Result<String, NetworkingError>>, NetworkingError>
    where
        T: Send + 'static,
        F: Fn(Client, T) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<String, NetworkingError>> + Send,
    {
        if mod_identifiers.is_empty() {
            return Err(NetworkingError::NoModsProvided);
        }

        trace!(
            "Starting project resolution for {} mods",
            mod_identifiers.len()
        );

        let resolver = Arc::new(resolver_fn);
        let mut tasks = Vec::new();

        for mod_id in mod_identifiers {
            let client = self.client.clone();
            let semaphore = self.semaphore.clone();
            let resolver = resolver.clone();
            let trace_requests = self.config.trace_requests;

            let task = tokio::spawn(async move {
                let _permit = semaphore.acquire().await?;

                if trace_requests {
                    trace!("Processing project resolution task");
                }

                let result = resolver(client, mod_id).await;

                if trace_requests {
                    match &result {
                        Ok(_) => trace!("Mod resolution task completed successfully"),
                        Err(e) => trace!("Mod resolution task failed: {}", e),
                    }
                }

                Ok(result)
            });

            tasks.push(task);
        }

        let mut results = Vec::new();
        for task in tasks {
            let task_result = task.await?;
            match task_result {
                Ok(resolver_result) => results.push(resolver_result),
                Err(e) => results.push(Err(e)),
            }
        }

        trace!("Completed project resolution for {} mods", results.len());
        Ok(results)
    }

    /// Get HTTP client for manual requests
    pub fn client(&self) -> &Client {
        &self.client
    }
}

#[cfg(test)]
mod tests {
    include!("mod.test.rs");
}
