//! Shared selected game content for reference and full recipes, before runtime assembly.
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
        BuildInventory, BuildSelection, ContentOwner, DownloadOrigins, InventoryInput,
        OptionalPolicy, Representation,
    },
    model::{AcquisitionSpec, ExpectedContent, NonEmpty, ResolvedFile},
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

/// Complete game projection and retained embedded bytes, not launcher/server runtime completeness.
pub struct PreparedGameContent {
    project: empack_core::model::ResolvedProject,
    inventory: BuildInventory,
    files: BTreeMap<PortableRelPath, AcquiredBuildFile>,
    observed: Vec<ObservedFileEvidence>,
    comparisons: Vec<BackendDigestComparison>,
}
impl PreparedGameContent {
    /// Encode the selected reference view; the returned tree still needs runtime assembly.
    pub fn packwiz(
        &self,
        interaction: crate::engine::packwiz::InstallerInteraction,
        cancel: &Cancellation,
    ) -> Result<crate::engine::packwiz::PackwizPlan> {
        let mut embedded = BTreeMap::new();
        for entry in self.inventory.entries() {
            if matches!(entry.representation, Representation::Embedded { .. }) {
                embedded.insert(
                    entry.owner.clone(),
                    self.files
                        .get(entry.destination.relative())
                        .context("Missing selected embedded bytes")?
                        .clone(),
                );
            }
        }
        crate::engine::packwiz::PackwizPlan::prepare(
            self.inventory.clone(),
            &self.project.intent().metadata,
            &self.project.lock().runtime,
            &embedded,
            interaction,
            cancel,
        )
    }
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
) -> Result<PreparedGameContent> {
    ensure!(
        matches!(target, BuildTarget::ClientFull | BuildTarget::ServerFull),
        "Game materialization requires a full target"
    );
    prepare_selected_content(workspace, external, target, optional, evidence, cancel)
}
/// Preserve representable references for a selected bootstrap environment.
pub fn prepare_bootstrap_game_content(
    workspace: &WorkspaceSnapshot,
    external: &BuildAcquisitions,
    target: BuildTarget,
    optional: &OptionalPolicy,
    evidence: SourceEvidencePolicy,
    cancel: &Cancellation,
) -> Result<PreparedGameContent> {
    ensure!(
        matches!(
            target,
            BuildTarget::Client | BuildTarget::Server | BuildTarget::CurseForge
        ),
        "Bootstrap content needs a reference target"
    );
    prepare_selected_content(workspace, external, target, optional, evidence, cancel)
}
fn prepare_selected_content(
    workspace: &WorkspaceSnapshot,
    external: &BuildAcquisitions,
    target: BuildTarget,
    optional: &OptionalPolicy,
    evidence: SourceEvidencePolicy,
    cancel: &Cancellation,
) -> Result<PreparedGameContent> {
    let references = matches!(
        target,
        BuildTarget::Client | BuildTarget::Server | BuildTarget::CurseForge
    );
    let (selection, _) =
        super::acquisition::select_game_inputs(workspace, external, target, optional, cancel)?;
    let selected: BTreeSet<_> = selection
        .entries()
        .iter()
        .map(|entry| entry.owner.clone())
        .collect();
    let captured = capture_build_content(workspace, external, evidence, Some(&selected), cancel)?;
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
            let representation = if references {
                reference_for_target(file, supplied, target)?.unwrap_or(representation)
            } else {
                representation
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
                let (input, acquired, evidence) = if references {
                    file.into_reference()
                } else {
                    file.into_materialized()
                };
                leases.insert(input.owner.clone(), acquired);
                inputs.push(input);
                observed.push(evidence);
            }
            super::ObservedBuildContent::Unacquired { record, choice } => {
                if references
                    && selected.contains(&ContentOwner::Source(format!(
                        "backend:{}",
                        record.metadata_path.as_str()
                    )))
                    && let Some((input, evidence)) =
                        crate::engine::mrpack::ObservedFile::reference_input(
                            &record, &choice, evidence,
                        )?
                {
                    inputs.push(input);
                    observed.push(evidence);
                    continue;
                }
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
            ensure!(
                references && matches!(entry.representation, Representation::Download { .. }),
                "Full game inventory contains a reference"
            );
            collisions.insert_file(entry.destination.relative())?;
            continue;
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
    Ok(PreparedGameContent {
        project: captured.project,
        inventory,
        files,
        observed,
        comparisons: captured.comparisons,
    })
}

pub(super) fn reference_for_target(
    file: &empack_core::model::ResolvedFile,
    acquired: Option<&AcquiredBuildFile>,
    target: BuildTarget,
) -> Result<Option<Representation>> {
    if target == BuildTarget::CurseForge {
        let selection = match &file.acquisition {
            AcquisitionSpec::Provider { pin, slot, .. } => Some((pin, slot)),
            AcquisitionSpec::Manual { pin: Some(pin), .. } => Some((pin, &file.slot)),
            _ => None,
        };
        if let Some((pin, slot)) = selection {
            ensure!(
                acquired
                    .is_none_or(|file| !file.permissions.readonly && !file.permissions.executable),
                "CurseForge references cannot preserve custom file permissions"
            );
            return Ok(Some(Representation::Download {
                expected: file.expected.clone(),
                allowed: DownloadOrigins::Provider {
                    pin: pin.clone(),
                    slot: slot.clone(),
                },
            }));
        }
        if let AcquisitionSpec::Url(urls) = &file.acquisition {
            return Ok(Some(Representation::Download {
                expected: file.expected.clone(),
                allowed: DownloadOrigins::Urls(urls.clone()),
            }));
        }
        return Ok(None);
    }
    reference_for(file, acquired)
}

pub(super) fn reference_for(
    file: &ResolvedFile,
    acquired: Option<&AcquiredBuildFile>,
) -> Result<Option<Representation>> {
    if acquired.is_some_and(|file| file.permissions.readonly || file.permissions.executable) {
        return Ok(None);
    }
    let allowed = match &file.acquisition {
        AcquisitionSpec::Url(urls) => DownloadOrigins::Urls(urls.clone()),
        AcquisitionSpec::Provider { alternatives, .. } if !alternatives.is_empty() => {
            DownloadOrigins::Urls(NonEmpty::new(alternatives.clone())?)
        }
        AcquisitionSpec::Provider { pin, slot, .. }
            if matches!(
                pin.project,
                empack_core::identity::ProviderProjectId::CurseForge(_)
            ) =>
        {
            DownloadOrigins::Provider {
                pin: pin.clone(),
                slot: slot.clone(),
            }
        }
        _ => return Ok(None),
    };
    let mut expected = file.expected.clone();
    if let Some(acquired) = acquired {
        // All original assertions already matched. Computed export hashes do not replace
        // provenance: the returned project retains the original lock and source evidence.
        expected.digests = Some(acquired.content.observed_digests().clone());
        expected.size = Some(acquired.content.lease().len());
    }
    let Some(digests) = &expected.digests else {
        return Ok(None);
    };
    if let Some(observed) = &expected.accepted_observation
        && !digests.values().iter().any(|digest| {
            digest.algorithm() == empack_core::digest::DigestAlgorithm::Sha256
                && digest.bytes() == observed.bytes()
        })
    {
        return Ok(None);
    }
    Ok(Some(Representation::Download { expected, allowed }))
}

#[cfg(test)]
mod tests;
