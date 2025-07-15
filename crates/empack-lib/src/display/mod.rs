//! Terminal display system
//!
//! Provides semantic APIs for user-facing communication that automatically
//! adapt to terminal capabilities. Separates concerns between logging (tracing)
//! and user interaction (status, prompts, progress).

use crate::terminal::TerminalCapabilities;
use crate::primitives::ConfigError;
use std::sync::OnceLock;

pub mod styling;
pub mod status;
pub mod interactive;
pub mod progress;
pub mod structured;
pub mod providers;
pub mod live;
pub mod mock;

// Re-export provider traits and implementations for easy access
pub use providers::{
    DisplayProvider, StatusProvider, ProgressProvider, PromptProvider, StructuredProvider,
    ProgressTracker, MultiProgressProvider, OperationSummary, DisplayProviderExt,
};
pub use live::LiveDisplayProvider;
pub use mock::{MockDisplayProvider, DisplayCall, ResponseValue};

// Global display manager - initialized once with terminal capabilities
static GLOBAL_DISPLAY: OnceLock<Display> = OnceLock::new();

/// Main display manager that coordinates all user-facing communication
pub struct Display {
    capabilities: TerminalCapabilities,
    styling: styling::StyleManager,
}

impl Display {
    /// Initialize global display system with terminal capabilities
    pub fn init(capabilities: TerminalCapabilities) -> Result<&'static Self, ConfigError> {
        if GLOBAL_DISPLAY.get().is_some() {
            return Err(ConfigError::AlreadyInitialized);
        }

        let styling = styling::StyleManager::new(&capabilities);
        let display = Display {
            capabilities,
            styling,
        };

        GLOBAL_DISPLAY
            .set(display)
            .map_err(|_| ConfigError::AlreadyInitialized)?;

        Ok(GLOBAL_DISPLAY.get().unwrap())
    }

    /// Get global display reference
    pub fn global() -> &'static Self {
        GLOBAL_DISPLAY
            .get()
            .expect("Display not initialized - call Display::init() first")
    }

    /// Status updates with semantic intent
    pub fn status() -> status::StatusDisplay<'static> {
        let display = Self::global();
        status::StatusDisplay::new(&display.styling)
    }

    /// Interactive prompts and selections
    pub fn prompt() -> interactive::InteractiveDisplay<'static> {
        let display = Self::global();
        interactive::InteractiveDisplay::new(&display.styling, &display.capabilities)
    }

    /// Progress tracking for long operations
    pub fn progress() -> progress::ProgressDisplay<'static> {
        let display = Self::global();
        progress::ProgressDisplay::new(&display.styling)
    }

    /// Structured output (tables, lists)
    pub fn table() -> structured::StructuredDisplay<'static> {
        let display = Self::global();
        structured::StructuredDisplay::new(&display.styling, &display.capabilities)
    }

    /// Get terminal capabilities for advanced usage
    pub fn capabilities() -> &'static TerminalCapabilities {
        &Self::global().capabilities
    }

    /// Get style manager for advanced styling
    pub fn styling() -> &'static styling::StyleManager {
        &Self::global().styling
    }
}