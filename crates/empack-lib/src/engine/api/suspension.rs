//! Explicit durable suspension; saved data re-enters normal preparation and fresh approval.
use super::*;
use crate::{
    application::process_runtime::Cancellation,
    engine::{
        content::{
            InitialObservation,
            store::{CachedFileRequest, ContentStoreLimits, FileContentLookup, FileContentStore},
        },
        mrpack::AcquiredBuildFile,
        runtime::WorkScope,
    },
};
use empack_core::{digest::ExpectedDigest, files::FilePermissions};
use std::collections::BTreeSet;
mod record;
mod store;

pub struct SuspendedBuildReceipt {
    /// Exact published recipe observation for later resume and conditional cleanup
    pub saved: SavedBuildRecord,
    pub retained_files: usize,
    pub retained_bytes: u64,
    pub pending_files: usize,
    pub replaced: bool,
}
/// No record is removed during inspection. Prepared output still needs an execution grant.
pub enum SavedBuildResume {
    Missing,
    Stale,
    Prepared(Box<ResumedBuild>),
}
pub struct ResumedBuild {
    pub preparation: Preparation,
    pub saved: SavedBuildRecord,
}
/// Exact read-only observation, not serialized authority. Deletion is a separate host action.
#[derive(Debug)]
pub struct SavedBuildRecord {
    owner: Arc<()>,
    name: String,
    content: [u8; 32],
}
impl PartialEq for SavedBuildRecord {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.owner, &other.owner)
            && self.name == other.name
            && self.content == other.content
    }
}
impl Eq for SavedBuildRecord {}

fn record_memory(bytes: u64) -> Result<u64> {
    bytes
        .checked_mul(8)
        .and_then(|bytes| bytes.checked_add(256 << 10))
        .context("Pending record memory estimate overflow")
}
fn record_resources(bytes: u64) -> Result<ResourceRequest> {
    Ok(ResourceRequest {
        jobs: 1,
        memory_bytes: record_memory(bytes)?,
        open_files: 8,
        ..Default::default()
    })
}

