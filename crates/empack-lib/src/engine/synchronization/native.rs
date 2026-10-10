//! Restore recorded bytes within exact managed placements; retain unrelated installations.
use super::SynchronizationCandidate;
use crate::{
    application::process_runtime::Cancellation,
    engine::{
        content::{InitialObservation, SourceEvidencePolicy, verify_observation},
        dependency_content::{self, DependencyContent, DependencyContents},
        mrpack::{AcquiredBuildFile, LockedFileKey},
        project::{MutationSnapshot, WorkspaceSnapshot},
        publication::{PublicationReceipt, Publisher},
        snapshot::ProjectReadRoot,
        verification::{self, VerifiedFileChange},
    },
};
use anyhow::{Context, Result, ensure};
use empack_core::{
    digest::ContentId,
    files::{FileContent, FilePlan, ManagedPath, ObservedPath},
    model::ResolvedProject,
    path::{PathSyntax, PortableRelPath},
};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub struct PreparedSynchronization {
    root: ProjectReadRoot,
    change: VerifiedFileChange,
    candidate: SynchronizationCandidate,
    references: BTreeSet<LockedFileKey>,
}
pub struct SynchronizationReceipt {
    pub publication: PublicationReceipt,
    pub project: ResolvedProject,
}
impl PreparedSynchronization {
    pub fn references(&self) -> &BTreeSet<LockedFileKey> {
        &self.references
    }
    pub fn files(&self) -> &FilePlan {
        self.change.plan()
    }
    pub fn candidate(&self) -> &SynchronizationCandidate {
        &self.candidate
    }
    pub(in crate::engine) fn publish(
        self,
        publisher: &Publisher,
        cancel: &Cancellation,
    ) -> Result<SynchronizationReceipt> {
        let publication = publisher.publish(&self.root, self.change, cancel)?;
        Ok(SynchronizationReceipt {
            publication,
            project: self.candidate.project,
        })
    }
}
/// Supplied files satisfy every recorded slot, including retained non-root installations.
/// The caller must authorize the resulting replacement plan before publication.
pub fn prepare_synchronization(
    workspace: MutationSnapshot,
    acquired: BTreeMap<LockedFileKey, AcquiredBuildFile>,
    cancel: &Cancellation,
) -> Result<PreparedSynchronization> {
    let planned = plan_synchronization(workspace, acquired, cancel)?;
    planned.bytes()?;
    planned.stage(cancel)
}
pub(in crate::engine) struct SynchronizationPreparation {
    workspace: WorkspaceSnapshot,
    candidate: SynchronizationCandidate,
    plan: FilePlan,
    documents: BTreeMap<ManagedPath, Vec<u8>>,
    content: BTreeMap<ManagedPath, AcquiredBuildFile>,
    references: BTreeSet<LockedFileKey>,
}
impl SynchronizationPreparation {
    pub(in crate::engine) fn bytes(&self) -> Result<u64> {
        self.plan.expected().values().try_fold(0u64, |n, f| {
            n.checked_add(f.bytes)
                .context("Synchronization staging size overflow")
        })
    }
    pub(in crate::engine) fn stage(self, cancel: &Cancellation) -> Result<PreparedSynchronization> {
        let (root, change) = verification::stage_mutation(
            self.workspace,
            self.plan,
            &self.documents,
            &self.content,
            self.candidate.project(),
            cancel,
        )?;
        Ok(PreparedSynchronization {
            root,
            change,
            candidate: self.candidate,
            references: self.references,
        })
    }
}
pub(in crate::engine) fn plan_synchronization(
    workspace: MutationSnapshot,
    acquired: BTreeMap<LockedFileKey, AcquiredBuildFile>,
    cancel: &Cancellation,
) -> Result<SynchronizationPreparation> {
    plan_synchronization_with_resolution(
        workspace,
        dependency_content::materialized(acquired),
        None,
        cancel,
    )
}
pub(in crate::engine) fn plan_synchronization_with_resolution(
    workspace: MutationSnapshot,
    acquired: DependencyContents,
    proposed: Option<&ResolvedProject>,
    cancel: &Cancellation,
) -> Result<SynchronizationPreparation> {
    cancel.check()?;
    let workspace = workspace.into_workspace();
    let candidate = SynchronizationCandidate::prepare_input(
        workspace.intent(),
        workspace.prior_lock(),
        proposed,
    )?;
    let mut previous = BTreeMap::new();
    for dependency in workspace
        .prior_lock()
        .into_iter()
        .flat_map(|prior| prior.lock().dependencies.values())
    {
        for file in dependency.files.as_slice() {
            for placement in file.placements.as_slice() {
                previous.insert(
                    ManagedPath::Content {
                        layer: placement.layer,
                        path: placement.destination.relative().clone(),
                    },
                    (dependency, file, placement),
                );
            }
        }
    }
    let mut slots = BTreeSet::new();
    let mut content = BTreeMap::new();
    let mut references = BTreeSet::new();
    let mut referenced = BTreeMap::new();
    for (key, dependency) in &candidate.project().lock().dependencies {
        for file in dependency.files.as_slice() {
            let slot = LockedFileKey {
                dependency: key.clone(),
                slot: file.slot.clone(),
            };
            let bytes = acquired
                .get(&slot)
                .context("Synchronization is missing a recorded file")?;
            slots.insert(slot.clone());
            let DependencyContent::Materialized(bytes) = bytes else {
                dependency_content::validate_reference(file)?;
                references.insert(slot);
                for placement in file.placements.as_slice() {
                    let target = ManagedPath::Content {
                        layer: placement.layer,
                        path: placement.destination.relative().clone(),
                    };
                    ensure!(
                        referenced.insert(target.clone(), &file.expected).is_none(),
                        "Synchronization has duplicate reference placements"
                    );
                }
                continue;
            };
            verify_observation(
                &mut bytes.content.lease().open(),
                &file.expected,
                bytes.content.lease().len(),
                SourceEvidencePolicy::Compatibility,
                InitialObservation::RequireEvidence,
                cancel,
            )?;
            for placement in file.placements.as_slice() {
                let target = ManagedPath::Content {
                    layer: placement.layer,
                    path: placement.destination.relative().clone(),
                };
                ensure!(
                    content.insert(target.clone(), bytes.clone()).is_none(),
                    "Synchronization has duplicate placements"
                );
            }
        }
    }
    ensure!(
        acquired.keys().all(|key| slots.contains(key)),
        "Synchronization received unrequested content"
    );
    let observed_content = verification::observed_mutation_for(
        workspace.observations(),
        content
            .keys()
            .chain(referenced.keys())
            .chain(previous.keys())
            .cloned(),
    )?;
    ensure!(
        observed_content
            .values()
            .all(|value| !matches!(value, ObservedPath::Directory)),
        "Synchronization destination is a directory"
    );
    for (target, observed) in &observed_content {
        ensure!(
            !matches!(observed, ObservedPath::File(_)) || previous.contains_key(target),
            "Synchronization destination contains untracked content"
        );
    }
    let mut removals = BTreeSet::new();
    let mut retained = BTreeMap::new();
    for (target, expected) in &referenced {
        if let ObservedPath::File(before) = &observed_content[target] {
            let (_, old, _) = previous
                .get(target)
                .context("Reference destination contains untracked content")?;
            workspace.verify_file(
                &crate::engine::layout::ProjectLayout::path(target)?,
                &old.expected,
                cancel,
            )?;
            if &old.expected == *expected {
                retained.insert(target.clone(), before.clone());
            } else {
                removals.insert(target.clone());
            }
        } else if previous.contains_key(target) {
            removals.insert(target.clone());
        }
    }
    for (target, (_, file, _)) in &previous {
        if !content.contains_key(target) && !referenced.contains_key(target) {
            if matches!(&observed_content[target], ObservedPath::File(_)) {
                workspace.verify_file(
                    &crate::engine::layout::ProjectLayout::path(target)?,
                    &file.expected,
                    cancel,
                )?;
            }
            removals.insert(target.clone());
        }
    }
    verification::retain_acquisition_sources(
        workspace.observations(),
        candidate.project(),
        &mut removals,
        content.keys().chain(referenced.keys()),
    )?;
    let mut documents = BTreeMap::from([
        (
            ManagedPath::IntentDocument,
            candidate.intent_document().bytes.clone(),
        ),
        (
            ManagedPath::LockDocument,
            candidate.lock_document().to_vec(),
        ),
    ]);
    if candidate.preserves_lock_document() {
        documents.insert(
            ManagedPath::LockDocument,
            workspace
                .read_document(
                    &PortableRelPath::parse("empack.lock", PathSyntax::ProjectContent)?,
                    cancel,
                )?
                .context("Captured lock disappeared")?,
        );
    }
    let observed = verification::observed_mutation_for(
        workspace.observations(),
        documents
            .keys()
            .chain(content.keys())
            .chain(retained.keys())
            .chain(removals.iter())
            .cloned(),
    )?;
    let mut desired = retained;
    for (target, bytes) in &documents {
        let permissions = match &observed[target] {
            ObservedPath::File(before) => before.permissions,
            ObservedPath::Absent if *target == ManagedPath::LockDocument => {
                empack_core::files::FilePermissions {
                    readonly: false,
                    executable: false,
                }
            }
            _ => anyhow::bail!("Synchronization document is not a regular file"),
        };
        desired.insert(
            target.clone(),
            FileContent {
                content: ContentId::from_sha256(Sha256::digest(bytes).into()),
                bytes: bytes.len() as u64,
                permissions,
            },
        );
    }
    for (target, file) in &content {
        desired.insert(
            target.clone(),
            FileContent {
                content: file.content.lease().id(),
                bytes: file.content.lease().len(),
                permissions: file.permissions,
            },
        );
    }
    let plan = verification::plan_mutation_files(&observed, &desired, &removals)?;
    documents.retain(|target, _| plan.expected().contains_key(target));
    content.retain(|target, _| plan.expected().contains_key(target));
    verification::candidate_stage_limits(workspace.observations(), &plan)?;
    Ok(SynchronizationPreparation {
        workspace,
        candidate,
        plan,
        documents,
        content,
        references,
    })
}

#[cfg(test)]
mod tests;
