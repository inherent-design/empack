//! Restart records contain exact selections and content, never execution grants or host paths.
use super::*;
use crate::engine::{
    content::{
        ContentPool, InitialObservation, SourceEvidencePolicy,
        store::{CachedFileRequest, ContentStoreLimits, FileContentLookup, FileContentStore},
    },
    continuation_store::{self as store, Binding, BoundRecord, Kind},
    instance::{ConflictChoice, ConflictResolution},
    release::{self, ReleaseFile},
};
use empack_core::digest::ContentId;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Recipe {
    payload: String,
    authority: Authority,
    action: InstanceAction,
    side: InstanceSide,
    layout: Option<InstanceLayout>,
    choices: Vec<ChoiceSelection>,
    conflicts: Vec<Decision>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum Authority {
    Snapshot,
    Subscription { envelope: String },
    ExternalPublisher,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "choice", rename_all = "kebab-case", deny_unknown_fields)]
enum Decision {
    Preserve { destination: String },
    Replace { destination: String },
    Merge { destination: String },
}
impl Recipe {
    pub(super) fn normalize(&mut self, plan: &instance::InstancePlan) {
        for decision in &mut self.conflicts {
            if let Decision::Merge { destination } = decision
                && !plan.needed().any(|file| &file.destination == destination)
            {
                *decision = Decision::Preserve {
                    destination: destination.clone(),
                };
            }
        }
    }
    pub(super) fn capture(request: &instance::InstanceSelection) -> Result<Self> {
        Ok(Self {
            payload: String::from_utf8(request.release.release().bytes().to_vec())?,
            authority: match &request.release {
                SelectedRelease::Snapshot(_) => Authority::Snapshot,
                SelectedRelease::Subscribed(proof) => Authority::Subscription {
                    envelope: String::from_utf8(proof.envelope().to_vec())?,
                },
                SelectedRelease::Publisher(_) => Authority::ExternalPublisher,
            },
            action: request.action,
            side: request.side,
            layout: request.layout,
            choices: request.choices.clone(),
            conflicts: request
                .conflicts
                .iter()
                .map(|value| match value.choice {
                    ConflictChoice::Preserve => Decision::Preserve {
                        destination: value.destination.clone(),
                    },
                    ConflictChoice::Replace => Decision::Replace {
                        destination: value.destination.clone(),
                    },
                    ConflictChoice::Merge { .. } => Decision::Merge {
                        destination: value.destination.clone(),
                    },
                })
                .collect(),
        })
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    schema: u32,
    binding: Binding,
    base: [u8; 32],
    recipe: Recipe,
    candidate: InstanceRecord,
    /// Includes local merge obligations, explicitly distinct from publisher assertions.
    files: Vec<ReleaseFile>,
    manual: Vec<String>,
}
impl BoundRecord for Record {
    fn binding(&self) -> &Binding {
        &self.binding
    }
    fn schema(&self) -> u32 {
        self.schema
    }
}
pub struct SavedInstanceRecord {
    owner: Arc<()>,
    saved: store::SavedRecord,
}
pub struct SuspendedInstanceReceipt {
    pub saved: SavedInstanceRecord,
    pub retained_files: usize,
    pub pending_files: usize,
}
pub struct ResumedInstance {
    pub saved: SavedInstanceRecord,
    pub preparation: Preparation,
}
fn resources(memory_bytes: u64) -> ResourceRequest {
    ResourceRequest {
        jobs: 1,
        memory_bytes,
        open_files: 8,
        ..Default::default()
    }
}
fn version() -> Result<semver::Version> {
    Ok(semver::Version::parse(
        if env!("CARGO_PKG_VERSION") == "0.0.0-dev" {
            "0.6.0-beta"
        } else {
            env!("CARGO_PKG_VERSION")
        },
    )?)
}
impl Engine {
    /// Explicit retention action. Previews never call this method.
    pub async fn suspend_instance(
        &self,
        pending: PreparationContinuation,
        prior: Option<SavedInstanceRecord>,
    ) -> Result<SuspendedInstanceReceipt> {
        ensure!(
            Arc::ptr_eq(&self.owner, &pending.prepared.owner),
            "Continuation belongs to another engine"
        );
        if let Some(prior) = &prior {
            ensure!(
                Arc::ptr_eq(&self.owner, &prior.owner),
                "Saved instance belongs to another engine"
            );
        }
        ensure!(
            matches!(&**pending.prepared.data, PreparedKind::Instance(_)),
            "Continuation is not an instance preparation"
        );
        let data = (*pending.prepared.data).map(|kind| match kind {
            PreparedKind::Instance(value) => *value,
            _ => unreachable!(),
        });
        let config = self.config.clone();
        let owner = self.owner.clone();
        let (sender, receiver) = oneshot::channel();
        let mut handle = self
            .preparations
            .start_ephemeral(move |mut scope| async move {
                let result = save(data, config, owner, prior, &mut scope).await;
                let _ = sender.send(result);
                Ok(())
            })?;
        let outcome = handle.wait().await;
        if let OperationOutcome::Failed(error) = &*outcome {
            return Err(error.clone().into());
        }
        receiver
            .await
            .context("Instance suspension result was not retained")?
    }
    /// Read-only reconstruction. Externally enrolled library callers resupply authentication;
    /// CLI subscriptions reverify their retained envelope against current durable enrollment.
    pub async fn resume_saved_instance(
        &self,
        root: PathBuf,
        files: BTreeMap<String, PathBuf>,
        publisher: Option<SelectedRelease>,
    ) -> Result<Option<ResumedInstance>> {
        let config = self.config.clone();
        let owner = self.owner.clone();
        let access = self
            .catalog
            .as_ref()
            .map(|(catalog, _)| catalog.availability())
            .unwrap_or_default();
        let cache = self.content_cache.clone();
        let (sender, receiver) = oneshot::channel();
        let mut handle = self
            .preparations
            .start_ephemeral(move |mut scope| async move {
                let result = resume(
                    root, files, publisher, config, owner, access, cache, &mut scope,
                )
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
            .context("Instance resume result was not retained")?
    }
    pub async fn discard_saved_instance(&self, saved: SavedInstanceRecord) -> Result<bool> {
        ensure!(
            Arc::ptr_eq(&self.owner, &saved.owner),
            "Saved instance belongs to another engine"
        );
        let (sender, receiver) = oneshot::channel();
        let mut handle = self.preparations.start_ephemeral(move |scope| async move {
            let result = async {
                let work = scope.spawn_blocking(
                    resources(256 << 10),
                    ResourceRequest::default(),
                    move |cancel| store::discard(&saved.saved.into_cleanup(), &cancel),
                )?;
                Ok::<_, anyhow::Error>(
                    scope
                        .accept_publication(work.wait().await?)?
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
            .context("Instance cleanup result was not retained")?
    }
}
async fn save(
    data: RetainedOutput<PreparedInstanceOperation>,
    config: EngineConfig,
    owner: Arc<()>,
    prior: Option<SavedInstanceRecord>,
    scope: &mut WorkScope,
) -> Result<SuspendedInstanceReceipt> {
    let prior = prior.or_else(|| {
        data.saved.clone().map(|saved| SavedInstanceRecord {
            owner: owner.clone(),
            saved,
        })
    });
    let target = data.target.clone();
    let retained_files = data.content.len();
    let pending_files = data.downloads.len();
    let state = config.state_root.clone();
    let record = scope.spawn_blocking(
        config.resources.capture,
        config.resources.prepared,
        move |cancel| {
            let (_, binding) = store::bind_for(Kind::Instance, &data.target, &cancel)?;
            let base = data.instance.base(&cancel)?;
            let record = Record {
                schema: 1,
                binding,
                base,
                recipe: data.resume.clone(),
                candidate: data.instance.record.clone(),
                files: data
                    .instance
                    .needed()
                    .filter(|f| data.content.contains_key(&f.key))
                    .cloned()
                    .collect(),
                manual: data.view.manual.iter().map(|f| f.key.clone()).collect(),
            };
            Ok::<_, anyhow::Error>((data, record))
        },
    )?;
    let record = scope.accept(record.wait().await?)?.transpose()?;
    let ((data, record), _record_reservation) = record.into_parts();
    let save_guard = crate::engine::retained_cleanup::begin_save(scope, state.clone()).await?;
    let content_state = state.clone();
    let work = scope.spawn_blocking(
        resources(256 << 10),
        ResourceRequest {
            open_files: 1,
            ..Default::default()
        },
        move |cancel| {
            cancel.check()?;
            FileContentStore::open(
                &content_state.join("pending-instance-content"),
                ContentStoreLimits::default(),
            )
        },
    )?;
    let content = scope.accept(work.wait().await?)?.transpose()?;
    for file in data.content.values() {
        content.publish_verified(scope, file.clone()).await?;
    }
    let memory = store::record_memory(store::encoded_bytes(&record, Kind::Instance.maximum())?)?;
    let work = scope.spawn_blocking(
        resources(memory),
        ResourceRequest::default(),
        move |cancel| {
            let _guard = save_guard;
            ensure!(
                data.instance.base(&cancel)? == record.base,
                "Instance changed before suspension"
            );
            store::save(
                &state,
                Kind::Instance,
                &target,
                &record,
                prior.as_ref().map(|p| &p.saved),
                &cancel,
            )
        },
    )?;
    let saved = scope
        .accept_publication(work.wait().await?)?
        .transpose()?
        .into_parts()
        .0;
    Ok(SuspendedInstanceReceipt {
        saved: SavedInstanceRecord { owner, saved },
        retained_files,
        pending_files,
    })
}
#[allow(clippy::too_many_arguments)]
async fn resume(
    root: PathBuf,
    files: BTreeMap<String, PathBuf>,
    publisher: Option<SelectedRelease>,
    config: EngineConfig,
    owner: Arc<()>,
    access: ProviderAvailability,
    cache: Option<crate::engine::content::cache::ContentCache>,
    scope: &mut WorkScope,
) -> Result<Option<ResumedInstance>> {
    let selected = root.clone();
    let state = config.state_root.clone();
    let work = scope.spawn_blocking(
        resources(256 << 10),
        ResourceRequest::default(),
        move |cancel| {
            cancel.check()?;
            store::record_bytes(&state, Kind::Instance, &selected)
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
    let selected = root.clone();
    let state = config.state_root.clone();
    let memory = store::record_memory(bytes)?;
    let work = scope.spawn_blocking(
        resources(memory),
        ResourceRequest {
            memory_bytes: memory,
            ..Default::default()
        },
        move |cancel| store::read::<Record>(&state, Kind::Instance, &selected, bytes, &cancel),
    )?;
    let (record, reservation) = scope.accept(work.wait().await?)?.transpose()?.into_parts();
    let (saved, record) = record.context("Saved instance disappeared")?;
    let record = RetainedOutput::from_parts(record, reservation);
    let state = config.state_root.clone();
    let work = scope.spawn_blocking(
        resources(256 << 10),
        ResourceRequest {
            open_files: 1,
            ..Default::default()
        },
        move |cancel| {
            cancel.check()?;
            FileContentLookup::open_existing(
                &state.join("pending-instance-content"),
                ContentStoreLimits::default(),
            )
        },
    )?;
    let lookup = scope.accept(work.wait().await?)?.transpose()?;
    let mut supplied = BTreeMap::new();
    let mut seen = std::collections::BTreeSet::new();
    let mut pool = ContentPool::owned(scope, config.transfer.transfer_bytes).await?;
    for file in &record.files {
        ensure!(
            seen.insert(file.key.clone()),
            "Duplicate saved instance file"
        );
        if let Some(lookup) = lookup.as_ref()
            && let Some(content) = lookup
                .retain(
                    scope,
                    CachedFileRequest {
                        id: ContentId::from_sha256(release::decode_hex(&file.sha256)?),
                        expected: file.expected()?,
                        maximum: config.transfer.file_bytes,
                        evidence: SourceEvidencePolicy::Compatibility,
                        initial: InitialObservation::RequireEvidence,
                    },
                )
                .await?
        {
            supplied.insert(
                file.key.clone(),
                pool.consolidate_owned(scope, content).await?,
            );
        }
    }
    let selected = root.clone();
    let state = config.state_root.clone();
    let record_recipe = record.recipe.clone();
    let merge_bytes = record
        .recipe
        .conflicts
        .iter()
        .try_fold(0u64, |sum, decision| {
            let bytes = match decision {
                Decision::Merge { destination } => {
                    record
                        .files
                        .iter()
                        .find(|file| &file.destination == destination)
                        .context("Saved merge obligation is missing")?
                        .bytes
                }
                _ => 0,
            };
            sum.checked_add(bytes).context("Saved merge size overflow")
        })?;
    let merge_resources = ResourceRequest {
        scratch_bytes: merge_bytes,
        memory_bytes: memory.max(config.resources.prepared.memory_bytes),
        open_files: 8.max(config.resources.prepared.open_files),
        ..resources(memory)
    };
    let merge_retained = ResourceRequest {
        scratch_bytes: merge_bytes,
        ..config.resources.prepared
    };
    let merged = supplied.clone();
    let saved_files = record.files.clone();
    let work = scope.spawn_blocking(merge_resources, merge_retained, move |cancel| {
        let decoded = release::DecodedRelease::decode(record_recipe.payload.as_bytes())?;
        let release = match &record_recipe.authority {
            Authority::Snapshot => {
                SelectedRelease::Snapshot(release::trust::SelectedSnapshot::select(
                    decoded.bytes(),
                    decoded.id(),
                    &version()?,
                )?)
            }
            Authority::Subscription { envelope } => {
                SelectedRelease::Subscribed(Arc::new(instance::subscription::select_release(
                    &selected,
                    envelope.as_bytes(),
                    SystemTime::now()
                        .duration_since(UNIX_EPOCH)?
                        .as_secs()
                        .try_into()?,
                    &version()?,
                    RecoveryReader::new(state),
                    &cancel,
                )?))
            }
            Authority::ExternalPublisher => {
                let proof = publisher
                    .context("Resuming external publisher input requires fresh authentication")?;
                ensure!(
                    matches!(proof, SelectedRelease::Publisher(_)),
                    "External publisher input requires publisher authentication"
                );
                proof
            }
        };
        ensure!(
            release.release().id() == decoded.id(),
            "Saved instance authentication selects another release"
        );
        let temporary = tempfile::tempdir()?;
        let mut conflicts = Vec::new();
        let mut supplied = merged;
        for (index, decision) in record_recipe.conflicts.iter().enumerate() {
            let (destination, choice) = match decision {
                Decision::Preserve { destination } => (destination, ConflictChoice::Preserve),
                Decision::Replace { destination } => (destination, ConflictChoice::Replace),
                Decision::Merge { destination } => {
                    let file = saved_files
                        .iter()
                        .find(|f| &f.destination == destination)
                        .context("Saved merge obligation is missing")?;
                    let content = supplied
                        .remove(&file.key)
                        .context("Saved merge content is unavailable; reselect the operation")?;
                    let path = temporary.path().join(index.to_string());
                    let mut out = std::fs::File::create(&path)?;
                    crate::engine::io::copy_bounded(
                        &mut content.lease().open(),
                        &mut out,
                        file.bytes,
                        &cancel,
                    )?;
                    (destination, ConflictChoice::Merge { file: path })
                }
            };
            conflicts.push(ConflictResolution {
                destination: destination.clone(),
                choice,
            });
        }
        Ok::<_, anyhow::Error>((
            temporary,
            InstallInstanceRequest {
                release,
                conflicts,
                action: record_recipe.action,
                side: record_recipe.side,
                layout: record_recipe.layout,
                choices: record_recipe.choices,
                supplied,
                local_files: files,
                assets: None,
            },
        ))
    })?;
    let ((temporary, request), _permit) =
        scope.accept(work.wait().await?)?.transpose()?.into_parts();
    let prepared = prepare(
        ProjectTarget::Existing(root),
        request,
        &config,
        access,
        cache.as_ref(),
        scope,
    )
    .await?;
    drop(temporary);
    let previous = saved.clone();
    let work = scope.spawn_blocking(
        config.resources.capture,
        ResourceRequest::default(),
        move |cancel| {
            let (mut prepared, reservation) = prepared.into_parts();
            ensure!(
                prepared.instance.base(&cancel)? == record.base
                    && prepared.instance.record == record.candidate,
                "Saved instance is stale: selected files or candidate changed"
            );
            for key in &record.manual {
                if !prepared.content.contains_key(key)
                    && !prepared.view.manual.iter().any(|f| &f.key == key)
                {
                    let file = prepared
                        .downloads
                        .iter()
                        .find(|f| &f.key == key)
                        .context("Saved manual obligation changed")?;
                    prepared.view.manual.push(InstanceInputRequirement {
                        key: key.clone(),
                        sha256: file.sha256.clone(),
                        bytes: file.bytes,
                    });
                }
            }
            prepared.saved = Some(previous);
            Ok::<_, anyhow::Error>(RetainedOutput::from_parts(prepared, reservation))
        },
    )?;
    let prepared = scope
        .accept(work.wait().await?)?
        .transpose()?
        .into_parts()
        .0;
    let data = prepared.map(|p| PreparedKind::Instance(Box::new(p)));
    let preparation = PreparedOperation {
        owner: owner.clone(),
        view: Box::new(data.view()),
        data: Box::new(data),
    }
    .classify();
    Ok(Some(ResumedInstance {
        saved: SavedInstanceRecord { owner, saved },
        preparation,
    }))
}
