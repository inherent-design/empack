//! Display provider traits for dependency injection
//!
//! Abstracts all user communication behind testable traits, enabling the Handle Pattern
//! for display operations while maintaining the rich display capabilities.

use std::fmt;

/// Provider trait for all user-facing communication
///
/// This trait abstracts all display operations, enabling dependency injection
/// and comprehensive testing of business logic without side effects.
pub trait DisplayProvider {
    /// Display status messages with semantic meaning
    fn status(&self) -> Box<dyn StatusProvider>;

    /// Display progress for long-running operations  
    fn progress(&self) -> Box<dyn ProgressProvider>;

    /// Display interactive prompts and selections
    fn prompt(&self) -> Box<dyn PromptProvider>;

    /// Display structured output (tables, lists)
    fn table(&self) -> Box<dyn StructuredProvider>;
}

/// Provider trait for status updates and user feedback
pub trait StatusProvider {
    /// Display a working/checking status
    fn checking(&self, task: &str);

    /// Display a success status with optional details
    fn success(&self, item: &str, details: &str);

    /// Display an error status with details
    fn error(&self, item: &str, details: &str);

    /// Display a warning status with details
    fn warning(&self, message: &str);

    /// Display an info status
    fn info(&self, message: &str);

    /// Display a simple message without status symbols
    fn message(&self, text: &str);

    /// Display an emphasized message
    fn emphasis(&self, text: &str);

    /// Display a subtle/secondary message
    fn subtle(&self, text: &str);

    /// Display a list of items with bullets
    fn list(&self, items: &[&str]);

    /// Display a completion message
    fn complete(&self, task: &str);

    /// Check and report tool availability
    fn tool_check(&self, tool: &str, available: bool, version: &str);

    /// Display a header for a section of work
    fn section(&self, title: &str);

    /// Display a step in a multi-step process
    fn step(&self, current: usize, total: usize, description: &str);
}

/// Provider trait for progress tracking
pub trait ProgressProvider {
    /// Create a progress bar for operations with known total
    fn bar(&self, total: u64) -> Box<dyn ProgressTracker>;

    /// Create a spinner for operations with unknown duration
    fn spinner(&self, message: &str) -> Box<dyn ProgressTracker>;

    /// Create a multi-progress manager for parallel operations
    fn multi(&self) -> Box<dyn MultiProgressProvider>;
}

/// Individual progress tracker interface
pub trait ProgressTracker {
    /// Set the current position
    fn set_position(&self, pos: u64);

    /// Increment position by 1
    fn inc(&self);

    /// Increment position by n
    fn inc_by(&self, n: u64);

    /// Update the message
    fn set_message(&self, message: &str);

    /// Update message with current item info
    fn tick(&self, item: &str);

    /// Finish with success message
    fn finish(&self, message: &str);

    /// Abandon with error message
    fn abandon(&self, message: &str);

    /// Finish and clear the progress bar
    fn finish_clear(&self);
}

/// Multi-progress manager interface
pub trait MultiProgressProvider {
    /// Add a progress bar to the multi-progress
    fn add_bar(&self, total: u64, message: &str) -> Box<dyn ProgressTracker>;

    /// Add a spinner to the multi-progress
    fn add_spinner(&self, message: &str) -> Box<dyn ProgressTracker>;

    /// Clear all progress bars
    fn clear(&self);
}

/// Provider trait for interactive prompts
pub trait PromptProvider {
    /// Display a confirmation prompt
    fn confirm(&self, message: &str) -> bool;

    /// Display a text input prompt
    fn input(&self, message: &str) -> Option<String>;

    /// Display a selection prompt with options
    fn select(&self, message: &str, options: &[&str]) -> Option<usize>;

    /// Display a multi-selection prompt
    fn multi_select(&self, message: &str, options: &[&str]) -> Vec<usize>;
}

/// Provider trait for structured output
pub trait StructuredProvider {
    /// Display data in a table format
    fn table(&self, headers: &[&str], rows: &[Vec<&str>]);

    /// Display a simple list
    fn list(&self, items: &[&str]);

    /// Display key-value pairs
    fn properties(&self, pairs: &[(&str, &str)]);
}

/// Summary information for batch operations
#[derive(Debug, Clone)]
pub struct OperationSummary {
    pub successful: usize,
    pub failed: usize,
    pub total: usize,
}

impl OperationSummary {
    pub fn new(successful: usize, failed: usize) -> Self {
        Self {
            successful,
            failed,
            total: successful + failed,
        }
    }

    pub fn success_rate(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            self.successful as f64 / self.total as f64
        }
    }

    pub fn is_success(&self) -> bool {
        self.failed == 0 && self.successful > 0
    }

    pub fn is_partial_success(&self) -> bool {
        self.successful > 0 && self.failed > 0
    }

    pub fn is_failure(&self) -> bool {
        self.failed > 0 && self.successful == 0
    }
}

impl fmt::Display for OperationSummary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} successful, {} failed", self.successful, self.failed)
    }
}

/// Extension trait for common display patterns used in commands
pub trait DisplayProviderExt: DisplayProvider {
    /// Display an operation summary with appropriate status
    fn display_summary(&self, operation: &str, summary: &OperationSummary) {
        let status = self.status();

        if summary.is_success() {
            status.complete(&format!("{} completed successfully", operation));
            status.subtle(&format!(
                "   ✅ {} operation{} successful",
                summary.successful,
                if summary.successful == 1 { "" } else { "s" }
            ));
        } else if summary.is_partial_success() {
            status.warning(&format!("{} completed with issues", operation));
            status.subtle(&format!(
                "   ✅ {} successful, ❌ {} failed",
                summary.successful, summary.failed
            ));
        } else if summary.is_failure() {
            status.error(&operation, "failed");
            status.subtle(&format!(
                "   ❌ {} operation{} failed",
                summary.failed,
                if summary.failed == 1 { "" } else { "s" }
            ));
        } else {
            status.info(&format!("{} - no operations performed", operation));
        }
    }

    /// Display a step with progress context
    fn display_step(&self, current: usize, total: usize, description: &str, detail: Option<&str>) {
        let status = self.status();
        status.step(current, total, description);
        if let Some(detail) = detail {
            status.subtle(&format!("   {}", detail));
        }
    }
}

impl<T: DisplayProvider> DisplayProviderExt for T {}
