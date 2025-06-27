use crate::primitives::*;
use serde::Deserialize;
use std::io::{self, IsTerminal};
use std::process::Command;
use std::time::Duration;

use super::graphics::{TerminalGraphicsCaps, KittyGraphicsCaps, GraphicsDetectionMethod};
use super::capabilities::*;
use super::probing::*;

// ============================================================================
// ENVIRONMENT CONFIGURATION
// ============================================================================

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct TerminalEnvConfig {
    /// COLORTERM environment variable (color support indicator)
    pub colorterm: Option<String>,
    /// TERM environment variable (terminal type)
    pub term: Option<String>,
    /// TERM_PROGRAM environment variable (specific terminal application)
    pub term_program: Option<String>,
    /// TERM_PROGRAM_VERSION environment variable (terminal version)
    pub term_program_version: Option<String>,
    /// LANG environment variable (locale information)
    pub lang: Option<String>,
    /// LC_CTYPE environment variable (character classification)
    pub lc_ctype: Option<String>,
    /// LC_ALL environment variable (locale override)
    pub lc_all: Option<String>,
    /// VSCODE_INJECTION environment variable (VS Code terminal detection)
    pub vscode_injection: Option<String>,
    /// WT_SESSION environment variable (Windows Terminal detection)
    pub wt_session: Option<String>,
    /// KITTY_WINDOW_ID environment variable (Kitty terminal detection)
    pub kitty_window_id: Option<String>,
    /// KITTY_PID environment variable (Kitty process detection)
    pub kitty_pid: Option<String>,
    /// WEZTERM_PANE environment variable (WezTerm pane detection)
    pub wezterm_pane: Option<String>,
    /// WEZTERM_UNIX_SOCKET environment variable (WezTerm socket)
    pub wezterm_unix_socket: Option<String>,
    /// LINES environment variable (terminal rows)
    pub lines: Option<String>,
    /// COLUMNS environment variable (terminal columns)
    pub columns: Option<String>,
}

// ============================================================================
// TERMINAL SPECIFIC CAPABILITY PROFILES
// ============================================================================

impl TerminalSpecificCaps {
    pub fn kitty_full() -> Self {
        Self {
            expected_color: TerminalColorCaps::TrueColor,
            expected_unicode: TerminalUnicodeCaps::ExtendedUnicode,
            expected_graphics: TerminalGraphicsCaps::Kitty(KittyGraphicsCaps {
                supports_direct: true,
                supports_file: true,
                supports_temp_file: true,
                supports_shared_memory: true,
                supports_animation: true,
                supports_unicode_placeholders: true,
                supports_z_index: true,
                detection_method: GraphicsDetectionMethod::EnvironmentReliable,
                ..Default::default()
            }),
            expected_interactivity: TerminalInteractivity {
                supports_queries: true,
                supports_mouse: true,
                supports_focus_events: true,
                supports_paste_mode: true,
            },
            reliability: CapabilityReliability::EnvironmentReliable,
        }
    }

    pub fn ghostty_full() -> Self {
        Self {
            expected_color: TerminalColorCaps::TrueColor,
            expected_unicode: TerminalUnicodeCaps::ExtendedUnicode,
            expected_graphics: TerminalGraphicsCaps::Kitty(KittyGraphicsCaps {
                supports_direct: true,
                supports_file: true,
                supports_temp_file: true,
                detection_method: GraphicsDetectionMethod::EnvironmentReliable,
                ..Default::default()
            }),
            expected_interactivity: TerminalInteractivity {
                supports_queries: true,
                supports_mouse: true,
                supports_focus_events: true,
                supports_paste_mode: true,
            },
            reliability: CapabilityReliability::EnvironmentReliable,
        }
    }

    pub fn wezterm_full() -> Self {
        Self {
            expected_color: TerminalColorCaps::TrueColor,
            expected_unicode: TerminalUnicodeCaps::ExtendedUnicode,
            expected_graphics: TerminalGraphicsCaps::Kitty(KittyGraphicsCaps {
                supports_direct: true,
                supports_file: true,
                detection_method: GraphicsDetectionMethod::EnvironmentReliable,
                ..Default::default()
            }),
            expected_interactivity: TerminalInteractivity {
                supports_queries: true,
                supports_mouse: true,
                supports_focus_events: true,
                supports_paste_mode: true,
            },
            reliability: CapabilityReliability::EnvironmentReliable,
        }
    }

    pub fn alacritty_full() -> Self {
        Self {
            expected_color: TerminalColorCaps::TrueColor,
            expected_unicode: TerminalUnicodeCaps::ExtendedUnicode,
            expected_graphics: TerminalGraphicsCaps::None,
            expected_interactivity: TerminalInteractivity {
                supports_queries: true,
                supports_mouse: true,
                supports_focus_events: true,
                supports_paste_mode: true,
            },
            reliability: CapabilityReliability::EnvironmentReliable,
        }
    }

