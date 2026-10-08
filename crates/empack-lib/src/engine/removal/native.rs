//! Exact removal owns placements and matching derivative metadata, never acquisition paths.
use super::*;
use crate::{
    application::process_runtime::Cancellation,
    engine::{
        layout::ProjectLayout,
        project::{MutationSnapshot, WorkspaceSnapshot},
        publication::{PublicationReceipt, Publisher},
        snapshot::ProjectReadRoot,
        verification::{self, VerifiedFileChange},
    },
};
use anyhow::{Context, ensure};
use empack_core::{
    digest::{ContentId, ExpectedDigest},
    files::{FileContent, FilePlan, ManagedPath, ObservedPath},
    model::{ContentLayer, ExpectedContent},
    path::{PathSyntax, PortableRelPath},
};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

/// Exact observed metadata and byte assertions, without secret-bearing locators.
#[derive(Debug, Clone)]
pub struct ObservedRemovalSelection {
    pub metadata_path: PortableRelPath,
    pub destination: empack_core::path::InstallDestination,
    pub provider: Option<crate::engine::backend::ProviderObservation>,
    pub digest: ExpectedDigest,
}
pub struct PreparedRemoval {
    root: ProjectReadRoot,
    change: VerifiedFileChange,
    candidate: RemovalCandidate,
    observed: Vec<ObservedRemovalSelection>,
    untracked_evidence: Vec<PortableRelPath>,
}
pub struct RemovalReceipt {
    pub publication: PublicationReceipt,
    pub project: ResolvedProject,
    pub mode: RemovalMode,
    pub selected: BTreeSet<DependencyKey>,
    pub incomplete_evidence: Vec<DependencyKey>,
    pub observed: Vec<ObservedRemovalSelection>,
    pub untracked_evidence: Vec<PortableRelPath>,
}
impl PreparedRemoval {
    pub fn files(&self) -> &FilePlan {
        self.change.plan()
    }
    pub fn candidate(&self) -> &RemovalCandidate {
        &self.candidate
    }
    pub fn observed(&self) -> &[ObservedRemovalSelection] {
        &self.observed
    }
    pub fn untracked_evidence(&self) -> &[PortableRelPath] {
        &self.untracked_evidence
    }
    pub(in crate::engine) fn publish(
        self,
        publisher: &Publisher,
        cancel: &Cancellation,
    ) -> Result<RemovalReceipt> {
        let publication = publisher.publish(&self.root, self.change, cancel)?;
        Ok(RemovalReceipt {
            publication,
            observed: self.observed,
            untracked_evidence: self.untracked_evidence,
            project: self.candidate.project,
            mode: self.candidate.plan.mode(),
            incomplete_evidence: self.candidate.plan.incomplete_evidence().to_vec(),
            selected: self.candidate.plan.selected().keys().cloned().collect(),
        })
    }
}
fn path(name: &str) -> Result<PortableRelPath> {
    Ok(PortableRelPath::parse(name, PathSyntax::ProjectContent)?)
}
/// The reader must capture managed content and backend metadata, including selected placements.
/// This prepares private documents only. Publication is a separate approved operation.
pub fn prepare_removal(
    workspace: MutationSnapshot,
    selections: &NonEmpty<DependencyKey>,
    mode: RemovalMode,
    cancel: &Cancellation,
) -> Result<PreparedRemoval> {
    prepare_removal_with_policy(
        workspace,
        selections,
        mode,
        RemovalEvidencePolicy::RequireComplete,
        cancel,
    )
}
pub fn prepare_removal_with_policy(
    workspace: MutationSnapshot,
    selections: &NonEmpty<DependencyKey>,
    mode: RemovalMode,
    evidence: RemovalEvidencePolicy,
    cancel: &Cancellation,
) -> Result<PreparedRemoval> {
    plan_removal(workspace, selections, mode, evidence, cancel)?.stage(cancel)
}

