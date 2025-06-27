use std::str::FromStr;
use clap::ValueEnum;
use thiserror::Error;
use super::shared::impl_fromstr_for_value_enum;

// ============================================================================
// SHARED TERMINAL CAPABILITY TYPES
// ============================================================================

/// Runtime color detection intent
#[derive(Debug, Clone, Copy, PartialEq, serde::Deserialize, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum TerminalCapsDetectIntent {
    /// Let module detect
    /// alias: auto, automatic, detect, default
    Auto,

    /// Explicitly enable (useful in non-interactive)
    /// alias: always, force, on
    Always,

    /// Explicitly disable (also useful in non-interactive)
    /// alias: never, off
    Never,
}

/// Terminal color capability levels (shared across all modules)
#[derive(Debug, Clone, Copy, PartialEq, serde::Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TerminalColorCaps {
    None,
    Ansi16,
    Ansi256,
    TrueColor,
}

/// Terminal unicode capability levels (shared across all modules)
#[derive(Debug, Clone, Copy, PartialEq, serde::Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TerminalUnicodeCaps {
    Ascii,
    BasicUnicode,
    ExtendedUnicode,
}

/// Terminal graphics capability levels (shared across all modules)
#[derive(Debug, Clone, Copy, PartialEq, serde::Deserialize)]
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

/// Kitty graphics protocol capabilities
#[derive(Debug, Clone, Copy, PartialEq, serde::Deserialize)]
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

/// Sixel graphics capabilities
#[derive(Debug, Clone, Copy, PartialEq, serde::Deserialize)]
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

/// iTerm2 graphics capabilities
#[derive(Debug, Clone, Copy, PartialEq, serde::Deserialize)]
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

/// Graphics detection method used
#[derive(Debug, Clone, Copy, PartialEq, serde::Deserialize)]
pub enum GraphicsDetectionMethod {
    EnvironmentReliable,  // TERM_PROGRAM=kitty
    EnvironmentVariables, // KITTY_WINDOW_ID, etc.
    ProtocolProbe,        // Escape sequence query
    ProtocolProbeTimeout, // Probe with no response
}

/// Simple terminal capabilities container (shared interface)
#[derive(Debug, Clone)]
pub struct BasicTerminalCapabilities {
    pub color: TerminalColorCaps,
    pub unicode: TerminalUnicodeCaps,
    pub graphics: TerminalGraphicsCaps,
}


/// Terminal detection and capability probing errors
#[derive(Debug, Error)]
pub enum TerminalError {
    #[error("Cannot probe capabilities on non-interactive terminal")]
    NotInteractive,

    #[error("Terminal capability probing timed out after {timeout}ms")]
    ProbeTimeout { timeout: u64 },

    #[error("Graphics protocol not supported: {protocol}")]
    UnsupportedGraphics { protocol: String },

    #[error("Failed to read terminal response: {source}")]
    ResponseReadFailed {
        #[from]
        source: std::io::Error,
    },

    #[error("Terminal response contains invalid UTF-8: {source}")]
    InvalidUtf8Response {
        #[from]
        source: std::string::FromUtf8Error,
    },

    #[error("Terminal dimension detection failed: {reason}")]
    DimensionDetectionFailed { reason: String },

    #[error("Raw mode setup failed: {reason}")]
    RawModeSetupFailed { reason: String },

    #[error("Environment variable parsing failed: {source}")]
    EnvironmentParsingFailed {
        #[from]
        source: envy::Error,
    },

    #[error("Command execution failed: {command}")]
    CommandFailed { command: String },
}


impl_fromstr_for_value_enum!(
    TerminalCapsDetectIntent,
    "invalid terminal capability detection intent"
);

// ============================================================================
// COMPOSABLE TERMINAL PRIMITIVES
// ============================================================================

/// Composable terminal primitives that adapt to detected capabilities
/// These provide uniform interfaces regardless of terminal support level
#[derive(Debug, Clone)]
pub struct TerminalPrimitives {
    // Basic color primitives
    pub red: &'static str,
    pub green: &'static str,
    pub blue: &'static str,
    pub yellow: &'static str,
    pub cyan: &'static str,
    pub magenta: &'static str,
    pub white: &'static str,
    pub black: &'static str,
    
