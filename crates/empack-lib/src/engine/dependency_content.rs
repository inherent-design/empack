//! Explicit project representation obligations; a reference is not verified payload content.
use super::mrpack::{AcquiredBuildFile, LockedFileKey};
use anyhow::{Result, ensure};
use empack_core::model::{AcquisitionSpec, ResolvedFile};
use std::collections::BTreeMap;

#[derive(Clone)]
pub enum DependencyContent {
    /// Publish these immutable bytes after checking the selected file's source assertions.
    Materialized(AcquiredBuildFile),
    /// Record an exact remotely or manually acquirable obligation without requiring its bytes.
    /// Build projection independently decides whether the selected target needs acquisition.
    Reference,
}
impl From<AcquiredBuildFile> for DependencyContent {
    fn from(value: AcquiredBuildFile) -> Self {
        Self::Materialized(value)
    }
}
impl DependencyContent {
    pub fn materialized(&self) -> Option<&AcquiredBuildFile> {
        match self {
            Self::Materialized(file) => Some(file),
            Self::Reference => None,
        }
    }
}
pub type DependencyContents = BTreeMap<LockedFileKey, DependencyContent>;

pub(super) fn materialized(
    files: BTreeMap<LockedFileKey, AcquiredBuildFile>,
) -> DependencyContents {
    files
        .into_iter()
        .map(|(key, file)| (key, file.into()))
        .collect()
}

pub(super) fn validate_reference(file: &ResolvedFile) -> Result<()> {
    ensure!(
        matches!(
            file.acquisition,
            AcquisitionSpec::Provider { .. }
                | AcquisitionSpec::Url(_)
                | AcquisitionSpec::Manual { .. }
        ),
        "Local and archive-member dependencies require materialized content"
    );
    ensure!(
        file.expected.digests.is_some() || file.expected.accepted_observation.is_some(),
        "A deferred reference requires original content evidence"
    );
    Ok(())
}
