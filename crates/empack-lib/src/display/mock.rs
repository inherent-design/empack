//! Mock display provider implementation for testing
//!
//! Provides testable implementations that record all display calls
//! for assertion in unit tests.

use super::providers::*;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Mock implementation of DisplayProvider that records all calls
#[derive(Clone)]
pub struct MockDisplayProvider {
    calls: Arc<Mutex<Vec<DisplayCall>>>,
    responses: Arc<Mutex<HashMap<String, ResponseValue>>>,
}

impl MockDisplayProvider {
    pub fn new() -> Self {
        Self {
            calls: Arc::new(Mutex::new(Vec::new())),
            responses: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Get all recorded display calls for testing assertions
    pub fn get_calls(&self) -> Vec<DisplayCall> {
        self.calls.lock().unwrap().clone()
    }

    /// Clear all recorded calls
    pub fn clear_calls(&self) {
        self.calls.lock().unwrap().clear();
    }

    /// Set a response for interactive prompts
    pub fn set_response(&self, prompt: &str, response: ResponseValue) {
        self.responses
            .lock()
            .unwrap()
            .insert(prompt.to_string(), response);
    }

    /// Check if a specific call was made
    pub fn has_call(&self, expected: &DisplayCall) -> bool {
        self.get_calls().contains(expected)
    }

    /// Get count of calls of a specific type
    pub fn count_calls(&self, call_type: &str) -> usize {
        self.get_calls()
            .iter()
            .filter(|call| call.call_type() == call_type)
            .count()
    }
}

impl Default for MockDisplayProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl DisplayProvider for MockDisplayProvider {
    fn status(&self) -> Box<dyn StatusProvider> {
        Box::new(MockStatusProvider::new(self.calls.clone()))
    }

    fn progress(&self) -> Box<dyn ProgressProvider> {
        Box::new(MockProgressProvider::new(self.calls.clone()))
    }

    fn prompt(&self) -> Box<dyn PromptProvider> {
        Box::new(MockPromptProvider::new(
            self.calls.clone(),
            self.responses.clone(),
        ))
    }

    fn table(&self) -> Box<dyn StructuredProvider> {
        Box::new(MockStructuredProvider::new(self.calls.clone()))
    }
}

/// Recorded display call for testing
#[derive(Debug, Clone, PartialEq)]
pub enum DisplayCall {
    // Status calls
    StatusChecking {
        task: String,
    },
    StatusSuccess {
        item: String,
        details: String,
    },
    StatusError {
        item: String,
        details: String,
    },
    StatusWarning {
        message: String,
    },
    StatusInfo {
        message: String,
    },
    StatusMessage {
        text: String,
    },
    StatusEmphasis {
        text: String,
    },
    StatusSubtle {
        text: String,
    },
    StatusList {
        items: Vec<String>,
    },
    StatusComplete {
        task: String,
    },
    StatusToolCheck {
        tool: String,
        available: bool,
        version: String,
    },
    StatusSection {
        title: String,
    },
    StatusStep {
        current: usize,
        total: usize,
        description: String,
    },

    // Progress calls
    ProgressBar {
        total: u64,
    },
    ProgressSpinner {
        message: String,
    },
    ProgressSetPosition {
        tracker_id: usize,
        pos: u64,
    },
    ProgressInc {
        tracker_id: usize,
    },
    ProgressIncBy {
        tracker_id: usize,
        n: u64,
    },
    ProgressSetMessage {
        tracker_id: usize,
        message: String,
    },
    ProgressTick {
        tracker_id: usize,
        item: String,
    },
    ProgressFinish {
        tracker_id: usize,
        message: String,
    },
    ProgressAbandon {
        tracker_id: usize,
        message: String,
    },

    // Prompt calls
    PromptConfirm {
        message: String,
        response: bool,
    },
    PromptInput {
        message: String,
        response: Option<String>,
    },
    PromptSelect {
        message: String,
        options: Vec<String>,
        response: Option<usize>,
    },
    PromptMultiSelect {
        message: String,
        options: Vec<String>,
        response: Vec<usize>,
    },

    // Structured calls
    StructuredTable {
        headers: Vec<String>,
        rows: Vec<Vec<String>>,
    },
    StructuredList {
        items: Vec<String>,
    },
    StructuredProperties {
        pairs: Vec<(String, String)>,
    },
}

impl DisplayCall {
    pub fn call_type(&self) -> &'static str {
        match self {
            DisplayCall::StatusChecking { .. } => "status_checking",
            DisplayCall::StatusSuccess { .. } => "status_success",
            DisplayCall::StatusError { .. } => "status_error",
            DisplayCall::StatusWarning { .. } => "status_warning",
            DisplayCall::StatusInfo { .. } => "status_info",
            DisplayCall::StatusMessage { .. } => "status_message",
            DisplayCall::StatusEmphasis { .. } => "status_emphasis",
            DisplayCall::StatusSubtle { .. } => "status_subtle",
            DisplayCall::StatusList { .. } => "status_list",
            DisplayCall::StatusComplete { .. } => "status_complete",
            DisplayCall::StatusToolCheck { .. } => "status_tool_check",
            DisplayCall::StatusSection { .. } => "status_section",
            DisplayCall::StatusStep { .. } => "status_step",
            DisplayCall::ProgressBar { .. } => "progress_bar",
            DisplayCall::ProgressSpinner { .. } => "progress_spinner",
            DisplayCall::ProgressSetPosition { .. } => "progress_set_position",
            DisplayCall::ProgressInc { .. } => "progress_inc",
            DisplayCall::ProgressIncBy { .. } => "progress_inc_by",
            DisplayCall::ProgressSetMessage { .. } => "progress_set_message",
            DisplayCall::ProgressTick { .. } => "progress_tick",
            DisplayCall::ProgressFinish { .. } => "progress_finish",
            DisplayCall::ProgressAbandon { .. } => "progress_abandon",
            DisplayCall::PromptConfirm { .. } => "prompt_confirm",
            DisplayCall::PromptInput { .. } => "prompt_input",
            DisplayCall::PromptSelect { .. } => "prompt_select",
            DisplayCall::PromptMultiSelect { .. } => "prompt_multi_select",
            DisplayCall::StructuredTable { .. } => "structured_table",
            DisplayCall::StructuredList { .. } => "structured_list",
            DisplayCall::StructuredProperties { .. } => "structured_properties",
        }
    }
}

/// Response values for interactive prompts
#[derive(Debug, Clone)]
pub enum ResponseValue {
    Bool(bool),
    String(String),
    Index(usize),
    Indices(Vec<usize>),
}

/// Mock status provider
struct MockStatusProvider {
    calls: Arc<Mutex<Vec<DisplayCall>>>,
}

impl MockStatusProvider {
    fn new(calls: Arc<Mutex<Vec<DisplayCall>>>) -> Self {
        Self { calls }
    }

