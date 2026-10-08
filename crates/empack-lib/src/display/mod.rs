//! Terminal display system
//!
//! Provides semantic APIs for user-facing communication that automatically
//! adapt to terminal capabilities. Separates concerns between logging (tracing)
//! and user interaction (status, prompts, progress).

use crate::terminal::TerminalCapabilities;

pub mod live;
pub mod progress;
pub mod providers;
pub mod status;
pub mod structured;
pub mod styling;

#[cfg(test)]
pub mod test_utils;

pub use live::LiveDisplayProvider;
pub use providers::{
    DisplayProvider, MultiProgressProvider, ProgressProvider, ProgressTracker, StatusProvider,
    StructuredProvider,
};

/// Main display manager that coordinates all user-facing communication
pub struct Display {
    capabilities: TerminalCapabilities,
    styling: styling::StyleManager,
}

impl Display {
    /// Own terminal policy for this session; creating another display cannot change it.
    pub fn new(capabilities: TerminalCapabilities) -> Self {
        let styling = styling::StyleManager::new(&capabilities);
        Self {
            capabilities,
            styling,
        }
    }
    pub fn status(&self) -> status::StatusDisplay<'_> {
        status::StatusDisplay::new(&self.styling)
    }
    pub fn progress(&self) -> progress::ProgressDisplay<'_> {
        progress::ProgressDisplay::new(&self.styling)
    }
    pub fn table(&self) -> structured::StructuredDisplay<'_> {
        structured::StructuredDisplay::new(&self.styling, &self.capabilities)
    }
    pub fn capabilities(&self) -> &TerminalCapabilities {
        &self.capabilities
    }
    pub fn styling(&self) -> &styling::StyleManager {
        &self.styling
    }
}
impl Default for Display {
    fn default() -> Self {
        Self::new(TerminalCapabilities::minimal())
    }
}

#[cfg(test)]
mod tests {
    include!("display.test.rs");
}
