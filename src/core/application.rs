use crate::core::primitives::*;
use clap::Parser;
use serde::Deserialize;
use std::path::PathBuf;
use std::sync::OnceLock;

// FIXME: create a memoize map to lazy evaluate and hold the runtime values
const DEFAULT_LOG_LEVEL: &str = "2";
const DEFAULT_LOG_FORMAT: &str = "text";
const DEFAULT_NET_TIMEOUT: &str = "30";
const DEFAULT_CPU_PARALLELS: &str = "2";
const DEFAULT_LOG_OUTPUT: &str = "stderr";
const DEFAULT_TTY_CAPS_DETECT_INTENT: &str = "auto";

fn default_log_level() -> u8 {
    // FIXME: memoize these
    DEFAULT_LOG_LEVEL.parse().unwrap()
}

fn default_log_format() -> LogFormat {
    // FIXME: memoize these
    DEFAULT_LOG_FORMAT.parse().unwrap()
}

fn default_net_timeout() -> u64 {
    // FIXME: memoize these
    DEFAULT_NET_TIMEOUT.parse().unwrap()
}

fn default_cpu_parallels() -> usize {
    // FIXME: memoize these
    DEFAULT_CPU_PARALLELS.parse().unwrap()
}

fn default_log_output() -> LogOutput {
    // FIXME: memoize these
    DEFAULT_LOG_OUTPUT.parse().unwrap()
}

fn default_tty_caps_detect_intent() -> TerminalCapsDetectIntent {
    // FIXME: memoize these
    DEFAULT_TTY_CAPS_DETECT_INTENT.parse().unwrap()
}

// TODO: memoize map goes here

/// Single source of truth for all configuration
#[derive(Debug, Clone, Parser, Deserialize)]
#[command(name = "empack")]
#[command(about = "Intelligent Minecraft modpack management")]
#[command(version)]
pub struct AppConfig {
    /// Working directory for modpack operations
    #[arg(short, long, env = "EMPACK_WORKDIR")]
    #[serde(default)]
    pub workdir: Option<PathBuf>,

    /// Number of parallel API requests
    #[arg(short, long, env ="EMPACK_CPU_JOBS", default_value = DEFAULT_CPU_PARALLELS)]
    #[serde(default = "default_cpu_parallels")]
    pub cpu_jobs: usize,

    /// API timeout in seconds
    #[arg(short, long, env ="EMPACK_NET_TIMEOUT", default_value = DEFAULT_NET_TIMEOUT)]
    #[serde(default = "default_net_timeout")]
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
    #[arg(short, long, env="EMPACK_LOG_LEVEL", default_value = DEFAULT_LOG_LEVEL)]
    #[serde(default = "default_log_level")]
    pub log_level: u8,

    /// Output format (text, json, yaml)
    #[arg(short, long, env="EMPACK_LOG_FORMAT", default_value = DEFAULT_LOG_FORMAT)]
    #[serde(default = "default_log_format")]
    pub log_format: LogFormat,

    /// Log output stream (stderr, stdout)
    #[arg(short, long, env="EMPACK_LOG_OUTPUT", default_value = DEFAULT_LOG_OUTPUT)]
    #[serde(default = "default_log_output")]
    pub log_output: LogOutput,

