use crate::primitives::*;
use std::io::{self, IsTerminal};

use super::detection::*;
use crate::primitives::terminal::TerminalGraphicsCaps;

// ============================================================================
// CORE TERMINAL CAPABILITY STRUCTURES
// ============================================================================

#[derive(Debug, Clone)]
pub struct TerminalCapabilities {
    pub color: TerminalColorCaps,
    pub unicode: TerminalUnicodeCaps,
    pub graphics: TerminalGraphicsCaps,
    pub dimensions: TerminalDimensions,
    pub interactivity: TerminalInteractivity,
    pub is_tty: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TerminalDimensions {
    pub cols: u16,
    pub rows: u16,
    pub width_pixels: Option<u16>,
    pub height_pixels: Option<u16>,
    pub detection_source: DimensionSource,
}

impl Default for TerminalDimensions {
    fn default() -> Self {
        Self {
            cols: 80,
            rows: 24,
            width_pixels: None,
            height_pixels: None,
            detection_source: DimensionSource::Default,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum DimensionSource {
    Tiocgwinsz,  // Unix ioctl - most reliable
    CsiQuery,    // ESC[14t - cross-platform
    Environment, // COLUMNS/LINES - fallback
    Default,     // 80x24 assumption
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct TerminalInteractivity {
    pub supports_queries: bool,
    pub supports_mouse: bool,
    pub supports_focus_events: bool,
    pub supports_paste_mode: bool,
}

// Terminal-specific capability profiles
#[derive(Debug, Clone)]
pub struct TerminalSpecificCaps {
    pub expected_color: TerminalColorCaps,
    pub expected_unicode: TerminalUnicodeCaps,
    pub expected_graphics: TerminalGraphicsCaps,
    pub expected_interactivity: TerminalInteractivity,
    pub reliability: CapabilityReliability,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CapabilityReliability {
    EnvironmentReliable, // TERM_PROGRAM matches exactly
    TermVariableMatch,   // TERM variable pattern match
    EnvironmentHints,    // Secondary environment variables
    Unknown,             // Fallback detection
}

// ============================================================================
// MAIN DETECTION IMPLEMENTATION
// ============================================================================

impl TerminalCapabilities {
    /// Create minimal capabilities for non-interactive/fallback contexts.
    /// Used by Display::global() auto-init when no explicit init has occurred.
    pub fn minimal() -> Self {
        Self {
            color: TerminalColorCaps::None,
            unicode: TerminalUnicodeCaps::Ascii,
            graphics: TerminalGraphicsCaps::None,
            dimensions: TerminalDimensions::default(),
            interactivity: TerminalInteractivity::default(),
            is_tty: false,
        }
    }

    pub fn detect_from_config(
        color_intent: TerminalCapsDetectIntent,
    ) -> Result<Self, TerminalError> {
        // Load environment variables
        let env_config = envy::from_env::<TerminalEnvConfig>()
            .map_err(|e| TerminalError::EnvironmentParsingFailed { source: e })?;

        // Check if we're in a TTY first
        let is_tty = io::stdout().is_terminal();

        // Get terminal-specific capability expectations
        let terminal_specific = detect_terminal_specific_capabilities(&env_config);

        // Pure environment-based detection (no probing)
        let color = if is_tty && color_intent != TerminalCapsDetectIntent::Never {
            match color_intent {
                TerminalCapsDetectIntent::Always => {
                    let env_color = detect_color_from_environment(&env_config, &terminal_specific);
                    if env_color == TerminalColorCaps::None {
                        terminal_specific.expected_color
                    } else {
                        env_color
                    }
                }
                TerminalCapsDetectIntent::Auto => {
                    detect_color_from_environment(&env_config, &terminal_specific)
                }
                TerminalCapsDetectIntent::Never => TerminalColorCaps::None,
            }
        } else {
            match color_intent {
                TerminalCapsDetectIntent::Always => terminal_specific.expected_color,
                TerminalCapsDetectIntent::Auto => {
                    detect_color_from_environment(&env_config, &terminal_specific)
                }
                TerminalCapsDetectIntent::Never => TerminalColorCaps::None,
            }
        };

        let graphics = if is_tty {
            terminal_specific.expected_graphics
        } else {
            TerminalGraphicsCaps::None
        };

        let dimensions = detect_dimensions_from_env();

        // Detect unicode capabilities
        let unicode = detect_unicode_capabilities(&env_config, is_tty)?;

        Ok(Self {
            color,
            unicode,
            graphics,
            dimensions,
            interactivity: terminal_specific.expected_interactivity,
            is_tty,
        })
    }
}

/// Detect terminal dimensions from environment variables.
fn detect_dimensions_from_env() -> TerminalDimensions {
    let cols = std::env::var("COLUMNS").ok().and_then(|s| s.parse().ok());
    let rows = std::env::var("LINES").ok().and_then(|s| s.parse().ok());

    match (cols, rows) {
        (Some(c), Some(r)) => TerminalDimensions {
            cols: c,
            rows: r,
            width_pixels: None,
            height_pixels: None,
            detection_source: DimensionSource::Environment,
        },
        (Some(c), None) => TerminalDimensions {
            cols: c,
            rows: 24,
            width_pixels: None,
            height_pixels: None,
            detection_source: DimensionSource::Environment,
        },
        (None, Some(r)) => TerminalDimensions {
            cols: 80,
            rows: r,
            width_pixels: None,
            height_pixels: None,
            detection_source: DimensionSource::Environment,
        },
        (None, None) => TerminalDimensions::default(),
    }
}

#[cfg(test)]
mod tests {
    include!("capabilities.test.rs");
}