    // Background colors
    pub bg_red: &'static str,
    pub bg_green: &'static str,
    pub bg_blue: &'static str,
    pub bg_yellow: &'static str,
    pub bg_cyan: &'static str,
    pub bg_magenta: &'static str,
    pub bg_white: &'static str,
    pub bg_black: &'static str,
    
    // Semantic colors (for UX messaging)
    pub success: &'static str,
    pub error: &'static str,
    pub warning: &'static str,
    pub info: &'static str,
    pub debug: &'static str,
    pub muted: &'static str,
    
    // Style primitives
    pub bold: &'static str,
    pub dim: &'static str,
    pub italic: &'static str,
    pub underline: &'static str,
    pub reverse: &'static str,
    pub strikethrough: &'static str,
    pub reset: &'static str,
    
    // Unicode symbols (adapt based on unicode support)
    pub checkmark: &'static str,
    pub cross: &'static str,
    pub warning_symbol: &'static str,
    pub info_symbol: &'static str,
    pub bullet: &'static str,
    pub arrow: &'static str,
}

impl TerminalPrimitives {
    /// Create primitives based on detected terminal capabilities
    pub fn new(caps: &BasicTerminalCapabilities) -> Self {
        match caps.color {
            TerminalColorCaps::TrueColor => Self::truecolor_primitives(caps),
            TerminalColorCaps::Ansi256 => Self::ansi256_primitives(caps),
            TerminalColorCaps::Ansi16 => Self::ansi16_primitives(caps),
            TerminalColorCaps::None => Self::no_color_primitives(caps),
        }
    }
    
    /// True color (24-bit) primitives for modern terminals
    fn truecolor_primitives(caps: &BasicTerminalCapabilities) -> Self {
        Self {
            // Enhanced true color palette
            red: "\x1b[38;2;220;50;47m",
            green: "\x1b[38;2;0;136;0m", 
            blue: "\x1b[38;2;38;139;210m",
            yellow: "\x1b[38;2;181;137;0m",
            cyan: "\x1b[38;2;42;161;152m",
            magenta: "\x1b[38;2;211;54;130m",
            white: "\x1b[38;2;238;232;213m",
            black: "\x1b[38;2;0;43;54m",
            
            // Background colors
            bg_red: "\x1b[48;2;220;50;47m",
            bg_green: "\x1b[48;2;0;136;0m",
            bg_blue: "\x1b[48;2;38;139;210m", 
            bg_yellow: "\x1b[48;2;181;137;0m",
            bg_cyan: "\x1b[48;2;42;161;152m",
            bg_magenta: "\x1b[48;2;211;54;130m",
            bg_white: "\x1b[48;2;238;232;213m",
            bg_black: "\x1b[48;2;0;43;54m",
            
            // Semantic colors
            success: "\x1b[38;2;0;136;0m",     // Green
            error: "\x1b[38;2;220;50;47m",     // Red  
            warning: "\x1b[38;2;181;137;0m",   // Yellow
            info: "\x1b[38;2;38;139;210m",     // Blue
            debug: "\x1b[38;2;42;161;152m",    // Cyan
            muted: "\x1b[38;2;147;161;161m",   // Gray
            
            // Enhanced styling for modern terminals
            bold: "\x1b[1m",
            dim: "\x1b[2m", 
            italic: "\x1b[3m",
            underline: "\x1b[4m",
            reverse: "\x1b[7m",
            strikethrough: "\x1b[9m",
            reset: "\x1b[0m",
            
            // Unicode symbols based on unicode support
            checkmark: if caps.unicode == TerminalUnicodeCaps::ExtendedUnicode { "✓" } else { "+" },
            cross: if caps.unicode == TerminalUnicodeCaps::ExtendedUnicode { "✗" } else { "x" },
            warning_symbol: if caps.unicode == TerminalUnicodeCaps::ExtendedUnicode { "⚠" } else { "!" },
            info_symbol: if caps.unicode == TerminalUnicodeCaps::ExtendedUnicode { "ℹ" } else { "i" },
            bullet: if caps.unicode == TerminalUnicodeCaps::ExtendedUnicode { "●" } else { "*" },
            arrow: if caps.unicode == TerminalUnicodeCaps::ExtendedUnicode { "→" } else { "->" },
        }
    }
    
