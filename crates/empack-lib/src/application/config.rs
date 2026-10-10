//! Configuration precedence: defaults, .env, env vars, CLI args.

use crate::primitives::*;
use clap::Parser;
use serde::Deserialize;
use std::path::PathBuf;

pub mod defaults {
    pub const LOG_LEVEL: &str = "0"; // Error-only logging by default
    pub const LOG_FORMAT: &str = "text";
    pub const NET_TIMEOUT: &str = "300";
    pub const CPU_PARALLELS: &str = "2";
    pub const LOG_OUTPUT: &str = "stderr";
    pub const TTY_CAPS_DETECT_INTENT: &str = "auto";
    pub const CURSEFORGE_API_CLIENT_KEY: &str =
        "$2a$10$78GooA4YTCKFQI9vgZ1oEeVM.jNyeNKSIFUhFkwiA0L/Uwv19BFAq";
}

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

    pub fn curseforge_api_client_key() -> Option<String> {
        Some(defaults::CURSEFORGE_API_CLIENT_KEY.to_string())
    }
}

#[derive(Debug, Clone, Parser, Deserialize)]
pub struct AppConfig {
    /// Working directory for modpack operations
    #[arg(short, long, env = "EMPACK_WORKDIR")]
    #[serde(default)]
    pub workdir: Option<PathBuf>,

    /// Durable operation and recovery storage, separate from disposable caches
    #[arg(long, env = "EMPACK_STATE_DIR", global = true)]
    #[serde(default)]
    pub state_dir: Option<PathBuf>,

    /// Disposable cache root; previews only inspect existing objects
    #[arg(long, env = "EMPACK_CACHE_DIR", global = true)]
    #[serde(default)]
    pub cache_dir: Option<PathBuf>,

    /// Maximum concurrent engine workers
    #[arg(short = 'j', long, env = "EMPACK_CPU_JOBS", default_value = defaults::CPU_PARALLELS)]
    #[serde(default = "default_fns::cpu_parallels")]
    pub cpu_jobs: usize,

    /// Cumulative deadline in seconds for each catalog or payload acquisition phase
    #[arg(short, long, env = "EMPACK_NET_TIMEOUT", default_value = defaults::NET_TIMEOUT)]
    #[serde(default = "default_fns::net_timeout")]
    pub net_timeout: u64,

    /// Reserved Modrinth client identifier (currently unused)
    #[arg(long, env = "EMPACK_ID_MODRINTH", hide_env_values = true)]
    #[serde(default)]
    pub modrinth_api_client_id: Option<String>,

    /// Reserved Modrinth client key (currently unused)
    #[arg(long, env = "EMPACK_KEY_MODRINTH", hide_env_values = true)]
    #[serde(default)]
    pub modrinth_api_client_key: Option<String>,

    /// CurseForge API Client Key
    #[arg(long, env = "EMPACK_KEY_CURSEFORGE",
        hide_default_value = true,
        hide_env_values = true, default_value = defaults::CURSEFORGE_API_CLIENT_KEY, hide_env_values = true)]
    #[serde(default = "default_fns::curseforge_api_client_key")]
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

    /// Run without prompts; unresolved content choices still fail (global non-interactive mode)
    #[arg(
        short = 'y',
        long,
        global = true,
        env = "EMPACK_YES",
        help = "Run without prompts; unresolved content choices still fail"
    )]
    #[serde(default)]
    pub yes: bool,

    /// Preview changes without modifying the project (global dry-run mode)
    #[arg(
        long,
        global = true,
        env = "EMPACK_DRY_RUN",
        help = "Preview changes without modifying the project"
    )]
    #[serde(default)]
    pub dry_run: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            workdir: None,
            state_dir: None,
            cache_dir: None,
            cpu_jobs: default_fns::cpu_parallels(),
            net_timeout: default_fns::net_timeout(),
            modrinth_api_client_id: None,
            modrinth_api_client_key: None,
            curseforge_api_client_key: default_fns::curseforge_api_client_key(),
            log_level: default_fns::log_level(),
            log_format: default_fns::log_format(),
            log_output: default_fns::log_output(),
            color: default_fns::tty_caps_detect_intent(),
            yes: false,
            dry_run: false,
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
        if other.state_dir.is_some() {
            self.state_dir = other.state_dir;
        }
        if other.cache_dir.is_some() {
            self.cache_dir = other.cache_dir;
        }
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

        if other.log_level != default_fns::log_level() {
            self.log_level = other.log_level;
        }
        if other.net_timeout != default_fns::net_timeout() {
            self.net_timeout = other.net_timeout;
        }
        if other.cpu_jobs != default_fns::cpu_parallels() {
            self.cpu_jobs = other.cpu_jobs;
        }

        if other.yes {
            self.yes = other.yes;
        }
        if other.dry_run {
            self.dry_run = other.dry_run;
        }

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
        // Resolve configuration inputs before constructing the command session at config
        // validation time, so std::env::current_dir() is the only option here.
        // Errors are properly typed as ConfigError::CurrentDirError.
        if self.workdir.is_none() {
            self.workdir = Some(
                std::env::current_dir().map_err(|e| ConfigError::CurrentDirError { source: e })?,
            );
        }
        if let Some(workdir) = self.workdir.as_mut()
            && workdir.is_relative()
        {
            *workdir = std::env::current_dir()
                .map_err(|source| ConfigError::CurrentDirError { source })?
                .join(&*workdir);
        }

        Ok(())
    }
}
