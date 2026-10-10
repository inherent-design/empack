//! Native instance preparation. Selected releases never resolve author dependencies.
use super::{
    content::AcquiredContent,
    layout::{CollisionIndex, ProjectLayout},
    publication::{PublicationReceipt, Publisher, RecoveryReader},
    release::{DecodedRelease, FilePolicy, Participation, ReleaseFile, ReleaseSource},
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

pub mod subscription;

/// Snapshot selection and publisher verification both retain exact validated bytes.
pub enum SelectedRelease {
    Snapshot(super::release::trust::SelectedSnapshot),
    Publisher(super::release::trust::AuthenticatedRelease),
    Subscribed(std::sync::Arc<subscription::SubscribedRelease>),
}
impl SelectedRelease {
    pub fn release(&self) -> &DecodedRelease {
        match self {
            Self::Snapshot(v) => v.release(),
            Self::Publisher(v) => v.release(),
            Self::Subscribed(v) => v.release(),
        }
    }
}
/// Fixed consumer directories are part of installed ownership, not arbitrary host paths.
#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum InstanceLayout {
    #[default]
    Game,
    Prism,
}
impl InstanceLayout {
    pub fn directory(self) -> &'static str {
        match self {
            Self::Game => "game",
            Self::Prism => ".minecraft",
        }
    }
    fn target(self, path: PortableRelPath) -> ManagedPath {
        match self {
            Self::Game => ManagedPath::InstanceFile(path),
            Self::Prism => ManagedPath::PrismFile(path),
        }
    }
}
/// Explicit selection, repair and rollback have different durable preconditions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstanceAction {
    /// Install the initial snapshot, or retain and verify the active compatible release.
    Prepare,
    Apply,
    Repair,
    Rollback,
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
    pub layout: InstanceLayout,
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
        ensure!(
            record.layout != InstanceLayout::Prism || record.side == InstanceSide::Client,
            "Prism instance record requires client environment"
        );
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
    subscription_expires: Option<i64>,
    asset_base: Option<reqwest::Url>,
}
pub(super) struct PreparedInstance {
    subscription_expires: Option<i64>,
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
        if let Some(expires) = self.subscription_expires {
            subscription::ensure_fresh(expires)?;
        }
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
    layout: InstanceLayout,
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
            ensure!(
                layout != InstanceLayout::Prism
                    || !file.destination.split('/').next().is_some_and(|name| [
                        ".empack-layout",
                        ".empack-consumer"
                    ]
                    .iter()
                    .any(|reserved| name.eq_ignore_ascii_case(reserved))),
                "Release destination collides with Prism layout control"
            );
            let target = path(&file.destination)?;
            let target = layout.target(target);
            if let Some(prior) = files.get(&target) {
                let prior: &ReleaseFile = prior;
                ensure!(
                    prior.layer != file.layer,
                    "Two selected files claim the same layer and destination"
                );
                if prior.layer > file.layer {
                    continue;
                }
            }
            files.insert(target, file.clone());
        }
    }
    for file in files.values() {
        collisions.insert_file(&path(&file.destination)?)?;
    }
    Ok(files)
}
pub(super) fn read_optional(
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
fn document_limits(mut limits: SnapshotLimits) -> SnapshotLimits {
    limits.file_bytes = limits
        .file_bytes
        .min(super::release::MAX_RELEASE_BYTES as u64);
    limits.total_bytes = limits
        .total_bytes
        .min(super::release::MAX_RELEASE_BYTES as u64);
    limits
}
/// Read a completed record and one retained descriptor without granting mutation authority.
pub fn inspect(
    selected_root: &Path,
    release: Option<&str>,
    recovery: RecoveryReader,
    limits: SnapshotLimits,
    cancel: &Cancellation,
) -> Result<(InstanceRecord, DecodedRelease)> {
    let root = ProjectReadRoot::open(selected_root)?;
    let _guard = recovery.enter(&root)?;
    let snapshot = root.capture(
        &[ProjectLayout::path(&ManagedPath::InstanceRecord)?],
        document_limits(limits),
        cancel,
    )?;
    let record = InstanceRecord::decode(
        &read_optional(&root, &snapshot, &ManagedPath::InstanceRecord, cancel)?
            .context("No completed instance exists")?,
    )?;
    ensure!(
        record.root == super::publication::root_key(&root)?,
        "Instance record belongs to another native root"
    );
    let id = release.unwrap_or(&record.release);
    ensure!(
        id == record.release || record.history.iter().any(|prior| prior == id),
        "Release is not retained by this instance"
    );
    let target = descriptor(id)?;
    let snapshot = snapshot.merge(root.capture(
        &[ProjectLayout::path(&target)?],
        document_limits(limits),
        cancel,
    )?)?;
    let payload = read_optional(&root, &snapshot, &target, cancel)?
        .context("Retained release descriptor is missing")?;
    let decoded = DecodedRelease::decode(&payload)?;
    ensure!(
        decoded.id() == id && decoded.document().pack == record.pack,
        "Retained release descriptor changed"
    );
    root.revalidate(&snapshot, cancel)?;
    Ok((record, decoded))
}

/// Capture precisely the previous and incoming inventories. Unrelated game data is not read.
pub(super) struct InstanceSelection {
    pub release: SelectedRelease,
    pub side: InstanceSide,
    pub layout: Option<InstanceLayout>,
    pub choices: Vec<ChoiceSelection>,
    pub action: InstanceAction,
}
pub(super) fn plan(
    selected_root: &Path,
    selection: InstanceSelection,
    recovery: RecoveryReader,
    limits: SnapshotLimits,
    cancel: &Cancellation,
) -> Result<InstancePlan> {
    let InstanceSelection {
        release: selected,
        side,
        layout,
        choices: requested_choices,
        action,
    } = selection;
    let asset_base = match &selected {
        SelectedRelease::Subscribed(proof) => Some(proof.assets()),
        _ => None,
    };
    let subscription_expires = match &selected {
        SelectedRelease::Subscribed(proof) => Some(proof.expires()),
        _ => None,
    };
    let root = ProjectReadRoot::open(selected_root)?;
    let _guard = recovery.enter(&root)?;
    let record_path = ProjectLayout::path(&ManagedPath::InstanceRecord)?;
    let subscription_path = ProjectLayout::path(&ManagedPath::InstanceSubscription)?;
    let mut snapshot = root.capture(
        &[record_path, subscription_path],
        document_limits(limits),
        cancel,
    )?;
    if let Some(bytes) =
        read_optional(&root, &snapshot, &ManagedPath::InstanceSubscription, cancel)?
    {
        let subscription = subscription::SubscriptionRecord::decode(&bytes)?;
        match &selected {
            SelectedRelease::Subscribed(proof) => {
                proof.validate_current(&super::publication::root_key(&root)?, &bytes)?
            }
            SelectedRelease::Publisher(_) => {
                anyhow::bail!("Select the release against the instance's enrolled subscription")
            }
            SelectedRelease::Snapshot(_) => {}
        }
        ensure!(
            subscription.root == super::publication::root_key(&root)?
                && subscription.pack == selected.release().document().pack,
            "Selected release belongs to another instance subscription"
        );
    } else {
        ensure!(
            !matches!(selected, SelectedRelease::Subscribed(_)),
            "Enrolled subscription was removed"
        );
    }
    let previous = read_optional(&root, &snapshot, &ManagedPath::InstanceRecord, cancel)?
        .map(|bytes| InstanceRecord::decode(&bytes))
        .transpose()?;
    let layout = layout
        .or_else(|| previous.as_ref().map(|record| record.layout))
        .unwrap_or_default();
    ensure!(
        layout != InstanceLayout::Prism || side == InstanceSide::Client,
        "Prism layout requires the client environment"
    );
    if layout == InstanceLayout::Prism {
        // Prism prefers minecraft/ whenever it exists, even beside .minecraft/.
        // Refuse that competing layout rather than updating a directory it will not use.
        match root.directory.symlink_metadata("minecraft") {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
            Ok(_) => anyhow::bail!(
                "Prism layout requires minecraft/ to be absent; selected content uses .minecraft/"
            ),
        }
        let absent = root.capture(
            &[path("minecraft")?],
            SnapshotLimits {
                entries: 1,
                depth: 1,
                file_bytes: 0,
                total_bytes: 0,
            },
            cancel,
        )?;
        ensure!(
            matches!(
                absent.entries().get(&path("minecraft")?),
                Some(super::snapshot::Observation::Absent)
            ),
            "Prism game directory changed during preparation"
        );
        snapshot = snapshot.merge(absent)?;
        snapshot = snapshot.merge(root.capture(
            &[ProjectLayout::path(&ManagedPath::PrismLayoutMarker)?],
            document_limits(limits),
            cancel,
        )?)?;
    }
    let active = if action == InstanceAction::Prepare {
        if let Some(previous) = &previous {
            ensure!(
                requested_choices.is_empty(),
                "Preparation retains completed choices; change them explicitly"
            );
            ensure!(
                previous.root == super::publication::root_key(&root)?,
                "Instance record belongs to another native root"
            );
            let target = descriptor(&previous.release)?;
            snapshot = snapshot.merge(root.capture(
                &[ProjectLayout::path(&target)?],
                document_limits(limits),
                cancel,
            )?)?;
            let bytes = read_optional(&root, &snapshot, &target, cancel)?
                .context("Completed instance descriptor is missing")?;
            let version = if env!("CARGO_PKG_VERSION") == "0.0.0-dev" {
                "0.6.0-beta"
            } else {
                env!("CARGO_PKG_VERSION")
            };
            let active = super::release::trust::SelectedSnapshot::select(
                &bytes,
                &previous.release,
                &semver::Version::parse(version)?,
            )?;
            ensure!(
                active.release().document().pack == selected.release().document().pack,
                "Consumer release belongs to another pack"
            );
            ensure!(
                active.release().document().runtime == selected.release().document().runtime,
                "Active runtime differs from this consumer; update its launcher/runtime integration before launch"
            );
            Some(active)
        } else {
            None
        }
    } else {
        None
    };
    let release = active
        .as_ref()
        .map(|current| current.release())
        .unwrap_or_else(|| selected.release());
    match action {
        InstanceAction::Prepare => {}
        InstanceAction::Apply => {}
        InstanceAction::Repair => {
            let previous = previous
                .as_ref()
                .context("Repair requires a completed instance")?;
            ensure!(
                previous.release == release.id(),
                "Repair cannot change the installed release"
            );
            ensure!(
                requested_choices.is_empty(),
                "Repair retains the installed choices"
            );
        }
        InstanceAction::Rollback => {
            let previous = previous
                .as_ref()
                .context("Rollback requires a completed instance")?;
            ensure!(
                previous.history.iter().any(|id| id == release.id()),
                "Rollback requires a retained completed release"
            );
        }
    }
    let root_identity = super::publication::root_key(&root)?;
    let mut old_files = BTreeMap::new();
    if let Some(previous) = &previous {
        ensure!(
            previous.root == root_identity,
            "Instance record belongs to another native root"
        );
        ensure!(
            previous.pack == release.document().pack
                && previous.side == side
                && previous.layout == layout,
            "Instance pack, side or layout cannot change during an update"
        );
        let target = descriptor(&previous.release)?;
        snapshot = snapshot.merge(root.capture(
            &[ProjectLayout::path(&target)?],
            document_limits(limits),
            cancel,
        )?)?;
        let bytes = read_optional(&root, &snapshot, &target, cancel)?
            .context("Installed release descriptor is missing")?;
        let old = DecodedRelease::decode(&bytes)?;
        ensure!(
            old.id() == previous.release && old.document().pack == previous.pack,
            "Installed release descriptor changed"
        );
        old_files = project_files(&old, side, layout, &previous.choices)?;
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
    let incoming = project_files(release, side, layout, &choices)?;
    let targets: BTreeSet<_> = old_files.keys().chain(incoming.keys()).cloned().collect();
    let release_target = descriptor(release.id())?;
    let scopes: BTreeSet<_> = targets
        .iter()
        .map(ProjectLayout::path)
        .collect::<Result<_>>()?;
    if previous.as_ref().is_none_or(|p| p.release != release.id()) {
        snapshot = snapshot.merge(root.capture(
            &[ProjectLayout::path(&release_target)?],
            document_limits(limits),
            cancel,
        )?)?;
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
            .chain([release_target.clone(), ManagedPath::InstanceRecord])
            .chain((layout == InstanceLayout::Prism).then_some(ManagedPath::PrismLayoutMarker)),
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
        layout,
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
    if layout == InstanceLayout::Prism {
        let marker = b"empack-prism-layout-v1\n";
        if let Some(bytes) =
            read_optional(&root, &snapshot, &ManagedPath::PrismLayoutMarker, cancel)?
        {
            ensure!(
                bytes == marker && previous.is_some(),
                "Prism layout marker is not owned by this completed instance"
            );
        }
        documents.insert(ManagedPath::PrismLayoutMarker, marker.to_vec());
    }
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
        asset_base,
        subscription_expires,
        root,
        snapshot,
        record,
        files,
        documents,
        needed,
    })
}
impl InstancePlan {
    /// Resolve signed relative asset identity to a transient transport locator.
    /// The persisted release and original source assertions remain unchanged.
    pub(super) fn download(&self, file: &ReleaseFile) -> Result<ReleaseFile> {
        let mut file = file.clone();
        if let (Some(base), ReleaseSource::Asset { path }) = (&self.asset_base, &file.source) {
            file.source = ReleaseSource::Url {
                alternatives: vec![subscription::asset_url(base, path)?],
            };
        }
        Ok(file)
    }
    pub(super) fn bytes(&self) -> Result<u64> {
        self.files.expected().values().try_fold(0u64, |sum, file| {
            sum.checked_add(file.bytes)
                .context("Instance staging size overflow")
        })
    }
    fn acquire_one(
        file: &ReleaseFile,
        supplied: &BTreeMap<String, AcquiredContent>,
        local_files: &BTreeMap<String, std::path::PathBuf>,
        assets: Option<&Path>,
        cancel: &Cancellation,
    ) -> Result<Option<AcquiredContent>> {
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
            } else if let (Some(root), Some(member)) = (assets, file.asset_path()) {
                ensure!(root.is_absolute(), "Instance asset root must be absolute");
                (ProjectReadRoot::open(root)?, path(member)?)
            } else {
                return Ok(None);
            };
            let input = (|| {
                let (parent, leaf) = super::native::parent(&root.directory, &relative)?;
                super::native::open_file(&parent, &leaf)
            })();
            let mut input = match input {
                Ok(file) => file,
                Err(error)
                    if !local_files.contains_key(&file.key)
                        && error
                            .downcast_ref::<std::io::Error>()
                            .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) =>
                {
                    return Ok(None);
                }
                Err(error) => return Err(error),
            };
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
        Ok(Some(content.clone()))
    }
    pub(super) fn needed(&self) -> impl Iterator<Item = &ReleaseFile> {
        self.needed.values()
    }
    pub(super) fn acquire_available(
        &self,
        supplied: &BTreeMap<String, AcquiredContent>,
        local_files: &BTreeMap<String, std::path::PathBuf>,
        assets: Option<&Path>,
        cancel: &Cancellation,
    ) -> Result<BTreeMap<String, AcquiredContent>> {
        let mut result = BTreeMap::new();
        let mut pool = super::content::ContentPool::new(self.bytes()?)?;
        for file in self.needed.values() {
            if let Some(content) = Self::acquire_one(file, supplied, local_files, assets, cancel)? {
                result.insert(file.key.clone(), pool.insert(content, cancel)?);
            }
        }
        Ok(result)
    }
    pub(super) fn stage(
        self,
        supplied: &BTreeMap<String, AcquiredContent>,
        local_files: &BTreeMap<String, std::path::PathBuf>,
        assets: Option<&Path>,
        cancel: &Cancellation,
    ) -> Result<PreparedInstance> {
        let limits = verification::candidate_stage_limits(&self.snapshot, &self.files)?;
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
            let content = Self::acquire_one(file, supplied, local_files, assets, cancel)?.with_context(|| format!("Instance requires exact content for {}; supply a verified file association", file.key))?;
            stage.write_attributed(
                &ProjectLayout::path(target)?,
                &mut content.lease().open(),
                file.bytes,
                file.content()?.permissions,
                cancel,
            )?;
        }
        let stage = stage.freeze(limits, cancel)?;
        let change = VerifiedFileChange::verify_instance(self.snapshot, self.files, stage)?;
        Ok(PreparedInstance {
            subscription_expires: self.subscription_expires,
            root: self.root,
            change,
            record: self.record,
        })
    }
}
