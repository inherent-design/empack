//! Restore recorded bytes within exact managed placements; retain unrelated installations.
use super::SynchronizationCandidate;
use crate::{
    application::process_runtime::Cancellation,
    engine::{
        content::{InitialObservation, SourceEvidencePolicy, verify_observation},
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
    model::{ContentLayer, ResolvedProject},
    path::{PathSyntax, PortableRelPath},
};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub struct PreparedSynchronization {
    root: ProjectReadRoot,
    change: VerifiedFileChange,
    candidate: SynchronizationCandidate,
}
pub struct SynchronizationReceipt {
    pub publication: PublicationReceipt,
    pub project: ResolvedProject,
}
impl PreparedSynchronization {
    pub fn files(&self) -> &FilePlan {
        self.change.plan()
    }
    pub fn candidate(&self) -> &SynchronizationCandidate {
        &self.candidate
    }
    pub fn publish(
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
            cancel,
        )?;
        Ok(PreparedSynchronization {
            root,
            change,
            candidate: self.candidate,
        })
    }
}
pub(in crate::engine) fn plan_synchronization(
    workspace: MutationSnapshot,
    acquired: BTreeMap<LockedFileKey, AcquiredBuildFile>,
    cancel: &Cancellation,
) -> Result<SynchronizationPreparation> {
    plan_synchronization_with_resolution(workspace, acquired, None, cancel)
}
pub(in crate::engine) fn plan_synchronization_with_resolution(
    workspace: MutationSnapshot,
    acquired: BTreeMap<LockedFileKey, AcquiredBuildFile>,
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
    let mut owned = BTreeMap::new();
    for (key, dependency) in &candidate.project().lock().dependencies {
        for file in dependency.files.as_slice() {
            let slot = LockedFileKey {
                dependency: key.clone(),
                slot: file.slot.clone(),
            };
            let bytes = acquired
                .get(&slot)
                .context("Synchronization is missing a recorded file")?;
            slots.insert(slot);
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
                owned.insert(target, (dependency, placement));
            }
        }
    }
    ensure!(
        acquired.keys().all(|key| slots.contains(key)),
        "Synchronization received unrequested content"
    );
    let observed_content = verification::observed_mutation_for(
        workspace.observations(),
        content.keys().chain(previous.keys()).cloned(),
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
    for (target, (_, file, _)) in &previous {
        if !content.contains_key(target) {
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
    for record in workspace.backend_files(cancel)? {
        let target = ManagedPath::Content {
            layer: ContentLayer::Common,
            path: record.destination.relative().clone(),
        };
        let Some((dependency, placement)) = owned.get(&target) else {
            if let Some((dependency, file, placement)) = previous.get(&target) {
                ensure!(
                    record.matches_selection_and_requirements(
                        dependency.selected.as_ref(),
                        &placement.requirements
                    )?,
                    "Obsolete metadata has different ownership"
                );
                if let Some(expected) = file.expected.digests.as_ref().and_then(|set| {
                    set.values()
                        .iter()
                        .find(|digest| digest.algorithm() == record.digest.algorithm())
                }) {
                    ensure!(
                        *expected == record.digest,
                        "Obsolete metadata has different source bytes"
                    );
                } else {
                    let observed = if matches!(observed_content[&target], ObservedPath::File(_)) {
                        workspace.verify_file(
                            &crate::engine::layout::ProjectLayout::path(&target)?,
                            &file.expected,
                            cancel,
                        )?
                    } else {
                        // A missing old payload cannot supply another algorithm's digest.
                        // Immutable acquired bytes can, but only if every old assertion
                        // also matches; the new selection alone does not prove ownership.
                        acquired
                            .values()
                            .find(|bytes| {
                                let content = &bytes.content;
                                file.expected
                                    .size
                                    .is_none_or(|size| size == content.lease().len())
                                    && file.expected.digests.as_ref().is_none_or(|digests| {
                                        digests.check(content.observed_digests().values()).is_ok()
                                    })
                                    && file
                                        .expected
                                        .accepted_observation
                                        .as_ref()
                                        .is_none_or(|prior| prior == &content.lease().id())
                            })
                            .context("Missing obsolete payload has no matching content evidence")?
                            .content
                            .observed_digests()
                            .clone()
                    };
                    ensure!(
                        observed.values().contains(&record.digest),
                        "Obsolete metadata has different observed bytes"
                    );
                }
                removals.insert(ManagedPath::BackendDocument(record.metadata_path));
            }
            continue;
        };
        ensure!(
            previous.contains_key(&target),
            "Synchronization destination has untracked backend ownership"
        );
        // Restore the selected locked identity, including observed backend drift. The exact
        // captured metadata path is part of the approved plan; other installations remain.
        if !record.matches_selection_and_requirements(
            dependency.selected.as_ref(),
            &placement.requirements,
        )? || !content[&target]
            .content
            .observed_digests()
            .values()
            .contains(&record.digest)
        {
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
    refresh_pack_metadata(&workspace, candidate.project(), &mut documents, cancel)?;
    let observed = verification::observed_mutation_for(
        workspace.observations(),
        documents
            .keys()
            .chain(content.keys())
            .chain(removals.iter())
            .cloned(),
    )?;
    let mut desired = BTreeMap::new();
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
    })
}

fn refresh_pack_metadata(
    workspace: &WorkspaceSnapshot,
    project: &ResolvedProject,
    documents: &mut BTreeMap<ManagedPath, Vec<u8>>,
    cancel: &Cancellation,
) -> Result<()> {
    let target = ManagedPath::BackendDocument(PortableRelPath::parse(
        "pack.toml",
        PathSyntax::ProjectContent,
    )?);
    let source = match documents.get(&target) {
        Some(bytes) => Some(bytes.clone()),
        None => {
            match &verification::observed_mutation_for(workspace.observations(), [target.clone()])?
                [&target]
            {
                ObservedPath::Absent => None,
                ObservedPath::Directory => anyhow::bail!("Backend pack document is a directory"),
                ObservedPath::File(_) => workspace.read_document(
                    &PortableRelPath::parse("pack/pack.toml", PathSyntax::ProjectContent)?,
                    cancel,
                )?,
            }
        }
    };
    let Some(source) = source else {
        return Ok(());
    };
    let mut pack: toml::Value = toml::from_str(std::str::from_utf8(&source)?)?;
    let original = pack.clone();
    let table = pack
        .as_table_mut()
        .context("Backend pack must be a table")?;
    let metadata = &project.intent().metadata;
    table.insert("name".into(), toml::Value::String(metadata.name.clone()));
    table.insert(
        "version".into(),
        toml::Value::String(metadata.version.clone()),
    );
    for (field, value) in [
        ("author", &metadata.author),
        ("description", &metadata.description),
    ] {
        if let Some(value) = value {
            table.insert(field.into(), toml::Value::String(value.clone()));
        } else {
            table.remove(field);
        }
    }
    let runtime = &project.lock().runtime;
    let mut versions = toml::Table::new();
    versions.insert(
        "minecraft".into(),
        toml::Value::String(runtime.minecraft.as_str().into()),
    );
    use empack_core::model::LoaderKind;
    let loader = match runtime.loader {
        LoaderKind::Vanilla => None,
        LoaderKind::Fabric => Some("fabric"),
        LoaderKind::Quilt => Some("quilt"),
        LoaderKind::Forge => Some("forge"),
        LoaderKind::NeoForge => Some("neoforge"),
    };
    if let Some(loader) = loader {
        versions.insert(
            loader.into(),
            toml::Value::String(
                runtime
                    .loader_version
                    .as_ref()
                    .context("Locked loader lacks a version")?
                    .as_str()
                    .into(),
            ),
        );
    }
    table.insert("versions".into(), toml::Value::Table(versions));
    if pack != original {
        documents.insert(target, toml::to_string(&pack)?.into_bytes());
    }
    Ok(())
}

#[cfg(test)]
mod tests;
