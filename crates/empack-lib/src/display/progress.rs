//! Progress display for long-running operations
//!
//! Provides progress bars and spinners using indicatif with
//! terminal-capability-aware styling.

use super::styling::StyleManager;
use crate::terminal::display_line;
use indicatif::{MultiProgress, ProgressBar, ProgressDrawTarget, ProgressStyle};
use std::time::Duration;

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
    /// # use empack_lib::display::Display;
    /// let owner = Display::default();
    /// let display = owner.progress();
    /// let progress = display.bar(25);
    /// progress.set_message("Downloading mods");
    ///
    /// for i in 0..25 {
    ///     progress.set_position(i);
    ///     // ... download mod
    /// }
    ///
    /// progress.finish("Downloaded 25 mods");
    /// ```
    pub fn bar(&self, total: u64) -> ProgressTracker<'_> {
        self.bar_on(total, ProgressDrawTarget::stderr())
    }

    pub(crate) fn bar_on(&self, total: u64, target: ProgressDrawTarget) -> ProgressTracker<'_> {
        let pb = ProgressBar::with_draw_target(Some(total), target);

        pb.set_style(bar_style(self.styling, true));
        pb.enable_steady_tick(Duration::from_millis(100));

        ProgressTracker::new(pb, self.styling)
    }

    /// Create a spinner for operations with unknown duration
    ///
    /// Example:
    /// ```
    /// # use empack_lib::display::Display;
    /// let owner = Display::default();
    /// let display = owner.progress();
    /// let spinner = display.spinner("Resolving dependencies");
    ///
    /// // ... long operation
    ///
    /// spinner.finish("Dependencies resolved");
    /// ```
    pub fn spinner(&self, message: &str) -> ProgressTracker<'_> {
        self.spinner_on(message, ProgressDrawTarget::stderr())
    }

    pub(crate) fn spinner_on(
        &self,
        message: &str,
        target: ProgressDrawTarget,
    ) -> ProgressTracker<'_> {
        let pb = ProgressBar::with_draw_target(None, target);

        pb.set_style(spinner_style(self.styling));
        pb.set_message(display_line(message).into_owned());
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
        self.bar.set_message(display_line(message).into_owned());
    }

    /// Update message with current item info
    pub fn tick(&self, item: &str) {
        self.bar.tick();
        self.bar.set_message(display_line(item).into_owned());
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

        pb.set_style(bar_style(self.styling, false));
        pb.set_message(display_line(message).into_owned());
        pb.enable_steady_tick(Duration::from_millis(100));

        ProgressTracker::new(pb, self.styling)
    }

    /// Add a spinner to the multi-progress
    pub fn add_spinner(&self, message: &str) -> ProgressTracker<'_> {
        if self.multi.is_hidden() {
            self.multi.set_draw_target(ProgressDrawTarget::stderr());
        }

        let pb = self.multi.add(ProgressBar::new_spinner());

        pb.set_style(spinner_style(self.styling));
        pb.set_message(display_line(message).into_owned());
        pb.enable_steady_tick(Duration::from_millis(100));

        ProgressTracker::new(pb, self.styling)
    }

    /// Clear all progress bars
    pub fn clear(&self) {
        self.multi.clear().unwrap_or(());
    }
}

// Template dotted styles consult console's process-global stdout policy. The
// session's selected palette already owns color for this progress destination.
fn spinner_style(styling: &StyleManager) -> ProgressStyle {
    let palette = styling.primitives();
    ProgressStyle::with_template(&format!(
        "{}{{spinner}}{} {{msg}}",
        palette.success, palette.reset
    ))
    .unwrap()
    .tick_strings(spinner_frames(styling))
}

fn bar_style(styling: &StyleManager, show_eta: bool) -> ProgressStyle {
    let palette = styling.primitives();
    ProgressStyle::with_template(&format!(
        "{}{{spinner}}{} {{msg}} [{}{{wide_bar}}{}] {{pos}}/{{len}}{}",
        palette.success,
        palette.reset,
        palette.info,
        palette.reset,
        if show_eta { " ({eta})" } else { "" }
    ))
    .unwrap()
    .tick_strings(spinner_frames(styling))
    .progress_chars(if styling.has_unicode() {
        "█▉▊▋▌▍▎▏  "
    } else {
        "##-"
    })
}

fn spinner_frames(styling: &StyleManager) -> &'static [&'static str] {
    if styling.has_unicode() {
        &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]
    } else {
        &["-", "\\", "|", "/"]
    }
}
