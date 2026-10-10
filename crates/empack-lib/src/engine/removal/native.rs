//! Exact removal owns verified native placements, never acquisition paths or foreign records.
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
    digest::ContentId,
    files::{FileContent, FilePlan, ManagedPath, ObservedPath},
};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub struct PreparedRemoval {
    root: ProjectReadRoot,
    change: VerifiedFileChange,
    candidate: RemovalCandidate,
}
pub struct RemovalReceipt {
    pub publication: PublicationReceipt,
    pub project: ResolvedProject,
    pub mode: RemovalMode,
    pub selected: BTreeSet<DependencyKey>,
    pub incomplete_evidence: Vec<DependencyKey>,
}
impl PreparedRemoval {
    pub fn files(&self) -> &FilePlan {
        self.change.plan()
    }
    pub fn candidate(&self) -> &RemovalCandidate {
        &self.candidate
    }
    pub(in crate::engine) fn publish(
        self,
        publisher: &Publisher,
        cancel: &Cancellation,
    ) -> Result<RemovalReceipt> {
        let publication = publisher.publish(&self.root, self.change, cancel)?;
        Ok(RemovalReceipt {
            publication,
            project: self.candidate.project,
            mode: self.candidate.plan.mode(),
            incomplete_evidence: self.candidate.plan.incomplete_evidence().to_vec(),
            selected: self.candidate.plan.selected().keys().cloned().collect(),
        })
    }
}
/// Preparation binds exact native placements; publication requires a separate grant.
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
    let selections = selection::resolve(&current, selectors)?;
    let candidate = RemovalCandidate::prepare_selected(
        workspace.intent(),
        workspace
            .prior_lock()
            .context("Removal requires an exact lock")?,
        &selections,
        mode,
        evidence,
    )?;
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
    }
    verification::retain_acquisition_sources(
        workspace.observations(),
        candidate.project(),
        &mut removals,
        std::iter::empty(),
    )?;
    let documents = BTreeMap::from([
        (ManagedPath::IntentDocument, candidate.intent.bytes.clone()),
        (ManagedPath::LockDocument, candidate.lock.clone()),
    ]);
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
    })
}

#[cfg(test)]
mod tests;
