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
    include!("env.test.rs");
}
