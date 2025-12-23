use clap::ValueEnum;
use serde::Deserialize;
use std::str::FromStr;
use std::time::Duration;
use thiserror::Error;

use crate::impl_fromstr_for_value_enum;

/// Networking configuration primitives for HTTP clients and request handling

/// HTTP client configuration strategy
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HttpClientStrategy {
    /// Basic client with default settings
    #[value(alias = "basic")]
    Basic,
    /// Connection pooling enabled for performance
    #[value(alias = "pooled")]
    ConnectionPooling,
    /// Enhanced with retry logic and backoff
    #[value(alias = "resilient")]
    Resilient,
    /// Adaptive configuration based on system resources
    #[value(alias = "adaptive")]
    Adaptive,
}

impl_fromstr_for_value_enum!(HttpClientStrategy, "HTTP client strategy");

impl Default for HttpClientStrategy {
    fn default() -> Self {
        Self::ConnectionPooling
    }
}

/// Request timeout strategy for different operations
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimeoutStrategy {
    /// Fixed timeout for all operations
    #[value(alias = "fixed")]
    Fixed,
    /// Different timeouts based on operation type
    #[value(alias = "adaptive")]
    Adaptive,
    /// Progressive timeout with retries
    #[value(alias = "progressive")]
    Progressive,
}

impl_fromstr_for_value_enum!(TimeoutStrategy, "timeout strategy");

impl Default for TimeoutStrategy {
    fn default() -> Self {
        Self::Adaptive
    }
}

/// Concurrency control method for parallel requests
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConcurrencyMethod {
    /// Fixed number of concurrent requests
    #[value(alias = "fixed")]
    Fixed,
    /// Based on system resources (CPU/memory)
    #[value(alias = "resource")]
    ResourceBased,
    /// Adaptive based on response times and error rates
    #[value(alias = "adaptive")]
    Adaptive,
    /// Semaphore-controlled with backpressure
    #[value(alias = "semaphore")]
    Semaphore,
}

impl_fromstr_for_value_enum!(ConcurrencyMethod, "concurrency method");

impl Default for ConcurrencyMethod {
    fn default() -> Self {
        Self::Semaphore
    }
}

/// Request tracing level for debugging and monitoring
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestTracingLevel {
    /// No request tracing
    #[value(alias = "none")]
    None,
    /// Basic request/response logging
    #[value(alias = "basic")]
    Basic,
    /// Detailed timing and header information
    #[value(alias = "detailed")]
    Detailed,
    /// Full request/response body logging (debug only)
    #[value(alias = "full")]
    Full,
}

impl_fromstr_for_value_enum!(RequestTracingLevel, "request tracing level");

impl Default for RequestTracingLevel {
    fn default() -> Self {
        Self::Basic
    }
}

/// Networking capability flags for feature detection
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkingCapabilities {
    /// HTTP/1.1 support
    pub http1: bool,
    /// HTTP/2 support
    pub http2: bool,
    /// TLS/SSL support
    pub tls: bool,
    /// Connection pooling available
    pub connection_pooling: bool,
    /// Request compression support
    pub compression: bool,
    /// Proxy support
    pub proxy_support: bool,
    /// Custom headers support
    pub custom_headers: bool,
    /// Authentication schemes supported
    pub authentication: bool,
}

impl Default for NetworkingCapabilities {
    fn default() -> Self {
        Self {
            http1: true,
            http2: true,
            tls: true,
            connection_pooling: true,
            compression: true,
            proxy_support: true,
            custom_headers: true,
            authentication: true,
        }
    }
}

/// Network configuration for different API endpoints
#[derive(Debug, Clone)]
pub struct EndpointConfig {
    /// Base URL for the API
    pub base_url: String,
    /// Default timeout for requests
    pub timeout: Duration,
    /// Required headers
    pub headers: Vec<(String, String)>,
    /// Rate limiting information
    pub rate_limit: Option<RateLimit>,
    /// Authentication configuration
    pub auth: Option<AuthConfig>,
}

/// Rate limiting configuration
#[derive(Debug, Clone)]
pub struct RateLimit {
    /// Requests per second allowed
    pub requests_per_second: f32,
    /// Burst capacity
    pub burst_capacity: u32,
    /// Backoff strategy when rate limited
    pub backoff_strategy: BackoffStrategy,
}

