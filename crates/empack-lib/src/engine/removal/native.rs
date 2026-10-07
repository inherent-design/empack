//! Exact removal owns placements and matching derivative metadata, never acquisition paths.
use super::*;
use crate::{
    application::process_runtime::Cancellation,
    engine::{
        content::SourceEvidencePolicy,
        layout::ProjectLayout,
        project::{MutationSnapshot, WorkspaceSnapshot},
        publication::{PublicationReceipt, Publisher},
        snapshot::{Observation, ProjectReadRoot},
        staging::MutableStage,
        verification::{self, VerifiedFileChange},
    },
};
use anyhow::{Context, ensure};
use empack_core::{
    digest::{ContentId, ExpectedDigest},
    files::{FileContent, FilePlan, ManagedPath, ObservedPath},
    model::ContentLayer,
    path::{PathSyntax, PortableRelPath},
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
    pub fn publish(self, publisher: &Publisher, cancel: &Cancellation) -> Result<RemovalReceipt> {
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
    plan_removal(workspace, selections, mode, cancel)?.stage(cancel)
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
        let limits = verification::candidate_stage_limits(workspace.observations(), &plan)?;
        let mut stage = MutableStage::empty()?;
        for (target, bytes) in &documents {
            let native = ProjectLayout::path(target)?;
            stage.write_attributed(
                &native,
                &mut bytes.as_slice(),
                bytes.len() as u64,
                plan.expected()[target].permissions,
                cancel,
            )?;
        }
        let stage = stage.freeze(limits, cancel)?;
        let (root, base) = workspace.into_native();
        let change = VerifiedFileChange::verify_mutation(base, plan, stage)?;
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
    plan_selected_removal(
        workspace,
        &selectors,
        mode,
        RemovalEvidencePolicy::RequireComplete,
        cancel,
    )
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
    let candidate = RemovalCandidate::prepare_with_policy(
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
    let mut documents = BTreeMap::from([
        (ManagedPath::IntentDocument, candidate.intent.bytes.clone()),
        (ManagedPath::LockDocument, candidate.lock.clone()),
    ]);
    if mode == RemovalMode::RemoveContent {
        refresh_index(&workspace, &removals, &mut documents, cancel)?;
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
    })
}

/// Preserve remaining index entries and all extension fields. Rebind the pack's index digest
/// after deleting exact observed entries; never pass a manifest label to a backend command.
fn refresh_index(
    workspace: &WorkspaceSnapshot,
    removals: &BTreeSet<ManagedPath>,
    documents: &mut BTreeMap<ManagedPath, Vec<u8>>,
    cancel: &Cancellation,
) -> Result<()> {
    let removed: BTreeSet<_> = removals
        .iter()
        .filter_map(|target| match target {
            ManagedPath::BackendDocument(path)
            | ManagedPath::Content {
                layer: ContentLayer::Common,
                path,
            } => Some(path.clone()),
            _ => None,
        })
        .collect();
    if removed.is_empty() {
        return Ok(());
    }
    let index_path = path("pack/index.toml")?;
    let pack_path = path("pack/pack.toml")?;
    let read = |path: &PortableRelPath| -> Result<Option<Vec<u8>>> {
        match workspace.observations().entries().get(path) {
            Some(Observation::File(_)) => workspace.read_document(path, cancel),
            Some(Observation::Directory { .. } | Observation::Ancestor(_)) => {
                anyhow::bail!("Backend document is a directory")
            }
            _ => Ok(None),
        }
    };
    let (index, pack) = (read(&index_path)?, read(&pack_path)?);
    let (Some(index), Some(pack)) = (&index, &pack) else {
        ensure!(
            index.is_none() && pack.is_none(),
            "Backend index and pack documents must both exist"
        );
        return Ok(());
    };
    let index_bytes = index;
    let mut index: toml::Value = toml::from_str(std::str::from_utf8(index_bytes)?)?;
    let mut pack: toml::Value = toml::from_str(std::str::from_utf8(pack)?)?;
    let no_hashes = pack
        .get("options")
        .and_then(|options| options.get("no-internal-hashes"))
        .map(|value| value.as_bool().context("Invalid internal-hash option"))
        .transpose()?
        .unwrap_or(false);
    let reference = pack
        .as_table_mut()
        .context("Pack document must be a table")?
        .entry("index")
        .or_insert_with(|| toml::Value::Table(toml::Table::new()))
        .as_table_mut()
        .context("Pack index reference must be a table")?;
    let selected = reference
        .get("file")
        .map(|value| value.as_str().context("Index path must be text"))
        .transpose()?
        .unwrap_or("index.toml");
    ensure!(
        selected.is_empty() || selected == "index.toml",
        "Unexpected backend index path"
    );
    reference.insert("file".into(), toml::Value::String("index.toml".into()));
    let hash = reference
        .get("hash")
        .map(|value| value.as_str().context("Index digest must be text"))
        .transpose()?
        .filter(|value| !value.is_empty());
    let publish_hash = hash.is_some() && !no_hashes;
    if let Some(hash) = hash {
        let declaration = ExpectedDigest::parse(
            reference
                .get("hash-format")
                .and_then(toml::Value::as_str)
                .context("Index digest has no algorithm")?,
            hash,
        )?;
        crate::engine::content::verify_observation(
            &mut index_bytes.as_slice(),
            &empack_core::model::ExpectedContent {
                digests: Some(empack_core::digest::DigestSet::new(vec![declaration])?),
                size: Some(index_bytes.len() as u64),
                accepted_observation: None,
            },
            index_bytes.len() as u64,
            SourceEvidencePolicy::Compatibility,
            crate::engine::content::InitialObservation::RequireEvidence,
            cancel,
        )?;
    }
    let files = index
        .as_table_mut()
        .context("Index must be a table")?
        .entry("files")
        .or_insert_with(|| toml::Value::Array(vec![]))
        .as_array_mut()
        .context("Index file entries must be an array")?;
    let mut paths = crate::engine::layout::CollisionIndex::default();
    let mut retained = Vec::new();
    for entry in files.drain(..) {
        let name = path(
            entry
                .get("file")
                .and_then(toml::Value::as_str)
                .context("Index entry lacks a file")?,
        )?;
        paths.insert_file(&name)?;
        if !removed.contains(&name) {
            for target in &removed {
                let mut collision = crate::engine::layout::CollisionIndex::default();
                collision.insert_file(target)?;
                collision
                    .insert_file(&name)
                    .context("Index aliases a selected removal target")?;
            }
            retained.push(entry);
        }
    }
    *files = retained;
    let index = toml::to_string(&index)?.into_bytes();
    if publish_hash {
        reference.insert("hash-format".into(), toml::Value::String("sha256".into()));
        reference.insert(
            "hash".into(),
            toml::Value::String(ExpectedDigest::Sha256(Sha256::digest(&index).into()).hex()),
        );
    } else {
        reference.remove("hash");
    }
    documents.insert(ManagedPath::BackendDocument(path("index.toml")?), index);
    documents.insert(
        ManagedPath::BackendDocument(path("pack.toml")?),
        toml::to_string(&pack)?.into_bytes(),
    );
    Ok(())
}

#[cfg(test)]
mod tests;
