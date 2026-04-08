use crate::primitives::ConfigError;
use std::ffi::OsString;
use std::sync::OnceLock;

use super::{cli::CliConfig, config::AppConfig, env::EnvironmentConfig};

static GLOBAL_CONFIG: OnceLock<AppConfig> = OnceLock::new();

fn load_dotenv_files() -> Result<(), ConfigError> {
    use dotenvy::from_filename;

    for env_file in [".env.local", ".env"] {
        if !std::path::Path::new(env_file).exists() {
            continue;
        }

        from_filename(env_file).map_err(|source| ConfigError::EnvFileError {
            file: env_file.to_string(),
            source,
        })?;
    }

    Ok(())
}

impl AppConfig {
    /// Load config: defaults -> .env.local -> .env -> env vars -> CLI
    pub fn load() -> Result<Self, ConfigError> {
        let mut config = Self::default();

        load_dotenv_files()?;

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
        let mut config = Self::default();

        load_dotenv_files()?;

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
