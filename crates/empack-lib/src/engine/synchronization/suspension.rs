//! Exact materialization selections survive restarts without retaining publication approval.
use crate::{
    application::process_runtime::Cancellation,
    engine::{
        content::{
            ContentPool, InitialObservation, SourceEvidencePolicy,
            store::{CachedFileRequest, ContentStoreLimits, FileContentLookup, FileContentStore},
        },
        continuation_store::{self as store, Binding, BoundRecord, Kind},
        documents::DocumentCodec,
        mrpack::{AcquiredBuildFile, LockedFileKey},
        project::{ProjectRevision, WorkspaceSnapshot},
        publication::root_key,
        resources::ResourceRequest,
        runtime::{RetainedOutput, WorkScope},
        snapshot::Observation,
    },
};
use anyhow::{Context, Result, ensure};
use empack_core::{
    digest::ContentId,
    files::FilePermissions,
    model::{DependencyKey, FileSlot, ResolvedProject},
    path::{PathSyntax, PortableRelPath},
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Clone)]
pub struct SyncInputContext {
    target: PathBuf,
    binding: Binding,
    revision: ProjectRevision,
}
impl SyncInputContext {
    /// Bind the exact captured documents, including an absent first lock.
    pub fn capture(
        target: PathBuf,
        workspace: &WorkspaceSnapshot,
        cancel: &Cancellation,
    ) -> Result<Self> {
        let (_, binding) = store::bind(&target, cancel)?;
        ensure!(
            binding.project.as_ref() == Some(&root_key(workspace.root())?),
            "Synchronization target changed"
        );
        for (index, name) in ["empack.yml", "empack.lock"].iter().enumerate() {
            let path = PortableRelPath::parse(name, PathSyntax::ProjectContent)?;
            let content = match workspace.observations().entries().get(&path) {
                Some(Observation::File(file)) => Some(file.content),
                Some(Observation::Absent) => None,
                _ => anyhow::bail!("Synchronization document is not a file"),
            };
            ensure!(
                binding.documents[index] == content,
                "Synchronization documents changed"
            );
        }
        workspace
            .root()
            .revalidate(workspace.observations(), cancel)?;
        Ok(Self {
            target,
            binding,
            revision: workspace.revision(),
        })
    }
    pub fn revision(&self) -> ProjectRevision {
        self.revision
    }
}
pub struct SavedSyncRecord(store::SavedRecord);
pub struct PendingSyncCleanup(store::Cleanup);
pub struct ResumedSync {
    pub project: ResolvedProject,
    pub acquired: BTreeMap<LockedFileKey, AcquiredBuildFile>,
    pub saved: SavedSyncRecord,
    pub context: SyncInputContext,
    _record: RetainedOutput<Record>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    schema: u32,
    binding: Binding,
    intent: String,
    lock: String,
    strong: bool,
    files: Vec<SavedFile>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedFile {
    dependency: String,
    slot: String,
    content: [u8; 32],
    readonly: bool,
    executable: bool,
}
impl BoundRecord for Record {
    fn binding(&self) -> &Binding {
        &self.binding
    }
    fn schema(&self) -> u32 {
        self.schema
    }
}
fn resources(memory_bytes: u64) -> ResourceRequest {
    ResourceRequest {
        jobs: 1,
        memory_bytes,
        open_files: 12,
        ..Default::default()
    }
}
fn remote(file: &empack_core::model::ResolvedFile) -> bool {
    !matches!(
        file.acquisition,
        empack_core::model::AcquisitionSpec::Local(_)
            | empack_core::model::AcquisitionSpec::Embedded { .. }
    )
}
fn selected<'a>(
    project: &'a ResolvedProject,
    key: &LockedFileKey,
) -> Result<&'a empack_core::model::ResolvedFile> {
    project
        .lock()
        .dependencies
        .get(&key.dependency)
        .and_then(|dep| {
            dep.files
                .as_slice()
                .iter()
                .find(|file| file.slot == key.slot && remote(file))
        })
        .context("Saved file has no exact synchronization obligation")
}
/// Read-only reconstruction. Saved paths and encoded model values never grant a writer.
pub async fn load_pending_sync(
    scope: &mut WorkScope,
    state: PathBuf,
    context: SyncInputContext,
    policy: SourceEvidencePolicy,
    limits: crate::engine::acquisition::TransferLimits,
) -> Result<Option<ResumedSync>> {
    let selected_state = state.clone();
    let target = context.target.clone();
    let work = scope.spawn_blocking(
        resources(1 << 20),
        ResourceRequest::default(),
        move |cancel| {
            cancel.check()?;
            store::record_bytes(&selected_state, Kind::Synchronization, &target)
        },
    )?;
    let Some(bytes) = scope
        .accept(work.wait().await?)?
        .transpose()?
        .into_parts()
        .0
    else {
        return Ok(None);
    };
    let memory = bytes
        .checked_mul(8)
        .and_then(|n| n.checked_add(32 << 20))
        .context("Sync record memory overflow")?;
    let selected_state = state.clone();
    let target = context.target.clone();
    let work = scope.spawn_blocking(
        resources(memory),
        ResourceRequest {
            memory_bytes: memory,
            ..Default::default()
        },
        move |cancel| {
            store::read::<Record>(
                &selected_state,
                Kind::Synchronization,
                &target,
                bytes,
                &cancel,
            )
        },
    )?;
    let (loaded, permit) = scope.accept(work.wait().await?)?.transpose()?.into_parts();
    let (saved, record) = loaded.context("Saved synchronization disappeared during inspection")?;
    ensure!(
        record.binding == context.binding,
        "Saved synchronization is stale"
    );
    ensure!(
        !record.strong || policy == SourceEvidencePolicy::StrongSourceRequired,
        "Saved synchronization requires strong evidence"
    );
    let record = RetainedOutput::from_parts(record, permit);
    let intent = DocumentCodec.decode_intent(record.intent.as_bytes(), "saved synchronization")?;
    let project =
        DocumentCodec.decode_lock(record.lock.as_bytes(), &intent, "saved synchronization")?;
    let work = scope.spawn_blocking(
        resources(1 << 20),
        ResourceRequest {
            open_files: 1,
            ..Default::default()
        },
        move |cancel| {
            cancel.check()?;
            FileContentLookup::open_existing(
                &state.join("pending-sync-content"),
                ContentStoreLimits::default(),
            )
        },
    )?;
    let lookup = scope.accept(work.wait().await?)?.transpose()?;
    let mut seen = std::collections::BTreeSet::new();
    let mut acquired = BTreeMap::new();
    let mut total = 0u64;
    let mut pool = ContentPool::owned(scope, limits.transfer_bytes).await?;
    for saved in &record.files {
        let key = LockedFileKey {
            dependency: DependencyKey::parse(&saved.dependency)?,
            slot: FileSlot::parse(&saved.slot)?,
        };
        let file = selected(&project, &key)?;
        ensure!(
            seen.insert(key.clone()),
            "Duplicate saved synchronization file"
        );
        if let Some(lookup) = lookup.as_ref()
            && let Some(content) = lookup
                .retain(
                    scope,
                    CachedFileRequest {
                        id: ContentId::from_sha256(saved.content),
                        expected: file.expected.clone(),
                        maximum: limits.file_bytes.min(limits.transfer_bytes - total),
                        evidence: policy,
                        initial: InitialObservation::RequireEvidence,
                    },
                )
                .await?
        {
            total = total
                .checked_add(content.lease().len())
                .context("Sync byte count overflow")?;
            acquired.insert(
                key,
                AcquiredBuildFile {
                    content: pool.consolidate_owned(scope, content).await?,
                    permissions: FilePermissions {
                        readonly: saved.readonly,
                        executable: saved.executable,
                    },
                },
            );
        }
    }
    Ok(Some(ResumedSync {
        project,
        acquired,
        saved: SavedSyncRecord(saved),
        context,
        _record: record,
    }))
}
/// Authorized host persistence only. Source documents must still match their original capture.
pub struct SyncRetentionRequest {
    pub state: PathBuf,
    pub context: SyncInputContext,
    pub project: ResolvedProject,
    pub acquired: BTreeMap<LockedFileKey, AcquiredBuildFile>,
    pub policy: SourceEvidencePolicy,
    pub prior: Option<SavedSyncRecord>,
    pub limits: crate::engine::acquisition::TransferLimits,
}
pub async fn save_pending_sync(
    scope: &mut WorkScope,
    request: SyncRetentionRequest,
) -> Result<SavedSyncRecord> {
    let SyncRetentionRequest {
        state,
        context,
        project,
        acquired,
        policy,
        prior,
        limits,
    } = request;
    let _metadata = scope.reserve_storage(ResourceRequest {
        memory_bytes: (acquired.len() as u64)
            .checked_mul(4096)
            .context("Saved file metadata overflow")?,
        ..Default::default()
    })?;
    let mut files = Vec::new();
    let mut total = 0u64;
    for (key, content) in &acquired {
        let file = selected(&project, key)?;
        crate::engine::content::validate_expectation(
            &file.expected,
            limits.file_bytes,
            policy,
            InitialObservation::RequireEvidence,
        )?;
        if let Some(digests) = &file.expected.digests {
            digests.check(content.content.observed_digests().values())?;
        }
        ensure!(
            file.expected
                .size
                .is_none_or(|size| size == content.content.lease().len())
                && file
                    .expected
                    .accepted_observation
                    .as_ref()
                    .is_none_or(|id| *id == content.content.lease().id()),
            "Saved synchronization bytes differ from source assertions"
        );
        total = total
            .checked_add(content.content.lease().len())
            .context("Sync byte count overflow")?;
        ensure!(
            total <= limits.transfer_bytes,
            "Saved synchronization exceeds byte allowance"
        );
        files.push(SavedFile {
            dependency: key.dependency.as_str().into(),
            slot: key.slot.as_str().into(),
            content: *content.content.lease().id().bytes(),
            readonly: content.permissions.readonly,
            executable: content.permissions.executable,
        });
    }
    let binding = context.binding;
    let record = scope.spawn_blocking(
        resources(64 << 20),
        ResourceRequest {
            memory_bytes: 64 << 20,
            ..Default::default()
        },
        move |cancel| {
            cancel.check()?;
            let intent = String::from_utf8(DocumentCodec.encode_intent(project.intent())?)?;
            let lock = String::from_utf8(DocumentCodec.encode_lock(&project)?)?;
            Ok::<_, anyhow::Error>(Record {
                schema: 1,
                binding,
                intent,
                lock,
                strong: policy == SourceEvidencePolicy::StrongSourceRequired,
                files,
            })
        },
    )?;
    let record = scope.accept(record.wait().await?)?.transpose()?;
    let selected_state = state.clone();
    let work = scope.spawn_blocking(
        resources(1 << 20),
        ResourceRequest {
            open_files: 1,
            ..Default::default()
        },
        move |cancel| {
            cancel.check()?;
            FileContentStore::open(
                &selected_state.join("pending-sync-content"),
                ContentStoreLimits::default(),
            )
        },
    )?;
    let content = scope.accept(work.wait().await?)?.transpose()?;
    for file in acquired.into_values() {
        content.publish_verified(scope, file.content).await?;
    }
    let memory = (record.intent.len() as u64 + record.lock.len() as u64)
        .checked_mul(8)
        .and_then(|n| n.checked_add(32 << 20))
        .context("Sync record memory overflow")?;
    let work = scope.spawn_blocking(
        resources(memory),
        ResourceRequest::default(),
        move |cancel| {
            store::save(
                &state,
                Kind::Synchronization,
                &context.target,
                &*record,
                prior.as_ref().map(|saved| &saved.0),
                &cancel,
            )
            .map(SavedSyncRecord)
        },
    )?;
    Ok(scope
        .accept(work.wait().await?)?
        .transpose()?
        .into_parts()
        .0)
}
pub async fn observe_pending_sync(
    scope: &mut WorkScope,
    state: PathBuf,
    target: PathBuf,
) -> Result<Option<PendingSyncCleanup>> {
    let work = scope.spawn_blocking(
        resources(1 << 20),
        ResourceRequest::default(),
        move |cancel| {
            store::observe(&state, Kind::Synchronization, &target, &cancel)
                .map(|value| value.map(PendingSyncCleanup))
        },
    )?;
    Ok(scope
        .accept(work.wait().await?)?
        .transpose()?
        .into_parts()
        .0)
}
pub async fn discard_observed_sync(
    scope: &mut WorkScope,
    saved: PendingSyncCleanup,
) -> Result<bool> {
    let work = scope.spawn_blocking(
        resources(1 << 20),
        ResourceRequest::default(),
        move |cancel| store::discard(&saved.0, &cancel),
    )?;
    Ok(scope
        .accept(work.wait().await?)?
        .transpose()?
        .into_parts()
        .0)
}
pub async fn discard_pending_sync(scope: &mut WorkScope, saved: SavedSyncRecord) -> Result<bool> {
    discard_observed_sync(scope, PendingSyncCleanup(saved.0.into_cleanup())).await
}