pub(in crate::engine) struct RemovalPreparation {
    workspace: WorkspaceSnapshot,
    candidate: RemovalCandidate,
    plan: FilePlan,
    documents: BTreeMap<ManagedPath, Vec<u8>>,
    observed: Vec<ObservedRemovalSelection>,
    untracked_evidence: Vec<PortableRelPath>,
}
impl RemovalPreparation {
    pub(in crate::engine) fn bytes(&self) -> Result<u64> {
        self.plan.expected().values().try_fold(0u64, |total, file| {
            total
                .checked_add(file.bytes)
                .context("Removal staging size overflow")
        })
    }
    pub(in crate::engine) fn stage(self, cancel: &Cancellation) -> Result<PreparedRemoval> {
        let Self {
            workspace,
            candidate,
            plan,
            documents,
            observed,
            untracked_evidence,
        } = self;
        let (root, change) = verification::stage_mutation(
            workspace,
            plan,
            &documents,
            &BTreeMap::new(),
            candidate.project(),
            cancel,
        )?;
        Ok(PreparedRemoval {
            root,
            change,
            candidate,
            observed,
            untracked_evidence,
        })
    }
}
/// Read and verify observations without retaining payload copies; staging admission follows sizing.
pub(in crate::engine) fn plan_removal(
    workspace: MutationSnapshot,
    selections: &NonEmpty<DependencyKey>,
    mode: RemovalMode,
    evidence: RemovalEvidencePolicy,
    cancel: &Cancellation,
) -> Result<RemovalPreparation> {
    let selectors = NonEmpty::new(
        selections
            .as_slice()
            .iter()
            .cloned()
            .map(RemovalSelector::Key)
            .collect(),
    )?;
    plan_selected_removal(workspace, &selectors, mode, evidence, cancel)
}