    /// Color output control (auto, always, never)
    #[arg(short, long, env="EMPACK_COLOR", default_value = DEFAULT_TTY_CAPS_DETECT_INTENT)]
    #[serde(default = "default_tty_caps_detect_intent")]
    pub color: TerminalCapsDetectIntent,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            workdir: None,
            cpu_jobs: default_cpu_parallels(),
            net_timeout: default_net_timeout(),
            modrinth_api_client_id: None,
            modrinth_api_client_key: None,
            curseforge_api_client_key: None,
            log_level: default_log_level(),
            log_format: default_log_format(),
            log_output: default_log_output(),
            color: default_tty_caps_detect_intent(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct CommonEnvConfig {
    /// NO_COLOR environment variable (any value = disable color)
    pub no_color: Option<String>,
    /// FORCE_COLOR environment variable (0/false = disable, 1/2/3/true = enable)
    pub force_color: Option<String>,
    /// CLICOLOR environment variable (0 = disable color)
    pub clicolor: Option<String>,
    /// CI environment variable (any value = CI mode)
    pub ci: Option<String>,
}

// Global configuration available throughout the application
static GLOBAL_CONFIG: OnceLock<AppConfig> = OnceLock::new();

impl AppConfig {
    /// Load configuration with proper precedence:
    /// defaults -> .env file -> environment variables -> CLI args
    pub fn load() -> Result<Self, ConfigError> {
        use dotenvy::from_filename;
        use envy::from_env;

        // 1. Start with defaults
        let mut config = Self::default();

        // 2. Load .env file (if it exists, don't error if missing)
        // Try .env.local first, then .env
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
        let env_config = from_env::<CommonEnvConfig>()
            .map_err(|e| ConfigError::EnvironmentParsingFailed { source: e })?;
        
        // Apply color environment variables with proper precedence:
        // CLICOLOR=0 < NO_COLOR < FORCE_COLOR (most historically universal)
        
        // 1. CLICOLOR=0 (BSD/macOS standard - disable color)
        if let Some(clicolor) = &env_config.clicolor {
            if clicolor == "0" {
                config.color = TerminalCapsDetectIntent::Never;
            }
        }
        
        // 2. NO_COLOR (universal standard - any non-empty value disables color)
        if let Some(no_color) = &env_config.no_color {
            if !no_color.is_empty() {
                config.color = TerminalCapsDetectIntent::Never;
            }
        }
        
        // 3. FORCE_COLOR (Node.js/modern standard - highest precedence)
        if let Some(force_color) = &env_config.force_color {
            match force_color.as_str() {
                "0" | "false" => config.color = TerminalCapsDetectIntent::Never,
                "1" | "2" | "3" | "true" => config.color = TerminalCapsDetectIntent::Always,
                _ => {} // Invalid values ignored
            }
        }
        
        // 4. CI detection (disable interactive features)
        if env_config.ci.is_some() {
            config.color = TerminalCapsDetectIntent::Never;
        }

        // 4. Override with CLI arguments (highest precedence)
        let cli_config = Self::parse();

        // 5. Merge CLI overrides into config (Option fields take CLI values if provided)
        if cli_config.log_level != default_log_level() {
            config.log_level = cli_config.log_level;
        }
        if !matches!(cli_config.log_format, LogFormat::Text) {
            config.log_format = cli_config.log_format;
        }
        if cli_config.workdir.is_some() {
            config.workdir = cli_config.workdir;
        }
        if cli_config.net_timeout != default_net_timeout() {
            config.net_timeout = cli_config.net_timeout;
        }
        if cli_config.cpu_jobs != default_cpu_parallels() {
            config.cpu_jobs = cli_config.cpu_jobs;
        }
        if cli_config.log_output != default_log_output() {
            config.log_output = cli_config.log_output;
        }
        if cli_config.color != default_tty_caps_detect_intent() {
            config.color = cli_config.color;
        }
        if cli_config.modrinth_api_client_id.is_some() {
            config.modrinth_api_client_id = cli_config.modrinth_api_client_id;
        }
        if cli_config.modrinth_api_client_key.is_some() {
            config.modrinth_api_client_key = cli_config.modrinth_api_client_key;
        }
        if cli_config.curseforge_api_client_key.is_some() {
            config.curseforge_api_client_key = cli_config.curseforge_api_client_key;
        }

        // 6. Post-process and validate
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

    /// Merge this config with another, taking non-default values from other
    fn merge_with(mut self, other: Self) -> Self {
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
        if other.log_level != default_log_level() {
            self.log_level = other.log_level;
        }
        if other.net_timeout != default_net_timeout() {
            self.net_timeout = other.net_timeout;
        }
        if other.cpu_jobs != default_cpu_parallels() {
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
    fn validate(&mut self) -> Result<(), ConfigError> {
        // Resolve working directory (simple fallback only)
        if self.workdir.is_none() {
            self.workdir = Some(std::env::current_dir().map_err(|e| ConfigError::CurrentDirError { source: e })?);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    // Helper to clean environment before tests
    fn clean_test_env() {
        unsafe {
            env::remove_var("NO_COLOR");
            env::remove_var("FORCE_COLOR");
            env::remove_var("CLICOLOR");
            env::remove_var("CI");
        }
    }

    #[test]
    fn test_default_config() {
        let config = AppConfig::default();
        assert_eq!(config.log_level, 2);
        assert_eq!(config.net_timeout, 30);
        assert_eq!(config.color, TerminalCapsDetectIntent::Auto);
    }

    #[test]
    fn test_merge_configs() {
        let base = AppConfig::default();
        let override_config = AppConfig {
            log_level: 4,
            color: TerminalCapsDetectIntent::Always,
            cpu_jobs: 16,
            ..AppConfig::default()
        };

        let merged = base.merge_with(override_config);
        assert_eq!(merged.log_level, 4);
        assert_eq!(merged.color, TerminalCapsDetectIntent::Always);
        assert_eq!(merged.cpu_jobs, 16);
        assert_eq!(merged.net_timeout, 30);
    }

    #[test]
    fn test_no_color_environment_variable() {
        clean_test_env();
        unsafe { env::set_var("NO_COLOR", "1"); }
        
        let env_config: CommonEnvConfig = envy::from_env().unwrap();
        assert_eq!(env_config.no_color, Some("1".to_string()));
        
        // Test that NO_COLOR disables color
        let mut config = AppConfig::default();
        if let Some(no_color) = &env_config.no_color {
            if !no_color.is_empty() {
                config.color = TerminalCapsDetectIntent::Never;
            }
        }
        assert_eq!(config.color, TerminalCapsDetectIntent::Never);
        
        clean_test_env();
    }

    #[test]
    fn test_force_color_environment_variable() {
        clean_test_env();
        unsafe { env::set_var("FORCE_COLOR", "1"); }
        
        let env_config: CommonEnvConfig = envy::from_env().unwrap();
        assert_eq!(env_config.force_color, Some("1".to_string()));
        
        // Test that FORCE_COLOR=1 enables color
        let mut config = AppConfig::default();
        if let Some(force_color) = &env_config.force_color {
            match force_color.as_str() {
                "0" | "false" => config.color = TerminalCapsDetectIntent::Never,
                "1" | "2" | "3" | "true" => config.color = TerminalCapsDetectIntent::Always,
                _ => {}
            }
        }
        assert_eq!(config.color, TerminalCapsDetectIntent::Always);
        
        clean_test_env();
    }

    #[test]
    fn test_force_color_false_disables_color() {
        clean_test_env();
        unsafe { env::set_var("FORCE_COLOR", "0"); }
        
        let env_config: CommonEnvConfig = envy::from_env().unwrap();
        assert_eq!(env_config.force_color, Some("0".to_string()));
        
        let mut config = AppConfig::default();
        if let Some(force_color) = &env_config.force_color {
            match force_color.as_str() {
                "0" | "false" => config.color = TerminalCapsDetectIntent::Never,
                "1" | "2" | "3" | "true" => config.color = TerminalCapsDetectIntent::Always,
                _ => {}
            }
        }
        assert_eq!(config.color, TerminalCapsDetectIntent::Never);
        
        clean_test_env();
    }

    #[test]
    fn test_clicolor_environment_variable() {
        clean_test_env();
        unsafe { env::set_var("CLICOLOR", "0"); }
        
        let env_config: CommonEnvConfig = envy::from_env().unwrap();
        assert_eq!(env_config.clicolor, Some("0".to_string()));
        
        // Test that CLICOLOR=0 disables color
        let mut config = AppConfig::default();
        if let Some(clicolor) = &env_config.clicolor {
            if clicolor == "0" {
                config.color = TerminalCapsDetectIntent::Never;
            }
        }
        assert_eq!(config.color, TerminalCapsDetectIntent::Never);
        
        clean_test_env();
    }

    #[test]
    fn test_ci_environment_variable() {
        clean_test_env();
        unsafe { env::set_var("CI", "true"); }
        
        let env_config: CommonEnvConfig = envy::from_env().unwrap();
        assert_eq!(env_config.ci, Some("true".to_string()));
        
        // Test that CI disables color
        let mut config = AppConfig::default();
        if env_config.ci.is_some() {
            config.color = TerminalCapsDetectIntent::Never;
        }
        assert_eq!(config.color, TerminalCapsDetectIntent::Never);
        
        clean_test_env();
    }

    #[test]
    fn test_environment_variable_precedence() {
        clean_test_env();
        
        // Set all environment variables that affect color
        unsafe {
            env::set_var("CLICOLOR", "0");   // Should disable color
            env::set_var("NO_COLOR", "1");   // Should override CLICOLOR and disable color
            env::set_var("FORCE_COLOR", "1"); // Should override everything and enable color
        }
        
        let env_config: CommonEnvConfig = envy::from_env().unwrap();
        let mut config = AppConfig::default();
        
        // Apply precedence: CLICOLOR < NO_COLOR < FORCE_COLOR
        // 1. CLICOLOR=0 (BSD/macOS standard)
        if let Some(clicolor) = &env_config.clicolor {
            if clicolor == "0" {
                config.color = TerminalCapsDetectIntent::Never;
            }
        }
        
        // 2. NO_COLOR (universal standard - overrides CLICOLOR)
        if let Some(no_color) = &env_config.no_color {
            if !no_color.is_empty() {
                config.color = TerminalCapsDetectIntent::Never;
            }
        }
        
        // 3. FORCE_COLOR (highest precedence)
        if let Some(force_color) = &env_config.force_color {
            match force_color.as_str() {
                "0" | "false" => config.color = TerminalCapsDetectIntent::Never,
                "1" | "2" | "3" | "true" => config.color = TerminalCapsDetectIntent::Always,
                _ => {}
            }
        }
        
        // FORCE_COLOR=1 should win, enabling color despite NO_COLOR and CLICOLOR
        assert_eq!(config.color, TerminalCapsDetectIntent::Always);
        
        clean_test_env();
    }

    #[test]
    fn test_empty_no_color_is_ignored() {
        clean_test_env();
        unsafe { env::set_var("NO_COLOR", ""); } // Empty value should be ignored
        
        let env_config: CommonEnvConfig = envy::from_env().unwrap();
        let mut config = AppConfig::default();
        
        if let Some(no_color) = &env_config.no_color {
            if !no_color.is_empty() {
                config.color = TerminalCapsDetectIntent::Never;
            }
        }
        
        // Empty NO_COLOR should not change the default
        assert_eq!(config.color, TerminalCapsDetectIntent::Auto);
        
        clean_test_env();
    }

    #[test]
    fn test_invalid_force_color_values_ignored() {
        clean_test_env();
        unsafe { env::set_var("FORCE_COLOR", "invalid"); }
        
        let env_config: CommonEnvConfig = envy::from_env().unwrap();
        let mut config = AppConfig::default();
        
        if let Some(force_color) = &env_config.force_color {
            match force_color.as_str() {
                "0" | "false" => config.color = TerminalCapsDetectIntent::Never,
                "1" | "2" | "3" | "true" => config.color = TerminalCapsDetectIntent::Always,
                _ => {} // Invalid values should be ignored
            }
        }
        
        // Invalid FORCE_COLOR should not change the default
        assert_eq!(config.color, TerminalCapsDetectIntent::Auto);
        
        clean_test_env();
    }
}
