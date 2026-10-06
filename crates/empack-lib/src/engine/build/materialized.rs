//! Exact game-content projection for full distributions, before runtime/template composition.
use super::{BuildAcquisitions, capture_build_content};
use crate::{
    application::process_runtime::Cancellation,
    engine::{
        backend::BackendDigestComparison,
        content::SourceEvidencePolicy,
        layout::CollisionIndex,
        mrpack::{AcquiredBuildFile, LockedFileKey, ObservedFileEvidence},
        project::WorkspaceSnapshot,
    },
};
use anyhow::{Context, Result, ensure};
use empack_core::{
    inventory::{
        BuildInventory, BuildSelection, ContentOwner, InventoryInput, OptionalPolicy,
        Representation,
    },
    model::ExpectedContent,
    path::PortableRelPath,
    projection::BuildTarget,
};
use std::collections::{BTreeMap, BTreeSet};

/// Exact logical files still needed after side, override and optional selection.
#[derive(Debug, thiserror::Error)]
#[error("Acquire selected game content before full build: {files:?}; observed: {observed:?}")]
pub struct MissingGameContent {
    pub files: Vec<LockedFileKey>,
    pub observed: Vec<PortableRelPath>,
}

/// Complete materialized game files; this does not prove launcher/server runtime completeness.
pub struct MaterializedGame {
    project: empack_core::model::ResolvedProject,
    inventory: BuildInventory,
    files: BTreeMap<PortableRelPath, AcquiredBuildFile>,
    observed: Vec<ObservedFileEvidence>,
    comparisons: Vec<BackendDigestComparison>,
}
impl MaterializedGame {
    pub fn project(&self) -> &empack_core::model::ResolvedProject {
        &self.project
    }
    pub fn inventory(&self) -> &BuildInventory {
        &self.inventory
    }
    pub fn files(&self) -> &BTreeMap<PortableRelPath, AcquiredBuildFile> {
        &self.files
    }
    pub fn observed(&self) -> &[ObservedFileEvidence] {
        &self.observed
    }
    pub fn backend_comparisons(&self) -> &[BackendDigestComparison] {
        &self.comparisons
    }
}
fn representation(file: &AcquiredBuildFile) -> Representation {
    Representation::Embedded {
        content: file.content.lease().id(),
        bytes: file.content.lease().len(),
        permissions: file.permissions,
    }
}
fn check_expected(file: &AcquiredBuildFile, expected: &ExpectedContent) -> Result<()> {
    if let Some(digests) = &expected.digests {
        digests.check(file.content.observed_digests().values())?;
    }
    ensure!(
        expected
            .size
            .is_none_or(|size| size == file.content.lease().len()),
        "Materialized content size differs from lock"
    );
    ensure!(
        expected
            .accepted_observation
            .as_ref()
            .is_none_or(|id| id == &file.content.lease().id()),
        "Materialized content differs from accepted observation"
    );
    Ok(())
}
/// Select included game files before requiring missing byte acquisitions. No placeholders can
/// enter the completed inventory, and disabled optional replacements retain their common fallback.
pub fn prepare_game_content(
    workspace: &WorkspaceSnapshot,
    external: &BuildAcquisitions,
    target: BuildTarget,
    optional: &OptionalPolicy,
    evidence: SourceEvidencePolicy,
    cancel: &Cancellation,
) -> Result<MaterializedGame> {
    ensure!(
        matches!(target, BuildTarget::ClientFull | BuildTarget::ServerFull),
        "Game materialization requires a full target"
    );
    let captured = capture_build_content(workspace, external, evidence, cancel)?;
    let mut inputs = Vec::new();
    let mut leases = BTreeMap::new();
    let mut used = BTreeSet::new();
    for (key, dependency) in &captured.project.lock().dependencies {
        for file in dependency.files.as_slice() {
            cancel.check()?;
            let logical = LockedFileKey {
                dependency: key.clone(),
                slot: file.slot.clone(),
            };
            let supplied = captured.acquired.get(&logical);
            let representation = if let Some(acquired) = supplied {
                used.insert(logical);
                check_expected(acquired, &file.expected)?;
                leases.insert(
                    ContentOwner::Dependency {
                        key: key.clone(),
                        slot: file.slot.clone(),
                    },
                    acquired.clone(),
                );
                representation(acquired)
            } else {
                Representation::Unacquired {
                    expected: file.expected.clone(),
                }
            };
            for placement in file.placements.as_slice() {
                inputs.push(InventoryInput {
                    owner: ContentOwner::Dependency {
                        key: key.clone(),
                        slot: file.slot.clone(),
                    },
                    destination: placement.destination.clone(),
                    layer: placement.layer,
                    requirements: placement.requirements.clone(),
                    representation: representation.clone(),
                });
            }
        }
    }
    ensure!(
        used.len() == captured.acquired.len(),
        "Acquisition contains a file outside the exact lock"
    );
    for source in captured.sources {
        let file = AcquiredBuildFile {
            content: source.content,
            permissions: source.permissions,
        };
        let representation = representation(&file);
        leases.insert(ContentOwner::Source(source.label.clone()), file);
        inputs.push(InventoryInput {
            owner: ContentOwner::Source(source.label),
            destination: source.destination,
            layer: source.layer,
            requirements: source.requirements,
            representation,
        });
    }
    let mut observed = Vec::new();
    let mut observed_labels = BTreeMap::new();
    for file in captured.observed {
        match file {
            super::ObservedBuildContent::Verified(file) => {
                let (input, acquired, evidence) = file.into_materialized();
                leases.insert(input.owner.clone(), acquired);
                inputs.push(input);
                observed.push(evidence);
            }
            super::ObservedBuildContent::Unacquired { record, choice } => {
                let input = crate::engine::mrpack::ObservedFile::pending_input(&record, &choice)?;
                let ContentOwner::Source(label) = &input.owner else {
                    unreachable!()
                };
                observed_labels.insert(label.clone(), record.metadata_path);
                inputs.push(input);
            }
        }
    }
    let selection = BuildSelection::select(&inputs, target, optional)?;
    let mut missing = BTreeSet::new();
    let mut missing_observed = BTreeSet::new();
    for entry in selection.entries() {
        if matches!(entry.representation, Representation::Unacquired { .. }) {
            match &entry.owner {
                ContentOwner::Dependency { key, slot } => {
                    missing.insert(LockedFileKey {
                        dependency: key.clone(),
                        slot: slot.clone(),
                    });
                }
                ContentOwner::Source(label) => {
                    missing_observed.insert(
                        observed_labels
                            .get(label)
                            .context("Unacquired observed content has no owner")?
                            .clone(),
                    );
                }
                ContentOwner::Runtime(_) => {
                    anyhow::bail!("Game selection unexpectedly requires runtime content")
                }
            }
        }
    }
    if !missing.is_empty() || !missing_observed.is_empty() {
        return Err(MissingGameContent {
            files: missing.into_iter().collect(),
            observed: missing_observed.into_iter().collect(),
        }
        .into());
    }
    let inventory = selection.finish()?;
    let mut collisions = CollisionIndex::default();
    let mut files = BTreeMap::new();
    for entry in inventory.entries() {
        cancel.check()?;
        let Representation::Embedded {
            content,
            bytes,
            permissions,
        } = &entry.representation
        else {
            anyhow::bail!("Full game inventory contains a reference");
        };
        let path = entry.destination.relative().clone();
        collisions.insert_file(&path)?;
        let mut file = leases
            .get(&entry.owner)
            .context("Selected content has no retained acquisition")?
            .clone();
        ensure!(
            file.content.lease().len() == *bytes && file.content.lease().id() == *content,
            "Retained content length differs from projection"
        );
        // Identical bytes may have different portable attributes in distinct placements.
        file.permissions = *permissions;
        files.insert(path, file);
    }
    Ok(MaterializedGame {
        project: captured.project,
        inventory,
        files,
        observed,
        comparisons: captured.comparisons,
    })
}

#[cfg(test)]
mod tests;
