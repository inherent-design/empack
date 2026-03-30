//! Terminal display system
//!
//! Provides semantic APIs for user-facing communication that automatically
//! adapt to terminal capabilities. Separates concerns between logging (tracing)
//! and user interaction (status, prompts, progress).

use crate::terminal::TerminalCapabilities;
use std::sync::OnceLock;

pub mod live;
pub mod mock;
pub mod progress;
pub mod providers;
pub mod status;
pub mod structured;
pub mod styling;

#[cfg(test)]
pub mod test_utils;

pub use live::LiveDisplayProvider;
pub use mock::{DisplayCall, MockDisplayProvider};
pub use providers::{
    DisplayProvider, DisplayProviderExt, MultiProgressProvider, OperationSummary, ProgressProvider,
    ProgressTracker, StatusProvider, StructuredProvider,
};

static GLOBAL_DISPLAY: OnceLock<Display> = OnceLock::new();

/// Main display manager that coordinates all user-facing communication
pub struct Display {
    capabilities: TerminalCapabilities,
    styling: styling::StyleManager,
}

impl Display {
    /// Initialize global display system with terminal capabilities.
    /// Idempotent: if already initialized, returns the existing instance.
    pub fn init_or_get(capabilities: TerminalCapabilities) -> &'static Self {
        GLOBAL_DISPLAY.get_or_init(|| {
            let styling = styling::StyleManager::new(&capabilities);
            Display {
                capabilities,
                styling,
            }
        })
    }

    /// Get global display reference.
    /// Auto-initializes with minimal capabilities if not yet initialized.
    pub fn global() -> &'static Self {
        GLOBAL_DISPLAY.get_or_init(|| {
            tracing::warn!(
                "Display initialized with minimal capabilities; session may not have started yet"
            );
            let capabilities = TerminalCapabilities::minimal();
            let styling = styling::StyleManager::new(&capabilities);
            Display {
                capabilities,
                styling,
            }
        })
    }

    /// Status updates with semantic intent
    pub fn status() -> status::StatusDisplay<'static> {
        let display = Self::global();
        status::StatusDisplay::new(&display.styling)
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

#[cfg(test)]
mod tests {
    include!("display.test.rs");
}
