//! Resolved additions use captured plans, explicit replacement and retained engine outcomes.
use super::*;
use crate::engine::{
    addition::{self as native_addition, PreparedAddition},
    mrpack::{AcquiredBuildFile, LockedFileKey},
    publication::{Publisher, RecoveryRequired},
    runtime::WorkScope,
};
use empack_core::{
    addition::AdditionGroup,
    files::{FileChange, FilePlan, ObservedPath},
    model::{DependencyKey, ResolvedProject},
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ExistingDependencyPolicy {
    #[default]
    RejectExisting,
    UpdateSameIdentity,
}
/// Resolved request and immutable acquired bytes. Provider/local/URL hosts resolve before this
/// boundary; neither the supplied group nor its content grants project publication authority.
pub struct AddRequest {
    pub group: AdditionGroup,
    pub content: BTreeMap<LockedFileKey, AcquiredBuildFile>,
    pub existing: ExistingDependencyPolicy,
}
#[derive(Clone)]
pub struct AddPreview {
    pub plan: PlanId,
    pub bindings: BTreeMap<DependencyKey, DependencyKey>,
    pub existing_roots: BTreeSet<DependencyKey>,
    pub existing: ExistingDependencyPolicy,
    pub files: FilePlan,
    pub replacement: ReplacementSummary,
}
pub struct AddReceipt {
    pub plan: PlanId,
    pub publication: PublicationReceipt,
    pub project: ResolvedProject,
    pub bindings: BTreeMap<DependencyKey, DependencyKey>,
}
pub(super) struct PreparedAdditionOperation {
    pub(super) view: AddPreview,
    addition: PreparedAddition,
}
pub(super) async fn prepare(
    project: ProjectTarget,
    request: AddRequest,
    config: &EngineConfig,
    scope: &mut WorkScope,
) -> Result<RetainedOutput<PreparedAdditionOperation>> {
    let ProjectTarget::Existing(project) = project else {
        anyhow::bail!("Addition requires an existing project")
    };
    ensure!(project.is_absolute(), "Project selection must be absolute");
    let state = config.state_root.clone();
    let limits = config.snapshot;
    let policy = request.existing;
    let work = scope.spawn_blocking(config.resources.capture, config.resources.prepared, move |cancel| {
        let snapshot = ProjectReader::new(RecoveryReader::new(state)).capture_addition(&project, &request.group, limits, &cancel)?;
        let planned = native_addition::plan_addition(snapshot, &request.group, request.content, &cancel)?;
        ensure!(policy == ExistingDependencyPolicy::UpdateSameIdentity || planned.candidate().plan().existing_roots().is_empty(), "Requested dependency already exists; updating the same identity requires explicit authorization");
        Ok::<_, anyhow::Error>(planned)
    })?;
    let planned = scope.accept(work.wait().await?)?.transpose()?;
    let (resources, retained) = project_change::resources(planned.bytes()?, config)?;
    let work = scope.spawn_blocking(resources, retained, move |cancel| {
        let (planned, _reservation) = planned.into_parts();
        let addition = planned.stage(&cancel)?;
        let files = addition.files().clone();
        let view = AddPreview {
            plan: PlanId(
                NEXT_PLAN
                    .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
                    .map_err(|_| anyhow::anyhow!("Plan identifier exhausted"))?,
            ),
            bindings: addition.candidate().plan().bindings().clone(),
            existing_roots: addition.candidate().plan().existing_roots().clone(),
            existing: policy,
            replacement: project_change::summary(&files)?,
            files,
        };
        Ok::<_, anyhow::Error>(PreparedAdditionOperation { view, addition })
    })?;
    scope.accept(work.wait().await?)?.transpose()
}
pub(super) async fn run(
    prepared: RetainedOutput<PreparedAdditionOperation>,
    config: EngineConfig,
    mut scope: WorkScope,
) -> Result<ExecutionOutcome, RuntimeError> {
    let cancel = scope.cancellation();
    let result = execute(prepared, config, &mut scope).await;
    Ok(match result {
        Ok(receipt) => ExecutionOutcome::Completed(ExecutionReceipt::Add(Box::new(receipt))),
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
    prepared: RetainedOutput<PreparedAdditionOperation>,
    config: EngineConfig,
    scope: &mut WorkScope,
) -> Result<RetainedOutput<AddReceipt>> {
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
            .context("Addition publication size overflow")?;
            sum.checked_add(bytes)
                .context("Addition publication size overflow")
        })?;
    let mut resources = config.resources.assembly;
    resources.scratch_bytes = resources.scratch_bytes.max(bytes);
    let work = scope.spawn_blocking(resources, config.resources.receipt, move |cancel| {
        cancel.check()?;
        let (prepared, _reservation) = prepared.into_parts();
        let receipt = prepared
            .addition
            .publish(&Publisher::open(&config.state_root)?, &cancel)?;
        Ok::<_, anyhow::Error>(AddReceipt {
            plan: prepared.view.plan,
            publication: receipt.publication,
            project: receipt.project,
            bindings: prepared.view.bindings,
        })
    })?;
    scope
        .accept_publication(work.wait().await.map_err(PublicationWorkerFailed)?)?
        .transpose()
}
