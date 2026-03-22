pub mod builds;
pub mod config;
pub mod packwiz;
pub mod parsing;
pub mod resolved_project;
pub mod search;
pub mod search_intent;
pub mod state;
pub mod templates;
pub mod versions;

// Re-export main types for convenience
pub use builds::{BuildOrchestrator, BuildResult, PackInfo};
pub use config::{
    ConfigManager, Dependency, DependencyEntry, DependencyRecord, DependencySearch,
    DependencyStatus, EmpackConfig, ProjectPlan, ProjectSpec,
};
pub use packwiz::{PackwizError, PackwizInstaller, PackwizMetadata, PackwizOps};
#[cfg(feature = "test-utils")]
pub use packwiz::MockPackwizOps;
pub use state::{PackStateManager, StateTransitionResult};

// Re-export primitives types for convenience
pub use crate::primitives::{
    BuildTarget, PackState, ProjectPlatform, ProjectType, StateTransition,
};

#[cfg(test)]
mod tests {
    include!("mod.test.rs");
}
