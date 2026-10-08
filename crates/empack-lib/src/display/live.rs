//! Live display provider implementation
//!
//! Production implementation of display providers using the existing
//! display system with indicatif.

use super::Display;
use super::providers::*;
use indicatif::{MultiProgress, ProgressBar};
use std::sync::Arc;

/// Live implementation of DisplayProvider that owns display state for command lifecycle
pub struct LiveDisplayProvider {
    multi_progress: Arc<MultiProgress>,
    display: Arc<Display>,
}

impl LiveDisplayProvider {
    pub fn new() -> Self {
        Self {
            multi_progress: Arc::new(MultiProgress::new()),
            display: Arc::new(Display::default()),
        }
    }

    pub fn with_capabilities(
        mut self,
        capabilities: crate::terminal::TerminalCapabilities,
    ) -> Self {
        self.display = Arc::new(Display::new(capabilities));
        self
    }

    pub fn new_with_arc(multi_progress: Arc<MultiProgress>) -> Self {
        Self {
            multi_progress,
            display: Arc::new(Display::default()),
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
            display: self.display.clone(),
        })
    }

    fn progress(&self) -> Box<dyn ProgressProvider> {
        Box::new(LiveProgressProvider {
            parent: self.multi_progress.clone(),
        })
    }

    fn table(&self) -> Box<dyn StructuredProvider> {
        Box::new(LiveStructuredProvider {
            display: self.display.clone(),
        })
    }
}

/// Live implementation of StatusProvider
struct LiveStatusProvider {
    display: Arc<Display>,
}

impl StatusProvider for LiveStatusProvider {
    fn checking(&self, task: &str) {
        self.display.status().checking(task);
    }

    fn success(&self, item: &str, details: &str) {
        self.display.status().success(item, details);
    }

    fn error(&self, item: &str, details: &str) {
        self.display.status().error(item, details);
    }

    fn warning(&self, message: &str) {
        self.display.status().warning(message);
    }

    fn info(&self, message: &str) {
        self.display.status().info(message);
    }

    fn message(&self, text: &str) {
        self.display.status().message(text);
    }

    fn emphasis(&self, text: &str) {
        self.display.status().emphasis(text);
    }

    fn subtle(&self, text: &str) {
        self.display.status().subtle(text);
    }

    fn list(&self, items: &[&str]) {
        self.display.status().list(items);
    }

    fn complete(&self, task: &str) {
        self.display.status().complete(task);
    }

    fn tool_check(&self, tool: &str, available: bool, version: &str) {
        self.display.status().tool_check(tool, available, version);
    }

    fn section(&self, title: &str) {
        self.display.status().section(title);
    }

    fn step(&self, current: usize, total: usize, description: &str) {
        self.display.status().step(current, total, description);
    }
}

/// Live implementation of ProgressProvider with owned state
struct LiveProgressProvider {
    parent: Arc<MultiProgress>,
}

impl ProgressProvider for LiveProgressProvider {
    fn bar(&self, total: u64) -> Box<dyn ProgressTracker> {
        let progress_bar = indicatif::ProgressBar::new(total);
        let bar = self.parent.add(progress_bar);
        Box::new(SimpleProgressTracker::new(bar))
    }

    fn spinner(&self, message: &str) -> Box<dyn ProgressTracker> {
        let progress_bar = indicatif::ProgressBar::new_spinner();
        progress_bar.set_message(message.to_string());
        let bar = self.parent.add(progress_bar);
        Box::new(SimpleProgressTracker::new(bar))
    }

    fn multi(&self) -> Box<dyn MultiProgressProvider> {
        Box::new(LiveMultiProgressProvider {
            parent: self.parent.clone(),
        })
    }
}

/// Multi-progress provider with owned state
struct LiveMultiProgressProvider {
    parent: Arc<MultiProgress>,
}

impl MultiProgressProvider for LiveMultiProgressProvider {
    fn add_bar(&self, total: u64, _message: &str) -> Box<dyn ProgressTracker> {
        let progress_bar = indicatif::ProgressBar::new(total);
        let bar = self.parent.add(progress_bar);
        Box::new(SimpleProgressTracker::new(bar))
    }

    fn add_spinner(&self, message: &str) -> Box<dyn ProgressTracker> {
        let progress_bar = indicatif::ProgressBar::new_spinner();
        progress_bar.set_message(message.to_string());
        let bar = self.parent.add(progress_bar);
        Box::new(SimpleProgressTracker::new(bar))
    }

    fn clear(&self) {
        // no-op: independent progress bars clear themselves on finish/abandon
    }
}

/// Live implementation of StructuredProvider
struct LiveStructuredProvider {
    display: Arc<Display>,
}

impl StructuredProvider for LiveStructuredProvider {
    fn table(&self, headers: &[&str], rows: &[Vec<&str>]) {
        let display = self.display.table();
        let mut table = display.table().header(headers);
        for row in rows {
            table = table.row(row);
        }
        table.render();
    }

    fn list(&self, items: &[&str]) {
        self.display.table().list(items);
    }

    fn properties(&self, pairs: &[(&str, &str)]) {
        self.display.table().pairs(pairs);
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
        self.bar.set_message(message.to_string());
    }

    fn tick(&self, item: &str) {
        self.bar.tick();
        self.bar.set_message(item.to_string());
    }

    fn finish(&self, message: &str) {
        self.bar.finish_with_message(message.to_string());
    }

    fn abandon(&self, message: &str) {
        self.bar.abandon_with_message(message.to_string());
    }

    fn finish_clear(&self) {
        self.bar.finish_and_clear();
    }
}

#[cfg(test)]
mod tests {
    include!("live.test.rs");
}