    fn record_call(&self, call: DisplayCall) {
        self.calls.lock().unwrap().push(call);
    }
}

impl StatusProvider for MockStatusProvider {
    fn checking(&self, task: &str) {
        self.record_call(DisplayCall::StatusChecking {
            task: task.to_string(),
        });
    }

    fn success(&self, item: &str, details: &str) {
        self.record_call(DisplayCall::StatusSuccess {
            item: item.to_string(),
            details: details.to_string(),
        });
    }

    fn error(&self, item: &str, details: &str) {
        self.record_call(DisplayCall::StatusError {
            item: item.to_string(),
            details: details.to_string(),
        });
    }

    fn warning(&self, message: &str) {
        self.record_call(DisplayCall::StatusWarning {
            message: message.to_string(),
        });
    }

    fn info(&self, message: &str) {
        self.record_call(DisplayCall::StatusInfo {
            message: message.to_string(),
        });
    }

    fn message(&self, text: &str) {
        self.record_call(DisplayCall::StatusMessage {
            text: text.to_string(),
        });
    }

    fn emphasis(&self, text: &str) {
        self.record_call(DisplayCall::StatusEmphasis {
            text: text.to_string(),
        });
    }

    fn subtle(&self, text: &str) {
        self.record_call(DisplayCall::StatusSubtle {
            text: text.to_string(),
        });
    }

    fn list(&self, items: &[&str]) {
        self.record_call(DisplayCall::StatusList {
            items: items.iter().map(|s| s.to_string()).collect(),
        });
    }

