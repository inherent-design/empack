//! Instance changes use the same plan grants, task ownership and publisher as author operations.
use super::*;
pub use crate::engine::instance::{
    ChoiceSelection, InstanceAction, InstanceLayout, InstanceRecord, InstanceSide, SelectedRelease,
};
use crate::engine::{
    content::AcquiredContent, instance, publication::Publisher, runtime::WorkScope,
};
use empack_core::files::{FileChange, FilePlan, ObservedPath};
mod acquisition;
mod continuation;
mod suspension;
mod suspension_cleanup;
pub use suspension::{ResumedInstance, SavedInstanceRecord, SuspendedInstanceReceipt};
pub use suspension_cleanup::PendingInstanceCleanup;

pub struct InstallInstanceRequest {
    /// Require live explicit publisher enrollment before even initial snapshot installation.
    pub require_subscription: bool,
    pub conflicts: Vec<crate::engine::instance::ConflictResolution>,
    pub action: InstanceAction,
    pub release: SelectedRelease,
    pub side: InstanceSide,
    /// Keep the completed layout when omitted, otherwise default to game/.
    pub layout: Option<InstanceLayout>,
    pub choices: Vec<ChoiceSelection>,
    /// Already acquired exact bytes, keyed by release logical file identity.
    pub supplied: BTreeMap<String, AcquiredContent>,
    /// Explicit host files associated with exact release logical keys.
    pub local_files: BTreeMap<String, PathBuf>,
    /// Root for immutable relative assets, read through no-follow native capabilities.
    pub assets: Option<PathBuf>,
}
#[derive(Debug, Clone)]
pub struct InstanceInputRequirement {
    pub key: String,
    pub sha256: String,
    pub bytes: u64,
}
#[derive(Debug, thiserror::Error)]
#[error("Instance requires exact manual inputs: {0:?}")]
pub(super) struct MissingInstanceInputs(pub Vec<InstanceInputRequirement>);
#[derive(Clone)]
pub struct InstancePreview {
    pub plan: PlanId,
    pub record: InstanceRecord,
    pub files: FilePlan,
    pub replacement: ReplacementSummary,
    pub downloads: Vec<String>,
    pub manual: Vec<InstanceInputRequirement>,
}
pub struct InstanceReceipt {
    pub plan: PlanId,
    pub record: InstanceRecord,
    pub publication: PublicationReceipt,
}
pub(super) struct PreparedInstanceOperation {
    pub(super) view: InstancePreview,
    instance: instance::InstancePlan,
    content: BTreeMap<String, AcquiredContent>,
    downloads: Vec<crate::engine::release::ReleaseFile>,
    resume: suspension::Recipe,
    target: PathBuf,
    saved: Option<crate::engine::continuation_store::SavedRecord>,
}
pub(super) async fn prepare(
    target: ProjectTarget,
    request: InstallInstanceRequest,
    config: &EngineConfig,
    provider_access: ProviderAvailability,
    cache: Option<&crate::engine::content::cache::ContentCache>,
    scope: &mut WorkScope,
) -> Result<RetainedOutput<PreparedInstanceOperation>> {
    let ProjectTarget::Existing(root) = target else {
        anyhow::bail!("Select an existing instance directory");
    };
    ensure!(root.is_absolute(), "Instance root must be absolute");
    let state = config.state_root.clone();
    let limits = config.snapshot;
    let selected_target = root.clone();
    let InstallInstanceRequest {
        require_subscription,
        conflicts,
        action,
        release,
        side,
        layout,
        choices,
        supplied,
        local_files,
        assets,
    } = request;
    let work = scope.spawn_blocking(
        config.resources.capture,
        config.resources.prepared,
        move |cancel| {
            let selection = instance::InstanceSelection {
                require_subscription,
                conflicts,
                release,
                side,
                layout,
                choices,
                action,
            };
            let resume = suspension::Recipe::capture(&selection)?;
            let planned = instance::plan(
                &root,
                selection,
                RecoveryReader::new(state),
                limits,
                &cancel,
            )?;
            Ok::<_, anyhow::Error>((planned, resume))
        },
    )?;
    let planned = scope.accept(work.wait().await?)?.transpose()?;
    let (resources, retained) = project_change::resources(planned.0.bytes()?, config)?;
    let work = scope.spawn_blocking(resources, retained, move |cancel| {
        let ((planned, resume), _reservation) = planned.into_parts();
        let content =
            planned.acquire_available(&supplied, &local_files, assets.as_deref(), &cancel)?;
        Ok::<_, anyhow::Error>((planned, resume, content))
    })?;
    let available = scope.accept(work.wait().await?)?.transpose()?;
    let ((planned, mut resume, mut content), reservation) = available.into_parts();
    resume.normalize(&planned);
    if let Some(cache) = cache {
        acquisition::acquire_cached(&planned, &mut content, cache, config, scope).await?;
    }
    let downloads: Vec<_> = planned
        .needed()
        .filter(|file| !content.contains_key(&file.key))
        .map(|file| planned.download(file))
        .collect::<Result<_>>()?;
    let manual = manual_inputs(&downloads, provider_access)?;
    let view = InstancePreview {
        manual,
        downloads: downloads.iter().map(|file| file.key.clone()).collect(),
        plan: PlanId(
            NEXT_PLAN
                .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
                .map_err(|_| anyhow::anyhow!("Plan identifier exhausted"))?,
        ),
        record: planned.record.clone(),
        replacement: project_change::summary(&planned.files)?,
        files: planned.files.clone(),
    };
    scope.cancellation().check()?;
    Ok(RetainedOutput::from_parts(
        PreparedInstanceOperation {
            view,
            resume,
            target: selected_target,
            saved: None,
            instance: planned,
            content,
            downloads,
        },
        reservation,
    ))
}
pub(super) async fn run(
    prepared: RetainedOutput<PreparedInstanceOperation>,
    config: EngineConfig,
    transport: HttpAcquisition,
    catalog: Option<(ProviderCatalog, CatalogLimits)>,
    cache: Option<crate::engine::content::cache::ContentCache>,
    owner: Arc<()>,
    mut scope: WorkScope,
) -> Result<ExecutionOutcome, RuntimeError> {
    let cancel = scope.cancellation();
    let result = execute(
        prepared, config, transport, catalog, cache, owner, &mut scope,
    )
    .await;
    Ok(match result {
        Ok(outcome) => outcome,
        Err(error) => ExecutionOutcome::failed(error, cancel.is_cancelled()),
    })
}
async fn execute(
    prepared: RetainedOutput<PreparedInstanceOperation>,
    config: EngineConfig,
    transport: HttpAcquisition,
    catalog: Option<(ProviderCatalog, CatalogLimits)>,
    cache: Option<crate::engine::content::cache::ContentCache>,
    owner: Arc<()>,
    scope: &mut WorkScope,
) -> Result<ExecutionOutcome> {
    let bytes = prepared
        .view
        .files
        .changes()
        .iter()
        .try_fold(0u64, |sum, change| {
            let bytes = match change {
                FileChange::Replace { before, after, .. } => {
                    after.bytes.checked_add(match before {
                        ObservedPath::File(file) => file.bytes,
                        _ => 0,
                    })
                }
                FileChange::Remove { before, .. } => Some(before.bytes),
            }
            .context("Instance publication size overflow")?;
            sum.checked_add(bytes)
                .context("Instance publication size overflow")
        })?;
    let (mut prepared, reservation) = prepared.into_parts();
    if !prepared.downloads.is_empty() {
        match acquisition::acquire(
            &prepared.downloads,
            &transport,
            catalog.as_ref(),
            &config,
            scope,
        )
        .await
        {
            Ok(content) => prepared.content.extend(content),
            Err(error) => {
                if let Some(missing) = error.downcast_ref::<MissingInstanceInputs>() {
                    prepared.view.manual = missing.0.clone();
                    let instance_requirements = missing.0.clone();
                    let data = RetainedOutput::from_parts(
                        PreparedKind::Instance(Box::new(prepared)),
                        reservation,
                    );
                    return Ok(ExecutionOutcome::NeedsInput(ExecutionInput {
                        requirements: Vec::new(),
                        instance_requirements,
                        continuation: std::sync::Mutex::new(Some(PreparationContinuation {
                            prepared: PreparedOperation {
                                owner,
                                view: Box::new(data.view()),
                                data: Box::new(data),
                            },
                        })),
                    }));
                }
                return Err(error);
            }
        }
    }
    if let Some(cache) = cache {
        // Release addresses remain distinct from original provider assertions.
        cache
            .publish(scope, prepared.content.values().cloned().collect())
            .await?;
    }
    let mut resources = config.resources.assembly;
    resources.scratch_bytes = resources.scratch_bytes.max(bytes);
    let work = scope.spawn_blocking(resources, config.resources.receipt, move |cancel| {
        let _reservation = reservation;
        let instance =
            prepared
                .instance
                .stage(&prepared.content, &BTreeMap::new(), None, &cancel)?;
        let (publication, record) =
            instance.publish(&Publisher::open(&config.state_root)?, &cancel)?;
        Ok::<_, anyhow::Error>(InstanceReceipt {
            plan: prepared.view.plan,
            publication,
            record,
        })
    })?;
    let receipt = scope
        .accept_publication(work.wait().await.map_err(PublicationWorkerFailed)?)?
        .transpose()?;
    Ok(ExecutionOutcome::Completed(ExecutionReceipt::Instance(
        Box::new(receipt),
    )))
}

#[cfg(test)]
mod tests;

fn manual_inputs(
    files: &[crate::engine::release::ReleaseFile],
    access: ProviderAvailability,
) -> Result<Vec<InstanceInputRequirement>> {
    files
        .iter()
        .filter_map(|file| match acquisition::available(file, access) {
            Ok(true) => None,
            Ok(false) => Some(Ok(InstanceInputRequirement {
                key: file.key.clone(),
                sha256: file.sha256.clone(),
                bytes: file.bytes,
            })),
            Err(error) => Some(Err(error)),
        })
        .collect()
}
