//! Host interaction fixtures; project effects always use native engine implementations.
use crate::Result;
use crate::application::{config::AppConfig, session::*};
use crate::display::{DisplayProvider, LiveDisplayProvider};
use crate::terminal::TerminalCapabilities;
use indicatif::MultiProgress;
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{Arc, Mutex},
};

#[derive(Default)]
pub struct MockInvocationProvider {
    current_dir: PathBuf,
}
impl MockInvocationProvider {
    pub fn new() -> Self {
        Self {
            current_dir: std::env::current_dir().expect("invocation directory"),
        }
    }
    pub fn with_current_dir(mut self, path: PathBuf) -> Self {
        self.current_dir = path;
        self
    }
}
impl InvocationProvider for MockInvocationProvider {
    fn current_dir(&self) -> Result<PathBuf> {
        Ok(self.current_dir.clone())
    }
}

pub struct MockConfigProvider {
    pub app_config: AppConfig,
}

impl MockConfigProvider {
    pub fn new(app_config: AppConfig) -> Self {
        Self { app_config }
    }
}

impl ConfigProvider for MockConfigProvider {
    fn app_config(&self) -> &AppConfig {
        &self.app_config
    }
}

/// Typed response for the queue-based mock interactive provider.
///
/// Each variant corresponds to one `InteractiveProvider` trait method.
/// Queued responses are consumed in FIFO order; when the front element
/// matches the expected type it is popped and returned. When the queue
/// is empty or the front element is the wrong type, the provider falls
/// back to yes_mode, then the static response, then the default value.
#[derive(Debug, Clone, PartialEq)]
pub enum MockResponse {
    Text(String),
    Confirm(bool),
    Select(usize),
    FuzzySelect(Option<usize>),
}

/// Mock interactive provider for testing
pub struct MockInteractiveProvider {
    yes_mode: bool,
    pub text_input_calls: Arc<Mutex<Vec<(String, String)>>>, // (prompt, default)
    pub confirm_calls: Arc<Mutex<Vec<(String, bool)>>>,      // (prompt, default)
    pub select_calls: Arc<Mutex<Vec<String>>>,               // prompt
    pub fuzzy_select_calls: Arc<Mutex<Vec<String>>>,         // prompt
    pub text_input_response: Arc<Mutex<Option<String>>>,
    pub confirm_response: Arc<Mutex<Option<bool>>>,
    pub select_response: Arc<Mutex<Option<usize>>>,
    pub fuzzy_select_response: Arc<Mutex<Option<usize>>>,
    pub response_queue: Arc<Mutex<VecDeque<MockResponse>>>,
}

impl MockInteractiveProvider {
    pub fn new() -> Self {
        Self {
            yes_mode: false,
            text_input_calls: Arc::new(Mutex::new(Vec::new())),
            confirm_calls: Arc::new(Mutex::new(Vec::new())),
            select_calls: Arc::new(Mutex::new(Vec::new())),
            fuzzy_select_calls: Arc::new(Mutex::new(Vec::new())),
            text_input_response: Arc::new(Mutex::new(None)),
            confirm_response: Arc::new(Mutex::new(None)),
            select_response: Arc::new(Mutex::new(None)),
            fuzzy_select_response: Arc::new(Mutex::new(None)),
            response_queue: Arc::new(Mutex::new(VecDeque::new())),
        }
    }

    pub fn with_yes_mode(mut self, yes_mode: bool) -> Self {
        self.yes_mode = yes_mode;
        self
    }

    pub fn with_text_input(self, response: String) -> Self {
        *self.text_input_response.lock().unwrap() = Some(response);
        self
    }

    pub fn with_confirm(self, response: bool) -> Self {
        *self.confirm_response.lock().unwrap() = Some(response);
        self
    }

    pub fn with_select(self, response: usize) -> Self {
        *self.select_response.lock().unwrap() = Some(response);
        self
    }

    pub fn with_fuzzy_select(self, response: usize) -> Self {
        *self.fuzzy_select_response.lock().unwrap() = Some(response);
        self
    }

    pub fn queue_text(self, response: &str) -> Self {
        self.response_queue
            .lock()
            .unwrap()
            .push_back(MockResponse::Text(response.to_string()));
        self
    }

    pub fn queue_confirm(self, response: bool) -> Self {
        self.response_queue
            .lock()
            .unwrap()
            .push_back(MockResponse::Confirm(response));
        self
    }

    pub fn queue_select(self, index: usize) -> Self {
        self.response_queue
            .lock()
            .unwrap()
            .push_back(MockResponse::Select(index));
        self
    }

    pub fn queue_fuzzy_select(self, index: Option<usize>) -> Self {
        self.response_queue
            .lock()
            .unwrap()
            .push_back(MockResponse::FuzzySelect(index));
        self
    }

    /// Get recorded text input calls
    pub fn get_text_input_calls(&self) -> Vec<(String, String)> {
        self.text_input_calls.lock().unwrap().clone()
    }

    /// Get recorded confirm calls
    pub fn get_confirm_calls(&self) -> Vec<(String, bool)> {
        self.confirm_calls.lock().unwrap().clone()
    }

    /// Get recorded select calls
    pub fn get_select_calls(&self) -> Vec<String> {
        self.select_calls.lock().unwrap().clone()
    }