    fn complete(&self, task: &str) {
        self.record_call(DisplayCall::StatusComplete {
            task: task.to_string(),
        });
    }

    fn tool_check(&self, tool: &str, available: bool, version: &str) {
        self.record_call(DisplayCall::StatusToolCheck {
            tool: tool.to_string(),
            available,
            version: version.to_string(),
        });
    }

    fn section(&self, title: &str) {
        self.record_call(DisplayCall::StatusSection {
            title: title.to_string(),
        });
    }

    fn step(&self, current: usize, total: usize, description: &str) {
        self.record_call(DisplayCall::StatusStep {
            current,
            total,
            description: description.to_string(),
        });
    }
}

/// Mock progress provider with tracker ID generation
struct MockProgressProvider {
    calls: Arc<Mutex<Vec<DisplayCall>>>,
    next_tracker_id: Arc<Mutex<usize>>,
}

impl MockProgressProvider {
    fn new(calls: Arc<Mutex<Vec<DisplayCall>>>) -> Self {
        Self {
            calls,
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

impl ProgressProvider for MockProgressProvider {
    fn bar(&self, total: u64) -> Box<dyn ProgressTracker> {
        let tracker_id = self.get_next_tracker_id();
        self.calls
            .lock()
            .unwrap()
            .push(DisplayCall::ProgressBar { total });
        Box::new(MockProgressTracker::new(tracker_id, self.calls.clone()))
    }

    fn spinner(&self, message: &str) -> Box<dyn ProgressTracker> {
        let tracker_id = self.get_next_tracker_id();
        self.calls
            .lock()
            .unwrap()
            .push(DisplayCall::ProgressSpinner {
                message: message.to_string(),
            });
        Box::new(MockProgressTracker::new(tracker_id, self.calls.clone()))
    }

    fn multi(&self) -> Box<dyn MultiProgressProvider> {
        Box::new(MockMultiProgressProvider::new(self.calls.clone()))
    }
}

/// Mock progress tracker
struct MockProgressTracker {
    id: usize,
    calls: Arc<Mutex<Vec<DisplayCall>>>,
}

impl MockProgressTracker {
    fn new(id: usize, calls: Arc<Mutex<Vec<DisplayCall>>>) -> Self {
        Self { id, calls }
    }
}

impl ProgressTracker for MockProgressTracker {
    fn set_position(&self, pos: u64) {
        self.calls
            .lock()
            .unwrap()
            .push(DisplayCall::ProgressSetPosition {
                tracker_id: self.id,
                pos,
            });
    }

    fn inc(&self) {
        self.calls.lock().unwrap().push(DisplayCall::ProgressInc {
            tracker_id: self.id,
        });
    }

    fn inc_by(&self, n: u64) {
        self.calls.lock().unwrap().push(DisplayCall::ProgressIncBy {
            tracker_id: self.id,
            n,
        });
    }

    fn set_message(&self, message: &str) {
        self.calls
            .lock()
            .unwrap()
            .push(DisplayCall::ProgressSetMessage {
                tracker_id: self.id,
                message: message.to_string(),
            });
    }

    fn tick(&self, item: &str) {
        self.calls.lock().unwrap().push(DisplayCall::ProgressTick {
            tracker_id: self.id,
            item: item.to_string(),
        });
    }

    fn finish(&self, message: &str) {
        self.calls
            .lock()
            .unwrap()
            .push(DisplayCall::ProgressFinish {
                tracker_id: self.id,
                message: message.to_string(),
            });
    }

    fn abandon(&self, message: &str) {
        self.calls
            .lock()
            .unwrap()
            .push(DisplayCall::ProgressAbandon {
                tracker_id: self.id,
                message: message.to_string(),
            });
    }

    fn finish_clear(&self) {
        // For mocking, we can just record this as a finish with empty message
        self.calls
            .lock()
            .unwrap()
            .push(DisplayCall::ProgressFinish {
                tracker_id: self.id,
                message: String::new(),
            });
    }
}

/// Mock multi-progress provider
struct MockMultiProgressProvider {
    calls: Arc<Mutex<Vec<DisplayCall>>>,
    next_tracker_id: Arc<Mutex<usize>>,
}

impl MockMultiProgressProvider {
    fn new(calls: Arc<Mutex<Vec<DisplayCall>>>) -> Self {
        Self {
            calls,
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

impl MultiProgressProvider for MockMultiProgressProvider {
    fn add_bar(&self, total: u64, message: &str) -> Box<dyn ProgressTracker> {
        let tracker_id = self.get_next_tracker_id();
        self.calls
            .lock()
            .unwrap()
            .push(DisplayCall::ProgressBar { total });
        Box::new(MockProgressTracker::new(tracker_id, self.calls.clone()))
    }

    fn add_spinner(&self, message: &str) -> Box<dyn ProgressTracker> {
        let tracker_id = self.get_next_tracker_id();
        self.calls
            .lock()
            .unwrap()
            .push(DisplayCall::ProgressSpinner {
                message: message.to_string(),
            });
        Box::new(MockProgressTracker::new(tracker_id, self.calls.clone()))
    }

    fn clear(&self) {
        // For mocking, we can ignore this or record it if needed
    }
}

/// Mock prompt provider
struct MockPromptProvider {
    calls: Arc<Mutex<Vec<DisplayCall>>>,
    responses: Arc<Mutex<HashMap<String, ResponseValue>>>,
}

impl MockPromptProvider {
    fn new(
        calls: Arc<Mutex<Vec<DisplayCall>>>,
        responses: Arc<Mutex<HashMap<String, ResponseValue>>>,
    ) -> Self {
        Self { calls, responses }
    }

    fn get_response(&self, prompt: &str) -> Option<ResponseValue> {
        self.responses.lock().unwrap().get(prompt).cloned()
    }
}

impl PromptProvider for MockPromptProvider {
    fn confirm(&self, message: &str) -> bool {
        let response = match self.get_response(message) {
            Some(ResponseValue::Bool(value)) => value,
            _ => false, // Default to false
        };

        self.calls.lock().unwrap().push(DisplayCall::PromptConfirm {
            message: message.to_string(),
            response,
        });

        response
    }

    fn input(&self, message: &str) -> Option<String> {
        let response = match self.get_response(message) {
            Some(ResponseValue::String(value)) => Some(value),
            _ => None,
        };

        self.calls.lock().unwrap().push(DisplayCall::PromptInput {
            message: message.to_string(),
            response: response.clone(),
        });

        response
    }

    fn select(&self, message: &str, options: &[&str]) -> Option<usize> {
        let response = match self.get_response(message) {
            Some(ResponseValue::Index(value)) => Some(value),
            _ => None,
        };

        self.calls.lock().unwrap().push(DisplayCall::PromptSelect {
            message: message.to_string(),
            options: options.iter().map(|s| s.to_string()).collect(),
            response,
        });

        response
    }

    fn multi_select(&self, message: &str, options: &[&str]) -> Vec<usize> {
        let response = match self.get_response(message) {
            Some(ResponseValue::Indices(value)) => value,
            _ => vec![],
        };

        self.calls
            .lock()
            .unwrap()
            .push(DisplayCall::PromptMultiSelect {
                message: message.to_string(),
                options: options.iter().map(|s| s.to_string()).collect(),
                response: response.clone(),
            });

        response
    }
}

/// Mock structured provider
struct MockStructuredProvider {
    calls: Arc<Mutex<Vec<DisplayCall>>>,
}

impl MockStructuredProvider {
    fn new(calls: Arc<Mutex<Vec<DisplayCall>>>) -> Self {
        Self { calls }
    }
}

impl StructuredProvider for MockStructuredProvider {
    fn table(&self, headers: &[&str], rows: &[Vec<&str>]) {
        self.calls
            .lock()
            .unwrap()
            .push(DisplayCall::StructuredTable {
                headers: headers.iter().map(|s| s.to_string()).collect(),
                rows: rows
                    .iter()
                    .map(|row| row.iter().map(|s| s.to_string()).collect())
                    .collect(),
            });
    }

    fn list(&self, items: &[&str]) {
        self.calls
            .lock()
            .unwrap()
            .push(DisplayCall::StructuredList {
                items: items.iter().map(|s| s.to_string()).collect(),
            });
    }

    fn properties(&self, pairs: &[(&str, &str)]) {
        self.calls
            .lock()
            .unwrap()
            .push(DisplayCall::StructuredProperties {
                pairs: pairs
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect(),
            });
    }
}
