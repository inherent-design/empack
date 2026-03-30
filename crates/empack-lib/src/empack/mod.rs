pub mod archive;
pub mod builds;
pub mod config;
pub mod fuzzy;
pub mod packwiz;
pub mod parsing;
pub mod search;
pub mod state;
pub mod templates;
pub mod versions;

pub use archive::{ArchiveError, ArchiveFormat};
pub use builds::{BuildOrchestrator, BuildResult, PackInfo};
pub use config::{
    ConfigManager, Dependency, DependencyEntry, DependencyRecord, DependencySearch,
    DependencyStatus, EmpackConfig, ProjectPlan, ProjectSpec,
};
#[cfg(feature = "test-utils")]
pub use packwiz::MockPackwizOps;
pub use packwiz::{PackwizError, PackwizInstaller, PackwizMetadata, PackwizOps};
pub use state::{PackStateManager, StateTransitionResult};

// Re-export primitives types for convenience
pub use crate::primitives::{
    BuildTarget, PackState, ProjectPlatform, ProjectType, StateTransition,
};

#[cfg(test)]
mod tests {
    include!("mod.test.rs");
}
