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
}

impl LiveDisplayProvider {
    pub fn new() -> Self {
        Self {
            multi_progress: Arc::new(MultiProgress::new()),
        }
    }

    pub fn new_with_arc(multi_progress: Arc<MultiProgress>) -> Self {
        Self { multi_progress }
    }
}

impl Default for LiveDisplayProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl DisplayProvider for LiveDisplayProvider {
    fn status(&self) -> Box<dyn StatusProvider> {
        Box::new(LiveStatusProvider)
    }

    fn progress(&self) -> Box<dyn ProgressProvider> {
        Box::new(LiveProgressProvider {
            parent: self.multi_progress.clone(),
        })
    }

    fn table(&self) -> Box<dyn StructuredProvider> {
        Box::new(LiveStructuredProvider)
    }
}

/// Live implementation of StatusProvider
struct LiveStatusProvider;

impl StatusProvider for LiveStatusProvider {
    fn checking(&self, task: &str) {
        Display::status().checking(task);
    }

    fn success(&self, item: &str, details: &str) {
        Display::status().success(item, details);
    }

    fn error(&self, item: &str, details: &str) {
        Display::status().error(item, details);
    }

    fn warning(&self, message: &str) {
        Display::status().warning(message);
    }

    fn info(&self, message: &str) {
        Display::status().info(message);
    }

    fn message(&self, text: &str) {
        Display::status().message(text);
    }

    fn emphasis(&self, text: &str) {
        Display::status().emphasis(text);
    }

    fn subtle(&self, text: &str) {
        Display::status().subtle(text);
    }

    fn list(&self, items: &[&str]) {
        Display::status().list(items);
    }

    fn complete(&self, task: &str) {
        Display::status().complete(task);
    }

    fn tool_check(&self, tool: &str, available: bool, version: &str) {
        Display::status().tool_check(tool, available, version);
    }

    fn section(&self, title: &str) {
        Display::status().section(title);
    }

    fn step(&self, current: usize, total: usize, description: &str) {
        Display::status().step(current, total, description);
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
struct LiveStructuredProvider;

impl StructuredProvider for LiveStructuredProvider {
    fn table(&self, headers: &[&str], rows: &[Vec<&str>]) {
        let display = Display::table();
        let mut table = display.table().header(headers);
        for row in rows {
            table = table.row(row);
        }
        table.render();
    }

    fn list(&self, items: &[&str]) {
        Display::table().list(items);
    }

    fn properties(&self, pairs: &[(&str, &str)]) {
        Display::table().pairs(pairs);
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
