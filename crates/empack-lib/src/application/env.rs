//! Environment variable handling for application configuration
//!
//! Manages standard environment variables for color and CI detection
//! following established conventions.

use crate::primitives::{ConfigError, TerminalCapsDetectIntent};
use serde::Deserialize;

/// Environment variables that affect application behavior
#[derive(Debug, Clone, Deserialize)]
pub struct EnvironmentConfig {
    /// NO_COLOR environment variable (any value = disable color)
    pub no_color: Option<String>,
    /// FORCE_COLOR environment variable (0/false = disable, 1/2/3/true = enable)
    pub force_color: Option<String>,
    /// CLICOLOR environment variable (0 = disable color)
    pub clicolor: Option<String>,
    /// CI environment variable (any value = CI mode)
    pub ci: Option<String>,
}

impl EnvironmentConfig {
    /// Load environment configuration from current environment
    pub fn load() -> Result<Self, ConfigError> {
        use envy::from_env;
        from_env().map_err(|e| ConfigError::EnvironmentParsingFailed { source: e })
    }

    /// Apply environment variables to color configuration
    ///
    /// Precedence: CI > CLICOLOR < NO_COLOR < FORCE_COLOR
    pub fn apply_color_config(
        &self,
        mut color: TerminalCapsDetectIntent,
    ) -> TerminalCapsDetectIntent {
        // 1. CI detection (disable interactive features)
        if self.ci.is_some() {
            return TerminalCapsDetectIntent::Never;
        }

        // 2. CLICOLOR=0 (BSD/macOS standard - disable color)
        if let Some(clicolor) = &self.clicolor {
            if clicolor == "0" {
                color = TerminalCapsDetectIntent::Never;
            }
        }

        // 3. NO_COLOR (universal standard - any non-empty value disables color)
        if let Some(no_color) = &self.no_color {
            if !no_color.is_empty() {
                color = TerminalCapsDetectIntent::Never;
            }
        }

        // 4. FORCE_COLOR (Node.js/modern standard - highest precedence)
        if let Some(force_color) = &self.force_color {
            match force_color.as_str() {
                "0" | "false" => color = TerminalCapsDetectIntent::Never,
                "1" | "2" | "3" | "true" => color = TerminalCapsDetectIntent::Always,
                _ => {} // Invalid values ignored
            }
        }

        color
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
    fn test_no_color_environment_variable() {
        clean_test_env();
        unsafe {
            env::set_var("NO_COLOR", "1");
        }

        let env_config = EnvironmentConfig::load().unwrap();
        let color = env_config.apply_color_config(TerminalCapsDetectIntent::Auto);
        assert_eq!(color, TerminalCapsDetectIntent::Never);

        clean_test_env();
    }

    #[test]
    fn test_force_color_environment_variable() {
        clean_test_env();
        unsafe {
            env::set_var("FORCE_COLOR", "1");
        }

        let env_config = EnvironmentConfig::load().unwrap();
        let color = env_config.apply_color_config(TerminalCapsDetectIntent::Auto);
        assert_eq!(color, TerminalCapsDetectIntent::Always);

        clean_test_env();
    }

    #[test]
    fn test_environment_variable_precedence() {
        clean_test_env();

        // Set all environment variables that affect color
        unsafe {
            env::set_var("CLICOLOR", "0"); // Should disable color
            env::set_var("NO_COLOR", "1"); // Should override CLICOLOR and disable color
            env::set_var("FORCE_COLOR", "1"); // Should override everything and enable color
        }

        let env_config = EnvironmentConfig::load().unwrap();
        let color = env_config.apply_color_config(TerminalCapsDetectIntent::Auto);

        // FORCE_COLOR=1 should win, enabling color despite NO_COLOR and CLICOLOR
        assert_eq!(color, TerminalCapsDetectIntent::Always);

        clean_test_env();
    }

    #[test]
    fn test_ci_environment_variable() {
        clean_test_env();
        unsafe {
            env::set_var("CI", "true");
        }

        let env_config = EnvironmentConfig::load().unwrap();
        let color = env_config.apply_color_config(TerminalCapsDetectIntent::Auto);
        assert_eq!(color, TerminalCapsDetectIntent::Never);

        clean_test_env();
    }

    #[test]
    fn test_empty_no_color_is_ignored() {
        clean_test_env();
        unsafe {
            env::set_var("NO_COLOR", "");
        }

        let env_config = EnvironmentConfig::load().unwrap();
        let color = env_config.apply_color_config(TerminalCapsDetectIntent::Auto);
        assert_eq!(color, TerminalCapsDetectIntent::Auto);

        clean_test_env();
    }

    #[test]
    fn test_invalid_force_color_values_ignored() {
        clean_test_env();
        unsafe {
            env::set_var("FORCE_COLOR", "invalid");
        }

        let env_config = EnvironmentConfig::load().unwrap();
        let color = env_config.apply_color_config(TerminalCapsDetectIntent::Auto);
        assert_eq!(color, TerminalCapsDetectIntent::Auto);

        clean_test_env();
    }
}