pub(in crate::engine) fn plan_selected_removal(
    workspace: MutationSnapshot,
    selectors: &NonEmpty<RemovalSelector>,
    mode: RemovalMode,
    evidence: RemovalEvidencePolicy,
    cancel: &Cancellation,
) -> Result<RemovalPreparation> {
    cancel.check()?;
    let workspace = workspace.into_workspace();
    let current = workspace.require_resolved()?;
    let records = workspace.backend_files(cancel)?;
    let selections = selection::resolve(&current, &records, selectors)?;
    let candidate = RemovalCandidate::prepare_selected(
        workspace.intent(),
        workspace
            .prior_lock()
            .context("Removal requires an exact lock")?,
        &selections,
        mode,
        evidence,
    )?;
    let untracked_evidence = super::untracked_evidence(&current, &records, mode, evidence)?;
    let mut selected_observed = Vec::new();
    let mut removals = BTreeSet::new();
    if mode == RemovalMode::RemoveContent {
        for dependency in candidate.plan.selected().values() {
            for file in dependency.files.as_slice() {
                ensure!(
                    file.expected.digests.is_some() || file.expected.accepted_observation.is_some(),
                    "Removal requires locked content evidence"
                );
                for placement in file.placements.as_slice() {
                    let target = ManagedPath::Content {
                        layer: placement.layer,
                        path: placement.destination.relative().clone(),
                    };
                    let native = ProjectLayout::path(&target)?;
                    let observed = verification::observed_mutation_for(
                        workspace.observations(),
                        [target.clone()],
                    )?;
                    match &observed[&target] {
                        ObservedPath::File(_) => {
                            // Recheck original declarations; an edited file is not disposable content.
                            workspace.verify_file(&native, &file.expected, cancel)?;
                            removals.insert(target);
                        }
                        ObservedPath::Absent => {}
                        ObservedPath::Directory => {
                            anyhow::bail!("Removal destination is a directory: {}", native.as_str())
                        }
                    }
                }
            }
        }
        for metadata_path in &selections.observed {
            let record = records
                .iter()
                .find(|record| record.metadata_path == *metadata_path)
                .context("Selected metadata disappeared")?;
            ensure!(
                record.locked_owner(&current)?.is_none(),
                "Observed selection acquired a locked owner"
            );
            let target = ManagedPath::Content {
                layer: ContentLayer::Common,
                path: record.destination.relative().clone(),
            };
            let native = ProjectLayout::path(&target)?;
            let observed =
                verification::observed_mutation_for(workspace.observations(), [target.clone()])?;
            match &observed[&target] {
                ObservedPath::File(_) => {
                    workspace.verify_file(
                        &native,
                        &ExpectedContent {
                            digests: Some(empack_core::digest::DigestSet::new(vec![
                                record.digest.clone(),
                            ])?),
                            size: None,
                            accepted_observation: None,
                        },
                        cancel,
                    )?;
                    removals.insert(target);
                }
                ObservedPath::Absent => {}
                ObservedPath::Directory => {
                    anyhow::bail!("Observed removal destination is a directory")
                }
            }
            removals.insert(ManagedPath::BackendDocument(record.metadata_path.clone()));
            selected_observed.push(ObservedRemovalSelection {
                metadata_path: record.metadata_path.clone(),
                destination: record.destination.clone(),
                provider: record.provider.clone(),
                digest: record.digest.clone(),
            });
        }
        for record in records {
            if !candidate.plan.selected().values().any(|dependency| {
                dependency.files.as_slice().iter().any(|file| {
                    file.placements.as_slice().iter().any(|placement| {
                        placement.layer == ContentLayer::Common
                            && placement.destination == record.destination
                    })
                })
            }) {
                continue;
            }
            let Some((key, file)) = record.locked_owner(&current)? else {
                continue;
            };
            if !candidate.plan.selected().contains_key(key) {
                continue;
            }
            if let Some(digest) = file.expected.digests.as_ref().and_then(|set| {
                set.values()
                    .iter()
                    .find(|digest| digest.algorithm() == record.digest.algorithm())
            }) {
                ensure!(
                    *digest == record.digest,
                    "Backend digest differs from selected content"
                );
            } else {
                let native = ProjectLayout::path(&ManagedPath::Content {
                    layer: ContentLayer::Common,
                    path: record.destination.relative().clone(),
                })?;
                let observed = workspace
                    .verify_file(&native, &file.expected, cancel)
                    .context("Cannot verify derivative metadata against selected content")?;
                ensure!(
                    observed.values().contains(&record.digest),
                    "Backend digest differs from selected bytes"
                );
            }
            removals.insert(ManagedPath::BackendDocument(record.metadata_path));
        }
    }
    verification::retain_acquisition_sources(
        workspace.observations(),
        candidate.project(),
        &mut removals,
        std::iter::empty(),
    )?;
    let mut documents = BTreeMap::from([
        (ManagedPath::IntentDocument, candidate.intent.bytes.clone()),
        (ManagedPath::LockDocument, candidate.lock.clone()),
    ]);
    if candidate.plan.selected().is_empty() {
        // Observed-only removal changes no logical documents, including user formatting.
        documents.insert(
            ManagedPath::LockDocument,
            workspace
                .read_document(&path("empack.lock")?, cancel)?
                .context("Captured lock disappeared")?,
        );
    }
    if mode == RemovalMode::RemoveContent {
        crate::engine::backend::index::refresh_index(
            &workspace,
            &removals,
            &mut documents,
            cancel,
        )?;
    }
    let observed = verification::observed_mutation_for(
        workspace.observations(),
        documents.keys().chain(removals.iter()).cloned(),
    )?;
    let mut desired = BTreeMap::new();
    for (target, bytes) in &documents {
        let ObservedPath::File(before) = &observed[target] else {
            anyhow::bail!("Mutation document is not an existing regular file");
        };
        desired.insert(
            target.clone(),
            FileContent {
                content: ContentId::from_sha256(Sha256::digest(bytes).into()),
                bytes: bytes.len() as u64,
                permissions: before.permissions,
            },
        );
    }
    let plan = verification::plan_files(&observed, &desired, &removals)?;
    verification::candidate_stage_limits(workspace.observations(), &plan)?;
    Ok(RemovalPreparation {
        workspace,
        candidate,
        plan,
        documents,
        observed: selected_observed,
        untracked_evidence,
    })
}

#[cfg(test)]
mod tests;
