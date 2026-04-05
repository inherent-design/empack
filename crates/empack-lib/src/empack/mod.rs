pub mod archive;
pub mod builds;
pub mod config;
pub mod content;
pub mod fuzzy;
pub mod import;
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
pub use content::{
    ApiJarResolver, JarIdentifyRequest, JarIdentity, JarResolver, OverrideCategory, OverrideSide,
    SideEnv, SideRequirement, UrlClassifyError, UrlKind,
};
pub use import::{
    ContentEntry, EmbeddedJar, ImportConfig, ImportError, ImportResult,
    ImportStats, ModpackManifest, OverrideEntry, PackIdentity, PlatformRef, ResolvedManifest,
    RuntimeTarget, SourceKind, classify_override, detect_local_source, execute_import,
    parse_curseforge_zip, parse_modrinth_mrpack, resolve_manifest,
};
#[cfg(feature = "test-utils")]
pub use packwiz::MockPackwizOps;
pub use packwiz::{
    InstallResult, PackwizError, PackwizInstaller, PackwizMetadata, PackwizOps,
    RestrictedModInfo, write_pack_toml_options,
};
pub use state::{PackStateManager, StateTransitionResult};

// Re-export primitives types for convenience
pub use crate::primitives::{
    BuildTarget, PackState, ProjectPlatform, ProjectType, StateTransition,
};

#[cfg(test)]
mod tests {
    include!("mod.test.rs");
}
