pub mod builds;
pub mod config;
pub mod packwiz;
pub mod parsing;
pub mod resolved_project;
pub mod search;
pub mod search_intent;
pub mod state;
pub mod versions;

// Re-export main types for convenience
pub use builds::{BuildOrchestrator, BuildResult, PackInfo};
pub use config::{ConfigManager, EmpackConfig, ProjectPlan, ProjectSpec};
pub use packwiz::{PackwizError, PackwizInstaller, PackwizMetadata};
pub use state::PackStateManager;

// Re-export primitives types for convenience
pub use crate::primitives::{BuildTarget, PackState, ProjectPlatform, ProjectType, StateTransition};

#[cfg(test)]
mod tests {
    include!("mod.test.rs");
}
