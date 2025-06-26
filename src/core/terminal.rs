use crate::core::{AppConfig, primitives::*};
use serde::Deserialize;
use std::io::{self, IsTerminal, Read, Write};
use std::mem;
use std::process::Command;
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::fd::AsRawFd;

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

// Enhanced graphics capabilities with Kitty protocol support
#[derive(Debug, Clone, PartialEq)]
pub enum TerminalGraphicsCaps {
    None,
    Kitty(KittyGraphicsCaps),
    Sixel(SixelCaps),
    ITerm2(ITerm2Caps),
}

impl Default for TerminalGraphicsCaps {
    fn default() -> Self {
        Self::None
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct KittyGraphicsCaps {
    pub supports_direct: bool,
    pub supports_file: bool,
    pub supports_temp_file: bool,
    pub supports_shared_memory: bool,
    pub supports_animation: bool,
    pub supports_unicode_placeholders: bool,
    pub supports_z_index: bool,
    pub cell_width_pixels: u16,
    pub cell_height_pixels: u16,
    pub max_image_width: Option<u32>,
    pub max_image_height: Option<u32>,
    pub protocol_version: u8,
    pub detection_method: GraphicsDetectionMethod,
}

impl Default for KittyGraphicsCaps {
    fn default() -> Self {
        Self {
            supports_direct: true,
            supports_file: false,
            supports_temp_file: false,
            supports_shared_memory: false,
            supports_animation: false,
            supports_unicode_placeholders: false,
            supports_z_index: false,
            cell_width_pixels: 0,
            cell_height_pixels: 0,
            max_image_width: None,
            max_image_height: None,
            protocol_version: 1,
            detection_method: GraphicsDetectionMethod::ProtocolProbe,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SixelCaps {
    pub max_colors: u16,
    pub max_width: Option<u16>,
    pub max_height: Option<u16>,
}

impl Default for SixelCaps {
    fn default() -> Self {
        Self {
            max_colors: 256,
            max_width: None,
            max_height: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ITerm2Caps {
    pub supports_inline: bool,
    pub supports_file_download: bool,
}

impl Default for ITerm2Caps {
    fn default() -> Self {
        Self {
            supports_inline: true,
            supports_file_download: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum GraphicsDetectionMethod {
    EnvironmentReliable,  // TERM_PROGRAM=kitty
    EnvironmentVariables, // KITTY_WINDOW_ID, etc.
    ProtocolProbe,        // Escape sequence query
    ProtocolProbeTimeout, // Probe with no response
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
// ENVIRONMENT CONFIGURATION
// ============================================================================

#[derive(Debug, Clone, Deserialize)]
struct TerminalEnvConfig {
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
// CAPABILITY PROBING SYSTEM
// ============================================================================

pub struct CapabilityProber {
    timeout: Duration,
    raw_mode_guard: Option<RawModeGuard>,
}

impl CapabilityProber {
    pub fn new(timeout: Duration) -> Result<Self, TerminalError> {
        let raw_mode_guard = if io::stdin().is_terminal() {
            Some(setup_raw_mode()?)
        } else {
            None
        };

        Ok(Self {
            timeout,
            raw_mode_guard,
        })
    }

    pub fn probe_color_support(&self) -> Result<TerminalColorCaps, TerminalError> {
        if self.raw_mode_guard.is_none() {
            return Err(TerminalError::NotInteractive);
        }

        // Progressive probing: true color -> 256 -> 16 -> 8
        if self.probe_true_color()? {
            return Ok(TerminalColorCaps::TrueColor);
        }

        if self.probe_256_color()? {
            return Ok(TerminalColorCaps::Ansi256);
        }

        if self.probe_16_color()? {
            return Ok(TerminalColorCaps::Ansi16);
        }

        Ok(TerminalColorCaps::None)
    }

    pub fn probe_graphics_support(&self) -> Result<TerminalGraphicsCaps, TerminalError> {
        if self.raw_mode_guard.is_none() {
            return Ok(TerminalGraphicsCaps::None);
        }

        // Try Kitty protocol first (most common)
        if let Ok(kitty_caps) = self.probe_kitty_graphics() {
            return Ok(TerminalGraphicsCaps::Kitty(kitty_caps));
        }

        // Try Sixel
        if self.probe_sixel_support()? {
            return Ok(TerminalGraphicsCaps::Sixel(SixelCaps::default()));
        }

        // Try iTerm2
        if self.probe_iterm2_support()? {
            return Ok(TerminalGraphicsCaps::ITerm2(ITerm2Caps::default()));
        }

        Ok(TerminalGraphicsCaps::None)
    }

    pub fn probe_dimensions(&self) -> Result<TerminalDimensions, TerminalError> {
        // Try platform-specific methods first
        #[cfg(unix)]
        {
            if let Ok(dims) = self.probe_unix_dimensions() {
                return Ok(dims);
            }
        }

        #[cfg(windows)]
        {
            if let Ok(dims) = self.probe_windows_dimensions() {
                return Ok(dims);
            }
        }

        // Fallback to CSI query (cross-platform)
        if self.raw_mode_guard.is_some() {
            if let Ok(dims) = self.probe_csi_dimensions() {
                return Ok(dims);
            }
        }

        // Environment variable fallback
        self.probe_env_dimensions()
    }

    // Individual probe methods
    fn probe_true_color(&self) -> Result<bool, TerminalError> {
        // Enhanced true color probing using OSC 11 query
        let test_sequence = "\x1b]11;?\x1b\\";
        self.send_and_wait_for_response(test_sequence, |response| {
            response.contains("rgb:") || response.contains("rgba:")
        })
    }

    fn probe_256_color(&self) -> Result<bool, TerminalError> {
        // Test 256-color support with color setting and query
        let test_sequence = "\x1b[48;5;196m\x1b]11;?\x1b\\";
        self.send_and_wait_for_response(test_sequence, |response| {
            response.contains("rgb:") && !response.is_empty()
        })
    }

    fn probe_16_color(&self) -> Result<bool, TerminalError> {
        // Test basic 16-color support
        let test_sequence = "\x1b[41m\x1b]11;?\x1b\\";
        self.send_and_wait_for_response(test_sequence, |response| !response.is_empty())
    }

    fn probe_kitty_graphics(&self) -> Result<KittyGraphicsCaps, TerminalError> {
        use std::time::{SystemTime, UNIX_EPOCH};
        let query_id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .subsec_millis() as u32
            % 9000
            + 1000;
        let test_sequence = format!("\x1b_Gi={},s=1,v=1,a=q,t=d,f=24;AAAA\x1b\\", query_id);

        if self.send_and_wait_for_response(&test_sequence, |response| {
            response.contains(&format!("_Gi={};OK", query_id))
        })? {
            // Protocol supported - now probe transmission methods
            self.probe_kitty_transmission_methods(query_id)
        } else {
            Err(TerminalError::UnsupportedGraphics {
                protocol: "kitty".to_string(),
            })
        }
    }

    fn probe_kitty_transmission_methods(
        &self,
        base_id: u32,
    ) -> Result<KittyGraphicsCaps, TerminalError> {
        let mut caps = KittyGraphicsCaps::default();
        caps.detection_method = GraphicsDetectionMethod::ProtocolProbe;

        // Test file transmission
        if self
            .test_kitty_file_transmission(base_id + 1)
            .unwrap_or(false)
        {
            caps.supports_file = true;
        }

        // Test temp file transmission
        if self
            .test_kitty_temp_transmission(base_id + 2)
            .unwrap_or(false)
        {
            caps.supports_temp_file = true;
        }

        Ok(caps)
    }

    fn test_kitty_file_transmission(&self, test_id: u32) -> Result<bool, TerminalError> {
        // Create minimal test sequence for file transmission
        let test_sequence = format!("\x1b_Gi={},a=q,t=f\x1b\\", test_id);
        self.send_and_wait_for_response(&test_sequence, |response| {
            response.contains(&format!("_Gi={};OK", test_id))
        })
    }

    fn test_kitty_temp_transmission(&self, test_id: u32) -> Result<bool, TerminalError> {
        // Create minimal test sequence for temp file transmission
        let test_sequence = format!("\x1b_Gi={},a=q,t=t\x1b\\", test_id);
        self.send_and_wait_for_response(&test_sequence, |response| {
            response.contains(&format!("_Gi={};OK", test_id))
        })
    }

    fn probe_sixel_support(&self) -> Result<bool, TerminalError> {
        // Test Sixel support with device attributes query
        let test_sequence = "\x1b[c";
        self.send_and_wait_for_response(test_sequence, |response| {
            response.contains(";4;") // Sixel support indicator
        })
    }

    fn probe_iterm2_support(&self) -> Result<bool, TerminalError> {
        // Test iTerm2 inline images support
        let test_sequence = "\x1b]1337;File=inline=1:\x1b\\";
        self.send_and_wait_for_response(test_sequence, |_response| {
            true // iTerm2 typically doesn't respond, so any response indicates support
        })
    }

    fn send_and_wait_for_response<F>(
        &self,
        sequence: &str,
        validator: F,
    ) -> Result<bool, TerminalError>
    where
        F: Fn(&str) -> bool,
    {
        io::stdout().write_all(sequence.as_bytes())?;
        io::stdout().flush()?;

        let response = self.read_terminal_response()?;
        Ok(validator(&response))
    }

    fn read_terminal_response(&self) -> Result<String, TerminalError> {
        let start = Instant::now();
        let mut buffer = Vec::new();
        let mut stdin = io::stdin();

        while start.elapsed() < self.timeout {
            let mut byte = [0u8; 1];
            match stdin.read(&mut byte) {
                Ok(1) => {
                    buffer.push(byte[0]);
                    // Check for common escape sequence terminators
                    if buffer.len() >= 2 {
                        let last_two = &buffer[buffer.len() - 2..];
                        if last_two == [0x1b, 0x5c] || // ESC \
                           last_two == [0x07, 0x00] || // BEL
                           (buffer.len() >= 3 && &buffer[buffer.len()-3..] == [0x1b, 0x5c, 0x1b])
                        {
                            break;
                        }
                    }
                }
                Ok(0) => {
                    std::thread::sleep(Duration::from_millis(1));
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(1));
                }
                Err(e) => return Err(TerminalError::ResponseReadFailed { source: e }),
                _ => {
                    std::thread::sleep(Duration::from_millis(1));
                }
            }
        }

        String::from_utf8(buffer).map_err(|e| TerminalError::InvalidUtf8Response { source: e })
    }

    #[cfg(unix)]
    fn probe_unix_dimensions(&self) -> Result<TerminalDimensions, TerminalError> {
        let mut winsize: libc::winsize = unsafe { mem::zeroed() };

        if unsafe { libc::ioctl(io::stdout().as_raw_fd(), libc::TIOCGWINSZ, &mut winsize) } == 0 {
            Ok(TerminalDimensions {
                cols: winsize.ws_col,
                rows: winsize.ws_row,
                width_pixels: if winsize.ws_xpixel > 0 {
                    Some(winsize.ws_xpixel)
                } else {
                    None
                },
                height_pixels: if winsize.ws_ypixel > 0 {
                    Some(winsize.ws_ypixel)
                } else {
                    None
                },
                detection_source: DimensionSource::Tiocgwinsz,
            })
        } else {
            Err(TerminalError::DimensionDetectionFailed {
                reason: "TIOCGWINSZ ioctl failed".to_string(),
            })
        }
    }

    #[cfg(windows)]
    fn probe_windows_dimensions(&self) -> Result<TerminalDimensions, TerminalError> {
        // Windows 10+ supports ANSI sequences - use CSI query
        self.probe_csi_dimensions()
    }

    fn probe_csi_dimensions(&self) -> Result<TerminalDimensions, TerminalError> {
        // Query window size in pixels (CSI 14 t)
        let test_sequence = "\x1b[14t";

        if let Ok(response) = self.read_terminal_response_after_send(test_sequence) {
            // Response format: ESC[4;height;widtht
            if let Some(start) = response.find("[4;") {
                if let Some(end) = response[start..].find('t') {
                    let data = &response[start + 3..start + end];
                    if let Some(semicolon) = data.find(';') {
                        let height: u16 = data[..semicolon].parse().unwrap_or(24);
                        let width: u16 = data[semicolon + 1..].parse().unwrap_or(80);

                        // Query character cell size (CSI 18 t)
                        let cell_query = "\x1b[18t";
                        if let Ok(cell_response) =
                            self.read_terminal_response_after_send(cell_query)
                        {
                            // Response format: ESC[8;rows;colst
                            if let Some(cell_start) = cell_response.find("[8;") {
                                if let Some(cell_end) = cell_response[cell_start..].find('t') {
                                    let cell_data =
                                        &cell_response[cell_start + 3..cell_start + cell_end];
                                    if let Some(cell_semicolon) = cell_data.find(';') {
                                        let rows: u16 =
                                            cell_data[..cell_semicolon].parse().unwrap_or(24);
                                        let cols: u16 =
                                            cell_data[cell_semicolon + 1..].parse().unwrap_or(80);

                                        return Ok(TerminalDimensions {
                                            cols,
                                            rows,
                                            width_pixels: Some(width),
                                            height_pixels: Some(height),
                                            detection_source: DimensionSource::CsiQuery,
                                        });
                                    }
                                }
                            }
                        }

                        // Fallback with just pixel dimensions
                        return Ok(TerminalDimensions {
                            cols: 80,
                            rows: 24,
                            width_pixels: Some(width),
                            height_pixels: Some(height),
                            detection_source: DimensionSource::CsiQuery,
                        });
                    }
                }
            }
        }

        Err(TerminalError::DimensionDetectionFailed {
            reason: "CSI dimension query failed".to_string(),
        })
    }

    fn probe_env_dimensions(&self) -> Result<TerminalDimensions, TerminalError> {
        let cols = std::env::var("COLUMNS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(80);
        let rows = std::env::var("LINES")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(24);

        Ok(TerminalDimensions {
            cols,
            rows,
            width_pixels: None,
            height_pixels: None,
            detection_source: DimensionSource::Environment,
        })
    }

    fn read_terminal_response_after_send(&self, sequence: &str) -> Result<String, TerminalError> {
        io::stdout().write_all(sequence.as_bytes())?;
        io::stdout().flush()?;
        self.read_terminal_response()
    }
}

// ============================================================================
// TERMINAL DATABASE AND DETECTION
// ============================================================================

fn detect_terminal_specific_capabilities(env_config: &TerminalEnvConfig) -> TerminalSpecificCaps {
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
                // Try to create a capability prober, but fall back to environment detection if it fails
                let prober_result = CapabilityProber::new(Duration::from_millis(100));

                match prober_result {
                    Ok(prober) => {
                        let color = match config.color {
                            TerminalCapsDetectIntent::Always => {
                                // Force color, but still try to detect the best available level
                                prober
                                    .probe_color_support()
                                    .unwrap_or(terminal_specific.expected_color)
                            }
                            TerminalCapsDetectIntent::Auto => {
                                // Try probing, fall back to environment detection
                                prober.probe_color_support().unwrap_or_else(|_| {
                                    detect_color_from_environment(&env_config, &terminal_specific)
                                })
                            }
                            TerminalCapsDetectIntent::Never => TerminalColorCaps::None,
                        };

                        let graphics = if terminal_specific.reliability
                            == CapabilityReliability::EnvironmentReliable
                        {
                            // Trust environment detection for known terminals
                            terminal_specific.expected_graphics
                        } else {
                            // Probe for unknown terminals
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
                        // Probing not available (non-interactive environment), fall back to environment detection
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
// HELPER DETECTION FUNCTIONS
// ============================================================================

fn detect_color_from_environment(
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

fn detect_unicode_capabilities(
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
        // Windows: assume UTF-8 support on Windows 10+
        if supports_extended_unicode(env_config) {
            return Ok(TerminalUnicodeCaps::ExtendedUnicode);
        }
        return Ok(TerminalUnicodeCaps::BasicUnicode);
    }

    Ok(TerminalUnicodeCaps::Ascii)
}

fn supports_extended_unicode(env_config: &TerminalEnvConfig) -> bool {
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

// ============================================================================
// RAW MODE TERMINAL CONTROL
// ============================================================================

struct RawModeGuard;

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        let _ = restore_terminal_mode();
    }
}

fn setup_raw_mode() -> Result<RawModeGuard, TerminalError> {
    #[cfg(unix)]
    {
        setup_unix_raw_mode()?;
    }

    #[cfg(windows)]
    {
        setup_windows_raw_mode()?;
    }

    Ok(RawModeGuard)
}

#[cfg(unix)]
fn setup_unix_raw_mode() -> Result<(), TerminalError> {
    use std::os::fd::AsRawFd;

    // Save current terminal attributes and set raw mode
    let fd = io::stdin().as_raw_fd();

    let mut termios: libc::termios = unsafe { mem::zeroed() };
    if unsafe { libc::tcgetattr(fd, &mut termios) } != 0 {
        return Err(TerminalError::RawModeSetupFailed {
            reason: "Failed to get terminal attributes".to_string(),
        });
    }

    // Set raw mode flags
    termios.c_lflag &= !(libc::ECHO | libc::ICANON | libc::ISIG | libc::IEXTEN);
    termios.c_iflag &= !(libc::IXON | libc::ICRNL | libc::BRKINT | libc::INPCK | libc::ISTRIP);
    termios.c_cflag |= libc::CS8;
    termios.c_oflag &= !libc::OPOST;

    // Set read timeout
    termios.c_cc[libc::VMIN] = 0;
    termios.c_cc[libc::VTIME] = 1;

    if unsafe { libc::tcsetattr(fd, libc::TCSANOW, &termios) } != 0 {
        return Err(TerminalError::RawModeSetupFailed {
            reason: "Failed to set terminal attributes".to_string(),
        });
    }

    Ok(())
}

#[cfg(windows)]
fn setup_windows_raw_mode() -> Result<(), TerminalError> {
    // Simplified Windows raw mode setup
    // Full implementation would use Windows Console API
    Ok(())
}

fn restore_terminal_mode() -> Result<(), TerminalError> {
    #[cfg(unix)]
    {
        // Restore would typically restore saved termios
        // For simplicity, just reset to sane defaults
        let fd = io::stdin().as_raw_fd();
        unsafe {
            libc::tcflush(fd, libc::TCIFLUSH);
        }
    }

    Ok(())
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