    /// 256-color primitives for capable terminals
    fn ansi256_primitives(caps: &BasicTerminalCapabilities) -> Self {
        Self {
            // 256-color enhanced palette
            red: "\x1b[38;5;160m",
            green: "\x1b[38;5;28m",
            blue: "\x1b[38;5;33m", 
            yellow: "\x1b[38;5;136m",
            cyan: "\x1b[38;5;37m",
            magenta: "\x1b[38;5;125m",
            white: "\x1b[38;5;230m",
            black: "\x1b[38;5;235m",
            
            // Background colors
            bg_red: "\x1b[48;5;160m",
            bg_green: "\x1b[48;5;28m",
            bg_blue: "\x1b[48;5;33m",
            bg_yellow: "\x1b[48;5;136m", 
            bg_cyan: "\x1b[48;5;37m",
            bg_magenta: "\x1b[48;5;125m",
            bg_white: "\x1b[48;5;230m",
            bg_black: "\x1b[48;5;235m",
            
            // Semantic colors
            success: "\x1b[38;5;28m",    // Green
            error: "\x1b[38;5;160m",     // Red
            warning: "\x1b[38;5;136m",   // Yellow  
            info: "\x1b[38;5;33m",       // Blue
            debug: "\x1b[38;5;37m",      // Cyan
            muted: "\x1b[38;5;243m",     // Gray
            
            // Standard styling
            bold: "\x1b[1m",
            dim: "\x1b[2m",
            italic: "\x1b[3m", 
            underline: "\x1b[4m",
            reverse: "\x1b[7m",
            strikethrough: "\x1b[9m",
            reset: "\x1b[0m",
            
            // Unicode symbols based on unicode support
            checkmark: if caps.unicode != TerminalUnicodeCaps::Ascii { "✓" } else { "+" },
            cross: if caps.unicode != TerminalUnicodeCaps::Ascii { "✗" } else { "x" },
            warning_symbol: if caps.unicode != TerminalUnicodeCaps::Ascii { "⚠" } else { "!" },
            info_symbol: if caps.unicode != TerminalUnicodeCaps::Ascii { "ℹ" } else { "i" },
            bullet: if caps.unicode != TerminalUnicodeCaps::Ascii { "●" } else { "*" },
            arrow: if caps.unicode != TerminalUnicodeCaps::Ascii { "→" } else { "->" },
        }
    }
    
    /// 16-color primitives for basic terminals
    fn ansi16_primitives(_caps: &BasicTerminalCapabilities) -> Self {
        Self {
            // Bright 16-color ANSI
            red: "\x1b[0;91m",      // Bright red
            green: "\x1b[0;92m",    // Bright green
            blue: "\x1b[0;94m",     // Bright blue
            yellow: "\x1b[0;93m",   // Bright yellow
            cyan: "\x1b[0;96m",     // Bright cyan
            magenta: "\x1b[0;95m",  // Bright magenta
            white: "\x1b[0;97m",    // Bright white
            black: "\x1b[0;30m",    // Normal black
            
            // Background colors
            bg_red: "\x1b[0;101m",
            bg_green: "\x1b[0;102m", 
            bg_blue: "\x1b[0;104m",
            bg_yellow: "\x1b[0;103m",
            bg_cyan: "\x1b[0;106m",
            bg_magenta: "\x1b[0;105m",
            bg_white: "\x1b[0;107m",
            bg_black: "\x1b[0;40m",
            
            // Semantic colors
            success: "\x1b[0;92m",    // Bright green
            error: "\x1b[0;91m",      // Bright red
            warning: "\x1b[0;93m",    // Bright yellow
            info: "\x1b[0;94m",       // Bright blue
            debug: "\x1b[0;96m",      // Bright cyan
            muted: "\x1b[2;37m",      // Dim white
            
            // Basic styling
            bold: "\x1b[1m",
            dim: "\x1b[2m",
            italic: "\x1b[3m",
            underline: "\x1b[4m", 
            reverse: "\x1b[7m",
            strikethrough: "",        // Not supported
            reset: "\x1b[0m",
            
            // ASCII symbols only
            checkmark: "+",
            cross: "x", 
            warning_symbol: "!",
            info_symbol: "i",
            bullet: "*",
            arrow: "->",
        }
    }
    
