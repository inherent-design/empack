//! Initialization and import share one complete, verified project replacement footprint.
use super::{import::ImportCandidate, initialize::InitializeCandidate};
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
            VerifiedFileChange, candidate_stage_limits, observed_project_replacement_for,
            plan_files,
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

/// Complete semantic candidates share publication, without treating initialization as an archive.
pub enum ProjectCandidate {
    Import(Box<ImportCandidate>),
    Initialize(Box<InitializeCandidate>),
}
impl From<ImportCandidate> for ProjectCandidate {
    fn from(value: ImportCandidate) -> Self {
        Self::Import(Box::new(value))
    }
}
impl From<InitializeCandidate> for ProjectCandidate {
    fn from(value: InitializeCandidate) -> Self {
        Self::Initialize(Box::new(value))
    }
}
impl ProjectCandidate {
    pub fn project(&self) -> &ResolvedProject {
        match self {
            Self::Import(value) => value.project(),
            Self::Initialize(value) => value.project(),
        }
    }
    pub fn publication_bytes(&self) -> u64 {
        match self {
            Self::Import(value) => value.publication_bytes(),
            Self::Initialize(value) => value.publication_bytes(),
        }
    }
    pub(super) fn template_paths(&self) -> Result<Vec<empack_core::path::PortableRelPath>> {
        match self {
            Self::Import(_) => Ok(vec![]),
            Self::Initialize(value) => value
                .templates()
                .keys()
                .map(|path| ProjectLayout::path(&ManagedPath::UserTemplate(path.clone())))
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectReplacementPolicy {
    RejectExisting,
    /// Replace only the captured authoring documents and managed content roots.
    /// The host must present and authorize the exact returned file plan before publication.
    ReplaceManagedContent,
}
pub struct PreparedProjectReplacement {
    root: ProjectReadRoot,
    change: VerifiedFileChange,
    project: ResolvedProject,
}
pub struct ProjectReplacementReceipt {
    pub publication: PublicationReceipt,
    pub project: ResolvedProject,
}
pub struct PreparedProjectCreation {
    change: crate::engine::publication::PreparedRootCreation,
    project: ResolvedProject,
}
impl PreparedProjectCreation {
    pub(in crate::engine) fn cache_parts(
        &mut self,
    ) -> (&ResolvedProject, &mut crate::engine::staging::FrozenStage) {
        (&self.project, self.change.stage_mut())
    }

    pub fn plan(&self) -> &FilePlan {
        self.change.plan()
    }
    pub fn project(&self) -> &ResolvedProject {
        &self.project
    }
    pub(in crate::engine) fn publish(
        self,
        publisher: &Publisher,
        cancel: &Cancellation,
    ) -> Result<ProjectReplacementReceipt> {
        Ok(ProjectReplacementReceipt {
            publication: publisher.publish_new(self.change, cancel)?,
            project: self.project,
        })
    }
}
/// Reuse complete project staging against an empty private workspace. The live destination remains
/// absent; its parent/child binding is carried separately into no-replace publication.
pub fn prepare_project_creation(
    target: crate::engine::project::NewProjectSnapshot,
    candidate: impl Into<ProjectCandidate>,
    limits: crate::engine::snapshot::SnapshotLimits,
    cancel: &Cancellation,
) -> Result<PreparedProjectCreation> {
    target.revalidate(cancel)?;
    let scratch = tempfile::tempdir()?;
    let candidate = candidate.into();
    let workspace = crate::engine::project::ProjectReader::new(
        crate::engine::publication::RecoveryReader::new(scratch.path().join("unused-host-state")),
    )
    .capture_replacement(scratch.path(), limits, cancel)?
    .capture_seed_templates(&candidate.template_paths()?, cancel)?;
    let prepared = prepare_project_replacement(
        workspace,
        candidate,
        ProjectReplacementPolicy::RejectExisting,
        cancel,
    )?;
    target.revalidate(cancel)?;
    let change =
        crate::engine::publication::PreparedRootCreation::from_verified(target, prepared.change)?;
    Ok(PreparedProjectCreation {
        change,
        project: prepared.project,
    })
}
impl PreparedProjectReplacement {
    pub(in crate::engine) fn cache_parts(
        &mut self,
    ) -> (&ResolvedProject, &mut crate::engine::staging::FrozenStage) {
        (&self.project, self.change.stage_mut())
    }

    pub fn plan(&self) -> &FilePlan {
        self.change.plan()
    }
    pub fn project(&self) -> &ResolvedProject {
        &self.project
    }
    /// Trusted host composition supplies publication only after approving this exact candidate.
    pub(in crate::engine) fn publish(
        self,
        publisher: &Publisher,
        cancel: &Cancellation,
    ) -> Result<ProjectReplacementReceipt> {
        let publication = publisher.publish(&self.root, self.change, cancel)?;
        Ok(ProjectReplacementReceipt {
            publication,
            project: self.project,
        })
    }
}
/// Synchronous private staging, intended for an admitted owned worker just like build preparation.
/// No live file is changed. A later source edit invalidates the complete publication plan.
pub fn prepare_project_replacement(
    workspace: ReplacementSnapshot,
    candidate: impl Into<ProjectCandidate>,
    policy: ProjectReplacementPolicy,
    cancel: &Cancellation,
) -> Result<PreparedProjectReplacement> {
    cancel.check()?;
    let candidate = candidate.into();
    let mut workspace = workspace.capture_seed_templates(&candidate.template_paths()?, cancel)?;
    if let ProjectCandidate::Initialize(value) = &candidate {
        let paths = value
            .scaffolds()
            .keys()
            .map(ProjectLayout::path)
            .collect::<Result<Vec<_>>>()?;
        workspace = workspace.capture_seed_files(&paths, cancel)?;
    }
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
    if let ProjectCandidate::Initialize(value) = &candidate {
        for (target, bytes) in value.scaffolds() {
            if workspace.seed_file_is_missing(&ProjectLayout::path(target)?) {
                documents.insert(target.clone(), bytes.clone());
            }
        }
        for (path, bytes) in value.templates() {
            if workspace.template_seed_is_missing(path)? {
                documents.insert(ManagedPath::UserTemplate(path.clone()), bytes.clone());
            }
        }
    }
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
    if let ProjectCandidate::Import(candidate) = &candidate {
        for ((key, slot), content_key) in candidate.bindings() {
            cancel.check()?;
            let file = project.lock().dependencies[key]
                .files
                .as_slice()
                .iter()
                .find(|file| &file.slot == slot)
                .context("Import candidate binding has no locked file")?;
            let (acquired, permissions) = candidate.bound_content(key, slot, content_key);
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
    let observed =
        observed_project_replacement_for(workspace.observations(), desired.keys().cloned())?;
    let existing: BTreeSet<_> = observed
        .iter()
        .filter_map(|(target, value)| {
            (matches!(value, ObservedPath::File(_))
                && *target != policy_path
                && !matches!(
                    target,
                    ManagedPath::UserTemplate(_) | ManagedPath::Scaffold(_)
                ))
            .then_some(target.clone())
        })
        .collect();
    ensure!(
        policy == ProjectReplacementPolicy::ReplaceManagedContent || existing.is_empty(),
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
    let change = VerifiedFileChange::verify_project_replacement(native, plan, frozen)?;
    Ok(PreparedProjectReplacement {
        root,
        change,
        project,
    })
}

#[cfg(test)]
mod tests;