    pub fn windows_terminal_full() -> Self {
        Self {
            expected_color: TerminalColorCaps::TrueColor,
            expected_unicode: TerminalUnicodeCaps::ExtendedUnicode,
            expected_graphics: TerminalGraphicsCaps::None,
            expected_interactivity: TerminalInteractivity {
                supports_queries: true,
                supports_mouse: true,
                supports_focus_events: true,
                supports_paste_mode: true,
            },
            reliability: CapabilityReliability::EnvironmentReliable,
        }
    }

    pub fn vscode_full() -> Self {
        Self {
            expected_color: TerminalColorCaps::TrueColor,
            expected_unicode: TerminalUnicodeCaps::BasicUnicode,
            expected_graphics: TerminalGraphicsCaps::None,
            expected_interactivity: TerminalInteractivity {
                supports_queries: false,
                supports_mouse: true,
                supports_focus_events: false,
                supports_paste_mode: true,
            },
            reliability: CapabilityReliability::EnvironmentReliable,
        }
    }

    pub fn multiplexer_passthrough() -> Self {
        Self {
            expected_color: TerminalColorCaps::Ansi256,
            expected_unicode: TerminalUnicodeCaps::BasicUnicode,
            expected_graphics: TerminalGraphicsCaps::None,
            expected_interactivity: TerminalInteractivity {
                supports_queries: false,
                supports_mouse: true,
                supports_focus_events: false,
                supports_paste_mode: false,
            },
            reliability: CapabilityReliability::TermVariableMatch,
        }
    }

    pub fn unknown() -> Self {
        Self {
            expected_color: TerminalColorCaps::Ansi16,
            expected_unicode: TerminalUnicodeCaps::Ascii,
            expected_graphics: TerminalGraphicsCaps::None,
            expected_interactivity: TerminalInteractivity::default(),
            reliability: CapabilityReliability::Unknown,
        }
    }
}

// ============================================================================
// TERMINAL DATABASE AND DETECTION
// ============================================================================

pub(crate) fn detect_terminal_specific_capabilities(env_config: &TerminalEnvConfig) -> TerminalSpecificCaps {
    // First check TERM_PROGRAM (most reliable)
    if let Some(ref term_program) = env_config.term_program {
        match term_program.as_str() {
            // Kitty family - Full graphics + true color
            "kitty" => return TerminalSpecificCaps::kitty_full(),

            // Ghostty - Modern terminal with full capabilities
            "ghostty" => return TerminalSpecificCaps::ghostty_full(),

            // WezTerm - Full modern terminal
            "wezterm" => return TerminalSpecificCaps::wezterm_full(),

            // Alacritty - GPU-accelerated, no graphics protocol
            "alacritty" => return TerminalSpecificCaps::alacritty_full(),

            // VS Code integrated terminal
            "vscode" => return TerminalSpecificCaps::vscode_full(),

            // Windows Terminal - Microsoft's modern terminal
            "Windows Terminal" | "WindowsTerminal" => {
                return TerminalSpecificCaps::windows_terminal_full();
            }

            _ => {}
        }
    }

    // Then check TERM variable patterns
    if let Some(ref term_var) = env_config.term {
        match term_var.as_str() {
            // Kitty variants
            "xterm-kitty" | "kitty" => return TerminalSpecificCaps::kitty_full(),

            // Ghostty variants
            "xterm-ghostty" | "ghostty" => return TerminalSpecificCaps::ghostty_full(),

            // WezTerm variants
            "wezterm" | "xterm-wezterm" => return TerminalSpecificCaps::wezterm_full(),

            // Screen/tmux - capability pass-through
            term if term.starts_with("screen") || term.starts_with("tmux") => {
                return TerminalSpecificCaps::multiplexer_passthrough();
            }

            _ => {}
        }
    }

    // Environment-specific detection
    if env_config.wt_session.is_some() {
        return TerminalSpecificCaps::windows_terminal_full();
    }

    if env_config.vscode_injection.is_some() {
        return TerminalSpecificCaps::vscode_full();
    }

    // Check Kitty-specific environment variables
    if env_config.kitty_window_id.is_some() || env_config.kitty_pid.is_some() {
        return TerminalSpecificCaps::kitty_full();
    }

    // Check WezTerm environment
    if env_config.wezterm_pane.is_some() || env_config.wezterm_unix_socket.is_some() {
        return TerminalSpecificCaps::wezterm_full();
    }

    TerminalSpecificCaps::unknown()
}


// ============================================================================
// HELPER DETECTION FUNCTIONS
// ============================================================================

