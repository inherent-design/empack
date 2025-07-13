use crate::application::AppConfig;
use crate::primitives::*;
use std::io::{self, IsTerminal};
use std::mem;
use std::time::Duration;

use super::detection::*;
use super::probing::*;
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

#[derive(Debug, Clone, PartialEq)]
pub struct TerminalInteractivity {
    pub supports_queries: bool,
    pub supports_mouse: bool,
    pub supports_focus_events: bool,
    pub supports_paste_mode: bool,
}

impl Default for TerminalInteractivity {
    fn default() -> Self {
        Self {
            supports_queries: false,
            supports_mouse: false,
            supports_focus_events: false,
            supports_paste_mode: false,
        }
    }
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
    pub fn detect_from_config(config: &AppConfig) -> Result<Self, TerminalError> {
        // Load environment variables
        let env_config = envy::from_env::<TerminalEnvConfig>()
            .map_err(|e| TerminalError::EnvironmentParsingFailed { source: e })?;

        // Check if we're in a TTY first
        let is_tty = io::stdout().is_terminal();

        // Get terminal-specific capability expectations
        let terminal_specific = detect_terminal_specific_capabilities(&env_config);

        // Use capability probing for interactive terminals when appropriate
        let (color, graphics, dimensions) =
            if is_tty && config.color != TerminalCapsDetectIntent::Never {
                // Probe capabilities if we can
                let prober_result = CapabilityProber::new(Duration::from_millis(100));

                match prober_result {
                    Ok(prober) => {
                        let color = match config.color {
                            TerminalCapsDetectIntent::Always => {
                                // Force color on, detect best level
                                prober
                                    .probe_color_support()
                                    .unwrap_or(terminal_specific.expected_color)
                            }
                            TerminalCapsDetectIntent::Auto => {
                                // Probe first, then environment
                                prober.probe_color_support().unwrap_or_else(|_| {
                                    detect_color_from_environment(&env_config, &terminal_specific)
                                })
                            }
                            TerminalCapsDetectIntent::Never => TerminalColorCaps::None,
                        };

                        let graphics = if terminal_specific.reliability
                            == CapabilityReliability::EnvironmentReliable
                        {
                            // Known terminals use environment detection
                            terminal_specific.expected_graphics
                        } else {
                            // Unknown terminals need probing
                            prober
                                .probe_graphics_support()
                                .unwrap_or(terminal_specific.expected_graphics)
                        };

                        let dimensions = prober
                            .probe_dimensions()
                            .unwrap_or_else(|_| TerminalDimensions::default());

                        (color, graphics, dimensions)
                    }
                    Err(_) => {
                        // Non-interactive: use environment detection
                        let color = match config.color {
                            TerminalCapsDetectIntent::Always => terminal_specific.expected_color,
                            TerminalCapsDetectIntent::Auto => {
                                detect_color_from_environment(&env_config, &terminal_specific)
                            }
                            TerminalCapsDetectIntent::Never => TerminalColorCaps::None,
                        };

                        (
                            color,
                            terminal_specific.expected_graphics,
                            TerminalDimensions::default(),
                        )
                    }
                }
            } else {
                // Non-interactive or forced-off
                let color = match config.color {
                    TerminalCapsDetectIntent::Always => terminal_specific.expected_color,
                    TerminalCapsDetectIntent::Auto => {
                        detect_color_from_environment(&env_config, &terminal_specific)
                    }
                    TerminalCapsDetectIntent::Never => TerminalColorCaps::None,
                };

                (
                    color,
                    TerminalGraphicsCaps::None,
                    TerminalDimensions::default(),
                )
            };

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

// ============================================================================
// RAW MODE TERMINAL CONTROL
// ============================================================================

pub(crate) struct RawModeGuard {
    #[cfg(unix)]
    original_termios: Option<libc::termios>,
    #[cfg(not(unix))]
    original_termios: Option<()>,
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        let _ = self.restore_terminal_mode();
    }
}

pub(crate) fn setup_raw_mode() -> Result<RawModeGuard, TerminalError> {
    #[cfg(unix)]
    {
        let original_termios = setup_unix_raw_mode()?;
        return Ok(RawModeGuard {
            original_termios: Some(original_termios),
        });
    }

    #[cfg(windows)]
    {
        setup_windows_raw_mode()?;
        return Ok(RawModeGuard {
            original_termios: None,
        });
    }

    #[cfg(not(any(unix, windows)))]
    {
        Ok(RawModeGuard {
            original_termios: None,
        })
    }
}

#[cfg(unix)]
fn setup_unix_raw_mode() -> Result<libc::termios, TerminalError> {
    use std::os::fd::AsRawFd;

    // Switch to raw mode for probing
    let fd = io::stdin().as_raw_fd();

    let mut original_termios: libc::termios = unsafe { mem::zeroed() };
    if unsafe { libc::tcgetattr(fd, &mut original_termios) } != 0 {
        return Err(TerminalError::RawModeSetupFailed {
            reason: "Failed to get terminal attributes".to_string(),
        });
    }

    // Configure raw mode
    let mut raw_termios = original_termios;

    // Set raw mode flags
    raw_termios.c_lflag &= !(libc::ECHO | libc::ICANON | libc::ISIG | libc::IEXTEN);
    raw_termios.c_iflag &= !(libc::IXON | libc::ICRNL | libc::BRKINT | libc::INPCK | libc::ISTRIP);
    raw_termios.c_cflag |= libc::CS8;
    raw_termios.c_oflag &= !libc::OPOST;

    // Set read timeout
    raw_termios.c_cc[libc::VMIN] = 0;
    raw_termios.c_cc[libc::VTIME] = 1;

    if unsafe { libc::tcsetattr(fd, libc::TCSANOW, &raw_termios) } != 0 {
        return Err(TerminalError::RawModeSetupFailed {
            reason: "Failed to set terminal attributes".to_string(),
        });
    }

    Ok(original_termios)
}

#[cfg(windows)]
fn setup_windows_raw_mode() -> Result<(), TerminalError> {
    // Simplified Windows raw mode setup
    // Full implementation would use Windows Console API
    Ok(())
}

impl RawModeGuard {
    fn restore_terminal_mode(&self) -> Result<(), TerminalError> {
        #[cfg(unix)]
        {
            if let Some(ref original_termios) = self.original_termios {
                use std::os::fd::AsRawFd;
                let fd = io::stdin().as_raw_fd();

                // Back to normal mode
                if unsafe { libc::tcsetattr(fd, libc::TCSANOW, original_termios) } != 0 {
                    return Err(TerminalError::RawModeSetupFailed {
                        reason: "Failed to restore terminal attributes".to_string(),
                    });
                }

                // Clear input buffer
                unsafe {
                    libc::tcflush(fd, libc::TCIFLUSH);
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    include!("capabilities.test.rs");
}