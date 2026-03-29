//! Progress display for long-running operations
//!
//! Provides progress bars and spinners using indicatif with
//! terminal-capability-aware styling.

use super::Display;
use super::styling::StyleManager;
use crate::primitives::terminal::TerminalUnicodeCaps;
use indicatif::{MultiProgress, ProgressBar, ProgressDrawTarget, ProgressStyle};
use std::time::Duration;

fn has_unicode() -> bool {
    Display::capabilities().unicode != TerminalUnicodeCaps::Ascii
}

/// Progress display manager for long-running operations
pub struct ProgressDisplay<'a> {
    styling: &'a StyleManager,
}

impl<'a> ProgressDisplay<'a> {
    pub(crate) fn new(styling: &'a StyleManager) -> Self {
        Self { styling }
    }

    /// Create a progress bar for operations with known total
    ///
    /// Example:
    /// ```
    /// let progress = Display::progress()
    ///     .message("Downloading mods")
    ///     .total(25);
    ///
    /// for i in 0..25 {
    ///     progress.set_position(i);
    ///     // ... download mod
    /// }
    ///
    /// progress.finish("Downloaded 25 mods");
    /// ```
    pub fn bar(&self, total: u64) -> ProgressTracker<'_> {
        let pb = ProgressBar::new(total);

        // Use terminal-appropriate progress style
        let style = if has_unicode() {
            // Unicode-capable terminal
            ProgressStyle::with_template(
                "{spinner:.green} {msg} [{wide_bar:.cyan/blue}] {pos}/{len} ({eta})",
            )
            .unwrap()
            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"])
            .progress_chars("█▉▊▋▌▍▎▏  ")
        } else {
            // ASCII-only terminal
            ProgressStyle::with_template("{spinner} {msg} [{wide_bar}] {pos}/{len} ({eta})")
                .unwrap()
                .tick_strings(&["-", "\\", "|", "/"])
                .progress_chars("##-")
        };

        pb.set_style(style);
        pb.enable_steady_tick(Duration::from_millis(100));

        ProgressTracker::new(pb, self.styling)
    }

    /// Create a spinner for operations with unknown duration
    ///
    /// Example:
    /// ```
    /// let spinner = Display::progress()
    ///     .spinner("Resolving dependencies");
    ///
    /// // ... long operation
    ///
    /// spinner.finish("Dependencies resolved");
    /// ```
    pub fn spinner(&self, message: &str) -> ProgressTracker<'_> {
        let pb = ProgressBar::new_spinner();

        let style = if has_unicode() {
            // Unicode spinner
            ProgressStyle::with_template("{spinner:.green} {msg}")
                .unwrap()
                .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"])
        } else {
            // ASCII spinner
            ProgressStyle::with_template("{spinner} {msg}")
                .unwrap()
                .tick_strings(&["-", "\\", "|", "/"])
        };

        pb.set_style(style);
        pb.set_message(message.to_string());
        pb.enable_steady_tick(Duration::from_millis(100));

        ProgressTracker::new(pb, self.styling)
    }

    /// Create a multi-progress manager for parallel operations
    pub fn multi(&self) -> MultiProgressTracker<'_> {
        MultiProgressTracker::new(self.styling)
    }
}

/// Individual progress tracker
pub struct ProgressTracker<'a> {
    bar: ProgressBar,
    styling: &'a StyleManager,
}

impl<'a> ProgressTracker<'a> {
    fn new(bar: ProgressBar, styling: &'a StyleManager) -> Self {
        Self { bar, styling }
    }

    /// Set the current position
    pub fn set_position(&self, pos: u64) {
        self.bar.set_position(pos);
    }

    /// Increment position by 1
    pub fn inc(&self) {
        self.bar.inc(1);
    }

    /// Increment position by n
    pub fn inc_by(&self, n: u64) {
        self.bar.inc(n);
    }

    /// Update the message
    pub fn set_message(&self, message: &str) {
        self.bar.set_message(message.to_string());
    }

    /// Update message with current item info
    pub fn tick(&self, item: &str) {
        self.bar.tick();
        self.bar.set_message(item.to_string());
    }

    /// Finish with success message
    pub fn finish(&self, message: &str) {
        self.bar
            .finish_with_message(self.styling.format_success(message));
    }

    /// Abandon with error message
    pub fn abandon(&self, message: &str) {
        self.bar
            .abandon_with_message(self.styling.format_error(message));
    }

    /// Finish and clear the progress bar
    pub fn finish_clear(&self) {
        self.bar.finish_and_clear();
    }

    /// Get a reference to the underlying ProgressBar
    pub fn bar(&self) -> &ProgressBar {
        &self.bar
    }
}

/// Multi-progress manager for parallel operations
pub struct MultiProgressTracker<'a> {
    multi: MultiProgress,
    styling: &'a StyleManager,
}

impl<'a> MultiProgressTracker<'a> {
    fn new(styling: &'a StyleManager) -> Self {
        let multi = MultiProgress::new();
        // Hide by default, shown when bars are added
        multi.set_draw_target(ProgressDrawTarget::hidden());

        Self { multi, styling }
    }

    /// Add a progress bar to the multi-progress
    pub fn add_bar(&self, total: u64, message: &str) -> ProgressTracker<'_> {
        // Show the multi-progress when first bar is added
        if self.multi.is_hidden() {
            self.multi.set_draw_target(ProgressDrawTarget::stderr());
        }

        let pb = self.multi.add(ProgressBar::new(total));

        let style = if has_unicode() {
            ProgressStyle::with_template(
                "{spinner:.green} {msg} [{wide_bar:.cyan/blue}] {pos}/{len}",
            )
            .unwrap()
            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"])
            .progress_chars("█▉▊▋▌▍▎▏  ")
        } else {
            ProgressStyle::with_template("{spinner} {msg} [{wide_bar}] {pos}/{len}")
                .unwrap()
                .tick_strings(&["-", "\\", "|", "/"])
                .progress_chars("##-")
        };

        pb.set_style(style);
        pb.set_message(message.to_string());
        pb.enable_steady_tick(Duration::from_millis(100));

        ProgressTracker::new(pb, self.styling)
    }

    /// Add a spinner to the multi-progress
    pub fn add_spinner(&self, message: &str) -> ProgressTracker<'_> {
        if self.multi.is_hidden() {
            self.multi.set_draw_target(ProgressDrawTarget::stderr());
        }

        let pb = self.multi.add(ProgressBar::new_spinner());

        let style = if has_unicode() {
            ProgressStyle::with_template("{spinner:.green} {msg}")
                .unwrap()
                .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"])
        } else {
            ProgressStyle::with_template("{spinner} {msg}")
                .unwrap()
                .tick_strings(&["-", "\\", "|", "/"])
        };

        pb.set_style(style);
        pb.set_message(message.to_string());
        pb.enable_steady_tick(Duration::from_millis(100));

        ProgressTracker::new(pb, self.styling)
    }

    /// Clear all progress bars
    pub fn clear(&self) {
        self.multi.clear().unwrap_or(());
    }
}
