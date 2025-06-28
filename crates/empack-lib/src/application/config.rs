//! Application configuration management
//!
//! Handles config loading, validation, and environment variable processing
//! following the precedence: defaults -> .env -> env vars -> CLI args.

use crate::primitives::*;
use clap::Parser;
use serde::Deserialize;
use std::path::PathBuf;

/// Default configuration values
pub mod defaults {
    pub const LOG_LEVEL: &str = "0"; // Error-only logging by default
    pub const LOG_FORMAT: &str = "text";
    pub const NET_TIMEOUT: &str = "30";
    pub const CPU_PARALLELS: &str = "2";
    pub const LOG_OUTPUT: &str = "stderr";
    pub const TTY_CAPS_DETECT_INTENT: &str = "auto";
}

/// Default value functions for configuration fields
mod default_fns {
    use super::*;
    use crate::primitives::{LogFormat, LogOutput, TerminalCapsDetectIntent};

    pub fn log_level() -> u8 {
        defaults::LOG_LEVEL.parse().unwrap()
    }

    pub fn log_format() -> LogFormat {
        defaults::LOG_FORMAT.parse().unwrap()
    }

    pub fn net_timeout() -> u64 {
        defaults::NET_TIMEOUT.parse().unwrap()
    }

    pub fn cpu_parallels() -> usize {
        defaults::CPU_PARALLELS.parse().unwrap()
    }

    pub fn log_output() -> LogOutput {
        defaults::LOG_OUTPUT.parse().unwrap()
    }

    pub fn tty_caps_detect_intent() -> TerminalCapsDetectIntent {
        defaults::TTY_CAPS_DETECT_INTENT.parse().unwrap()
    }
}

/// Application configuration structure
#[derive(Debug, Clone, Parser, Deserialize)]
pub struct AppConfig {
    /// Working directory for modpack operations
    #[arg(short, long, env = "EMPACK_WORKDIR")]
    #[serde(default)]
    pub workdir: Option<PathBuf>,

    /// Number of parallel API requests
    #[arg(short = 'j', long, env = "EMPACK_CPU_JOBS", default_value = defaults::CPU_PARALLELS)]
    #[serde(default = "default_fns::cpu_parallels")]
    pub cpu_jobs: usize,

    /// API timeout in seconds
    #[arg(short, long, env = "EMPACK_NET_TIMEOUT", default_value = defaults::NET_TIMEOUT)]
    #[serde(default = "default_fns::net_timeout")]
    pub net_timeout: u64,

    /// Modrinth API Client ID
    #[arg(long, env = "EMPACK_ID_MODRINTH", hide_env_values = true)]
    #[serde(default)]
    pub modrinth_api_client_id: Option<String>,

    /// Modrinth API Client Key
    #[arg(long, env = "EMPACK_KEY_MODRINTH", hide_env_values = true)]
    #[serde(default)]
    pub modrinth_api_client_key: Option<String>,

    /// CurseForge API Client Key
    #[arg(long, env = "EMPACK_KEY_CURSEFORGE", hide_env_values = true)]
    #[serde(default)]
    pub curseforge_api_client_key: Option<String>,

    /// Verbosity level (0=error, 1=warn, 2=info, 3=debug, 4=trace)
    #[arg(long, env = "EMPACK_LOG_LEVEL", default_value = defaults::LOG_LEVEL)]
    #[serde(default = "default_fns::log_level")]
    pub log_level: u8,

    /// Output format (text, json, yaml)
    #[arg(long, env = "EMPACK_LOG_FORMAT", default_value = defaults::LOG_FORMAT)]
    #[serde(default = "default_fns::log_format")]
    pub log_format: LogFormat,

    /// Log output stream (stderr, stdout)
    #[arg(long, env = "EMPACK_LOG_OUTPUT", default_value = defaults::LOG_OUTPUT)]
    #[serde(default = "default_fns::log_output")]
    pub log_output: LogOutput,

    /// Color output control (auto, always, never)
    #[arg(short, long, env = "EMPACK_COLOR", default_value = defaults::TTY_CAPS_DETECT_INTENT)]
    #[serde(default = "default_fns::tty_caps_detect_intent")]
    pub color: TerminalCapsDetectIntent,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            workdir: None,
            cpu_jobs: default_fns::cpu_parallels(),
            net_timeout: default_fns::net_timeout(),
            modrinth_api_client_id: None,
            modrinth_api_client_key: None,
            curseforge_api_client_key: None,
            log_level: default_fns::log_level(),
            log_format: default_fns::log_format(),
            log_output: default_fns::log_output(),
            color: default_fns::tty_caps_detect_intent(),
        }
    }
}

impl AppConfig {
    /// Create LoggerConfig from AppConfig and TerminalCapabilities
    pub fn to_logger_config(
        &self,
        terminal_caps: &crate::terminal::TerminalCapabilities,
    ) -> crate::primitives::LoggerConfig {
        crate::primitives::LoggerConfig {
            level: crate::primitives::LogLevel::from_verbosity(self.log_level),
            format: self.log_format,
            output: self.log_output,
            terminal_caps: terminal_caps.clone(),
        }
    }

    /// Merge this config with another, taking non-default values from other
    pub fn merge_with(mut self, other: Self) -> Self {
        // For Option fields, take other if it's Some
        if other.workdir.is_some() {
            self.workdir = other.workdir;
        }
        if other.modrinth_api_client_id.is_some() {
            self.modrinth_api_client_id = other.modrinth_api_client_id;
        }
        if other.modrinth_api_client_key.is_some() {
            self.modrinth_api_client_key = other.modrinth_api_client_key;
        }
        if other.curseforge_api_client_key.is_some() {
            self.curseforge_api_client_key = other.curseforge_api_client_key;
        }

        // For primitive fields, take other if it's not the default
        if other.log_level != default_fns::log_level() {
            self.log_level = other.log_level;
        }
        if other.net_timeout != default_fns::net_timeout() {
            self.net_timeout = other.net_timeout;
        }
        if other.cpu_jobs != default_fns::cpu_parallels() {
            self.cpu_jobs = other.cpu_jobs;
        }

        // For enums, detect if it's non-default
        if !matches!(other.log_format, LogFormat::Text) {
            self.log_format = other.log_format;
        }
        if !matches!(other.log_output, LogOutput::Stderr) {
            self.log_output = other.log_output;
        }
        if !matches!(other.color, TerminalCapsDetectIntent::Auto) {
            self.color = other.color;
        }

        self
    }

    /// Validate the final configuration
    pub fn validate(&mut self) -> Result<(), ConfigError> {
        // Resolve working directory (simple fallback only)
        if self.workdir.is_none() {
            self.workdir = Some(
                std::env::current_dir().map_err(|e| ConfigError::CurrentDirError { source: e })?,
            );
        }

        Ok(())
    }
}
