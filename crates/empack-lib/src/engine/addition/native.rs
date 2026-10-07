//! Addition authorizes only verified selected placements and coherent logical documents.
mod adoption;
use super::AdditionCandidate;
use crate::{
    application::process_runtime::Cancellation,
    engine::{
        content::{InitialObservation, SourceEvidencePolicy, verify_observation},
        dependency_content::{self, DependencyContent, DependencyContents},
        layout::ProjectLayout,
        mrpack::{AcquiredBuildFile, LockedFileKey},
        project::{MutationSnapshot, WorkspaceSnapshot},
        publication::{PublicationReceipt, Publisher},
        snapshot::ProjectReadRoot,
        verification::{self, VerifiedFileChange},
    },
};
pub(in crate::engine) use adoption::plan_adoption;
use anyhow::{Context, Result, ensure};
use empack_core::{
    addition::{AdditionGroup, ReplacementSelection},
    digest::ContentId,
    files::{FileContent, FilePlan, ManagedPath, ObservedPath},
    model::ResolvedProject,
    path::{PathSyntax, PortableRelPath},
};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub struct PreparedAddition {
    root: ProjectReadRoot,
    change: VerifiedFileChange,
    candidate: AdditionCandidate,
    references: BTreeSet<LockedFileKey>,
}
pub struct AdditionReceipt {
    pub publication: PublicationReceipt,
    pub project: ResolvedProject,
}
impl PreparedAddition {
    pub fn references(&self) -> &BTreeSet<LockedFileKey> {
        &self.references
    }
    pub fn files(&self) -> &FilePlan {
        self.change.plan()
    }
    pub fn candidate(&self) -> &AdditionCandidate {
        &self.candidate
    }
    pub fn publish(self, publisher: &Publisher, cancel: &Cancellation) -> Result<AdditionReceipt> {
        let publication = publisher.publish(&self.root, self.change, cancel)?;
        Ok(AdditionReceipt {
            publication,
            project: self.candidate.project,
        })
    }
}
/// Supplied content is keyed by the resolved request; canonical aliases are bound here.
/// Same-identity updates require the caller's replacement authorization before publication.
pub fn prepare_addition(
    workspace: MutationSnapshot,
    group: &AdditionGroup,
    acquired: BTreeMap<LockedFileKey, AcquiredBuildFile>,
    cancel: &Cancellation,
) -> Result<PreparedAddition> {
    let planned = plan_addition(
        workspace,
        group,
        dependency_content::materialized(acquired),
        cancel,
    )?;
    planned.bytes()?;
    planned.stage(cancel)
}
pub(in crate::engine) struct AdditionPreparation {
    workspace: WorkspaceSnapshot,
    candidate: AdditionCandidate,
    plan: FilePlan,
    documents: BTreeMap<ManagedPath, Vec<u8>>,
    content: BTreeMap<ManagedPath, AcquiredBuildFile>,
    references: BTreeSet<LockedFileKey>,
}
impl AdditionPreparation {
    pub(in crate::engine) fn candidate(&self) -> &AdditionCandidate {
        &self.candidate
    }
    pub(in crate::engine) fn bytes(&self) -> Result<u64> {
        self.plan.expected().values().try_fold(0u64, |n, f| {
            n.checked_add(f.bytes)
                .context("Addition staging size overflow")
        })
    }
    pub(in crate::engine) fn stage(self, cancel: &Cancellation) -> Result<PreparedAddition> {
        let (root, change) = verification::stage_mutation(
            self.workspace,
            self.plan,
            &self.documents,
            &self.content,
            cancel,
        )?;
        Ok(PreparedAddition {
            root,
            change,
            candidate: self.candidate,
            references: self.references,
        })
    }
}
pub(in crate::engine) fn plan_addition(
    workspace: MutationSnapshot,
    group: &AdditionGroup,
    acquired: DependencyContents,
    cancel: &Cancellation,
) -> Result<AdditionPreparation> {
    plan_change(workspace, group, acquired, ChangeMode::Add, cancel)
}
pub(in crate::engine) fn plan_update(
    workspace: MutationSnapshot,
    group: &AdditionGroup,
    acquired: DependencyContents,
    cancel: &Cancellation,
) -> Result<AdditionPreparation> {
    plan_change(workspace, group, acquired, ChangeMode::Update, cancel)
}
pub(in crate::engine) fn plan_replacement(
    workspace: MutationSnapshot,
    group: &AdditionGroup,
    acquired: DependencyContents,
    selection: &ReplacementSelection,
    cancel: &Cancellation,
) -> Result<AdditionPreparation> {
    plan_change(
        workspace,
        group,
        acquired,
        ChangeMode::Replace(selection),
        cancel,
    )
}
enum ChangeMode<'a> {
    Add,
    Update,
    Replace(&'a ReplacementSelection),
}
fn plan_change(
    workspace: MutationSnapshot,
    group: &AdditionGroup,
    acquired: DependencyContents,
    mode: ChangeMode<'_>,
    cancel: &Cancellation,
) -> Result<AdditionPreparation> {
    cancel.check()?;
    let workspace = workspace.into_workspace();
    let current = workspace.require_resolved()?;
    let prior = workspace.prior_lock().context("Addition requires a lock")?;
    let candidate = match mode {
        ChangeMode::Add => AdditionCandidate::prepare(workspace.intent(), prior, group)?,
        ChangeMode::Update => AdditionCandidate::prepare_update(workspace.intent(), prior, group)?,
        ChangeMode::Replace(selection) => {
            AdditionCandidate::prepare_replacement(workspace.intent(), prior, group, selection)?
        }
    };
    let mut bound = BTreeMap::new();
    for (slot, content) in acquired {
        let key = candidate
            .plan()
            .bindings()
            .get(&slot.dependency)
            .context("Addition received unrequested content")?;
        ensure!(
            bound
                .insert(
                    LockedFileKey {
                        dependency: key.clone(),
                        slot: slot.slot
                    },
                    content
                )
                .is_none(),
            "Addition repeats a canonical file"
        );
    }
    let acquired = bound;
    let selected: BTreeSet<_> = candidate.plan().bindings().values().cloned().collect();
    let mut expected_slots = BTreeSet::new();
    let mut content = BTreeMap::new();
    let mut references = BTreeSet::new();
    let mut referenced = BTreeMap::new();
    let mut old = BTreeMap::new();
    for key in &selected {
        if let Some(dependency) = current.lock().dependencies.get(key) {
            for file in dependency.files.as_slice() {
                for placement in file.placements.as_slice() {
                    old.insert(
                        ManagedPath::Content {
                            layer: placement.layer,
                            path: placement.destination.relative().clone(),
                        },
                        &file.expected,
                    );
                }
            }
        }
        for file in candidate.project().lock().dependencies[key]
            .files
            .as_slice()
        {
            let slot = LockedFileKey {
                dependency: key.clone(),
                slot: file.slot.clone(),
            };
            let bytes = acquired
                .get(&slot)
                .context("Addition is missing an exact acquired file")?;
            expected_slots.insert(slot.clone());
            let DependencyContent::Materialized(bytes) = bytes else {
                dependency_content::validate_reference(file)?;
                references.insert(slot);
                for placement in file.placements.as_slice() {
                    let target = ManagedPath::Content {
                        layer: placement.layer,
                        path: placement.destination.relative().clone(),
                    };
                    ensure!(
                        referenced.insert(target, &file.expected).is_none(),
                        "Addition has duplicate reference placements"
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
                    content.insert(target, bytes.clone()).is_none(),
                    "Addition has duplicate placements"
                );
            }
        }
    }
    ensure!(
        acquired.keys().all(|slot| expected_slots.contains(slot)),
        "Addition received unrequested content"
    );
    let mut removals: BTreeSet<_> = old
        .keys()
        .filter(|target| !content.contains_key(*target) && !referenced.contains_key(*target))
        .cloned()
        .collect();
    let observed_content = verification::observed_mutation_for(
        workspace.observations(),
        old.keys()
            .chain(content.keys())
            .chain(referenced.keys())
            .cloned(),
    )?;
    for (target, observed) in &observed_content {
        match observed {
            ObservedPath::File(_) => {
                // A matching hash alone does not authorize adoption of untracked user files.
                let expected = old
                    .get(target)
                    .context("Addition destination contains untracked content")?;
                workspace.verify_file(&ProjectLayout::path(target)?, expected, cancel)?;
            }
            ObservedPath::Directory => anyhow::bail!("Addition destination is a directory"),
            ObservedPath::Absent => {}
        }
    }
    let mut retained = BTreeMap::new();
    for (target, expected) in &referenced {
        if let ObservedPath::File(before) = &observed_content[target] {
            if old.get(target) == Some(expected) {
                retained.insert(target.clone(), before.clone());
            } else {
                // The old bytes were verified above. Reference-only updates retire that
                // old materialization rather than silently treating it as the new pin.
                removals.insert(target.clone());
            }
        } else if old.contains_key(target) {
            // Absence needs no file mutation, but any owned direct index entry is stale.
            removals.insert(target.clone());
        }
    }
    // Derivative backend records for changed selections cannot continue claiming the old pin.
    // Preserve unrelated metadata; known ownership conflicts at affected destinations fail.
    for record in workspace.backend_files(cancel)? {
        let target = ManagedPath::Content {
            layer: empack_core::model::ContentLayer::Common,
            path: record.destination.relative().clone(),
        };
        if !old.contains_key(&target)
            && !content.contains_key(&target)
            && !referenced.contains_key(&target)
        {
            continue;
        }
        let (key, file) = record
            .locked_owner(&current)?
            .context("Addition destination has untracked backend ownership")?;
        ensure!(
            selected.contains(key),
            "Addition overlaps retained backend ownership"
        );
        if !candidate.plan().changed().contains(key) && !referenced.contains_key(&target) {
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
            let digests =
                workspace.verify_file(&ProjectLayout::path(&target)?, &file.expected, cancel)?;
            ensure!(
                digests.values().contains(&record.digest),
                "Backend digest differs from selected bytes"
            );
        }
        if candidate.plan().changed().contains(key) {
            removals.insert(ManagedPath::BackendDocument(record.metadata_path));
        }
    }
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
    if candidate.project().lock() == current.lock() {
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
    let mut updated_index = BTreeMap::new();
    for (target, file) in &content {
        if !matches!(&observed_content[target], ObservedPath::File(before) if before.content == file.content.lease().id())
            && let ManagedPath::Content {
                layer: empack_core::model::ContentLayer::Common,
                path,
            } = target
        {
            updated_index.insert(
                path.clone(),
                empack_core::digest::ExpectedDigest::Sha256(*file.content.lease().id().bytes()),
            );
        }
    }
    crate::engine::backend::index::refresh_index_with_updates(
        &workspace,
        &removals,
        &updated_index,
        &mut documents,
        cancel,
    )?;
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
        let ObservedPath::File(before) = &observed[target] else {
            anyhow::bail!("Addition document is not an existing regular file")
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
    Ok(AdditionPreparation {
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
