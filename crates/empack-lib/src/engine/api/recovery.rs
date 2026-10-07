//! Recovery uses the same prepared-plan approval and owned execution lifecycle as other effects.
use super::*;
pub use crate::engine::publication::{RecoveryAction, RecoveryStatus};
use crate::engine::{
    publication::{PreparedRecovery, Publisher, RecoveryRequired},
    runtime::WorkScope,
    snapshot::ProjectReadRoot,
};
use empack_core::files::FilePlan;

pub struct RecoverRequest {
    /// Bind the recovery selected from inspection; another operation requires a new decision.
    pub operation: String,
    pub action: RecoveryAction,
}
#[derive(Clone)]
pub struct RecoverPreview {
    pub plan: PlanId,
    pub status: RecoveryStatus,
    pub action: RecoveryAction,
    pub files: FilePlan,
    pub replacement: ReplacementSummary,
}
pub struct RecoveryReceipt {
    pub plan: PlanId,
    pub publication: PublicationReceipt,
}
pub(super) struct PreparedRecoveryOperation {
    pub(super) view: RecoverPreview,
    root: ProjectReadRoot,
    recovery: PreparedRecovery,
}
impl Engine {
    /// Inspect host-owned recovery state without parsing project documents or creating state.
    pub async fn inspect_recovery(
        &self,
        target: impl Into<ProjectTarget>,
    ) -> Result<Option<RecoveryStatus>> {
        let target = target.into();
        let config = self.config.clone();
        let (sender, receiver) = oneshot::channel();
        let mut handle = self
            .preparations
            .start_ephemeral(move |mut scope| async move {
                let result: Result<_> = async {
                    let work = scope.spawn_blocking(
                        config.resources.capture,
                        ResourceRequest::default(),
                        move |cancel| {
                            cancel.check()?;
                            let ProjectTarget::Existing(path) = target else {
                                anyhow::bail!("Creation recovery requires its creation journal")
                            };
                            ensure!(path.is_absolute(), "Recovery selection must be absolute");
                            let root = ProjectReadRoot::open(&path)?;
                            let Some(publisher) = Publisher::open_existing(&config.state_root)?
                            else {
                                return Ok(None);
                            };
                            publisher.inspect_recovery(&root)
                        },
                    )?;
                    Ok(scope
                        .accept(work.wait().await?)?
                        .transpose()?
                        .into_parts()
                        .0)
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
            .context("Recovery inspection result was not retained")?
    }
}
pub(super) async fn prepare(
    target: ProjectTarget,
    request: RecoverRequest,
    config: &EngineConfig,
    scope: &mut WorkScope,
) -> Result<RetainedOutput<PreparedRecoveryOperation>> {
    let ProjectTarget::Existing(path) = target else {
        anyhow::bail!("Creation recovery requires its creation journal")
    };
    ensure!(path.is_absolute(), "Recovery selection must be absolute");
    let state = config.state_root.clone();
    let work = scope.spawn_blocking(
        config.resources.capture,
        config.resources.prepared,
        move |cancel| {
            cancel.check()?;
            let root = ProjectReadRoot::open(&path)?;
            let publisher =
                Publisher::open_existing(&state)?.context("No publication recovery exists")?;
            let recovery = publisher
                .prepare_recovery(&root, request.action)?
                .context("No pending publication recovery exists")?;
            ensure!(
                recovery.status().operation == request.operation,
                "Pending recovery operation changed"
            );
            let files = recovery.files().clone();
            let view = RecoverPreview {
                plan: PlanId(
                    NEXT_PLAN
                        .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
                        .map_err(|_| anyhow::anyhow!("Plan identifier exhausted"))?,
                ),
                status: recovery.status().clone(),
                action: request.action,
                replacement: project_change::summary(&files)?,
                files,
            };
            Ok::<_, anyhow::Error>(PreparedRecoveryOperation {
                root,
                recovery,
                view,
            })
        },
    )?;
    scope.accept(work.wait().await?)?.transpose()
}
pub(super) async fn run(
    prepared: RetainedOutput<PreparedRecoveryOperation>,
    config: EngineConfig,
    scope: WorkScope,
) -> Result<ExecutionOutcome, RuntimeError> {
    let cancel = scope.cancellation();
    let result: Result<RetainedOutput<RecoveryReceipt>> = async {
        let mut resources = config.resources.assembly;
        resources.scratch_bytes = resources
            .scratch_bytes
            .max(prepared.recovery.scratch_bytes());
        let work = scope.spawn_blocking(resources, config.resources.receipt, move |cancel| {
            cancel.check()?;
            let (prepared, _reservation) = prepared.into_parts();
            let publisher = Publisher::open_existing(&config.state_root)?
                .context("Recovery state disappeared")?;
            let publication = publisher.recover_prepared(&prepared.root, prepared.recovery)?;
            Ok::<_, anyhow::Error>(RecoveryReceipt {
                plan: prepared.view.plan,
                publication,
            })
        })?;
        scope
            .accept_publication(work.wait().await.map_err(PublicationWorkerFailed)?)?
            .transpose()
    }
    .await;
    Ok(match result {
        Ok(receipt) => ExecutionOutcome::Completed(ExecutionReceipt::Recovery(Box::new(receipt))),
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
