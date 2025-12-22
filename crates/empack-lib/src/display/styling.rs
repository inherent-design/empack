//! Terminal-aware styling using empack's primitive system
//!
//! Provides semantic color schemes and symbols that leverage the existing
//! terminal primitives and capability detection.

use crate::primitives::terminal::{
    TerminalPrimitives, from_terminal_capabilities, init_primitives, primitives,
};
use crate::terminal::TerminalCapabilities;

/// Style manager that uses empack's terminal primitives
pub struct StyleManager {
    primitives: &'static TerminalPrimitives,
}

impl StyleManager {
    pub fn new(capabilities: &TerminalCapabilities) -> Self {
        // Convert detailed capabilities to basic capabilities and initialize primitives
        let basic_caps = from_terminal_capabilities(capabilities);
        init_primitives(&basic_caps);

        Self {
            primitives: primitives(),
        }
    }

    /// Style text with semantic success intent
    pub fn style_success(&self, text: &str) -> String {
        format!(
            "{}{}{}",
            self.primitives.success, text, self.primitives.reset
        )
    }

    /// Style text with semantic error intent
    pub fn style_error(&self, text: &str) -> String {
        format!("{}{}{}", self.primitives.error, text, self.primitives.reset)
    }

    /// Style text with semantic warning intent
    pub fn style_warning(&self, text: &str) -> String {
        format!(
            "{}{}{}",
            self.primitives.warning, text, self.primitives.reset
        )
    }

    /// Style text with semantic info intent
    pub fn style_info(&self, text: &str) -> String {
        format!("{}{}{}", self.primitives.info, text, self.primitives.reset)
    }

    /// Style text with emphasis (bold)
    pub fn style_emphasis(&self, text: &str) -> String {
        format!("{}{}{}", self.primitives.bold, text, self.primitives.reset)
    }

    /// Style text as subtle/muted
    pub fn style_subtle(&self, text: &str) -> String {
        format!("{}{}{}", self.primitives.muted, text, self.primitives.reset)
    }

    /// Get access to all terminal primitives
    pub fn primitives(&self) -> &'static TerminalPrimitives {
        self.primitives
    }

    /// Format success message with symbol and styling
    pub fn format_success(&self, message: &str) -> String {
        format!(
            "{} {}",
            self.style_success(self.primitives.checkmark),
            message
        )
    }

    /// Format error message with symbol and styling
    pub fn format_error(&self, message: &str) -> String {
        format!("{} {}", self.style_error(self.primitives.cross), message)
    }

    /// Format warning message with symbol and styling
    pub fn format_warning(&self, message: &str) -> String {
        format!(
            "{} {}",
            self.style_warning(self.primitives.warning_symbol),
            message
        )
    }

    /// Format info message with symbol and styling
    pub fn format_info(&self, message: &str) -> String {
        format!(
            "{} {}",
            self.style_info(self.primitives.info_symbol),
            message
        )
    }

    /// Format working/progress message (using info color with arrow)
    pub fn format_working(&self, message: &str) -> String {
        format!("{} {}", self.style_info(self.primitives.arrow), message)
    }
}

/// Symbol access (using your primitives)
impl StyleManager {
    /// Get success symbol (checkmark) with appropriate styling
    pub fn success_symbol(&self) -> String {
        self.style_success(self.primitives.checkmark)
    }

    /// Get error symbol (cross) with appropriate styling
    pub fn error_symbol(&self) -> String {
        self.style_error(self.primitives.cross)
    }

    /// Get warning symbol with appropriate styling
    pub fn warning_symbol(&self) -> String {
        self.style_warning(self.primitives.warning_symbol)
    }

    /// Get info symbol with appropriate styling
    pub fn info_symbol(&self) -> String {
        self.style_info(self.primitives.info_symbol)
    }

    /// Get bullet symbol
    pub fn bullet(&self) -> &'static str {
        self.primitives.bullet
    }

    /// Get arrow symbol
    pub fn arrow(&self) -> &'static str {
        self.primitives.arrow
    }
}
