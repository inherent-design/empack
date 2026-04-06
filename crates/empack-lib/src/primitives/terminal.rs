use super::shared::impl_fromstr_for_value_enum;
use clap::ValueEnum;
use std::str::FromStr;
use thiserror::Error;

// ============================================================================
// SHARED TERMINAL CAPABILITY TYPES
// ============================================================================

/// Runtime color detection intent
#[derive(Debug, Clone, Copy, PartialEq, serde::Deserialize, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum TerminalCapsDetectIntent {
    /// Let module detect
    #[value(alias = "automatic", alias = "detect", alias = "default")]
    Auto,

    /// Explicitly enable (useful in non-interactive)
    #[value(alias = "force", alias = "on")]
    Always,

    /// Explicitly disable (also useful in non-interactive)
    #[value(alias = "off")]
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
#[derive(Debug, Clone, Copy, PartialEq, serde::Deserialize, Default)]
pub enum TerminalGraphicsCaps {
    #[default]
    None,
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
    #[error("Terminal response contains invalid UTF-8: {source}")]
    InvalidUtf8Response {
        #[from]
        source: std::string::FromUtf8Error,
    },

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

/// Terminal output that works everywhere
/// Same interface, different escape codes under the hood
#[derive(Debug, Clone)]
pub struct TerminalPrimitives {
    pub red: &'static str,
    pub green: &'static str,
    pub blue: &'static str,
    pub yellow: &'static str,
    pub cyan: &'static str,
    pub magenta: &'static str,
    pub white: &'static str,
    pub black: &'static str,

    pub bg_red: &'static str,
    pub bg_green: &'static str,
    pub bg_blue: &'static str,
    pub bg_yellow: &'static str,
    pub bg_cyan: &'static str,
    pub bg_magenta: &'static str,
    pub bg_white: &'static str,
    pub bg_black: &'static str,

    pub success: &'static str,
    pub error: &'static str,
    pub warning: &'static str,
    pub info: &'static str,
    pub debug: &'static str,
    pub muted: &'static str,

    pub bold: &'static str,
    pub dim: &'static str,
    pub italic: &'static str,
    pub underline: &'static str,
    pub reverse: &'static str,
    pub strikethrough: &'static str,
    pub reset: &'static str,

    pub checkmark: &'static str,
    pub cross: &'static str,
    pub warning_symbol: &'static str,
    pub info_symbol: &'static str,
    pub bullet: &'static str,
    pub arrow: &'static str,
}

impl TerminalPrimitives {
    /// Create primitives from detected capabilities
    pub fn new(caps: &BasicTerminalCapabilities) -> Self {
        match caps.color {
            TerminalColorCaps::TrueColor => Self::truecolor_primitives(caps),
            TerminalColorCaps::Ansi256 => Self::ansi256_primitives(caps),
            TerminalColorCaps::Ansi16 => Self::ansi16_primitives(caps),
            TerminalColorCaps::None => Self::no_color_primitives(caps),
        }
    }

    /// True color (24-bit) primitives
    fn truecolor_primitives(caps: &BasicTerminalCapabilities) -> Self {
        Self {
            red: "\x1b[38;2;220;50;47m",
            green: "\x1b[38;2;0;136;0m",
            blue: "\x1b[38;2;38;139;210m",
            yellow: "\x1b[38;2;181;137;0m",
            cyan: "\x1b[38;2;42;161;152m",
            magenta: "\x1b[38;2;211;54;130m",
            white: "\x1b[38;2;238;232;213m",
            black: "\x1b[38;2;0;43;54m",

            bg_red: "\x1b[48;2;220;50;47m",
            bg_green: "\x1b[48;2;0;136;0m",
            bg_blue: "\x1b[48;2;38;139;210m",
            bg_yellow: "\x1b[48;2;181;137;0m",
            bg_cyan: "\x1b[48;2;42;161;152m",
            bg_magenta: "\x1b[48;2;211;54;130m",
            bg_white: "\x1b[48;2;238;232;213m",
            bg_black: "\x1b[48;2;0;43;54m",

            success: "\x1b[38;2;0;136;0m",
            error: "\x1b[38;2;220;50;47m",
            warning: "\x1b[38;2;181;137;0m",
            info: "\x1b[38;2;38;139;210m",
            debug: "\x1b[38;2;42;161;152m",
            muted: "\x1b[38;2;147;161;161m",

            bold: "\x1b[1m",
            dim: "\x1b[2m",
            italic: "\x1b[3m",
            underline: "\x1b[4m",
            reverse: "\x1b[7m",
            strikethrough: "\x1b[9m",
            reset: "\x1b[0m",

            checkmark: if caps.unicode == TerminalUnicodeCaps::ExtendedUnicode {
                "✓"
            } else {
                "+"
            },
            cross: if caps.unicode == TerminalUnicodeCaps::ExtendedUnicode {
                "✗"
            } else {
                "x"
            },
            warning_symbol: if caps.unicode == TerminalUnicodeCaps::ExtendedUnicode {
                "⚠"
            } else {
                "!"
            },
            info_symbol: if caps.unicode == TerminalUnicodeCaps::ExtendedUnicode {
                "ℹ"
            } else {
                "i"
            },
            bullet: if caps.unicode == TerminalUnicodeCaps::ExtendedUnicode {
                "●"
            } else {
                "*"
            },
            arrow: if caps.unicode == TerminalUnicodeCaps::ExtendedUnicode {
                "→"
            } else {
                "->"
            },
        }
    }