/// Authentication configuration
#[derive(Debug, Clone)]
pub struct AuthConfig {
    /// Authentication type
    pub auth_type: AuthType,
    /// API key or token
    pub credentials: String,
    /// Header name for authentication
    pub header_name: String,
}

/// Authentication types supported
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthType {
    /// Bearer token authentication
    Bearer,
    /// API key authentication
    ApiKey,
    /// Basic authentication
    Basic,
    /// Custom header authentication
    Custom,
}

/// Backoff strategy for retries and rate limiting
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackoffStrategy {
    /// Linear backoff (fixed intervals)
    #[value(alias = "linear")]
    Linear,
    /// Exponential backoff
    #[value(alias = "exponential")]
    Exponential,
    /// Fibonacci sequence backoff
    #[value(alias = "fibonacci")]
    Fibonacci,
    /// Jittered exponential backoff
    #[value(alias = "jittered")]
    Jittered,
}

impl_fromstr_for_value_enum!(BackoffStrategy, "backoff strategy");

impl Default for BackoffStrategy {
    fn default() -> Self {
        Self::Exponential
    }
}

/// Networking configuration summary
#[derive(Debug, Clone)]
pub struct NetworkingConfig {
    /// HTTP client strategy
    pub client_strategy: HttpClientStrategy,
    /// Timeout configuration
    pub timeout_strategy: TimeoutStrategy,
    /// Concurrency control method
    pub concurrency_method: ConcurrencyMethod,
    /// Request tracing level
    pub tracing_level: RequestTracingLevel,
    /// Maximum concurrent requests
    pub max_concurrent_requests: Option<u32>,
    /// Default request timeout
    pub default_timeout: Duration,
    /// Enable request/response compression
    pub enable_compression: bool,
    /// User agent string
    pub user_agent: String,
    /// Endpoint configurations
    pub endpoints: Vec<EndpointConfig>,
}

impl Default for NetworkingConfig {
    fn default() -> Self {
        Self {
            client_strategy: HttpClientStrategy::default(),
            timeout_strategy: TimeoutStrategy::default(),
            concurrency_method: ConcurrencyMethod::default(),
            tracing_level: RequestTracingLevel::default(),
            max_concurrent_requests: None,
            default_timeout: Duration::from_secs(30),
            enable_compression: true,
            user_agent: "empack/0.1.0".to_string(),
            endpoints: Vec::new(),
        }
    }
}

/// Request metrics for monitoring and optimization
#[derive(Debug, Clone)]
pub struct RequestMetrics {
    /// Total requests made
    pub total_requests: u64,
    /// Successful requests
    pub successful_requests: u64,
    /// Failed requests
    pub failed_requests: u64,
    /// Average response time
    pub average_response_time: Duration,
    /// Current concurrent requests
    pub current_concurrent: u32,
    /// Peak concurrent requests
    pub peak_concurrent: u32,
    /// Total bytes transferred
    pub bytes_transferred: u64,
}

impl Default for RequestMetrics {
    fn default() -> Self {
        Self {
            total_requests: 0,
            successful_requests: 0,
            failed_requests: 0,
            average_response_time: Duration::from_millis(0),
            current_concurrent: 0,
            peak_concurrent: 0,
            bytes_transferred: 0,
        }
    }
}

impl RequestMetrics {
    /// Calculate success rate as percentage
    pub fn success_rate(&self) -> f32 {
        if self.total_requests == 0 {
            return 0.0;
        }
        (self.successful_requests as f32 / self.total_requests as f32) * 100.0
    }

    /// Format bytes transferred for display
    pub fn format_bytes_transferred(&self) -> String {
        const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
        let mut size = self.bytes_transferred as f64;
        let mut unit_index = 0;

        while size >= 1024.0 && unit_index < UNITS.len() - 1 {
            size /= 1024.0;
            unit_index += 1;
        }

        if unit_index == 0 {
            format!("{} {}", self.bytes_transferred, UNITS[unit_index])
        } else {
            format!("{:.1} {}", size, UNITS[unit_index])
        }
    }

    /// Get performance summary
    pub fn performance_summary(&self) -> String {
        format!(
            "{} requests ({:.1}% success), avg {}ms, {} transferred",
            self.total_requests,
            self.success_rate(),
            self.average_response_time.as_millis(),
            self.format_bytes_transferred()
        )
    }
}