impl Engine {
    /// Explicit host authorization to persist this continuation and verified content. Preview
    /// never calls this method; the saved record contains neither native locators nor approval.
    pub async fn suspend_build(
        &self,
        pending: PreparationContinuation,
    ) -> Result<SuspendedBuildReceipt> {
        self.suspend_build_selected(pending, None).await
    }
    /// Retain newly verified inputs only if the caller's saved recipe is still current.
    pub async fn extend_saved_build(
        &self,
        pending: PreparationContinuation,
        saved: SavedBuildRecord,
    ) -> Result<SuspendedBuildReceipt> {
        ensure!(
            Arc::ptr_eq(&self.owner, &saved.owner),
            "Saved build observation belongs to another engine"
        );
        self.suspend_build_selected(pending, Some(saved)).await
    }
    async fn suspend_build_selected(
        &self,
        pending: PreparationContinuation,
        saved: Option<SavedBuildRecord>,
    ) -> Result<SuspendedBuildReceipt> {
        ensure!(
            Arc::ptr_eq(&self.owner, &pending.prepared.owner),
            "Continuation belongs to another engine"
        );
        ensure!(
            matches!(&**pending.prepared.data, PreparedKind::Build(_)),
            "Continuation is not a build preparation"
        );
        let retained = (*pending.prepared.data).map(|kind| match kind {
            PreparedKind::Build(build) => *build,
            _ => unreachable!(),
        });
        let config = self.config.clone();
        let owner = self.owner.clone();
        let (sender, receiver) = oneshot::channel();
        let mut handle = self
            .preparations
            .start_ephemeral(move |mut scope| async move {
                let result = suspend(retained, config, owner, saved, &mut scope).await;
                let _ = sender.send(result);
                Ok(())
            })?;
        let outcome = handle.wait().await;
        if let OperationOutcome::Failed(error) = &*outcome {
            return Err(error.clone().into());
        }
        receiver
            .await
            .context("Suspension result was not retained")?
    }
    /// Read-only cleanup selection. Even an invalid recipe can be selected; its bytes are
    /// bounded and hashed but never interpreted as a request or an authoritative path.
    pub async fn observe_saved_build(&self, project: PathBuf) -> Result<Option<SavedBuildRecord>> {
        ensure!(project.is_absolute(), "Project selection must be absolute");
        let state = self.config.state_root.clone();
        let (sender, receiver) = oneshot::channel();
        let mut handle = self
            .preparations
            .start_ephemeral(move |mut scope| async move {
                let result = async {
                    let work = scope.spawn_blocking(
                        record_resources(0)?,
                        ResourceRequest::default(),
                        move |cancel| store::observe(&state, &project, &cancel),
                    )?;
                    Ok::<_, anyhow::Error>(
                        scope
                            .accept(work.wait().await?)?
                            .transpose()?
                            .into_parts()
                            .0,
                    )
                }
                .await;
                let _ = sender.send(result);
                Ok(())
            })?;
        let outcome = handle.wait().await;
        if let OperationOutcome::Failed(error) = &*outcome {
            return Err(error.clone().into());
        }
        Ok(receiver
            .await
            .context("Saved build inspection result was not retained")??
            .map(|(name, content)| SavedBuildRecord {
                owner: self.owner.clone(),
                name,
                content,
            }))
    }
    /// Explicit host cleanup, conditional on the exact observed record still being present.
    pub async fn discard_saved_build(&self, saved: SavedBuildRecord) -> Result<bool> {
        ensure!(
            Arc::ptr_eq(&self.owner, &saved.owner),
            "Saved build observation belongs to another engine"
        );
        let state = self.config.state_root.clone();
        let (sender, receiver) = oneshot::channel();
        let mut handle = self
            .preparations
            .start_ephemeral(move |mut scope| async move {
                let result = async {
                    let work = scope.spawn_blocking(
                        record_resources(0)?,
                        ResourceRequest::default(),
                        move |cancel| store::discard(&state, &saved.name, saved.content, &cancel),
                    )?;
                    Ok::<_, anyhow::Error>(
                        scope
                            .accept(work.wait().await?)?
                            .transpose()?
                            .into_parts()
                            .0,
                    )
                }
                .await;
                let _ = sender.send(result);
                Ok(())
            })?;
        let outcome = handle.wait().await;
        if let OperationOutcome::Failed(error) = &*outcome {
            return Err(error.clone().into());
        }
        receiver
            .await
            .context("Saved build cleanup result was not retained")?
    }
    /// Reconstruct a request, compare captured inputs and verify any retained cache content.
    /// The selected project and configured state root come from the host, never from saved data.
    pub async fn resume_saved_build(&self, project: PathBuf) -> Result<SavedBuildResume> {
        ensure!(project.is_absolute(), "Project selection must be absolute");
        let state = self.config.state_root.clone();
        let selected = project.clone();
        let (sender, receiver) = oneshot::channel();
        let mut handle = self
            .preparations
            .start_ephemeral(move |mut scope| async move {
                let result = async {
                    let work = scope.spawn_blocking(
                        record_resources(0)?,
                        // Selected retains the project root, state directory, lock and record.
                        ResourceRequest {
                            open_files: 4,
                            ..Default::default()
                        },
                        move |cancel| store::probe(&state, &selected, &cancel),
                    )?;
                    let selected = scope.accept(work.wait().await?)?.transpose()?;
                    let Some(bytes) = selected.as_ref().map(|selected| selected.bytes) else {
                        return Ok(selected.map(|_| None));
                    };
                    // The selected handles remain charged by their retained permit. Reading
                    // only needs the remainder for binding checks and document capture.
                    let work = scope.spawn_blocking(
                        ResourceRequest {
                            open_files: 4,
                            ..record_resources(bytes)?
                        },
                        ResourceRequest {
                            memory_bytes: record_memory(bytes)?,
                            ..Default::default()
                        },
                        move |cancel| {
                            let (selected, _guard) = selected.into_parts();
                            store::read(
                                selected.context("Selected pending record disappeared")?,
                                &cancel,
                            )
                        },
                    )?;
                    scope.accept(work.wait().await?)?.transpose()
                }
                .await;
                let _ = sender.send(result);
                Ok(())
            })?;
        let outcome = handle.wait().await;
        if let OperationOutcome::Failed(error) = &*outcome {
            return Err(error.clone().into());
        }
        let record = receiver
            .await
            .context("Saved build inspection result was not retained")??;
        let Some(loaded) = &*record else {
            return Ok(SavedBuildResume::Missing);
        };
        if !loaded.documents_match {
            return Ok(SavedBuildResume::Stale);
        }
        let saved = SavedBuildRecord {
            owner: self.owner.clone(),
            name: loaded.name.clone(),
            content: loaded.content,
        };
        let value = &loaded.record;
        ensure!(
            value.files.len() <= self.config.snapshot.entries,
            "Saved build content count limit exceeded"
        );
        let request = value.recipe.parse()?;
        let prepared = match self.prepare(project, request).await? {
            Preparation::Ready(prepared) => prepared,
            Preparation::NeedsInput(pending) => pending.prepared,
        };
        let PreparedKind::Build(build) = &**prepared.data else {
            unreachable!()
        };
        if build.workspace.observations().fingerprint() != value.fingerprint {
            return Ok(SavedBuildResume::Stale);
        }
        let expected: BTreeMap<_, _> = build
            .acquisition
            .pending
            .iter()
            .map(|need| (need.key.clone(), need.expected.clone()))
            .collect();
        let evidence = build.request.evidence;
        let mut keys = BTreeSet::new();
        for file in &value.files {
            let key = file.key.parse()?;
            let acquired = match &key {
                AcquisitionKey::Locked(key) => build.acquisition.acquired.locked.get(key),
            };
            ensure!(
                keys.insert(key.clone()) && (expected.contains_key(&key) || acquired.is_some()),
                "Saved content must match distinct current obligations"
            );
            let saved_id = file.id()?;
            if let Some(acquired) = acquired {
                ensure!(
                    acquired.content.lease().id() == saved_id
                        && acquired.permissions.readonly == file.readonly
                        && acquired.permissions.executable == file.executable,
                    "Saved content differs from the verified current input"
                );
            }
        }
        let config = self.config.clone();
        let (sender, receiver) = oneshot::channel();
        let mut handle = self
            .preparations
            .start_ephemeral(move |mut scope| async move {
                let result = restore(record, expected, evidence, config, &mut scope).await;
                let _ = sender.send(result);
                Ok(())
            })?;
        let outcome = handle.wait().await;
        if let OperationOutcome::Failed(error) = &*outcome {
            return Err(error.clone().into());
        }
        let content = receiver
            .await
            .context("Saved build content was not retained")??;
        let prepared = self
            .resume(PreparationContinuation { prepared }, content)
            .await?;
        Ok(SavedBuildResume::Prepared(Box::new(ResumedBuild {
            preparation: prepared,
            saved,
        })))
    }
}
async fn suspend(
    retained: RetainedOutput<PreparedBuild>,
    config: EngineConfig,
    owner: Arc<()>,
    saved: Option<SavedBuildRecord>,
    scope: &mut WorkScope,
) -> Result<SuspendedBuildReceipt> {
    let work = scope.spawn_blocking(
        config.resources.capture,
        ResourceRequest::default(),
        move |cancel| {
            retained
                .workspace
                .root()
                .revalidate(retained.workspace.observations(), &cancel)?;
            Ok::<_, anyhow::Error>(retained)
        },
    )?;
    let (retained, _) = scope.accept(work.wait().await?)?.transpose()?.into_parts();
    let (build, _retained_permit) = retained.into_parts();
    let retained_bytes = build.acquisition.acquired.retained_bytes()?;
    ensure!(
        retained_bytes <= config.transfer.transfer_bytes,
        "Retained build content exceeds byte limit"
    );
    let pending_files = build.acquisition.pending.len();
    let _metadata = scope.reserve_storage(ResourceRequest {
        memory_bytes: record_memory(record::estimated_bytes(
            &build.request,
            &build.acquisition.acquired,
        )?)?,
        ..Default::default()
    })?;
    let mut record = record::Record {
        schema: 2,
        fingerprint: build.workspace.observations().fingerprint(),
        documents: store::documents(build.workspace.observations())?,
        recipe: record::SavedRecipe::from(&build.request),
        files: vec![],
    };
    let save_guard =
        crate::engine::retained_cleanup::begin_save(scope, config.state_root.clone()).await?;
    let state = config.state_root.clone();
    let work = scope.spawn_blocking(
        record_resources(0)?,
        ResourceRequest::default(),
        move |cancel| {
            cancel.check()?;
            FileContentStore::open(
                &state.join("pending-content"),
                ContentStoreLimits::default(),
            )
        },
    )?;
    let (content_store, _) = scope.accept(work.wait().await?)?.transpose()?.into_parts();
    let files = build
        .acquisition
        .acquired
        .locked
        .into_iter()
        .map(|(key, file)| (AcquisitionKey::Locked(key), file));
    for (key, file) in files {
        scope.cancellation().check()?;
        let content = ExpectedDigest::Sha256(*file.content.lease().id().bytes()).hex();
        record.files.push(record::SavedFile {
            key: record::Key::from(&key),
            content,
            readonly: file.permissions.readonly,
            executable: file.permissions.executable,
        });
        content_store.publish_verified(scope, file.content).await?;
    }
    let retained_files = record.files.len();
    let work = scope.spawn_blocking(
        record_resources(0)?,
        ResourceRequest::default(),
        move |cancel| {
            let _save_guard = save_guard;
            store::save(
                &config.state_root,
                &build.workspace,
                &record,
                saved,
                &cancel,
            )
        },
    )?;
    let ((replaced, name, content), _) =
        scope.accept(work.wait().await?)?.transpose()?.into_parts();
    Ok(SuspendedBuildReceipt {
        saved: SavedBuildRecord {
            owner,
            name,
            content,
        },
        retained_files,
        retained_bytes,
        pending_files,
        replaced,
    })
}
async fn restore(
    record: RetainedOutput<Option<store::Loaded>>,
    expected: BTreeMap<AcquisitionKey, ExpectedContent>,
    evidence: SourceEvidencePolicy,
    config: EngineConfig,
    scope: &mut WorkScope,
) -> Result<BuildAcquisitions> {
    let state = config.state_root.clone();
    let work = scope.spawn_blocking(
        record_resources(0)?,
        ResourceRequest::default(),
        move |cancel| {
            cancel.check()?;
            FileContentLookup::open_existing(
                &state.join("pending-content"),
                ContentStoreLimits::default(),
            )
        },
    )?;
    let (lookup, _) = scope.accept(work.wait().await?)?.transpose()?.into_parts();
    let mut content = BuildAcquisitions::default();
    let Some(lookup) = lookup else {
        return Ok(content);
    };
    let mut total = 0u64;
    for file in &record
        .as_ref()
        .context("Missing inspected build record")?
        .record
        .files
    {
        let key = file.key.parse()?;
        // Preparation may already have verified this slot from ordinary cache storage.
        // Its saved address and permissions were checked before entering restore.
        let Some(expected) = expected.get(&key) else {
            continue;
        };
        let content_file = lookup
            .retain(
                scope,
                CachedFileRequest {
                    id: file.id()?,
                    expected: expected.clone(),
                    maximum: config
                        .transfer
                        .file_bytes
                        .min(config.transfer.transfer_bytes.saturating_sub(total)),
                    evidence,
                    initial: InitialObservation::RequireEvidence,
                },
            )
            .await?;
        let Some(acquired) = content_file else {
            continue;
        };
        total = total
            .checked_add(acquired.lease().len())
            .context("Saved content size overflow")?;
        ensure!(
            total <= config.transfer.transfer_bytes,
            "Saved build content exceeds byte limit"
        );
        let file = AcquiredBuildFile {
            content: acquired,
            permissions: FilePermissions {
                readonly: file.readonly,
                executable: file.executable,
            },
        };
        match key {
            AcquisitionKey::Locked(key) => {
                content.locked.insert(key, file);
            }
        }
    }
    Ok(content)
}
#[cfg(test)]
mod tests;
