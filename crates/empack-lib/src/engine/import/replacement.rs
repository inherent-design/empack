//! A complete file candidate precedes any live managed replacement.
use super::ImportCandidate;
use crate::{
    application::process_runtime::Cancellation,
    engine::{
        documents::DocumentCodec,
        layout::ProjectLayout,
        project::ReplacementSnapshot,
        publication::{PublicationReceipt, Publisher},
        snapshot::ProjectReadRoot,
        staging::MutableStage,
        verification::{
            VerifiedFileChange, candidate_stage_limits, observed_files_for, plan_files,
        },
    },
};
use anyhow::{Context, Result, ensure};
use empack_core::{
    digest::ContentId,
    files::{FileContent, FilePermissions, FilePlan, ManagedPath, ObservedPath},
    model::ResolvedProject,
};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportReplacementPolicy {
    RejectExisting,
    /// Replace only the captured authoring documents and managed content roots.
    /// The host must present and authorize the exact returned file plan before publication.
    ReplaceManagedContent,
}
pub struct PreparedImportReplacement {
    root: ProjectReadRoot,
    change: VerifiedFileChange,
    project: ResolvedProject,
}
pub struct ImportReplacementReceipt {
    pub publication: PublicationReceipt,
    pub project: ResolvedProject,
}
pub struct PreparedImportCreation {
    change: crate::engine::publication::PreparedRootCreation,
    project: ResolvedProject,
}
impl PreparedImportCreation {
    pub fn plan(&self) -> &FilePlan {
        self.change.plan()
    }
    pub fn project(&self) -> &ResolvedProject {
        &self.project
    }
    pub fn publish(
        self,
        publisher: &Publisher,
        cancel: &Cancellation,
    ) -> Result<ImportReplacementReceipt> {
        Ok(ImportReplacementReceipt {
            publication: publisher.publish_new(self.change, cancel)?,
            project: self.project,
        })
    }
}
/// Reuse complete import staging against an empty private workspace. The live destination remains
/// absent; its parent/child binding is carried separately into no-replace publication.
pub fn prepare_import_creation(
    target: crate::engine::project::NewProjectSnapshot,
    candidate: ImportCandidate,
    limits: crate::engine::snapshot::SnapshotLimits,
    cancel: &Cancellation,
) -> Result<PreparedImportCreation> {
    target.revalidate(cancel)?;
    let scratch = tempfile::tempdir()?;
    let workspace = crate::engine::project::ProjectReader::new(
        crate::engine::publication::RecoveryReader::new(scratch.path().join("unused-host-state")),
    )
    .capture_replacement(scratch.path(), limits, cancel)?;
    let prepared = prepare_import_replacement(
        workspace,
        candidate,
        ImportReplacementPolicy::RejectExisting,
        cancel,
    )?;
    target.revalidate(cancel)?;
    let change =
        crate::engine::publication::PreparedRootCreation::from_verified(target, prepared.change)?;
    Ok(PreparedImportCreation {
        change,
        project: prepared.project,
    })
}
impl PreparedImportReplacement {
    pub fn plan(&self) -> &FilePlan {
        self.change.plan()
    }
    pub fn project(&self) -> &ResolvedProject {
        &self.project
    }
    /// Trusted host composition supplies publication only after approving this exact candidate.
    pub fn publish(
        self,
        publisher: &Publisher,
        cancel: &Cancellation,
    ) -> Result<ImportReplacementReceipt> {
        let publication = publisher.publish(&self.root, self.change, cancel)?;
        Ok(ImportReplacementReceipt {
            publication,
            project: self.project,
        })
    }
}
/// Synchronous private staging, intended for an admitted owned worker just like build preparation.
/// No live file is changed. A later source edit invalidates the complete publication plan.
pub fn prepare_import_replacement(
    workspace: ReplacementSnapshot,
    candidate: ImportCandidate,
    policy: ImportReplacementPolicy,
    cancel: &Cancellation,
) -> Result<PreparedImportReplacement> {
    cancel.check()?;
    let project = candidate.project();
    let default_permissions = FilePermissions {
        readonly: false,
        executable: false,
    };
    let mut documents = BTreeMap::from([
        (
            ManagedPath::IntentDocument,
            DocumentCodec.encode_intent(project.intent())?,
        ),
        (
            ManagedPath::LockDocument,
            DocumentCodec.encode_lock(project)?,
        ),
    ]);
    let mut desired = BTreeMap::new();
    let mut payloads = BTreeMap::new();
    for (target, bytes) in &documents {
        desired.insert(
            target.clone(),
            FileContent {
                content: ContentId::from_sha256(Sha256::digest(bytes).into()),
                bytes: bytes.len() as u64,
                permissions: default_permissions,
            },
        );
    }
    for ((key, slot), content_key) in candidate.bindings() {
        cancel.check()?;
        let file = project.lock().dependencies[key]
            .files
            .as_slice()
            .iter()
            .find(|file| &file.slot == slot)
            .context("Import candidate binding has no locked file")?;
        let acquired = &candidate.source().content()[content_key];
        let permissions = candidate
            .source()
            .permissions()
            .get(content_key)
            .copied()
            .unwrap_or(default_permissions);
        for placement in file.placements.as_slice() {
            let target = ManagedPath::Content {
                layer: placement.layer,
                path: placement.destination.relative().clone(),
            };
            ensure!(
                desired
                    .insert(
                        target.clone(),
                        FileContent {
                            content: acquired.lease().id(),
                            bytes: acquired.lease().len(),
                            permissions,
                        }
                    )
                    .is_none(),
                "Import repeats a native destination"
            );
            payloads.insert(target, acquired.lease().clone());
        }
    }
    let targets = desired
        .keys()
        .map(ProjectLayout::path)
        .collect::<Result<Vec<_>>>()?;
    let workspace = workspace.complete_for(&targets, cancel)?;
    let policy_path = ManagedPath::Content {
        layer: empack_core::model::ContentLayer::Common,
        path: empack_core::path::PortableRelPath::parse(
            ".packwizignore",
            empack_core::path::PathSyntax::ProjectContent,
        )?,
    };
    if let Some((bytes, permissions)) = workspace.preserved_policy() {
        ensure!(
            !desired.contains_key(&policy_path),
            "Import cannot replace the existing source inclusion policy"
        );
        desired.insert(
            policy_path.clone(),
            FileContent {
                content: ContentId::from_sha256(Sha256::digest(bytes).into()),
                bytes: bytes.len() as u64,
                permissions: *permissions,
            },
        );
        documents.insert(policy_path.clone(), bytes.clone());
    }
    let observed = observed_files_for(workspace.observations(), desired.keys().cloned())?;
    let existing: BTreeSet<_> = observed
        .iter()
        .filter_map(|(target, value)| {
            (matches!(value, ObservedPath::File(_)) && *target != policy_path)
                .then_some(target.clone())
        })
        .collect();
    ensure!(
        policy == ImportReplacementPolicy::ReplaceManagedContent || existing.is_empty(),
        "Managed project content exists; replacement requires an explicit decision"
    );
    let removals = existing
        .difference(&desired.keys().cloned().collect())
        .cloned()
        .collect();
    let plan = plan_files(&observed, &desired, &removals)?;
    let limits = candidate_stage_limits(workspace.observations(), &plan)?;
    let mut stage = MutableStage::empty()?;
    for (target, bytes) in &documents {
        stage.write_attributed(
            &ProjectLayout::path(target)?,
            &mut bytes.as_slice(),
            bytes.len() as u64,
            desired[target].permissions,
            cancel,
        )?;
    }
    for (target, content) in payloads {
        let expected = &desired[&target];
        stage.write_attributed(
            &ProjectLayout::path(&target)?,
            &mut content.open(),
            expected.bytes,
            expected.permissions,
            cancel,
        )?;
    }
    let project = project.clone();
    // Retire archive/source leases before freezing the complete private publication tree.
    drop(candidate);
    let frozen = stage.freeze(limits, cancel)?;
    let (root, native) = workspace.into_native();
    let change = VerifiedFileChange::verify(native, plan, frozen)?;
    Ok(PreparedImportReplacement {
        root,
        change,
        project,
    })
}

#[cfg(test)]
mod tests;