    /// Get recorded fuzzy select calls
    pub fn get_fuzzy_select_calls(&self) -> Vec<String> {
        self.fuzzy_select_calls.lock().unwrap().clone()
    }
}

impl Default for MockInteractiveProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl InteractiveProvider for MockInteractiveProvider {
    fn can_choose(&self) -> bool {
        !self.yes_mode
    }

    fn text_input(&self, prompt: &str, default: String) -> Result<String> {
        self.text_input_calls
            .lock()
            .unwrap()
            .push((prompt.to_string(), default.clone()));

        // Queue takes priority: pop if front element is the matching type
        {
            let mut queue = self.response_queue.lock().unwrap();
            if let Some(MockResponse::Text(_)) = queue.front()
                && let Some(MockResponse::Text(value)) = queue.pop_front()
            {
                return Ok(value);
            }
        }

        if self.yes_mode {
            return Ok(default);
        }

        if let Some(response) = self.text_input_response.lock().unwrap().clone() {
            Ok(response)
        } else {
            Ok(default)
        }
    }

    fn confirm(&self, prompt: &str, default: bool) -> Result<bool> {
        self.confirm_calls
            .lock()
            .unwrap()
            .push((prompt.to_string(), default));

        {
            let mut queue = self.response_queue.lock().unwrap();
            if let Some(MockResponse::Confirm(_)) = queue.front()
                && let Some(MockResponse::Confirm(value)) = queue.pop_front()
            {
                return Ok(value);
            }
        }

        if self.yes_mode {
            return Ok(default);
        }

        if let Some(response) = *self.confirm_response.lock().unwrap() {
            Ok(response)
        } else {
            Ok(default)
        }
    }

    fn select(&self, prompt: &str, _options: &[&str]) -> Result<usize> {
        self.select_calls.lock().unwrap().push(prompt.to_string());

        {
            let mut queue = self.response_queue.lock().unwrap();
            if let Some(MockResponse::Select(_)) = queue.front()
                && let Some(MockResponse::Select(value)) = queue.pop_front()
            {
                return Ok(value);
            }
        }

        if self.yes_mode {
            return Ok(0);
        }

        if let Some(response) = *self.select_response.lock().unwrap() {
            Ok(response)
        } else {
            Ok(0)
        }
    }

    fn fuzzy_select(&self, prompt: &str, _options: &[String]) -> Result<Option<usize>> {
        self.fuzzy_select_calls
            .lock()
            .unwrap()
            .push(prompt.to_string());

        {
            let mut queue = self.response_queue.lock().unwrap();
            if let Some(MockResponse::FuzzySelect(_)) = queue.front()
                && let Some(MockResponse::FuzzySelect(value)) = queue.pop_front()
            {
                return Ok(value);
            }
        }

        if self.yes_mode {
            return Ok(Some(0));
        }

        if let Some(response) = *self.fuzzy_select_response.lock().unwrap() {
            Ok(Some(response))
        } else {
            Ok(Some(0))
        }
    }
}

pub struct MockCommandSession {
    pub multi_progress: Arc<MultiProgress>,
    pub display_provider: LiveDisplayProvider,
    pub terminal_capabilities: TerminalCapabilities,
    pub invocation_provider: MockInvocationProvider,
    pub process_provider: LiveProcessProvider,
    pub config_provider: MockConfigProvider,
    pub interactive_provider: MockInteractiveProvider,
}
impl MockCommandSession {
    pub fn new() -> Self {
        let capabilities = TerminalCapabilities::minimal();
        let multi_progress = Arc::new(MultiProgress::new());
        Self {
            display_provider: LiveDisplayProvider::new_with_arc(multi_progress.clone())
                .with_capabilities(capabilities.clone()),
            multi_progress,
            terminal_capabilities: capabilities,
            invocation_provider: MockInvocationProvider::new(),
            process_provider: LiveProcessProvider::new(),
            config_provider: MockConfigProvider::new(AppConfig::default()),
            interactive_provider: MockInteractiveProvider::new(),
        }
    }
    pub fn with_invocation(mut self, value: MockInvocationProvider) -> Self {
        self.invocation_provider = value;
        self
    }
    pub fn with_config(mut self, value: MockConfigProvider) -> Self {
        self.config_provider = value;
        self
    }
    pub fn with_interactive(mut self, value: MockInteractiveProvider) -> Self {
        self.interactive_provider = value;
        self
    }
    pub fn with_terminal_capabilities(mut self, value: TerminalCapabilities) -> Self {
        self.terminal_capabilities = value;
        self
    }
}
impl Default for MockCommandSession {
    fn default() -> Self {
        Self::new()
    }
}
impl Session for MockCommandSession {
    fn display(&self) -> &dyn DisplayProvider {
        &self.display_provider
    }
    fn invocation(&self) -> &dyn InvocationProvider {
        &self.invocation_provider
    }
    fn process(&self) -> &dyn ProcessProvider {
        &self.process_provider
    }
    fn config(&self) -> &dyn ConfigProvider {
        &self.config_provider
    }
    fn interactive(&self) -> &dyn InteractiveProvider {
        &self.interactive_provider
    }
    fn terminal(&self) -> &TerminalCapabilities {
        &self.terminal_capabilities
    }
}
