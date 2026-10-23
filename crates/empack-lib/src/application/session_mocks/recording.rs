//! Record the host message boundary; this is not executable stdout capture.
use crate::display::*;
use std::sync::{Arc, Mutex};

pub(super) struct RecordingDisplay {
    pub inner: Box<dyn DisplayProvider>,
    pub messages: Arc<Mutex<Vec<String>>>,
}
impl DisplayProvider for RecordingDisplay {
    fn status(&self) -> Box<dyn StatusProvider> {
        Box::new(RecordingStatus {
            inner: self.inner.status(),
            messages: self.messages.clone(),
        })
    }
    fn progress(&self) -> Box<dyn ProgressProvider> {
        self.inner.progress()
    }
    fn table(&self) -> Box<dyn StructuredProvider> {
        self.inner.table()
    }
}
struct RecordingStatus {
    inner: Box<dyn StatusProvider>,
    messages: Arc<Mutex<Vec<String>>>,
}
impl StatusProvider for RecordingStatus {
    fn message(&self, text: &str) {
        self.messages.lock().unwrap().push(text.to_owned());
        self.inner.message(text);
    }
    fn checking(&self, task: &str) {
        self.inner.checking(task);
    }
    fn success(&self, item: &str, details: &str) {
        self.inner.success(item, details);
    }
    fn error(&self, item: &str, details: &str) {
        self.inner.error(item, details);
    }
    fn warning(&self, message: &str) {
        self.inner.warning(message);
    }
    fn info(&self, message: &str) {
        self.inner.info(message);
    }
    fn emphasis(&self, text: &str) {
        self.inner.emphasis(text);
    }
    fn subtle(&self, text: &str) {
        self.inner.subtle(text);
    }
    fn list(&self, items: &[&str]) {
        self.inner.list(items);
    }
    fn complete(&self, task: &str) {
        self.inner.complete(task);
    }
    fn tool_check(&self, tool: &str, available: bool, version: &str) {
        self.inner.tool_check(tool, available, version);
    }
    fn section(&self, title: &str) {
        self.inner.section(title);
    }
    fn step(&self, current: usize, total: usize, description: &str) {
        self.inner.step(current, total, description);
    }
}
