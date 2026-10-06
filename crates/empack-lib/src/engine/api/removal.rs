//! Removal uses the same captured-plan approval, resource ownership and terminal outcomes.
use super::*;
pub use crate::engine::removal::RemovalSelector;
use crate::engine::{
    publication::{Publisher, RecoveryRequired},
    removal::{self as native_removal, PreparedRemoval},
    runtime::WorkScope,
};
use empack_core::{
    files::{FileChange, FilePlan, ObservedPath},
    model::{DependencyKey, ResolvedIdentity, ResolvedPin, ResolvedProject},
    removal::RemovalMode,
};
use std::collections::BTreeSet;

#[derive(Clone)]
pub struct RemoveRequest {
    pub selections: NonEmpty<RemovalSelector>,
    pub mode: RemovalMode,
}
/// Display identity excludes secret-bearing acquisition locators.
#[derive(Clone)]
pub struct RemovalSelection {
    pub key: DependencyKey,
    pub title: String,
    pub identity: ResolvedIdentity,
    pub version: Option<ResolvedPin>,
}
#[derive(Clone)]
pub struct RemovePreview {
    pub plan: PlanId,
    pub mode: RemovalMode,
    pub selected: Vec<RemovalSelection>,
    pub files: FilePlan,
    pub replacement: ReplacementSummary,
}
pub struct RemoveReceipt {
    pub plan: PlanId,
    pub publication: PublicationReceipt,
    pub project: ResolvedProject,
    pub mode: RemovalMode,
    pub selected: BTreeSet<DependencyKey>,
}
pub(super) struct PreparedRemovalOperation {
    pub(super) view: RemovePreview,
    removal: PreparedRemoval,
}
pub(super) async fn prepare(
    project: ProjectTarget,
    request: RemoveRequest,
    config: &EngineConfig,
    scope: &mut WorkScope,
) -> Result<RetainedOutput<PreparedRemovalOperation>> {
    let ProjectTarget::Existing(project) = project else {
        anyhow::bail!("Removal requires an existing project");
    };
    ensure!(project.is_absolute(), "Project selection must be absolute");
    let state = config.state_root.clone();
    let limits = config.snapshot;
    let work = scope.spawn_blocking(
        config.resources.capture,
        config.resources.prepared,
        move |cancel| {
            let snapshot = ProjectReader::new(RecoveryReader::new(state))
                .capture_mutation(&project, limits, &cancel)?;
            native_removal::plan_selected_removal(
                snapshot,
                &request.selections,
                request.mode,
                &cancel,
            )
        },
    )?;
    let planned = scope.accept(work.wait().await?)?.transpose()?;
    let (resources, retained) = project_change::resources(planned.bytes()?, config)?;
    let work = scope.spawn_blocking(resources, retained, move |cancel| {
        let (planned, _reservation) = planned.into_parts();
        let removal = planned.stage(&cancel)?;
        let files = removal.files().clone();
        let view = RemovePreview {
            plan: PlanId(
                NEXT_PLAN
                    .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
                    .map_err(|_| anyhow::anyhow!("Plan identifier exhausted"))?,
            ),
            mode: removal.candidate().plan().mode(),
            selected: removal
                .candidate()
                .plan()
                .selected()
                .iter()
                .map(|(key, dependency)| RemovalSelection {
                    key: key.clone(),
                    title: dependency.title.clone(),
                    identity: dependency.identity.clone(),
                    version: dependency.selected.clone(),
                })
                .collect(),
            replacement: project_change::summary(&files)?,
            files,
        };
        Ok::<_, anyhow::Error>(PreparedRemovalOperation { view, removal })
    })?;
    scope.accept(work.wait().await?)?.transpose()
}
pub(super) async fn run(
    prepared: RetainedOutput<PreparedRemovalOperation>,
    config: EngineConfig,
    mut scope: WorkScope,
) -> Result<ExecutionOutcome, RuntimeError> {
    let cancel = scope.cancellation();
    let result = execute(prepared, config, &mut scope).await;
    Ok(match result {
        Ok(receipt) => ExecutionOutcome::Completed(ExecutionReceipt::Remove(Box::new(receipt))),
        Err(error) if error.downcast_ref::<RecoveryRequired>().is_some() => {
            ExecutionOutcome::RecoveryRequired {
                operation: error
                    .downcast_ref::<RecoveryRequired>()
                    .unwrap()
                    .operation
                    .clone(),
                cause: error,
            }
        }
        Err(error) if error.downcast_ref::<PublicationWorkerFailed>().is_some() => {
            ExecutionOutcome::ExecutionUncertain(error)
        }
        Err(_) if cancel.is_cancelled() => ExecutionOutcome::InterruptedBeforePublication,
        Err(error) => ExecutionOutcome::FailedBeforePublication(error),
    })
}
async fn execute(
    prepared: RetainedOutput<PreparedRemovalOperation>,
    config: EngineConfig,
    scope: &mut WorkScope,
) -> Result<RetainedOutput<RemoveReceipt>> {
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
            .context("Removal publication size overflow")?;
            sum.checked_add(bytes)
                .context("Removal publication size overflow")
        })?;
    let mut resources = config.resources.assembly;
    resources.scratch_bytes = resources.scratch_bytes.max(bytes);
    let work = scope.spawn_blocking(resources, config.resources.receipt, move |cancel| {
        cancel.check()?;
        let (prepared, _reservation) = prepared.into_parts();
        let receipt = prepared
            .removal
            .publish(&Publisher::open(&config.state_root)?, &cancel)?;
        Ok::<_, anyhow::Error>(RemoveReceipt {
            plan: prepared.view.plan,
            publication: receipt.publication,
            project: receipt.project,
            mode: receipt.mode,
            selected: receipt.selected,
        })
    })?;
    scope
        .accept_publication(work.wait().await.map_err(PublicationWorkerFailed)?)?
        .transpose()
}
