//! Native instance preparation. Selected releases never resolve author dependencies.
use super::{
    content::AcquiredContent,
    layout::{CollisionIndex, ProjectLayout},
    publication::{PublicationReceipt, Publisher, RecoveryReader},
    release::{DecodedRelease, FilePolicy, Participation, ReleaseFile},
    snapshot::{NativeSnapshot, ProjectReadRoot, SnapshotLimits},
    staging::MutableStage,
    verification::{self, VerifiedFileChange},
};
use crate::application::process_runtime::Cancellation;
use anyhow::{Context, Result, ensure};
use empack_core::{
    files::{FileContent, FilePlan, ManagedPath, ObservedPath},
    instance::{FileDecision, InstalledFile, ReleaseFile as DesiredFile, plan_file},
    path::{PathSyntax, PortableRelPath},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

/// Snapshot selection and publisher verification both retain exact validated bytes.
pub enum SelectedRelease {
    Snapshot(super::release::trust::SelectedSnapshot),
    Publisher(super::release::trust::AuthenticatedRelease),
}
impl SelectedRelease {
    pub fn release(&self) -> &DecodedRelease {
        match self {
            Self::Snapshot(v) => v.release(),
            Self::Publisher(v) => v.release(),
        }
    }
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum InstanceSide {
    Client,
    Server,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ChoiceSelection {
    pub key: String,
    pub value: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct InstanceRecord {
    pub schema: u32,
    /// Native root identity prevents copied state from silently granting ownership elsewhere.
    pub root: String,
    pub pack: String,
    pub release: String,
    pub side: InstanceSide,
    pub choices: Vec<ChoiceSelection>,
    /// Completed descriptor identities, separate from short-lived publication preimages.
    pub history: Vec<String>,
}
impl InstanceRecord {
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        ensure!(
            bytes.len() <= super::release::MAX_RELEASE_BYTES,
            "Instance record exceeds byte bound"
        );
        let record: Self = serde_json::from_slice(bytes).context("Invalid instance record")?;
        ensure!(record.schema == 1, "Unsupported instance schema");
        super::release::decode_hex::<32>(&record.release)?;
        let mut history = BTreeSet::new();
        for id in &record.history {
            super::release::decode_hex::<32>(id)?;
            ensure!(history.insert(id), "Duplicate instance history entry");
        }
        let mut choices = BTreeSet::new();
        for choice in &record.choices {
            ensure!(choices.insert(&choice.key), "Duplicate installed choice");
        }
        Ok(record)
    }
}
/// Every requested decision is exposed before content staging or live mutation.
#[derive(Debug, thiserror::Error)]
#[error("Instance has unresolved file conflicts: {files:?}")]
pub struct InstanceConflicts {
    pub files: Vec<(String, empack_core::instance::FileConflict)>,
}

pub(super) struct InstancePlan {
    root: ProjectReadRoot,
    snapshot: NativeSnapshot,
    pub(super) record: InstanceRecord,
    pub(super) files: FilePlan,
    documents: BTreeMap<ManagedPath, Vec<u8>>,
    needed: BTreeMap<ManagedPath, ReleaseFile>,
}
pub(super) struct PreparedInstance {
    root: ProjectReadRoot,
    change: VerifiedFileChange,
    pub(super) record: InstanceRecord,
}
impl PreparedInstance {
    pub(super) fn publish(
        self,
        publisher: &Publisher,
        cancel: &Cancellation,
    ) -> Result<(PublicationReceipt, InstanceRecord)> {
        let receipt = publisher.publish(&self.root, self.change, cancel)?;
        Ok((receipt, self.record))
    }
}
fn path(value: &str) -> Result<PortableRelPath> {
    Ok(PortableRelPath::parse(value, PathSyntax::ProjectContent)?)
}
fn descriptor(id: &str) -> Result<ManagedPath> {
    super::release::decode_hex::<32>(id)?;
    Ok(ManagedPath::InstanceRelease(path(&format!("{id}.json"))?))
}
fn policy(policy: FilePolicy) -> empack_core::instance::FilePolicy {
    match policy {
        FilePolicy::Managed => empack_core::instance::FilePolicy::Managed,
        FilePolicy::Seed => empack_core::instance::FilePolicy::Seed,
    }
}
fn project_files(
    release: &DecodedRelease,
    side: InstanceSide,
    choices: &[ChoiceSelection],
) -> Result<BTreeMap<ManagedPath, ReleaseFile>> {
    let selected: BTreeMap<_, _> = choices
        .iter()
        .map(|c| (c.key.as_str(), c.value.as_str()))
        .collect();
    ensure!(selected.len() == choices.len(), "Duplicate selected choice");
    ensure!(
        selected.len() == release.document().choices.len(),
        "Choice selection does not cover this release"
    );
    for choice in &release.document().choices {
        ensure!(
            selected
                .get(choice.key.as_str())
                .is_some_and(|v| choice.alternatives.iter().any(|a| a == v)),
            "Choice requires a valid explicit selection: {}",
            choice.key
        );
    }
    let mut files = BTreeMap::new();
    let mut collisions = CollisionIndex::default();
    for file in &release.document().files {
        let applies = match match side {
            InstanceSide::Client => &file.client,
            InstanceSide::Server => &file.server,
        } {
            Participation::Required => true,
            Participation::Unsupported => false,
            Participation::Choice { key, value } => {
                selected.get(key.as_str()) == Some(&value.as_str())
            }
        };
        if applies {
            let target = path(&file.destination)?;
            collisions.insert_file(&target)?;
            files.insert(ManagedPath::InstanceFile(target), file.clone());
        }
    }
    Ok(files)
}
fn read_optional(
    root: &ProjectReadRoot,
    snapshot: &NativeSnapshot,
    target: &ManagedPath,
    cancel: &Cancellation,
) -> Result<Option<Vec<u8>>> {
    let observed = verification::observed_files_for(snapshot, [target.clone()])?;
    match observed.get(target) {
        Some(ObservedPath::Absent) => Ok(None),
        Some(ObservedPath::File(_)) => super::project::read_document(
            root,
            snapshot,
            ProjectLayout::path(target)?.as_str(),
            cancel,
        ),
        _ => anyhow::bail!("Instance control document is not a regular file"),
    }
}
/// Capture precisely the previous and incoming inventories. Unrelated game data is not read.
pub(super) fn plan(
    selected_root: &Path,
    selected: SelectedRelease,
    side: InstanceSide,
    requested_choices: Vec<ChoiceSelection>,
    recovery: RecoveryReader,
    limits: SnapshotLimits,
    cancel: &Cancellation,
) -> Result<InstancePlan> {
    let root = ProjectReadRoot::open(selected_root)?;
    let _guard = recovery.enter(&root)?;
    let record_path = ProjectLayout::path(&ManagedPath::InstanceRecord)?;
    let mut snapshot = root.capture(&[record_path], limits, cancel)?;
    let previous = read_optional(&root, &snapshot, &ManagedPath::InstanceRecord, cancel)?
        .map(|bytes| InstanceRecord::decode(&bytes))
        .transpose()?;
    let release = selected.release();
    let root_identity = super::publication::root_key(&root)?;
    let mut old_files = BTreeMap::new();
    if let Some(previous) = &previous {
        ensure!(
            previous.root == root_identity,
            "Instance record belongs to another native root"
        );
        ensure!(
            previous.pack == release.document().pack && previous.side == side,
            "Instance pack or side cannot change during an update"
        );
        let target = descriptor(&previous.release)?;
        snapshot =
            snapshot.merge(root.capture(&[ProjectLayout::path(&target)?], limits, cancel)?)?;
        let bytes = read_optional(&root, &snapshot, &target, cancel)?
            .context("Installed release descriptor is missing")?;
        let old = DecodedRelease::decode(&bytes)?;
        ensure!(
            old.id() == previous.release && old.document().pack == previous.pack,
            "Installed release descriptor changed"
        );
        old_files = project_files(&old, side, &previous.choices)?;
    }
    let mut choices: BTreeMap<_, _> = requested_choices
        .iter()
        .map(|c| (c.key.clone(), c.value.clone()))
        .collect();
    ensure!(
        choices.len() == requested_choices.len(),
        "Duplicate requested choice"
    );
    ensure!(
        choices
            .keys()
            .all(|key| release.document().choices.iter().any(|c| c.key == *key)),
        "Unknown requested choice"
    );
    for definition in &release.document().choices {
        if !choices.contains_key(&definition.key) {
            let retained = previous
                .as_ref()
                .and_then(|p| p.choices.iter().find(|c| c.key == definition.key));
            let value = if let Some(retained) = retained {
                retained.value.clone()
            } else if previous.is_none() {
                definition.default.clone()
            } else {
                anyhow::bail!("New release choice requires a decision: {}", definition.key);
            };
            choices.insert(definition.key.clone(), value);
        }
    }
    let choices: Vec<_> = choices
        .into_iter()
        .map(|(key, value)| ChoiceSelection { key, value })
        .collect();
    let incoming = project_files(release, side, &choices)?;
    let targets: BTreeSet<_> = old_files.keys().chain(incoming.keys()).cloned().collect();
    let release_target = descriptor(release.id())?;
    let mut scopes: BTreeSet<_> = targets
        .iter()
        .map(ProjectLayout::path)
        .collect::<Result<_>>()?;
    if previous.as_ref().is_none_or(|p| p.release != release.id()) {
        scopes.insert(ProjectLayout::path(&release_target)?);
    }
    if !scopes.is_empty() {
        snapshot = snapshot.merge(root.capture(
            &scopes.into_iter().collect::<Vec<_>>(),
            limits,
            cancel,
        )?)?;
    }
    let observed = verification::observed_files_for(
        &snapshot,
        targets
            .iter()
            .cloned()
            .chain([release_target.clone(), ManagedPath::InstanceRecord]),
    )?;
    let mut desired = BTreeMap::new();
    let mut removals = BTreeSet::new();
    let mut needed = BTreeMap::new();
    let mut conflicts = Vec::new();
    for target in targets {
        let old = old_files
            .get(&target)
            .map(|file| {
                file.content().map(|baseline| InstalledFile {
                    baseline,
                    policy: policy(file.policy),
                })
            })
            .transpose()?;
        let new = incoming
            .get(&target)
            .map(|file| {
                file.content().map(|content| DesiredFile {
                    content,
                    policy: policy(file.policy),
                })
            })
            .transpose()?;
        let current = observed
            .get(&target)
            .context("Instance target lacks observation")?;
        match plan_file(
            old.as_ref(),
            current,
            new.as_ref(),
            verification::native_capabilities(),
        ) {
            FileDecision::Create(content) | FileDecision::Replace(content) => {
                desired.insert(target.clone(), content);
                needed.insert(target.clone(), incoming[&target].clone());
            }
            FileDecision::Remove => {
                removals.insert(target.clone());
            }
            FileDecision::Conflict(conflict) => {
                conflicts.push((ProjectLayout::path(&target)?.as_str().into(), conflict))
            }
            FileDecision::Unchanged => {
                // A matching content address does not excuse checking a changed source assertion.
                if let Some(file) = incoming.get(&target) {
                    let relative = ProjectLayout::path(&target)?;
                    let (parent, leaf) = super::native::parent(&root.directory, &relative)?;
                    let mut input = super::native::open_file(&parent, &leaf)?;
                    super::content::verify_observation(
                        &mut input,
                        &file.expected()?,
                        file.bytes,
                        super::content::SourceEvidencePolicy::Compatibility,
                        super::content::InitialObservation::RequireEvidence,
                        cancel,
                    )?;
                }
            }
            FileDecision::Preserve => {}
        }
    }
    ensure!(conflicts.is_empty(), InstanceConflicts { files: conflicts });
    let mut history = previous
        .as_ref()
        .map(|p| p.history.clone())
        .unwrap_or_default();
    if let Some(previous) = &previous
        && previous.release != release.id()
        && !history.contains(&previous.release)
    {
        history.push(previous.release.clone());
    }
    let record = InstanceRecord {
        schema: 1,
        root: root_identity,
        pack: release.document().pack.clone(),
        release: release.id().into(),
        side,
        choices,
        history,
    };
    let record_bytes = serde_json::to_vec(&record)?;
    InstanceRecord::decode(&record_bytes)?;
    let mut documents = BTreeMap::from([(ManagedPath::InstanceRecord, record_bytes)]);
    // Immutable descriptors cannot overwrite an unrelated or corrupt file, even if explicitly selected.
    if let Some(ObservedPath::File(existing)) = observed.get(&release_target) {
        ensure!(
            existing.content
                == empack_core::digest::ContentId::from_sha256(super::release::decode_hex(
                    release.id()
                )?)
                && existing.bytes == release.bytes().len() as u64,
            "Retained release descriptor does not match its content identity"
        );
    } else {
        documents.insert(release_target, release.bytes().to_vec());
    }
    for (target, bytes) in &documents {
        desired.insert(
            target.clone(),
            FileContent {
                content: empack_core::digest::ContentId::from_sha256(super::release::decode_hex(
                    &super::release::hash(bytes),
                )?),
                bytes: bytes.len() as u64,
                permissions: empack_core::files::FilePermissions {
                    readonly: false,
                    executable: false,
                },
            },
        );
    }
    let files = verification::plan_mutation_files(&observed, &desired, &removals)?;
    documents.retain(|target, _| files.expected().contains_key(target));
    root.revalidate(&snapshot, cancel)?;
    Ok(InstancePlan {
        root,
        snapshot,
        record,
        files,
        documents,
        needed,
    })
}
impl InstancePlan {
    pub(super) fn bytes(&self) -> Result<u64> {
        self.files.expected().values().try_fold(0u64, |sum, file| {
            sum.checked_add(file.bytes)
                .context("Instance staging size overflow")
        })
    }
    pub(super) fn stage(
        self,
        supplied: &BTreeMap<String, AcquiredContent>,
        local_files: &BTreeMap<String, std::path::PathBuf>,
        assets: Option<&Path>,
        cancel: &Cancellation,
    ) -> Result<PreparedInstance> {
        let mut stage = MutableStage::empty()?;
        for (target, bytes) in &self.documents {
            stage.write_attributed(
                &ProjectLayout::path(target)?,
                &mut bytes.as_slice(),
                bytes.len() as u64,
                self.files.expected()[target].permissions,
                cancel,
            )?;
        }
        for (target, file) in &self.needed {
            let expected = file.expected()?;
            let acquired;
            let content = if let Some(content) = supplied.get(&file.key) {
                content
            } else {
                let (root, relative) = if let Some(selected) = local_files.get(&file.key) {
                    ensure!(
                        selected.is_absolute(),
                        "Explicit instance inputs must be absolute"
                    );
                    let parent = selected.parent().context("Instance input has no parent")?;
                    let name = selected
                        .file_name()
                        .and_then(|v| v.to_str())
                        .context("Invalid instance input filename")?;
                    (ProjectReadRoot::open(parent)?, path(name)?)
                } else if let (Some(root), super::release::ReleaseSource::Asset { path: member }) =
                    (assets, &file.source)
                {
                    ensure!(root.is_absolute(), "Instance asset root must be absolute");
                    (ProjectReadRoot::open(root)?, path(member)?)
                } else {
                    anyhow::bail!(
                        "Instance requires exact content for {}; supply a verified file association",
                        file.key
                    );
                };
                let (parent, leaf) = super::native::parent(&root.directory, &relative)?;
                let mut input = super::native::open_file(&parent, &leaf)?;
                acquired = super::content::verify_stream(
                    &mut input,
                    &expected,
                    file.bytes,
                    super::content::SourceEvidencePolicy::Compatibility,
                    super::content::InitialObservation::RequireEvidence,
                    cancel,
                )?;
                &acquired
            };
            if let Some(digests) = &expected.digests {
                digests.check(content.observed_digests().values())?;
            }
            ensure!(
                Some(content.lease().id()) == expected.accepted_observation,
                "Instance content address mismatch"
            );
            ensure!(
                content.lease().len() == file.bytes,
                "Instance content byte count mismatch"
            );
            stage.write_attributed(
                &ProjectLayout::path(target)?,
                &mut content.lease().open(),
                file.bytes,
                file.content()?.permissions,
                cancel,
            )?;
        }
        let stage = stage.freeze(
            verification::candidate_stage_limits(&self.snapshot, &self.files)?,
            cancel,
        )?;
        let change = VerifiedFileChange::verify_instance(self.snapshot, self.files, stage)?;
        Ok(PreparedInstance {
            root: self.root,
            change,
            record: self.record,
        })
    }
}