    /// No-color primitives for very basic terminals
    fn no_color_primitives(_caps: &BasicTerminalCapabilities) -> Self {
        Self {
            // All colors are empty strings
            red: "",
            green: "",
            blue: "", 
            yellow: "",
            cyan: "",
            magenta: "",
            white: "",
            black: "",
            
            // No background colors
            bg_red: "",
            bg_green: "",
            bg_blue: "",
            bg_yellow: "",
            bg_cyan: "",
            bg_magenta: "",
            bg_white: "",
            bg_black: "",
            
            // No semantic colors
            success: "",
            error: "",
            warning: "",
            info: "",
            debug: "",
            muted: "",
            
            // Limited styling (only what works everywhere)
            bold: "\x1b[1m",      // Bold usually works
            dim: "",              // Skip dim
            italic: "",           // Skip italic
            underline: "",        // Skip underline
            reverse: "\x1b[7m",   // Reverse usually works
            strikethrough: "",    // Not supported
            reset: "\x1b[0m",     // Reset always works
            
            // ASCII symbols only
            checkmark: "+",
            cross: "x",
            warning_symbol: "!",
            info_symbol: "i", 
            bullet: "*",
            arrow: "->",
        }
    }
    
}

/// Global terminal primitives instance (initialized on first use)
use std::sync::OnceLock;
static TERMINAL_PRIMITIVES: OnceLock<TerminalPrimitives> = OnceLock::new();

/// Initialize global terminal primitives with detected capabilities
pub fn init_primitives(caps: &BasicTerminalCapabilities) {
    TERMINAL_PRIMITIVES.set(TerminalPrimitives::new(caps)).ok();
}

/// Get reference to global terminal primitives (auto-initializes with basic fallback)
pub fn primitives() -> &'static TerminalPrimitives {
    TERMINAL_PRIMITIVES.get_or_init(|| {
        // Fallback to no-color primitives if not explicitly initialized
        let basic_caps = BasicTerminalCapabilities {
            color: TerminalColorCaps::None,
            unicode: TerminalUnicodeCaps::Ascii,
            graphics: TerminalGraphicsCaps::None,
        };
        TerminalPrimitives::new(&basic_caps)
    })
}

// ============================================================================
// ADAPTER/BRIDGE FUNCTIONS FOR TERMINAL MODULE
// ============================================================================

