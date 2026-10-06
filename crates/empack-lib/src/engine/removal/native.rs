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
    model::{ContentLayer, ResolvedIdentity},
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
    cancel.check()?;
    let workspace = workspace.into_workspace();
    let candidate = RemovalCandidate::prepare(
        workspace.intent(),
        workspace
            .prior_lock()
            .context("Removal requires an exact lock")?,
        selections,
        mode,
    )?;
    let current = workspace.require_resolved()?;
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
                            workspace.acquire_file(
                                &native,
                                Some(&file.expected),
                                SourceEvidencePolicy::Compatibility,
                                cancel,
                            )?;
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
        for record in workspace.backend_files(cancel)? {
            let mut owners = Vec::new();
            let mut claimed = false;
            for (key, dependency) in &current.lock().dependencies {
                if record.provider.as_ref().is_some_and(|actual| {
                    matches!(&dependency.identity,
                    ResolvedIdentity::Provider(expected) if actual.project == *expected)
                }) {
                    claimed = true;
                }
                for file in dependency.files.as_slice() {
                    for placement in file.placements.as_slice() {
                        if placement.layer != ContentLayer::Common
                            || placement.destination != record.destination
                        {
                            continue;
                        }
                        claimed = true;
                        if record.matches_selection_and_requirements(
                            dependency.selected.as_ref(),
                            &placement.requirements,
                        )? {
                            owners.push((key, file));
                        }
                    }
                }
            }
            if owners.is_empty() && !claimed {
                continue;
            }
            ensure!(
                owners.len() == 1,
                "Backend metadata does not identify one exact locked file: {}",
                record.metadata_path.as_str()
            );
            let (key, file) = owners[0];
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
                let (content, _) = workspace
                    .acquire_file(
                        &native,
                        Some(&file.expected),
                        SourceEvidencePolicy::Compatibility,
                        cancel,
                    )
                    .context("Cannot verify derivative metadata against selected content")?;
                ensure!(
                    content.observed_digests().values().contains(&record.digest),
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
    let limits = verification::candidate_stage_limits(workspace.observations(), &plan)?;
    let mut stage = MutableStage::empty()?;
    for (target, bytes) in &documents {
        let native = ProjectLayout::path(target)?;
        stage.write_attributed(
            &native,
            &mut bytes.as_slice(),
            bytes.len() as u64,
            desired[target].permissions,
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
    let reference = pack
        .get_mut("index")
        .and_then(toml::Value::as_table_mut)
        .context("Pack lacks an index reference")?;
    ensure!(
        reference.get("file").and_then(toml::Value::as_str) == Some("index.toml"),
        "Unexpected backend index path"
    );
    let declaration = ExpectedDigest::parse(
        reference
            .get("hash-format")
            .and_then(toml::Value::as_str)
            .context("Index digest has no algorithm")?,
        reference
            .get("hash")
            .and_then(toml::Value::as_str)
            .context("Index digest is absent")?,
    )?;
    crate::engine::content::verify_stream(
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
    let files = index
        .get_mut("files")
        .and_then(toml::Value::as_array_mut)
        .context("Backend index lacks file entries")?;
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
    reference.insert("hash-format".into(), toml::Value::String("sha256".into()));
    reference.insert(
        "hash".into(),
        toml::Value::String(ExpectedDigest::Sha256(Sha256::digest(&index).into()).hex()),
    );
    documents.insert(ManagedPath::BackendDocument(path("index.toml")?), index);
    documents.insert(
        ManagedPath::BackendDocument(path("pack.toml")?),
        toml::to_string(&pack)?.into_bytes(),
    );
    Ok(())
}

#[cfg(test)]
mod tests;
