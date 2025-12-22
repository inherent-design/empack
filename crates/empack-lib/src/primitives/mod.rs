//! empack primitives - core types, errors, and coordination
//!
//! Central collection of shared types that form the foundation of empack.
//! Everything here works together: terminal caps inform logging, config
//! drives behavior, errors chain properly.

use clap::ValueEnum;
use std::str::FromStr;
use thiserror::Error;

// Shared macros and patterns
mod shared;
use shared::impl_fromstr_for_value_enum;

/// empack domain types and project resolution
pub mod empack;
pub use empack::*;

/// Terminal detection and graphics protocols
pub mod terminal;
pub use terminal::*;

/// System resource detection for job parallelism
pub mod platform;

/// Async HTTP client with concurrency limiting
pub mod networking;

/// Available log output streams
#[derive(Debug, Clone, Copy, PartialEq, serde::Deserialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum LogOutput {
    /// STDERR
    Stderr,
    /// STDOUT
    Stdout,
}

/// Log levels for structured logging
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    Error = 0,
    Warning = 1,
    Info = 2,
    Debug = 3,
    Trace = 4,
}

/// Output formats for structured logging
#[derive(Debug, Clone, Copy, PartialEq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogFormat {
    /// TEXT
    /// alias: text, txt, plain
    Text,

    /// JSON
    /// alias: json
    Json,

    /// YAML
    /// alias: yaml, yml
    Yaml,
}

// ============================================================================
// LOGGER CONFIGURATION TYPES
// ============================================================================

/// Logger configuration combining terminal capabilities with application config
#[derive(Debug, Clone)]
pub struct LoggerConfig {
    pub level: LogLevel,
    pub format: LogFormat,
    pub output: LogOutput,
    pub terminal_caps: crate::terminal::TerminalCapabilities,
}

/// Progress-aware logging context for operations that need progress tracking
#[derive(Debug, Clone)]
pub struct LogContext {
    pub operation: String,
    pub total_items: Option<u64>,
    pub current_item: Option<u64>,
}

impl LogContext {
    pub fn new(operation: &str) -> Self {
        Self {
            operation: operation.to_string(),
            total_items: None,
            current_item: None,
        }
    }

    pub fn with_progress(operation: &str, total: u64) -> Self {
        Self {
            operation: operation.to_string(),
            total_items: Some(total),
            current_item: None,
        }
    }

    pub fn set_progress(&mut self, current: u64) {
        self.current_item = Some(current);
    }
}

// ============================================================================
// STRUCTURED ERROR TYPES
// ============================================================================

/// Application configuration loading and validation errors
#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("Failed to load environment file '{file}': {source}")]
    EnvFileError {
        file: String,
        source: dotenvy::Error,
    },

    #[error("Global configuration already initialized")]
    AlreadyInitialized,

    #[error("Invalid working directory: {path}")]
    InvalidWorkDir { path: String },

    #[error("Failed to parse environment variables: {source}")]
    EnvironmentParsingFailed {
        #[from]
        source: envy::Error,
    },

    #[error("Configuration validation failed: {reason}")]
    ValidationFailed { reason: String },

    #[error("Failed to get current directory: {source}")]
    CurrentDirError {
        #[from]
        source: std::io::Error,
    },

    #[error("Failed to parse configuration value '{value}': {reason}")]
    ParseError { value: String, reason: String },
}

/// Logger initialization and operation errors
#[derive(Debug, Error)]
pub enum LoggerError {
    #[error("Failed to initialize tracing subscriber: {reason}")]
    InitializationFailed { reason: String },

    #[error("Logger already initialized")]
    AlreadyInitialized,

    #[error("Invalid log format configuration: {format:?}")]
    InvalidFormat { format: LogFormat },

    #[error("Writer initialization failed: {source}")]
    WriterError {
        #[from]
        source: std::io::Error,
    },

    #[error("Tracing subscriber build failed: {reason}")]
    SubscriberBuildFailed { reason: String },
}

impl LogLevel {
    /// Convert verbosity level from AppConfig to LogLevel
    pub fn from_verbosity(verbosity: u8) -> Self {
        match verbosity {
            0 => LogLevel::Error,
            1 => LogLevel::Warning,
            2 => LogLevel::Info,
            3 => LogLevel::Debug,
            4.. => LogLevel::Trace,
        }
    }

    /// Check if this log level should be displayed given current verbosity
    pub fn should_log(&self, current_level: LogLevel) -> bool {
        *self <= current_level
    }
}

impl ValueEnum for LogLevel {
    fn value_variants<'a>() -> &'a [Self] {
        &[
            Self::Error,
            Self::Warning,
            Self::Info,
            Self::Debug,
            Self::Trace,
        ]
    }

    fn to_possible_value(&self) -> Option<clap::builder::PossibleValue> {
        match self {
            Self::Error => Some(
                clap::builder::PossibleValue::new("error")
                    .alias("err")
                    .alias("fatal")
                    .alias("critical"),
            ),
            Self::Warning => Some(clap::builder::PossibleValue::new("warn").alias("warning")),
            Self::Info => Some(clap::builder::PossibleValue::new("info").alias("information")),
            Self::Debug => Some(clap::builder::PossibleValue::new("debug").alias("debugging")),
            Self::Trace => Some(
                clap::builder::PossibleValue::new("trace")
                    .alias("tracing")
                    .alias("verbose"),
            ),
        }
    }
}

impl ValueEnum for LogFormat {
    fn value_variants<'a>() -> &'a [Self] {
        &[Self::Text, Self::Json, Self::Yaml]
    }

    fn to_possible_value(&self) -> Option<clap::builder::PossibleValue> {
        match self {
            Self::Text => Some(
                clap::builder::PossibleValue::new("text")
                    .alias("txt")
                    .alias("plain"),
            ),
            Self::Json => Some(clap::builder::PossibleValue::new("json")),
            Self::Yaml => Some(clap::builder::PossibleValue::new("yaml").alias("yml")),
        }
    }
}

// Generate FromStr implementations for all ValueEnum types
impl_fromstr_for_value_enum!(LogLevel, "invalid log level");
impl_fromstr_for_value_enum!(LogFormat, "invalid log format");
impl_fromstr_for_value_enum!(LogOutput, "invalid log output stream");

#[cfg(test)]
mod empack_tests {
    use super::*;
    include!("empack.test.rs");
}

#[cfg(test)]
mod tests {
    include!("mod.test.rs");
}