pub(crate) fn detect_color_from_environment(
    env_config: &TerminalEnvConfig,
    terminal_specific: &TerminalSpecificCaps,
) -> TerminalColorCaps {
    // Check COLORTERM environment variable (as per termstandard doc)
    if let Some(ref colorterm) = env_config.colorterm {
        let colorterm_lower = colorterm.to_lowercase();
        if colorterm_lower == "truecolor" || colorterm_lower == "24bit" {
            return TerminalColorCaps::TrueColor;
        }
    }

    // Check specific terminal programs known to support truecolor
    if let Some(ref term_program) = env_config.term_program {
        match term_program.as_str() {
            // From termstandard: terminals that fully support truecolor
            "alacritty" | "kitty" | "wezterm" | "iTerm.app" => {
                return TerminalColorCaps::TrueColor;
            }
            "vscode" => return TerminalColorCaps::TrueColor,
            _ => {}
        }
    }

    // Check TERM variable for color hints
    if let Some(ref term_var) = env_config.term {
        if term_var.contains("truecolor") || term_var.contains("24bit") {
            return TerminalColorCaps::TrueColor;
        }
        if term_var.contains("256") || term_var.contains("256color") {
            return TerminalColorCaps::Ansi256;
        }
        if term_var.starts_with("xterm") || term_var.contains("color") {
            return TerminalColorCaps::Ansi16;
        }
        if term_var == "dumb" {
            return TerminalColorCaps::None;
        }
    }

    // Fall back to terminal-specific expectations
    terminal_specific.expected_color
}

pub(crate) fn detect_unicode_capabilities(
    env_config: &TerminalEnvConfig,
    is_tty: bool,
) -> Result<TerminalUnicodeCaps, TerminalError> {
    if !is_tty {
        return Ok(TerminalUnicodeCaps::Ascii);
    }

    // Check locale environment variables for UTF-8 support
    // Priority: LC_ALL > LC_CTYPE > LANG
    let locale_var = env_config
        .lc_all
        .as_ref()
        .or(env_config.lc_ctype.as_ref())
        .or(env_config.lang.as_ref());

    if let Some(locale) = locale_var {
        let locale_lower = locale.to_lowercase();
        if locale_lower.contains("utf") {
            // Check for extended unicode support (emoji, complex scripts)
            if supports_extended_unicode(env_config) {
                return Ok(TerminalUnicodeCaps::ExtendedUnicode);
            }
            return Ok(TerminalUnicodeCaps::BasicUnicode);
        }
    }

    // Platform-specific locale detection
    #[cfg(unix)]
    {
        if let Ok(charset) = get_unix_charset() {
            if charset.to_lowercase().contains("utf") {
                if supports_extended_unicode(env_config) {
                    return Ok(TerminalUnicodeCaps::ExtendedUnicode);
                }
                return Ok(TerminalUnicodeCaps::BasicUnicode);
            }
        }
    }

    #[cfg(windows)]
    {
        // Windows: Check code pages for UTF-8 support (65001)
        use windows_sys::Win32::Globalization::{GetACP, GetOEMCP};
        use windows_sys::Win32::System::Console::{GetConsoleCP, GetConsoleOutputCP};
        
        unsafe {
            let acp = GetACP();
            let oemp = GetOEMCP(); 
            let console_cp = GetConsoleCP();
            let console_output_cp = GetConsoleOutputCP();
            
            // Check if any code page is UTF-8 (65001)
            let is_utf8 = acp == 65001 || oemp == 65001 || console_cp == 65001 || console_output_cp == 65001;
            
            if is_utf8 {
                if supports_extended_unicode(env_config) {
                    return Ok(TerminalUnicodeCaps::ExtendedUnicode);
                }
                return Ok(TerminalUnicodeCaps::BasicUnicode);
            }
        }
        
        // No UTF-8 code pages detected, return Ascii
        return Ok(TerminalUnicodeCaps::Ascii);
    }

    Ok(TerminalUnicodeCaps::Ascii)
}

pub(crate) fn supports_extended_unicode(env_config: &TerminalEnvConfig) -> bool {
    // Modern terminals known to support emoji and extended unicode well
    if let Some(ref term_program) = env_config.term_program {
        match term_program.as_str() {
            "iTerm.app" | "wezterm" | "alacritty" | "kitty" => return true,
            "vscode" => return true,
            _ => {}
        }
    }

    // Windows Terminal
    if env_config.wt_session.is_some() {
        return true;
    }

    // VS Code integrated terminal
    if env_config.vscode_injection.is_some() {
        return true;
    }

    // macOS Terminal.app - generally good unicode support
    #[cfg(target_os = "macos")]
    {
        if let Some(ref term_program) = env_config.term_program {
            if term_program == "Terminal.app" || term_program == "Apple_Terminal" {
                return true;
            }
        }
    }

    false
}

#[cfg(unix)]
fn get_unix_charset() -> Result<String, TerminalError> {
    let output = Command::new("locale")
        .arg("charmap")
        .output()
        .map_err(|_| TerminalError::CommandFailed {
            command: "locale charmap".to_string(),
        })?;

    if output.status.success() {
        String::from_utf8(output.stdout)
            .map_err(|e| TerminalError::InvalidUtf8Response { source: e })
            .map(|s| s.trim().to_string())
    } else {
        Err(TerminalError::CommandFailed {
            command: "locale charmap".to_string(),
        })
    }
}
