//! Configuration loading and global state management
//!
//! Coordinates loading configuration from various sources and provides
//! global application configuration access.

use crate::primitives::ConfigError;
use std::sync::OnceLock;

use super::{cli::CliConfig, config::AppConfig, env::EnvironmentConfig};

// Global configuration available throughout the application
static GLOBAL_CONFIG: OnceLock<AppConfig> = OnceLock::new();

impl AppConfig {
    /// Load config: defaults -> .env -> env vars -> CLI
    pub fn load() -> Result<Self, ConfigError> {
        use dotenvy::from_filename;

        // 1. Start with defaults
        let mut config = Self::default();

        // 2. Load .env file (if it exists, don't error if missing)
        let env_files = [".env.local", ".env"];
        for env_file in &env_files {
            if let Err(e) = from_filename(env_file) {
                // Only warn if file exists but can't be read (not if file doesn't exist)
                if !e.to_string().contains("not found") && !e.to_string().contains("No such file") {
                    return Err(ConfigError::EnvFileError {
                        file: env_file.to_string(),
                        source: e,
                    });
                }
            }
        }

        // 3. Handle standard environment variables (override empack config if set)
        let env_config = EnvironmentConfig::load()?;
        config.color = env_config.apply_color_config(config.color);

        // 4. Override with CLI arguments (highest precedence)
        let cli_config = CliConfig::load()?;
        config = config.merge_with(cli_config.app_config);

        // 5. Post-process and validate
        config.validate()?;

        Ok(config)
    }

    /// Initialize global configuration (call once in main)
    pub fn init_global(config: AppConfig) -> Result<(), ConfigError> {
        GLOBAL_CONFIG
            .set(config)
            .map_err(|_| ConfigError::AlreadyInitialized)
    }

    /// Get global configuration reference
    pub fn global() -> &'static AppConfig {
        GLOBAL_CONFIG
            .get()
            .expect("Global config not initialized - call AppConfig::init_global() first")
    }
}

#[cfg(test)]
mod tests {
    include!("loader.test.rs");
}
