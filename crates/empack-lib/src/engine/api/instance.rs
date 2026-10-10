//! Instance changes use the same plan grants, task ownership and publisher as author operations.
use super::*;
pub use crate::engine::instance::{
    ChoiceSelection, InstanceAction, InstanceRecord, InstanceSide, SelectedRelease,
};
use crate::engine::{
    content::AcquiredContent, instance, publication::Publisher, runtime::WorkScope,
};
use empack_core::files::{FileChange, FilePlan, ObservedPath};
mod acquisition;

pub struct InstallInstanceRequest {
    pub action: InstanceAction,
    pub release: SelectedRelease,
    pub side: InstanceSide,
    pub choices: Vec<ChoiceSelection>,
    /// Already acquired exact bytes, keyed by release logical file identity.
    pub supplied: BTreeMap<String, AcquiredContent>,
    /// Explicit host files associated with exact release logical keys.
    pub local_files: BTreeMap<String, PathBuf>,
    /// Root for immutable relative assets, read through no-follow native capabilities.
    pub assets: Option<PathBuf>,
}
#[derive(Clone)]
pub struct InstancePreview {
    pub plan: PlanId,
    pub record: InstanceRecord,
    pub files: FilePlan,
    pub replacement: ReplacementSummary,
    pub downloads: Vec<String>,
}
pub struct InstanceReceipt {
    pub plan: PlanId,
    pub record: InstanceRecord,
    pub publication: PublicationReceipt,
}
pub(super) struct PreparedInstanceOperation {
    pub(super) view: InstancePreview,
    instance: InstanceCandidate,
}
enum InstanceCandidate {
    Staged(instance::PreparedInstance),
    Acquire {
        plan: instance::InstancePlan,
        content: BTreeMap<String, AcquiredContent>,
        downloads: Vec<crate::engine::release::ReleaseFile>,
    },
}
pub(super) async fn prepare(
    target: ProjectTarget,
    request: InstallInstanceRequest,
    config: &EngineConfig,
    provider_access: ProviderAvailability,
    scope: &mut WorkScope,
) -> Result<RetainedOutput<PreparedInstanceOperation>> {
    let ProjectTarget::Existing(root) = target else {
        anyhow::bail!("Select an existing instance directory");
    };
    ensure!(root.is_absolute(), "Instance root must be absolute");
    let state = config.state_root.clone();
    let limits = config.snapshot;
    let InstallInstanceRequest {
        action,
        release,
        side,
        choices,
        supplied,
        local_files,
        assets,
    } = request;
    let work = scope.spawn_blocking(
        config.resources.capture,
        config.resources.prepared,
        move |cancel| {
            instance::plan(
                &root,
                instance::InstanceSelection {
                    release,
                    side,
                    choices,
                    action,
                },
                RecoveryReader::new(state),
                limits,
                &cancel,
            )
        },
    )?;
    let planned = scope.accept(work.wait().await?)?.transpose()?;
    let (resources, retained) = project_change::resources(planned.bytes()?, config)?;
    let work = scope.spawn_blocking(resources, retained, move |cancel| {
        let (planned, _reservation) = planned.into_parts();
        let content =
            planned.acquire_available(&supplied, &local_files, assets.as_deref(), &cancel)?;
        let downloads: Vec<_> = planned
            .needed()
            .filter(|file| !content.contains_key(&file.key))
            .cloned()
            .collect();
        for file in &downloads {
            ensure!(
                acquisition::available(file, provider_access)?,
                "Instance requires exact content for {}; supply --file {}=PATH",
                file.key,
                file.key
            );
        }
        let view = InstancePreview {
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
        Ok::<_, anyhow::Error>(PreparedInstanceOperation {
            view,
            instance: if downloads.is_empty() {
                InstanceCandidate::Staged(planned.stage(
                    &content,
                    &BTreeMap::new(),
                    None,
                    &cancel,
                )?)
            } else {
                InstanceCandidate::Acquire {
                    plan: planned,
                    content,
                    downloads,
                }
            },
        })
    })?;
    scope.accept(work.wait().await?)?.transpose()
}
pub(super) async fn run(
    prepared: RetainedOutput<PreparedInstanceOperation>,
    config: EngineConfig,
    transport: HttpAcquisition,
    catalog: Option<(ProviderCatalog, CatalogLimits)>,
    mut scope: WorkScope,
) -> Result<ExecutionOutcome, RuntimeError> {
    let cancel = scope.cancellation();
    let result = execute(prepared, config, transport, catalog, &mut scope).await;
    Ok(match result {
        Ok(receipt) => ExecutionOutcome::Completed(ExecutionReceipt::Instance(Box::new(receipt))),
        Err(error) => ExecutionOutcome::failed(error, cancel.is_cancelled()),
    })
}
async fn execute(
    prepared: RetainedOutput<PreparedInstanceOperation>,
    config: EngineConfig,
    transport: HttpAcquisition,
    catalog: Option<(ProviderCatalog, CatalogLimits)>,
    scope: &mut WorkScope,
) -> Result<RetainedOutput<InstanceReceipt>> {
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
    if let InstanceCandidate::Acquire {
        content, downloads, ..
    } = &mut prepared.instance
    {
        content.extend(
            acquisition::acquire(downloads, &transport, catalog.as_ref(), &config, scope).await?,
        );
    }
    let mut resources = config.resources.assembly;
    resources.scratch_bytes = resources.scratch_bytes.max(bytes);
    let work = scope.spawn_blocking(resources, config.resources.receipt, move |cancel| {
        let _reservation = reservation;
        let instance = match prepared.instance {
            InstanceCandidate::Staged(instance) => instance,
            InstanceCandidate::Acquire { plan, content, .. } => {
                plan.stage(&content, &BTreeMap::new(), None, &cancel)?
            }
        };
        let (publication, record) =
            instance.publish(&Publisher::open(&config.state_root)?, &cancel)?;
        Ok::<_, anyhow::Error>(InstanceReceipt {
            plan: prepared.view.plan,
            publication,
            record,
        })
    })?;
    scope
        .accept_publication(work.wait().await.map_err(PublicationWorkerFailed)?)?
        .transpose()
}

#[cfg(test)]
mod tests;
