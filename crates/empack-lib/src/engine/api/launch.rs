//! Owned execution of a caller-selected runtime against a completed exact installation.
use super::*;
use crate::engine::{
    instance::{self, InstanceAction, InstanceSelection, SelectedRelease},
    runtime::WorkScope,
};
use std::ffi::OsString;

/// A local runtime chosen by the host, never a command supplied by a release payload.
pub struct LaunchInstanceRequest {
    /// Bind prelaunch/update decisions to the release the runtime will actually run.
    /// None explicitly selects whichever completed release is captured during preparation.
    pub expected_release: Option<String>,
    pub program: PathBuf,
    pub arguments: Vec<OsString>,
}
#[derive(Clone)]
pub struct LaunchInstancePreview {
    pub plan: PlanId,
    pub record: instance::InstanceRecord,
    pub program: PathBuf,
}
pub struct LaunchInstanceReceipt {
    pub plan: PlanId,
    pub release: String,
    pub status: std::process::ExitStatus,
}
#[derive(Debug, thiserror::Error)]
#[error(
    "Instance runtime recovery is required; stop remaining processes, then run instance recover-runtime --acknowledge-stopped"
)]
pub(in crate::engine) struct RuntimeRecoveryRequired;

pub(super) struct PreparedLaunch {
    pub(super) view: LaunchInstancePreview,
    instance: instance::InstancePlan,
    command: std::process::Command,
}
pub(super) async fn prepare(
    target: ProjectTarget,
    request: LaunchInstanceRequest,
    config: &EngineConfig,
    scope: &mut WorkScope,
) -> Result<RetainedOutput<PreparedLaunch>> {
    let ProjectTarget::Existing(root) = target else {
        anyhow::bail!("Launch requires a completed instance")
    };
    ensure!(root.is_absolute(), "Instance selection must be absolute");
    ensure!(
        request.program.is_absolute(),
        "Runtime executable must be an explicit absolute path"
    );
    let state = config.state_root.clone();
    let limits = config.snapshot;
    let work = scope.spawn_blocking(
        config.resources.capture,
        config.resources.prepared,
        move |cancel| {
            let recovery = RecoveryReader::new(state);
            let (record, release) =
                instance::inspect(&root, None, recovery.clone(), limits, &cancel)?;
            ensure!(
                request
                    .expected_release
                    .as_ref()
                    .is_none_or(|expected| *expected == record.release),
                "Completed release changed after the prelaunch decision; prepare launch again"
            );
            let version = semver::Version::parse(if env!("CARGO_PKG_VERSION") == "0.0.0-dev" {
                "0.6.0-beta"
            } else {
                env!("CARGO_PKG_VERSION")
            })?;
            let release = super::super::release::trust::SelectedSnapshot::select(
                release.bytes(),
                release.id(),
                &version,
            )?;
            let instance = instance::plan(
                &root,
                InstanceSelection {
                    require_subscription: false,
                    conflicts: Vec::new(),
                    release: SelectedRelease::Snapshot(release),
                    side: record.side,
                    layout: None,
                    choices: Vec::new(),
                    action: InstanceAction::Repair,
                },
                recovery,
                limits,
                &cancel,
            )?;
            ensure!(
                instance.files.changes().is_empty(),
                "Instance content requires repair before launch"
            );
            let plan = PlanId(
                NEXT_PLAN
                    .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
                    .map_err(|_| anyhow::anyhow!("Plan identifier exhausted"))?,
            );
            let mut command = std::process::Command::new(&request.program);
            command
                .args(request.arguments)
                .current_dir(root.join(record.layout.directory()));
            Ok::<_, anyhow::Error>(PreparedLaunch {
                view: LaunchInstancePreview {
                    plan,
                    record,
                    program: request.program,
                },
                instance,
                command,
            })
        },
    )?;
    scope.accept(work.wait().await?)?.transpose()
}
pub(super) async fn run(
    prepared: RetainedOutput<PreparedLaunch>,
    config: EngineConfig,
    mut scope: WorkScope,
) -> Result<ExecutionOutcome, RuntimeError> {
    let cancel = scope.cancellation();
    let result = async {
        let work = scope.spawn_blocking(
            config.resources.capture,
            ResourceRequest {
                open_files: config.resources.prepared.open_files.max(4),
                ..config.resources.prepared
            },
            move |cancel| {
                let lease = prepared
                    .instance
                    .launch_lease(&config.state_root, &cancel)?;
                Ok::<_, anyhow::Error>((prepared, lease))
            },
        )?;
        let leased = scope.accept(work.wait().await?)?.transpose()?;
        let work = scope.spawn(
            ResourceRequest {
                jobs: 1,
                memory_bytes: 256 << 10,
                open_files: 8,
                ..Default::default()
            },
            ResourceRequest {
                memory_bytes: 4096,
                ..Default::default()
            },
            move |cancel| async move {
                let ((prepared, mut lease), _lease_reservation) = leased.into_parts();
                let (prepared, _prepared_reservation) = prepared.into_parts();
                lease.begin()?;
                let retired = crate::application::process_runtime::execute_inherited(
                    prepared.command,
                    cancel,
                )
                .await.context(RuntimeRecoveryRequired)?;
                let status = retired.0;
                if let Err(cleanup) = lease.complete() {
                    return Err(match status {
                        Err(process) => process.context(format!("Runtime retired, but recovery evidence could not be cleared: {cleanup:#}")),
                        Ok(status) => cleanup.context(format!("Runtime exited with {status}, but recovery evidence could not be cleared")),
                    }.context(RuntimeRecoveryRequired));
                }
                Ok::<_, anyhow::Error>(LaunchInstanceReceipt {
                    plan: prepared.view.plan,
                    release: prepared.view.record.release,
                    status: status?,
                })
            },
        )?;
        scope.accept_retirement(work.wait().await.context(RuntimeRecoveryRequired)?)?.transpose()
    }
    .await;
    Ok(match result {
        Ok(receipt) => ExecutionOutcome::Completed(ExecutionReceipt::Launch(Box::new(receipt))),
        Err(error) => ExecutionOutcome::failed(error, cancel.is_cancelled()),
    })
}