/// Convert complex TerminalCapabilities to shared BasicTerminalCapabilities
/// This allows the terminal module to use its detailed detection results
/// with the shared primitives system
pub fn from_terminal_capabilities(caps: &crate::terminal::TerminalCapabilities) -> BasicTerminalCapabilities {
    // Convert the terminal module's graphics capabilities to the shared type
    let shared_graphics = match caps.graphics {
        crate::terminal::TerminalGraphicsCaps::None => TerminalGraphicsCaps::None,
        crate::terminal::TerminalGraphicsCaps::Kitty(kitty_caps) => TerminalGraphicsCaps::Kitty(KittyGraphicsCaps {
            supports_direct: kitty_caps.supports_direct,
            supports_file: kitty_caps.supports_file,
            supports_temp_file: kitty_caps.supports_temp_file,
            supports_shared_memory: kitty_caps.supports_shared_memory,
            supports_animation: kitty_caps.supports_animation,
            supports_unicode_placeholders: kitty_caps.supports_unicode_placeholders,
            supports_z_index: kitty_caps.supports_z_index,
            cell_width_pixels: kitty_caps.cell_width_pixels,
            cell_height_pixels: kitty_caps.cell_height_pixels,
            max_image_width: kitty_caps.max_image_width,
            max_image_height: kitty_caps.max_image_height,
            protocol_version: kitty_caps.protocol_version,
            detection_method: match kitty_caps.detection_method {
                crate::terminal::GraphicsDetectionMethod::EnvironmentReliable => GraphicsDetectionMethod::EnvironmentReliable,
                crate::terminal::GraphicsDetectionMethod::EnvironmentVariables => GraphicsDetectionMethod::EnvironmentVariables,
                crate::terminal::GraphicsDetectionMethod::ProtocolProbe => GraphicsDetectionMethod::ProtocolProbe,
                crate::terminal::GraphicsDetectionMethod::ProtocolProbeTimeout => GraphicsDetectionMethod::ProtocolProbeTimeout,
            },
        }),
        crate::terminal::TerminalGraphicsCaps::Sixel(sixel_caps) => TerminalGraphicsCaps::Sixel(SixelCaps {
            max_colors: sixel_caps.max_colors,
            max_width: sixel_caps.max_width,
            max_height: sixel_caps.max_height,
        }),
        crate::terminal::TerminalGraphicsCaps::ITerm2(iterm2_caps) => TerminalGraphicsCaps::ITerm2(ITerm2Caps {
            supports_inline: iterm2_caps.supports_inline,
            supports_file_download: iterm2_caps.supports_file_download,
        }),
    };

    BasicTerminalCapabilities {
        color: caps.color,
        unicode: caps.unicode,
        graphics: shared_graphics,
    }
}


/// Direct access to primitive escape codes for advanced usage
pub mod codes {
    use super::primitives;

    /// Get red color escape code for current terminal
    pub fn red() -> &'static str {
        primitives().red
    }

    /// Get green color escape code for current terminal
    pub fn green() -> &'static str {
        primitives().green
    }

    /// Get blue color escape code for current terminal
    pub fn blue() -> &'static str {
        primitives().blue
    }

    /// Get yellow color escape code for current terminal
    pub fn yellow() -> &'static str {
        primitives().yellow
    }

    /// Get cyan color escape code for current terminal
    pub fn cyan() -> &'static str {
        primitives().cyan
    }

    /// Get magenta color escape code for current terminal
    pub fn magenta() -> &'static str {
        primitives().magenta
    }

    /// Get white color escape code for current terminal
    pub fn white() -> &'static str {
        primitives().white
    }

    /// Get black color escape code for current terminal
    pub fn black() -> &'static str {
        primitives().black
    }

    /// Get bold style escape code for current terminal
    pub fn bold() -> &'static str {
        primitives().bold
    }

    /// Get dim style escape code for current terminal
    pub fn dim() -> &'static str {
        primitives().dim
    }

    /// Get italic style escape code for current terminal
    pub fn italic() -> &'static str {
        primitives().italic
    }

    /// Get underline style escape code for current terminal
    pub fn underline() -> &'static str {
        primitives().underline
    }

    /// Get reset escape code for current terminal
    pub fn reset() -> &'static str {
        primitives().reset
    }

    /// Get success color escape code for current terminal
    pub fn success() -> &'static str {
        primitives().success
    }

    /// Get error color escape code for current terminal
    pub fn error() -> &'static str {
        primitives().error
    }

    /// Get warning color escape code for current terminal
    pub fn warning() -> &'static str {
        primitives().warning
    }

    /// Get info color escape code for current terminal
    pub fn info() -> &'static str {
        primitives().info
    }

    /// Get debug color escape code for current terminal
    pub fn debug() -> &'static str {
        primitives().debug
    }

    /// Get muted color escape code for current terminal
    pub fn muted() -> &'static str {
        primitives().muted
    }
}
