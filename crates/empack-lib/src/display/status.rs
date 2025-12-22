//! Status display for user feedback
//!
//! Provides semantic status updates that replace raw println! usage
//! with terminal-capability-aware formatting.

use super::styling::StyleManager;
use std::io::{self, Write};

/// Status display manager for semantic user feedback
pub struct StatusDisplay<'a> {
    styling: &'a StyleManager,
}

impl<'a> StatusDisplay<'a> {
    pub(crate) fn new(styling: &'a StyleManager) -> Self {
        Self { styling }
    }

    /// Display a working/checking status
    ///
    /// Example: `Display::status().checking("tool dependencies")`
    /// Output: `⠋ Checking tool dependencies...`
    pub fn checking(&self, task: &str) {
        let message = format!("Checking {}...", task);
        println!("{}", self.styling.format_working(&message));
        io::stdout().flush().unwrap_or(());
    }

    /// Display a success status with optional details
    ///
    /// Example: `Display::status().success("packwiz", "v0.16.1")`
    /// Output: `✓ packwiz: v0.16.1`
    pub fn success(&self, item: &str, details: &str) {
        let message = if details.is_empty() {
            item.to_string()
        } else {
            format!("{}: {}", item, details)
        };
        println!("{}", self.styling.format_success(&message));
    }

    /// Display an error status with details
    ///
    /// Example: `Display::status().error("packwiz", "not found")`
    /// Output: `✗ packwiz: not found`
    pub fn error(&self, item: &str, details: &str) {
        let message = if details.is_empty() {
            item.to_string()
        } else {
            format!("{}: {}", item, details)
        };
        println!("{}", self.styling.format_error(&message));
    }

    /// Display a warning status with details
    ///
    /// Example: `Display::status().warning("experimental feature enabled")`
    /// Output: `! experimental feature enabled`
    pub fn warning(&self, message: &str) {
        println!("{}", self.styling.format_warning(message));
    }

    /// Display an info status
    ///
    /// Example: `Display::status().info("using default configuration")`
    /// Output: `· using default configuration`
    pub fn info(&self, message: &str) {
        println!("{}", self.styling.format_info(message));
    }

    /// Display a simple message without status symbols
    ///
    /// Example: `Display::status().message("Empack modpack manager")`
    /// Output: `Empack modpack manager`
    pub fn message(&self, text: &str) {
        println!("{}", text);
    }

    /// Display an emphasized message
    ///
    /// Example: `Display::status().emphasis("Configuration complete")`
    /// Output: `**Configuration complete**` (styled)
    pub fn emphasis(&self, text: &str) {
        println!("{}", self.styling.style_emphasis(text));
    }

    /// Display a subtle/secondary message
    ///
    /// Example: `Display::status().subtle("Run 'empack --help' for usage")`
    /// Output: subtle gray styled text
    pub fn subtle(&self, text: &str) {
        println!("{}", self.styling.style_subtle(text));
    }

    /// Display a list of items with bullets
    ///
    /// Example:
    /// ```
    /// Display::status().list(&[
    ///     "packwiz installed",
    ///     "archive tools available",
    ///     "configuration loaded"
    /// ]);
    /// ```
    pub fn list(&self, items: &[&str]) {
        for item in items {
            println!("  {} {}", self.styling.bullet(), item);
        }
    }

    /// Display a completion message
    ///
    /// Example: `Display::status().complete("Dependencies checked")`
    /// Output: `✓ Dependencies checked`
    pub fn complete(&self, task: &str) {
        println!("{}", self.styling.format_success(task));
    }
}

/// Convenience functions for common status patterns
impl<'a> StatusDisplay<'a> {
    /// Check and report tool availability
    ///
    /// Example: `Display::status().tool_check("packwiz", true, "v0.16.1")`
    pub fn tool_check(&self, tool: &str, available: bool, version: &str) {
        if available {
            self.success(tool, version);
        } else {
            self.error(tool, "not found");
        }
    }

    /// Display a header for a section of work
    ///
    /// Example: `Display::status().section("Checking Dependencies")`
    pub fn section(&self, title: &str) {
        println!();
        println!("{}", self.styling.style_emphasis(title));
    }

    /// Display a step in a multi-step process
    ///
    /// Example: `Display::status().step(1, 3, "Loading configuration")`
    /// Output: `[1/3] Loading configuration`
    pub fn step(&self, current: usize, total: usize, description: &str) {
        let prefix = format!("[{}/{}]", current, total);
        println!("{} {}", self.styling.style_subtle(&prefix), description);
    }
}
