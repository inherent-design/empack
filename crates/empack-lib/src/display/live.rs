//! Live display provider implementation
//!
//! Production implementation of display providers using the existing
//! display system with indicatif and dialoguer.

use super::providers::*;
use super::{Display, progress::{ProgressTracker as ConcreteProgressTracker, MultiProgressTracker}};
use indicatif::{MultiProgress, ProgressBar};
use std::sync::{Arc, Mutex};
use std::collections::HashMap;

/// Live implementation of DisplayProvider that owns display state for command lifecycle
pub struct LiveDisplayProvider {
    // Owned state for the entire command duration
    multi_progress: Arc<MultiProgress>,
    progress_trackers: Arc<Mutex<HashMap<usize, ConcreteProgressTracker<'static>>>>,
    next_tracker_id: Arc<Mutex<usize>>,
}

impl LiveDisplayProvider {
    pub fn new() -> Self {
        Self {
            multi_progress: Arc::new(MultiProgress::new()),
            progress_trackers: Arc::new(Mutex::new(HashMap::new())),
            next_tracker_id: Arc::new(Mutex::new(0)),
        }
    }
    
    pub fn new_with_multi_progress(multi_progress: &MultiProgress) -> Self {
        Self {
            multi_progress: Arc::new(multi_progress.clone()),
            progress_trackers: Arc::new(Mutex::new(HashMap::new())),
            next_tracker_id: Arc::new(Mutex::new(0)),
        }
    }
    
    fn get_next_tracker_id(&self) -> usize {
        let mut id = self.next_tracker_id.lock().unwrap();
        let current = *id;
        *id += 1;
        current
    }
}

impl DisplayProvider for LiveDisplayProvider {
    fn status(&self) -> Box<dyn StatusProvider> {
        Box::new(LiveStatusProvider)
    }
    
    fn progress(&self) -> Box<dyn ProgressProvider> {
        Box::new(LiveProgressProvider {
            parent: self.multi_progress.clone(),
            trackers: self.progress_trackers.clone(),
            next_id: self.next_tracker_id.clone(),
        })
    }
    
    fn prompt(&self) -> Box<dyn PromptProvider> {
        Box::new(LivePromptProvider)
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
    trackers: Arc<Mutex<HashMap<usize, ConcreteProgressTracker<'static>>>>,
    next_id: Arc<Mutex<usize>>,
}

impl ProgressProvider for LiveProgressProvider {
    fn bar(&self, total: u64) -> Box<dyn ProgressTracker> {
        // Create a simple progress bar adapter for the session-owned MultiProgress
        let progress_bar = indicatif::ProgressBar::new(total);
        let bar = self.parent.add(progress_bar);
        Box::new(SimpleProgressTracker::new(bar))
    }
    
    fn spinner(&self, message: &str) -> Box<dyn ProgressTracker> {
        // Create a simple spinner adapter for the session-owned MultiProgress
        let progress_bar = indicatif::ProgressBar::new_spinner();
        progress_bar.set_message(message.to_string());
        let bar = self.parent.add(progress_bar);
        Box::new(SimpleProgressTracker::new(bar))
    }
    
    fn multi(&self) -> Box<dyn MultiProgressProvider> {
        Box::new(LiveMultiProgressProvider {
            parent: self.parent.clone(),
            trackers: self.trackers.clone(),
            next_id: self.next_id.clone(),
        })
    }
}

/// Owned wrapper around progress tracker that doesn't depend on borrowed data
struct OwnedProgressTracker {
    tracker: ConcreteProgressTracker<'static>,
}

impl OwnedProgressTracker {
    fn new(tracker: ConcreteProgressTracker<'static>) -> Self {
        Self { tracker }
    }
}

impl ProgressTracker for OwnedProgressTracker {
    fn set_position(&self, pos: u64) {
        self.tracker.set_position(pos);
    }
    
    fn inc(&self) {
        self.tracker.inc();
    }
    
    fn inc_by(&self, n: u64) {
        self.tracker.inc_by(n);
    }
    
    fn set_message(&self, message: &str) {
        self.tracker.set_message(message);
    }
    
    fn tick(&self, item: &str) {
        self.tracker.tick(item);
    }
    
    fn finish(&self, message: &str) {
        self.tracker.finish(message);
    }
    
    fn abandon(&self, message: &str) {
        self.tracker.abandon(message);
    }
    
    fn finish_clear(&self) {
        self.tracker.finish_clear();
    }
}

/// Multi-progress provider with owned state
struct LiveMultiProgressProvider {
    parent: Arc<MultiProgress>,
    trackers: Arc<Mutex<HashMap<usize, ConcreteProgressTracker<'static>>>>,
    next_id: Arc<Mutex<usize>>,
}

impl MultiProgressProvider for LiveMultiProgressProvider {
    fn add_bar(&self, total: u64, _message: &str) -> Box<dyn ProgressTracker> {
        // Create a progress bar and add it to the session's MultiProgress
        let progress_bar = indicatif::ProgressBar::new(total);
        let bar = self.parent.add(progress_bar);
        Box::new(SimpleProgressTracker::new(bar))
    }
    
    fn add_spinner(&self, message: &str) -> Box<dyn ProgressTracker> {
        // Create a spinner and add it to the session's MultiProgress
        let progress_bar = indicatif::ProgressBar::new_spinner();
        progress_bar.set_message(message.to_string());
        let bar = self.parent.add(progress_bar);
        Box::new(SimpleProgressTracker::new(bar))
    }
    
    fn clear(&self) {
        // Clear implementation would interact with the MultiProgress
        // For now, this is a no-op since we're using independent progress bars
    }
}

/// Live implementation of PromptProvider
struct LivePromptProvider;

impl PromptProvider for LivePromptProvider {
    fn confirm(&self, message: &str) -> bool {
        Display::prompt()
            .confirm(message)
            .default(false)
            .interact()
            .unwrap_or(false)
    }
    
    fn input(&self, message: &str) -> Option<String> {
        Display::prompt()
            .input(message)
            .interact()
            .ok()
    }
    
    fn select(&self, message: &str, options: &[&str]) -> Option<usize> {
        Display::prompt()
            .select(message)
            .options(options)
            .interact()
            .ok()
    }
    
    fn multi_select(&self, message: &str, options: &[&str]) -> Vec<usize> {
        // For now, convert single select to multi-select for compatibility
        // TODO: Implement proper multi-select when needed
        match self.select(message, options) {
            Some(index) => vec![index],
            None => vec![],
        }
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