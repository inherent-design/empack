//! Live display provider implementation
//!
//! Production implementation of display providers using the existing
//! display system with indicatif.

use super::Display;
use super::providers::*;
use crate::terminal::display_line;
use indicatif::{MultiProgress, ProgressBar, ProgressDrawTarget};
use std::sync::Arc;

/// Live implementation of DisplayProvider that owns display state for command lifecycle
pub struct LiveDisplayProvider {
    multi_progress: Arc<MultiProgress>,
    stdout: Arc<Display>,
    stderr: Arc<Display>,
}

impl LiveDisplayProvider {
    pub fn new() -> Self {
        Self {
            multi_progress: Arc::new(MultiProgress::new()),
            stdout: Arc::new(Display::default()),
            stderr: Arc::new(Display::default()),
        }
    }

    pub fn with_stream_capabilities(
        mut self,
        stdout: crate::terminal::TerminalCapabilities,
        stderr: crate::terminal::TerminalCapabilities,
    ) -> Self {
        self.stdout = Arc::new(Display::new(stdout));
        self.stderr = Arc::new(Display::new(stderr));
        self
    }

    pub fn new_with_arc(multi_progress: Arc<MultiProgress>) -> Self {
        Self {
            multi_progress,
            stdout: Arc::new(Display::default()),
            stderr: Arc::new(Display::default()),
        }
    }
}

impl Default for LiveDisplayProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl DisplayProvider for LiveDisplayProvider {
    fn status(&self) -> Box<dyn StatusProvider> {
        Box::new(LiveStatusProvider {
            parent: self.multi_progress.clone(),
            stdout: self.stdout.clone(),
            stderr: self.stderr.clone(),
        })
    }

    fn progress(&self) -> Box<dyn ProgressProvider> {
        Box::new(LiveProgressProvider {
            parent: self.multi_progress.clone(),
            display: self.stderr.clone(),
        })
    }

    fn table(&self) -> Box<dyn StructuredProvider> {
        Box::new(LiveStructuredProvider {
            parent: self.multi_progress.clone(),
            display: self.stdout.clone(),
        })
    }
}

/// Live implementation of StatusProvider
struct LiveStatusProvider {
    parent: Arc<MultiProgress>,
    stdout: Arc<Display>,
    stderr: Arc<Display>,
}

impl StatusProvider for LiveStatusProvider {
    fn checking(&self, task: &str) {
        self.parent.suspend(|| self.stderr.status().checking(task));
    }

    fn success(&self, item: &str, details: &str) {
        self.parent
            .suspend(|| self.stdout.status().success(item, details));
    }

    fn error(&self, item: &str, details: &str) {
        self.parent
            .suspend(|| self.stderr.status().error(item, details));
    }

    fn warning(&self, message: &str) {
        self.parent
            .suspend(|| self.stderr.status().warning(message));
    }

    fn info(&self, message: &str) {
        self.parent.suspend(|| self.stdout.status().info(message));
    }

    fn message(&self, text: &str) {
        self.parent.suspend(|| self.stdout.status().message(text));
    }

    fn emphasis(&self, text: &str) {
        self.parent.suspend(|| self.stdout.status().emphasis(text));
    }

    fn subtle(&self, text: &str) {
        self.parent.suspend(|| self.stdout.status().subtle(text));
    }

    fn list(&self, items: &[&str]) {
        self.parent.suspend(|| self.stdout.status().list(items));
    }

    fn complete(&self, task: &str) {
        self.parent.suspend(|| self.stdout.status().complete(task));
    }

    fn tool_check(&self, tool: &str, available: bool, version: &str) {
        let output = if available {
            &self.stdout
        } else {
            &self.stderr
        };
        self.parent
            .suspend(|| output.status().tool_check(tool, available, version));
    }

    fn section(&self, title: &str) {
        self.parent.suspend(|| self.stdout.status().section(title));
    }

    fn step(&self, current: usize, total: usize, description: &str) {
        self.parent
            .suspend(|| self.stdout.status().step(current, total, description));
    }
}

/// Live implementation of ProgressProvider with owned state
struct LiveProgressProvider {
    parent: Arc<MultiProgress>,
    display: Arc<Display>,
}

