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

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    fn clean_test_env() {
        let vars_to_clean = [
            "TERM",
            "COLORTERM",
            "TERM_PROGRAM",
            "TERM_PROGRAM_VERSION",
            "LANG",
            "LC_CTYPE",
            "LC_ALL",
            "VSCODE_INJECTION",
            "WT_SESSION",
            "KITTY_WINDOW_ID",
            "KITTY_PID",
            "WEZTERM_PANE",
            "WEZTERM_UNIX_SOCKET",
        ];

        for var in &vars_to_clean {
            unsafe {
                env::remove_var(var);
            }
        }
    }

    #[test]
    fn test_truecolor_detection_via_colorterm() {
        clean_test_env();
        unsafe {
            env::set_var("COLORTERM", "truecolor");
        }

        let env_config: TerminalEnvConfig = envy::from_env().unwrap();
        let terminal_specific = detect_terminal_specific_capabilities(&env_config);
        let result = detect_color_from_environment(&env_config, &terminal_specific);
        assert_eq!(result, TerminalColorCaps::TrueColor);

        clean_test_env();
    }

    #[test]
    fn test_256_color_detection_via_term() {
        clean_test_env();
        unsafe {
            env::set_var("TERM", "xterm-256color");
        }

        let env_config: TerminalEnvConfig = envy::from_env().unwrap();
        let terminal_specific = detect_terminal_specific_capabilities(&env_config);
        let result = detect_color_from_environment(&env_config, &terminal_specific);
        assert_eq!(result, TerminalColorCaps::Ansi256);

        clean_test_env();
    }

    #[test]
    fn test_unicode_detection_via_lang() {
        clean_test_env();
        unsafe {
            env::set_var("LANG", "en_US.UTF-8");
        }

        let env_config: TerminalEnvConfig = envy::from_env().unwrap();
        let result = detect_unicode_capabilities(&env_config, true).unwrap();
        assert_eq!(result, TerminalUnicodeCaps::BasicUnicode);

        clean_test_env();
    }

    #[test]
    fn test_kitty_terminal_detection() {
        clean_test_env();
        unsafe {
            env::set_var("TERM_PROGRAM", "kitty");
        }

        let env_config: TerminalEnvConfig = envy::from_env().unwrap();
        let result = detect_terminal_specific_capabilities(&env_config);
        assert_eq!(
            result.reliability,
            CapabilityReliability::EnvironmentReliable
        );
        assert_eq!(result.expected_color, TerminalColorCaps::TrueColor);

        clean_test_env();
    }

    #[test]
    fn test_windows_terminal_detection() {
        clean_test_env();
        unsafe {
            env::set_var("WT_SESSION", "some-session-id");
        }

        let env_config: TerminalEnvConfig = envy::from_env().unwrap();
        let result = detect_terminal_specific_capabilities(&env_config);
        assert_eq!(
            result.reliability,
            CapabilityReliability::EnvironmentReliable
        );
        assert_eq!(result.expected_color, TerminalColorCaps::TrueColor);

        clean_test_env();
    }

    #[test]
    fn test_terminal_dimensions_default() {
        let dims = TerminalDimensions::default();
        assert_eq!(dims.cols, 80);
        assert_eq!(dims.rows, 24);
        assert_eq!(dims.detection_source, DimensionSource::Default);
    }

    // =============================================================================
    // COMPREHENSIVE TERMINAL CAPABILITIES TESTING
    // =============================================================================

    #[test]
    fn test_terminal_capabilities_detect_from_config_auto() {
        clean_test_env();
        unsafe {
            env::set_var("TERM_PROGRAM", "kitty");
            env::set_var("COLORTERM", "truecolor");
        }

        let config = AppConfig {
            color: TerminalCapsDetectIntent::Auto,
            ..AppConfig::default()
        };

        let result = TerminalCapabilities::detect_from_config(&config);
        assert!(result.is_ok());

        let caps = result.unwrap();
        assert_eq!(caps.color, TerminalColorCaps::TrueColor);
        // Note: is_tty may be false in CI environments, which is expected

        clean_test_env();
    }

    #[test]
    fn test_terminal_capabilities_detect_forced_never() {
        clean_test_env();
        unsafe {
            env::set_var("COLORTERM", "truecolor");
        }

        let config = AppConfig {
            color: TerminalCapsDetectIntent::Never,
            ..AppConfig::default()
        };

        let result = TerminalCapabilities::detect_from_config(&config);
        assert!(result.is_ok());

        let caps = result.unwrap();
        assert_eq!(caps.color, TerminalColorCaps::None);

        clean_test_env();
    }

    #[test]
    fn test_terminal_capabilities_detect_forced_always() {
        clean_test_env();
        // Even without color environment variables, force should enable

        let config = AppConfig {
            color: TerminalCapsDetectIntent::Always,
            ..AppConfig::default()
        };

        let result = TerminalCapabilities::detect_from_config(&config);
        assert!(result.is_ok());

        let caps = result.unwrap();
        // Should get at least ANSI16 when forced, even without env hints
        assert!(matches!(
            caps.color,
            TerminalColorCaps::Ansi16 | TerminalColorCaps::Ansi256 | TerminalColorCaps::TrueColor
        ));

        clean_test_env();
    }

    // =============================================================================
    // ERROR HANDLING AND STRUCTURED ERRORS TESTING
    // =============================================================================

    #[test]
    fn test_terminal_error_display_formatting() {
        let error = TerminalError::NotInteractive;
        assert_eq!(
            error.to_string(),
            "Cannot probe capabilities on non-interactive terminal"
        );

        let error = TerminalError::ProbeTimeout { timeout: 5000 };
        assert_eq!(
            error.to_string(),
            "Terminal capability probing timed out after 5000ms"
        );

        let error = TerminalError::UnsupportedGraphics {
            protocol: "sixel".to_string(),
        };
        assert_eq!(error.to_string(), "Graphics protocol not supported: sixel");

        let error = TerminalError::DimensionDetectionFailed {
            reason: "ioctl failed".to_string(),
        };
        assert_eq!(
            error.to_string(),
            "Terminal dimension detection failed: ioctl failed"
        );
    }

    #[test]
    fn test_terminal_error_source_chain() {
        use std::error::Error;

        let io_error = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "access denied");
        let terminal_error = TerminalError::ResponseReadFailed { source: io_error };

        // Should be able to access the source
        assert!(terminal_error.source().is_some());
        assert_eq!(
            terminal_error.to_string(),
            "Failed to read terminal response: access denied"
        );
    }

    // =============================================================================
    // CAPABILITY PROBING SYSTEM TESTING
    // =============================================================================

    #[test]
    fn test_capability_prober_new_non_interactive() {
        use std::time::Duration;

        // This test assumes we're NOT running in a fully interactive terminal
        // In CI/testing environments, this should trigger the NotInteractive error
        // Since capability probing requires raw mode access

        let result = CapabilityProber::new(Duration::from_millis(100));
        // The result depends on the testing environment:
        // - In CI: Should be Err(TerminalError::NotInteractive)
        // - In interactive terminal: Should be Ok(probes)
        // Both are valid outcomes for different environments
        match result {
            Ok(_) => {
                // We're in an interactive terminal - probing is available
                assert!(
                    true,
                    "Capability probing available in interactive environment"
                );
            }
            Err(TerminalError::NotInteractive) => {
                // We're in a non-interactive environment - expected in CI
                assert!(true, "Correctly detected non-interactive environment");
            }
            Err(other) => {
                panic!("Unexpected error type: {:?}", other);
            }
        }
    }

    // =============================================================================
    // GRAPHICS CAPABILITIES TESTING
    // =============================================================================

    #[test]
    fn test_kitty_graphics_caps_construction() {
        let caps = KittyGraphicsCaps {
            supports_direct: true,
            supports_file: true,
            supports_temp_file: false,
            supports_shared_memory: false,
            supports_animation: true,
            supports_unicode_placeholders: true,
            supports_z_index: false,
            cell_width_pixels: 12,
            cell_height_pixels: 24,
            max_image_width: Some(1920),
            max_image_height: Some(1080),
            protocol_version: 2,
            detection_method: GraphicsDetectionMethod::ProtocolProbe,
        };

        assert!(caps.supports_direct);
        assert!(caps.supports_file);
        assert!(!caps.supports_temp_file);
        assert_eq!(caps.cell_width_pixels, 12);
        assert_eq!(caps.cell_height_pixels, 24);
        assert_eq!(caps.max_image_width, Some(1920));
    }

    #[test]
    fn test_graphics_caps_enum_variants() {
        let none_caps = TerminalGraphicsCaps::None;
        assert_eq!(none_caps, TerminalGraphicsCaps::None);

        let kitty_caps = KittyGraphicsCaps {
            supports_direct: true,
            supports_file: false,
            supports_temp_file: false,
            supports_shared_memory: false,
            supports_animation: false,
            supports_unicode_placeholders: false,
            supports_z_index: false,
            cell_width_pixels: 10,
            cell_height_pixels: 20,
            max_image_width: None,
            max_image_height: None,
            protocol_version: 1,
            detection_method: GraphicsDetectionMethod::EnvironmentReliable,
        };

        let kitty_variant = TerminalGraphicsCaps::Kitty(kitty_caps.clone());
        match kitty_variant {
            TerminalGraphicsCaps::Kitty(caps) => {
                assert_eq!(caps.cell_width_pixels, 10);
                assert_eq!(caps.cell_height_pixels, 20);
            }
            _ => panic!("Expected Kitty variant"),
        }
    }

    // =============================================================================
    // TERMINAL DIMENSION TESTING
    // =============================================================================

    #[test]
    fn test_terminal_dimensions_sources() {
        let tiocgwinsz_dims = TerminalDimensions {
            cols: 120,
            rows: 40,
            width_pixels: Some(1440),
            height_pixels: Some(900),
            detection_source: DimensionSource::Tiocgwinsz,
        };
        assert_eq!(
            tiocgwinsz_dims.detection_source,
            DimensionSource::Tiocgwinsz
        );
        assert_eq!(tiocgwinsz_dims.width_pixels, Some(1440));

        let csi_dims = TerminalDimensions {
            cols: 100,
            rows: 30,
            width_pixels: None,
            height_pixels: None,
            detection_source: DimensionSource::CsiQuery,
        };
        assert_eq!(csi_dims.detection_source, DimensionSource::CsiQuery);
        assert_eq!(csi_dims.width_pixels, None);

        let env_dims = TerminalDimensions {
            cols: 80,
            rows: 24,
            width_pixels: None,
            height_pixels: None,
            detection_source: DimensionSource::Environment,
        };
        assert_eq!(env_dims.detection_source, DimensionSource::Environment);
    }

    // =============================================================================
    // ENVIRONMENT VARIABLE ISOLATION TESTING (leveraging cargo-nextest)
    // =============================================================================

    #[test]
    fn test_environment_isolation_kitty_vs_iterm() {
        // This test demonstrates cargo-nextest's environment isolation
        // Each test gets its own process, so env vars don't interfere
        clean_test_env();
        unsafe {
            env::set_var("TERM_PROGRAM", "kitty");
            env::set_var("KITTY_WINDOW_ID", "123");
        }

        let env_config: TerminalEnvConfig = envy::from_env().unwrap();
        assert_eq!(env_config.term_program, Some("kitty".to_string()));
        assert_eq!(env_config.kitty_window_id, Some("123".to_string()));

        let terminal_specific = detect_terminal_specific_capabilities(&env_config);
        assert_eq!(
            terminal_specific.expected_color,
            TerminalColorCaps::TrueColor
        );

        clean_test_env();
    }

    #[test]
    fn test_environment_isolation_iterm_detection() {
        // This runs in a separate process from the kitty test above
        // Demonstrates that cargo-nextest prevents environment pollution
        clean_test_env();
        unsafe {
            env::set_var("TERM_PROGRAM", "iTerm.app");
            env::set_var("TERM_PROGRAM_VERSION", "3.4.0");
        }

        let env_config: TerminalEnvConfig = envy::from_env().unwrap();
        assert_eq!(env_config.term_program, Some("iTerm.app".to_string()));
        assert_eq!(env_config.term_program_version, Some("3.4.0".to_string()));
        // Should not have kitty variables from previous test
        assert_eq!(env_config.kitty_window_id, None);

        clean_test_env();
    }

    #[test]
    fn test_environment_isolation_unicode_locales() {
        clean_test_env();
        unsafe {
            env::set_var("LC_ALL", "C"); // ASCII-only locale
        }

        let env_config: TerminalEnvConfig = envy::from_env().unwrap();
        let result = detect_unicode_capabilities(&env_config, true).unwrap();
        assert_eq!(result, TerminalUnicodeCaps::Ascii);

        clean_test_env();
    }

    #[test]
    fn test_environment_isolation_utf8_locale() {
        clean_test_env();
        unsafe {
            env::set_var("LC_ALL", "en_US.UTF-8");
        }

        let env_config: TerminalEnvConfig = envy::from_env().unwrap();
        let result = detect_unicode_capabilities(&env_config, true).unwrap();
        assert_eq!(result, TerminalUnicodeCaps::BasicUnicode);

        clean_test_env();
    }

    // =============================================================================
    // TERMINAL INTERACTIVITY TESTING
    // =============================================================================

    #[test]
    fn test_terminal_interactivity_default() {
        let interactivity = TerminalInteractivity::default();
        assert!(!interactivity.supports_queries);
        assert!(!interactivity.supports_mouse);
        assert!(!interactivity.supports_focus_events);
        assert!(!interactivity.supports_paste_mode);
    }

    #[test]
    fn test_terminal_interactivity_construction() {
        let interactivity = TerminalInteractivity {
            supports_queries: true,
            supports_mouse: true,
            supports_focus_events: false,
            supports_paste_mode: true,
        };

        assert!(interactivity.supports_queries);
        assert!(interactivity.supports_mouse);
        assert!(!interactivity.supports_focus_events);
        assert!(interactivity.supports_paste_mode);
    }

    // =============================================================================
    // COMPREHENSIVE TERMINAL TYPE DETECTION
    // =============================================================================

    #[test]
    fn test_comprehensive_terminal_detection_vscode() {
        clean_test_env();
        unsafe {
            env::set_var("TERM_PROGRAM", "vscode");
            env::set_var("VSCODE_INJECTION", "1");
        }

        let env_config: TerminalEnvConfig = envy::from_env().unwrap();
        let terminal_specific = detect_terminal_specific_capabilities(&env_config);
        assert_eq!(
            terminal_specific.expected_color,
            TerminalColorCaps::TrueColor
        );

        clean_test_env();
    }

    #[test]
    fn test_comprehensive_terminal_detection_wezterm() {
        clean_test_env();
        unsafe {
            env::set_var("TERM_PROGRAM", "WezTerm");
            env::set_var("WEZTERM_PANE", "1");
        }

        let env_config: TerminalEnvConfig = envy::from_env().unwrap();
        let terminal_specific = detect_terminal_specific_capabilities(&env_config);
        assert_eq!(
            terminal_specific.expected_color,
            TerminalColorCaps::TrueColor
        );

        clean_test_env();
    }

    #[test]
    fn test_comprehensive_terminal_detection_fallback() {
        clean_test_env();
        // No specific terminal environment variables set

        let env_config: TerminalEnvConfig = envy::from_env().unwrap();
        let terminal_specific = detect_terminal_specific_capabilities(&env_config);
        // Should fall back to basic capabilities
        assert_eq!(
            terminal_specific.reliability,
            CapabilityReliability::Unknown
        );

        clean_test_env();
    }

    // =============================================================================
    // INTEGRATION TESTING - FULL PIPELINE
    // =============================================================================

    #[test]
    fn test_full_integration_modern_terminal() {
        clean_test_env();
        unsafe {
            env::set_var("TERM_PROGRAM", "kitty");
            env::set_var("COLORTERM", "truecolor");
            env::set_var("LANG", "en_US.UTF-8");
        }

        let config = AppConfig {
            color: TerminalCapsDetectIntent::Auto,
            ..AppConfig::default()
        };

        let result = TerminalCapabilities::detect_from_config(&config);
        assert!(result.is_ok());

        let caps = result.unwrap();
        assert_eq!(caps.color, TerminalColorCaps::TrueColor);
        // Note: unicode detection may return Ascii in CI environments due to is_tty check
        assert!(matches!(
            caps.unicode,
            TerminalUnicodeCaps::BasicUnicode | TerminalUnicodeCaps::Ascii
        ));
        // Note: is_tty may be false in CI environments, which is expected

        clean_test_env();
    }

    #[test]
    fn test_full_integration_legacy_terminal() {
        clean_test_env();
        unsafe {
            env::set_var("TERM", "xterm");
            env::set_var("LC_ALL", "C");
        }

        let config = AppConfig {
            color: TerminalCapsDetectIntent::Auto,
            ..AppConfig::default()
        };

        let result = TerminalCapabilities::detect_from_config(&config);
        assert!(result.is_ok());

        let caps = result.unwrap();
        assert_eq!(caps.color, TerminalColorCaps::Ansi16);
        assert_eq!(caps.unicode, TerminalUnicodeCaps::Ascii);

        clean_test_env();
    }
}