    /// 256-color primitives
    fn ansi256_primitives(caps: &BasicTerminalCapabilities) -> Self {
        Self {
            red: "\x1b[38;5;160m",
            green: "\x1b[38;5;28m",
            blue: "\x1b[38;5;33m",
            yellow: "\x1b[38;5;136m",
            cyan: "\x1b[38;5;37m",
            magenta: "\x1b[38;5;125m",
            white: "\x1b[38;5;230m",
            black: "\x1b[38;5;235m",

            bg_red: "\x1b[48;5;160m",
            bg_green: "\x1b[48;5;28m",
            bg_blue: "\x1b[48;5;33m",
            bg_yellow: "\x1b[48;5;136m",
            bg_cyan: "\x1b[48;5;37m",
            bg_magenta: "\x1b[48;5;125m",
            bg_white: "\x1b[48;5;230m",
            bg_black: "\x1b[48;5;235m",

            success: "\x1b[38;5;28m",
            error: "\x1b[38;5;160m",
            warning: "\x1b[38;5;136m",
            info: "\x1b[38;5;33m",
            debug: "\x1b[38;5;37m",
            muted: "\x1b[38;5;243m",

            bold: "\x1b[1m",
            dim: "\x1b[2m",
            italic: "\x1b[3m",
            underline: "\x1b[4m",
            reverse: "\x1b[7m",
            strikethrough: "\x1b[9m",
            reset: "\x1b[0m",

            checkmark: if caps.unicode != TerminalUnicodeCaps::Ascii {
                "✓"
            } else {
                "+"
            },
            cross: if caps.unicode != TerminalUnicodeCaps::Ascii {
                "✗"
            } else {
                "x"
            },
            warning_symbol: if caps.unicode != TerminalUnicodeCaps::Ascii {
                "⚠"
            } else {
                "!"
            },
            info_symbol: if caps.unicode != TerminalUnicodeCaps::Ascii {
                "ℹ"
            } else {
                "i"
            },
            bullet: if caps.unicode != TerminalUnicodeCaps::Ascii {
                "●"
            } else {
                "*"
            },
            arrow: if caps.unicode != TerminalUnicodeCaps::Ascii {
                "→"
            } else {
                "->"
            },
        }
    }

    /// 16-color primitives
    fn ansi16_primitives(_caps: &BasicTerminalCapabilities) -> Self {
        Self {
            red: "\x1b[0;91m",
            green: "\x1b[0;92m",
            blue: "\x1b[0;94m",
            yellow: "\x1b[0;93m",
            cyan: "\x1b[0;96m",
            magenta: "\x1b[0;95m",
            white: "\x1b[0;97m",
            black: "\x1b[0;30m",

            bg_red: "\x1b[0;101m",
            bg_green: "\x1b[0;102m",
            bg_blue: "\x1b[0;104m",
            bg_yellow: "\x1b[0;103m",
            bg_cyan: "\x1b[0;106m",
            bg_magenta: "\x1b[0;105m",
            bg_white: "\x1b[0;107m",
            bg_black: "\x1b[0;40m",

            success: "\x1b[0;92m",
            error: "\x1b[0;91m",
            warning: "\x1b[0;93m",
            info: "\x1b[0;94m",
            debug: "\x1b[0;96m",
            muted: "\x1b[2;37m",

            bold: "\x1b[1m",
            dim: "\x1b[2m",
            italic: "\x1b[3m",
            underline: "\x1b[4m",
            reverse: "\x1b[7m",
            strikethrough: "",
            reset: "\x1b[0m",

            checkmark: "+",
            cross: "x",
            warning_symbol: "!",
            info_symbol: "i",
            bullet: "*",
            arrow: "->",
        }
    }

    /// No-color primitives
    fn no_color_primitives(_caps: &BasicTerminalCapabilities) -> Self {
        Self {
            red: "",
            green: "",
            blue: "",
            yellow: "",
            cyan: "",
            magenta: "",
            white: "",
            black: "",

            bg_red: "",
            bg_green: "",
            bg_blue: "",
            bg_yellow: "",
            bg_cyan: "",
            bg_magenta: "",
            bg_white: "",
            bg_black: "",

            success: "",
            error: "",
            warning: "",
            info: "",
            debug: "",
            muted: "",

            bold: "",
            dim: "",
            italic: "",
            underline: "",
            reverse: "",
            strikethrough: "",
            reset: "",

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

/// Bridge detailed capabilities to basic shared types
pub fn from_terminal_capabilities(
    caps: &crate::terminal::TerminalCapabilities,
) -> BasicTerminalCapabilities {
    BasicTerminalCapabilities {
        color: caps.color,
        unicode: caps.unicode,
        graphics: TerminalGraphicsCaps::None,
    }
}