impl ProgressProvider for LiveProgressProvider {
    fn message(&self, message: &str) {
        self.parent.suspend(|| {
            use std::io::Write;
            let _ = writeln!(std::io::stderr().lock(), "{}", display_line(message));
        });
    }

    fn bar(&self, total: u64) -> Box<dyn ProgressTracker> {
        // Attach before any visible draw. A standalone frame cannot be cleared
        // later by MultiProgress after its draw target has been replaced.
        let progress_bar = self
            .display
            .progress()
            .bar_on(total, ProgressDrawTarget::hidden())
            .bar()
            .clone();
        let bar = self.parent.add(progress_bar);
        Box::new(SimpleProgressTracker::new(bar))
    }

    fn spinner(&self, message: &str) -> Box<dyn ProgressTracker> {
        let progress_bar = self
            .display
            .progress()
            .spinner_on(message, ProgressDrawTarget::hidden())
            .bar()
            .clone();
        let bar = self.parent.add(progress_bar);
        Box::new(SimpleProgressTracker::new(bar))
    }

    fn multi(&self) -> Box<dyn MultiProgressProvider> {
        Box::new(LiveMultiProgressProvider {
            parent: self.parent.clone(),
            display: self.display.clone(),
        })
    }
}

/// Multi-progress provider with owned state
struct LiveMultiProgressProvider {
    parent: Arc<MultiProgress>,
    display: Arc<Display>,
}

impl MultiProgressProvider for LiveMultiProgressProvider {
    fn add_bar(&self, total: u64, message: &str) -> Box<dyn ProgressTracker> {
        let progress_bar = self
            .display
            .progress()
            .bar_on(total, ProgressDrawTarget::hidden())
            .bar()
            .clone();
        progress_bar.set_message(display_line(message).into_owned());
        let bar = self.parent.add(progress_bar);
        Box::new(SimpleProgressTracker::new(bar))
    }

    fn add_spinner(&self, message: &str) -> Box<dyn ProgressTracker> {
        let progress_bar = self
            .display
            .progress()
            .spinner_on(message, ProgressDrawTarget::hidden())
            .bar()
            .clone();
        let bar = self.parent.add(progress_bar);
        Box::new(SimpleProgressTracker::new(bar))
    }

    fn clear(&self) {
        // no-op: independent progress bars clear themselves on finish/abandon
    }
}

/// Live implementation of StructuredProvider
struct LiveStructuredProvider {
    parent: Arc<MultiProgress>,
    display: Arc<Display>,
}

impl StructuredProvider for LiveStructuredProvider {
    fn table(&self, headers: &[&str], rows: &[Vec<&str>]) {
        self.parent.suspend(|| {
            let display = self.display.table();
            let mut table = display.table().header(headers);
            for row in rows {
                table = table.row(row);
            }
            table.render();
        });
    }

    fn list(&self, items: &[&str]) {
        self.parent.suspend(|| self.display.table().list(items));
    }

    fn properties(&self, pairs: &[(&str, &str)]) {
        self.parent.suspend(|| self.display.table().pairs(pairs));
    }
}

/// Simple progress tracker that wraps indicatif ProgressBar directly
struct SimpleProgressTracker {
    bar: ProgressBar,
}

impl SimpleProgressTracker {
    fn new(bar: ProgressBar) -> Self {
        Self { bar }
    }
}

impl ProgressTracker for SimpleProgressTracker {
    fn set_position(&self, pos: u64) {
        self.bar.set_position(pos);
    }

    fn inc(&self) {
        self.bar.inc(1);
    }

    fn inc_by(&self, n: u64) {
        self.bar.inc(n);
    }

    fn set_message(&self, message: &str) {
        self.bar.set_message(display_line(message).into_owned());
    }

    fn tick(&self, item: &str) {
        self.bar.tick();
        self.bar.set_message(display_line(item).into_owned());
    }

    fn finish(&self, message: &str) {
        self.bar
            .finish_with_message(display_line(message).into_owned());
    }

    fn abandon(&self, message: &str) {
        self.bar
            .abandon_with_message(display_line(message).into_owned());
    }

    fn finish_clear(&self) {
        self.bar.finish_and_clear();
    }
}

#[cfg(test)]
mod tests {
    include!("live.test.rs");
}
