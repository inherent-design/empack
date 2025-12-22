//! Interactive prompts and user input
//!
//! Provides semantic APIs for user interaction using dialoguer with
//! terminal-capability-aware styling.

use super::styling::StyleManager;
use crate::primitives::TerminalColorCaps;
use crate::terminal::TerminalCapabilities;
use anyhow::Result;
use dialoguer::{
    Confirm, Input, Select,
    theme::{ColorfulTheme, SimpleTheme, Theme},
};
use std::io;

/// Interactive display manager for prompts and selections
pub struct InteractiveDisplay<'a> {
    styling: &'a StyleManager,
    theme: Box<dyn Theme>,
}

impl<'a> InteractiveDisplay<'a> {
    pub(crate) fn new(styling: &'a StyleManager, capabilities: &TerminalCapabilities) -> Self {
        let theme: Box<dyn Theme> = match capabilities.color {
            TerminalColorCaps::None => {
                // Use simple theme for no-color terminals
                Box::new(SimpleTheme)
            }
            _ => {
                // Use colorful theme for color-capable terminals
                Box::new(ColorfulTheme::default())
            }
        };

        Self { styling, theme }
    }

    /// Create a yes/no confirmation prompt
    ///
    /// Example:
    /// ```
    /// let confirmed = Display::prompt()
    ///     .confirm("Overwrite existing files?")
    ///     .default(false)
    ///     .interact()?;
    /// ```
    pub fn confirm(&self, message: &str) -> ConfirmPrompt {
        ConfirmPrompt::new(message, &*self.theme)
    }

    /// Create a selection prompt
    ///
    /// Example:
    /// ```
    /// let choice = Display::prompt()
    ///     .select("Choose modloader:")
    ///     .options(&["Fabric", "Quilt", "NeoForge"])
    ///     .interact()?;
    /// ```
    pub fn select(&self, message: &str) -> SelectPrompt {
        SelectPrompt::new(message, &*self.theme)
    }

    /// Create a text input prompt
    ///
    /// Example:
    /// ```
    /// let name = Display::prompt()
    ///     .input("Project name:")
    ///     .default("my-modpack")
    ///     .interact()?;
    /// ```
    pub fn input(&self, message: &str) -> InputPrompt {
        InputPrompt::new(message, &*self.theme)
    }
}

/// Confirmation prompt builder
pub struct ConfirmPrompt<'a> {
    confirm: Confirm<'a>,
}

impl<'a> ConfirmPrompt<'a> {
    fn new(message: &str, theme: &'a dyn Theme) -> Self {
        let confirm = Confirm::with_theme(theme).with_prompt(message);

        Self { confirm }
    }

    /// Set default value for the confirmation
    pub fn default(mut self, default: bool) -> Self {
        self.confirm = self.confirm.default(default);
        self
    }

    /// Execute the prompt and get user response
    pub fn interact(self) -> Result<bool> {
        Ok(self.confirm.interact()?)
    }

    /// Execute the prompt and get user response (fallback to default on error)
    pub fn interact_opt(self) -> Result<Option<bool>> {
        Ok(self.confirm.interact_opt()?)
    }
}

/// Selection prompt builder
pub struct SelectPrompt<'a> {
    select: Select<'a>,
    options: Vec<String>,
}

impl<'a> SelectPrompt<'a> {
    fn new(message: &str, theme: &'a dyn Theme) -> Self {
        let select = Select::with_theme(theme).with_prompt(message);

        Self {
            select,
            options: Vec::new(),
        }
    }

    /// Set options for selection
    pub fn options(mut self, options: &[&str]) -> Self {
        self.options = options.iter().map(|s| s.to_string()).collect();
        for option in &self.options {
            self.select = self.select.item(option);
        }
        self
    }

    /// Set default selected index
    pub fn default(mut self, index: usize) -> Self {
        self.select = self.select.default(index);
        self
    }

    /// Execute the prompt and get selected index
    pub fn interact(self) -> Result<usize> {
        Ok(self.select.interact()?)
    }

    /// Execute the prompt and get selected value
    pub fn interact_value(self) -> Result<String> {
        let index = self.select.interact()?;
        Ok(self.options.get(index).unwrap_or(&String::new()).clone())
    }

    /// Execute the prompt and get selected index (fallback on error)
    pub fn interact_opt(self) -> Result<Option<usize>> {
        Ok(self.select.interact_opt()?)
    }
}

/// Text input prompt builder
pub struct InputPrompt<'a> {
    input: Input<'a, String>,
}

impl<'a> InputPrompt<'a> {
    fn new(message: &str, theme: &'a dyn Theme) -> Self {
        let input = Input::with_theme(theme).with_prompt(message);

        Self { input }
    }

    /// Set default value for input
    pub fn default(mut self, default: &str) -> Self {
        self.input = self.input.default(default.to_string());
        self
    }

    /// Allow empty input
    pub fn allow_empty(mut self, allow: bool) -> Self {
        self.input = self.input.allow_empty(allow);
        self
    }

    /// Execute the prompt and get user input
    pub fn interact(self) -> Result<String> {
        Ok(self.input.interact()?)
    }

    /// Execute the prompt with fallback on error
    pub fn interact_text(self) -> Result<Option<String>> {
        Ok(Some(self.input.interact_text()?))
    }
}

/// Convenience functions for common patterns
impl<'a> InteractiveDisplay<'a> {
    /// Quick yes/no prompt with default
    pub fn yes_no(&self, message: &str, default: bool) -> Result<bool> {
        self.confirm(message).default(default).interact()
    }

    /// Quick selection with string return
    pub fn choose(&self, message: &str, options: &[&str]) -> Result<String> {
        self.select(message).options(options).interact_value()
    }

    /// Quick text input with validation
    pub fn ask(&self, message: &str, default: Option<&str>) -> Result<String> {
        let mut prompt = self.input(message);
        if let Some(default) = default {
            prompt = prompt.default(default);
        }
        prompt.interact()
    }

    /// Handle non-interactive environment gracefully
    pub fn is_interactive() -> bool {
        use std::io::IsTerminal;
        io::stdin().is_terminal()
    }

    /// Prompt with non-interactive fallback
    pub fn confirm_or_default(&self, message: &str, default: bool) -> bool {
        if Self::is_interactive() {
            self.yes_no(message, default).unwrap_or(default)
        } else {
            default
        }
    }

    /// Select with non-interactive fallback to first option
    pub fn select_or_default(
        &self,
        message: &str,
        options: &[&str],
        default_index: usize,
    ) -> String {
        if Self::is_interactive() {
            self.select(message)
                .options(options)
                .default(default_index)
                .interact_value()
                .unwrap_or_else(|_| options.get(default_index).unwrap_or(&"").to_string())
        } else {
            options.get(default_index).unwrap_or(&"").to_string()
        }
    }
}
