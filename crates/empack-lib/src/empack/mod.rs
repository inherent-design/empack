pub mod builds;
pub mod config;
pub mod parsing;
pub mod resolved_project;
pub mod search;
pub mod search_intent;
pub mod state;

// Re-export main types for convenience
pub use builds::{BuildOrchestrator, BuildResult, PackInfo};
pub use config::{ConfigManager, EmpackConfig, ProjectPlan, ProjectSpec};
pub use state::ModpackStateManager;

// Re-export primitives types for convenience
pub use crate::primitives::{BuildTarget, ModpackState, ProjectType, StateTransition};