/// Networking error categories
#[derive(Debug, Error)]
pub enum NetworkingConfigError {
    #[error("Invalid timeout configuration: {reason}")]
    InvalidTimeout { reason: String },

    #[error("Invalid concurrency setting: {reason}")]
    InvalidConcurrency { reason: String },

    #[error("Endpoint configuration error: {endpoint} - {reason}")]
    EndpointConfigError { endpoint: String, reason: String },

    #[error("Authentication configuration error: {reason}")]
    AuthConfigError { reason: String },

    #[error("Rate limit configuration error: {reason}")]
    RateLimitConfigError { reason: String },
}

/// Convert networking module errors to shared primitive errors
impl From<crate::networking::NetworkingError> for NetworkingConfigError {
    fn from(err: crate::networking::NetworkingError) -> Self {
        match err {
            crate::networking::NetworkingError::InvalidJobCount { count } => {
                Self::InvalidConcurrency {
                    reason: format!("Invalid job count: {}", count),
                }
            }
            crate::networking::NetworkingError::NoModsProvided => Self::InvalidConcurrency {
                reason: "No items provided for processing".to_string(),
            },
            _ => Self::InvalidConcurrency {
                reason: "Networking error occurred".to_string(),
            },
        }
    }
}

/// Common endpoint configurations for Minecraft project platforms
pub mod endpoints {
    use super::*;

    /// Modrinth API endpoint configuration
    pub fn modrinth() -> EndpointConfig {
        EndpointConfig {
            base_url: "https://api.modrinth.com/v2".to_string(),
            timeout: Duration::from_secs(30),
            headers: vec![
                ("User-Agent".to_string(), "empack/0.1.0".to_string()),
                ("Accept".to_string(), "application/json".to_string()),
            ],
            rate_limit: Some(RateLimit {
                requests_per_second: 10.0,
                burst_capacity: 20,
                backoff_strategy: BackoffStrategy::Exponential,
            }),
            auth: None,
        }
    }

    /// CurseForge API endpoint configuration
    pub fn curseforge() -> EndpointConfig {
        EndpointConfig {
            base_url: "https://api.curseforge.com/v1".to_string(),
            timeout: Duration::from_secs(30),
            headers: vec![
                ("User-Agent".to_string(), "empack/0.1.0".to_string()),
                ("Accept".to_string(), "application/json".to_string()),
            ],
            rate_limit: Some(RateLimit {
                requests_per_second: 5.0,
                burst_capacity: 10,
                backoff_strategy: BackoffStrategy::Exponential,
            }),
            auth: None, // Will be configured with API key when available
        }
    }
}

/// Networking adapter functions for bridging with networking module
pub mod adapter {
    use super::*;

    /// Convert networking module config to shared primitive config
    pub fn networking_config_to_primitive(
        config: &crate::networking::NetworkingConfig,
    ) -> NetworkingConfig {
        NetworkingConfig {
            client_strategy: HttpClientStrategy::ConnectionPooling,
            timeout_strategy: TimeoutStrategy::Fixed,
            concurrency_method: ConcurrencyMethod::Semaphore,
            tracing_level: if config.trace_requests {
                RequestTracingLevel::Basic
            } else {
                RequestTracingLevel::None
            },
            max_concurrent_requests: config.max_jobs,
            default_timeout: Duration::from_secs(config.timeout_seconds),
            enable_compression: true,
            user_agent: "empack/0.1.0".to_string(),
            endpoints: vec![endpoints::modrinth(), endpoints::curseforge()],
        }
    }

    /// Create request metrics from networking manager state
    pub fn create_request_metrics(
        total_requests: u64,
        successful_requests: u64,
        current_concurrent: u32,
    ) -> RequestMetrics {
        RequestMetrics {
            total_requests,
            successful_requests,
            failed_requests: total_requests.saturating_sub(successful_requests),
            average_response_time: Duration::from_millis(0), // Would need actual timing data
            current_concurrent,
            peak_concurrent: current_concurrent,
            bytes_transferred: 0, // Would need actual transfer tracking
        }
    }

    /// Get networking capabilities based on available features
    pub fn detect_networking_capabilities() -> NetworkingCapabilities {
        NetworkingCapabilities {
            http1: true,
            http2: true, // reqwest has built-in HTTP/2 support
            tls: true,
            connection_pooling: true,
            compression: true, // reqwest supports gzip compression
            proxy_support: true,
            custom_headers: true,
            authentication: true,
        }
    }
}
