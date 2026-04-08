use crate::primitives::ConfigError;
use std::ffi::OsString;
use std::sync::OnceLock;

use super::{cli::CliConfig, config::AppConfig, env::EnvironmentConfig};

static GLOBAL_CONFIG: OnceLock<AppConfig> = OnceLock::new();

impl AppConfig {
    /// Load config: defaults -> .env -> env vars -> CLI
    pub fn load() -> Result<Self, ConfigError> {
        use dotenvy::from_filename;

        let mut config = Self::default();

        if std::path::Path::new(".env").exists() {
            from_filename(".env").map_err(|e| ConfigError::EnvFileError {
                file: ".env".to_string(),
                source: e,
            })?;
        }

        let env_config = EnvironmentConfig::load()?;
        config.color = env_config.apply_color_config(config.color);

        let cli_config = CliConfig::load()?;
        config = config.merge_with(cli_config.app_config);

        config.validate()?;
        Ok(config)
    }

    /// Load config from explicit command line arguments.
    pub fn load_from<I, T>(args: I) -> Result<Self, ConfigError>
    where
        I: IntoIterator<Item = T>,
        T: Into<OsString> + Clone,
    {
        use dotenvy::from_filename;

        let mut config = Self::default();

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

        let env_config = EnvironmentConfig::load()?;
        config.color = env_config.apply_color_config(config.color);

        let cli_config = CliConfig::load_from(args)?;
        config = config.merge_with(cli_config.app_config);

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