#[cfg(test)]
mod tests;

/// Explicit operator assertion that processes left by an interrupted host have stopped.
pub struct AcknowledgeStoppedRuntime;
#[derive(Clone)]
pub struct RuntimeRecoveryPreview {
    pub plan: PlanId,
}
pub struct RuntimeRecoveryReceipt {
    pub plan: PlanId,
}
pub(super) struct PreparedRuntimeRecovery {
    pub(super) view: RuntimeRecoveryPreview,
    root: crate::engine::snapshot::ProjectReadRoot,
    pending: crate::engine::publication::PendingRuntime,
}
pub(super) async fn prepare_recovery(
    target: ProjectTarget,
    config: &EngineConfig,
    scope: &mut WorkScope,
) -> Result<RetainedOutput<PreparedRuntimeRecovery>> {
    let ProjectTarget::Existing(root) = target else {
        anyhow::bail!("Runtime recovery requires an existing root")
    };
    let state = config.state_root.clone();
    let work = scope.spawn_blocking(
        config.resources.capture,
        config.resources.prepared,
        move |cancel| {
            cancel.check()?;
            let root = crate::engine::snapshot::ProjectReadRoot::open(&root)?;
            let publisher = crate::engine::publication::Publisher::open_existing(&state)?
                .context("No runtime state exists")?;
            let pending = publisher
                .observe_runtime(&root)?
                .context("No interrupted runtime needs acknowledgment")?;
            Ok::<_, anyhow::Error>(PreparedRuntimeRecovery {
                view: RuntimeRecoveryPreview {
                    plan: PlanId(
                        NEXT_PLAN
                            .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| {
                                id.checked_add(1)
                            })
                            .map_err(|_| anyhow::anyhow!("Plan identifier exhausted"))?,
                    ),
                },
                root,
                pending,
            })
        },
    )?;
    scope.accept(work.wait().await?)?.transpose()
}
pub(super) async fn recover(
    prepared: RetainedOutput<PreparedRuntimeRecovery>,
    config: EngineConfig,
    mut scope: WorkScope,
) -> Result<ExecutionOutcome, RuntimeError> {
    let cancel = scope.cancellation();
    let result = async {
        let work = scope.spawn_blocking(
            config.resources.capture,
            config.resources.receipt,
            move |cancel| {
                cancel.check()?;
                let (prepared, _reservation) = prepared.into_parts();
                crate::engine::publication::Publisher::open_existing(&config.state_root)?
                    .context("Runtime state disappeared")?
                    .acknowledge_stopped(&prepared.root, prepared.pending)?;
                Ok::<_, anyhow::Error>(RuntimeRecoveryReceipt {
                    plan: prepared.view.plan,
                })
            },
        )?;
        scope.accept(work.wait().await?)?.transpose()
    }
    .await;
    Ok(match result {
        Ok(receipt) => {
            ExecutionOutcome::Completed(ExecutionReceipt::RuntimeRecovered(Box::new(receipt)))
        }
        Err(error) => ExecutionOutcome::failed(error, cancel.is_cancelled()),
    })
}
