//! CLI composition for the shared engine. Host state is separate from disposable caches.
use super::{AppConfig, cli::CliRecoveryAction, session::Session};
use crate::engine::{
    acquisition::TransferLimits,
    api::{
        Engine, EngineConfig, ExecutionGrant, ExecutionOutcome, ExecutionReceipt,
        NetworkPermission, OperationResources, Preparation, PreparedOperation, ProjectTarget,
        RecoverRequest, RecoveryAction,
    },
    artifacts::ArchiveLimits,
    resources::{ResourceGovernor, ResourceRequest},
    runtime::{OperationOutcome, RuntimeError},
    server_runtime::installer::InstallerExecution,
    snapshot::SnapshotLimits,
};
use anyhow::{Context, Result, ensure};
use std::{
    future::Future,
    path::{Path, PathBuf},
    time::Duration,
};

fn absolute(invocation: &Path, selected: &Path) -> PathBuf {
    if selected.is_absolute() {
        selected.to_path_buf()
    } else {
        invocation.join(selected)
    }
}
fn state_root(config: &AppConfig, invocation: &Path) -> Result<PathBuf> {
    if let Some(path) = &config.state_dir {
        ensure!(
            !path.as_os_str().is_empty(),
            "State directory cannot be empty"
        );
        return Ok(absolute(invocation, path));
    }
    let directories = directories::ProjectDirs::from("design", "inherent", "empack")
        .context("Cannot determine durable state directory; set --state-dir")?;
    Ok(directories.data_local_dir().join("operations"))
}
fn content_cache_root(config: &AppConfig, invocation: &Path) -> Result<PathBuf> {
    let base = match &config.cache_dir {
        Some(path) => {
            ensure!(
                !path.as_os_str().is_empty(),
                "Cache directory cannot be empty"
            );
            path.clone()
        }
        None => crate::platform::cache::cache_root()?,
    };
    Ok(absolute(invocation, &base).join("content-v1"))
}
fn acquisition_with_cache_lookup(
    session: &dyn Session,
    transport: crate::engine::acquisition::HttpAcquisition,
) -> Result<crate::engine::acquisition::HttpAcquisition> {
    let (invocation, _) = project_path(session)?;
    Ok(
        transport.with_cache_lookup(crate::engine::content::cache::ContentCache::new(
            content_cache_root(session.config().app_config(), &invocation)?,
            crate::engine::content::store::ContentStoreLimits::default(),
        )?),
    )
}
fn governor(config: &AppConfig) -> ResourceGovernor {
    ResourceGovernor::new(ResourceRequest {
        jobs: config.cpu_jobs.max(1) as u64,
        // Includes the installer JVM (1 GiB heap plus overhead) and retained build inputs.
        memory_bytes: 2 << 30,
        scratch_bytes: 128 << 30,
        open_files: 2048,
    })
}
fn operation_resources() -> OperationResources {
    let work = ResourceRequest {
        jobs: 1,
        memory_bytes: 64 << 20,
        scratch_bytes: 64 << 20,
        open_files: 64,
    };
    let held = ResourceRequest {
        memory_bytes: 64 << 20,
        open_files: 8,
        ..Default::default()
    };
    OperationResources {
        capture: work,
        prepared: held,
        local_acquisition: work,
        acquired: ResourceRequest {
            scratch_bytes: 64 << 20,
            ..held
        },
        assembly: work,
        receipt: held,
    }
}
fn installer_execution() -> InstallerExecution {
    InstallerExecution {
        java: "java".into(),
        deadline: Duration::from_secs(1800),
        heap_megabytes: 1024,
        // Runtime output is bounded separately from the full project snapshot.
        // Its worst-case staging and retained leases must fit the host budget together.
        output: SnapshotLimits {
            entries: 1024,
            depth: 128,
            file_bytes: 2 << 30,
            total_bytes: 4 << 30,
        },
    }
}
fn engine(config: &AppConfig, invocation: &Path) -> Result<Engine> {
    engine_with_governor(config, invocation, governor(config))
}
fn engine_with_governor(
    config: &AppConfig,
    invocation: &Path,
    governor: ResourceGovernor,
) -> Result<Engine> {
    let cache = crate::engine::content::cache::ContentCache::new(
        content_cache_root(config, invocation)?,
        Default::default(),
    )?;
    Ok(Engine::new(
        EngineConfig {
            state_root: state_root(config, invocation)?,
            retained_operations: 4,
            resources: operation_resources(),
            snapshot: SnapshotLimits::default(),
            archive: ArchiveLimits::default(),
            transfer: TransferLimits {
                deadline: Duration::from_secs(config.net_timeout),
                ..Default::default()
            },
            installer: installer_execution(),
        },
        governor,
    )?
    .with_content_cache(cache))
}
/// Resolve one invocation-relative project selection without changing process cwd.
fn project_path(session: &dyn Session) -> Result<(PathBuf, PathBuf)> {
    let invocation = session.invocation().current_dir()?;
    let selected = session
        .config()
        .app_config()
        .workdir
        .as_deref()
        .unwrap_or(&invocation);
    Ok((invocation.clone(), absolute(&invocation, selected)))
}

/// Dropping a preparation future cancels its owned workers; the outer host always drains shutdown.
async fn cancellable<T>(session: &dyn Session, work: impl Future<Output = Result<T>>) -> Result<T> {
    tokio::pin!(work);
    let mut ticks = tokio::time::interval(Duration::from_millis(50));
    loop {
        tokio::select! {
            result = &mut work => return result,
            _ = ticks.tick() => session.process().check_cancelled()?,
        }
    }
}
pub async fn recover(
    session: &dyn Session,
    action: CliRecoveryAction,
    operation: Option<String>,
) -> Result<()> {
    let invocation = session.invocation().current_dir()?;
    let config = session.config().app_config();
    let selected = config.workdir.as_deref().unwrap_or(&invocation);
    let path = absolute(&invocation, selected);
    let target = match std::fs::symlink_metadata(&path) {
        Ok(_) => ProjectTarget::Existing(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => ProjectTarget::New(path),
        Err(error) => return Err(error.into()),
    };
    let engine = engine(config, &invocation)?;
    let result = recover_with_engine(session, &engine, target, action, operation).await;
    engine.shutdown().await;
    result
}
fn show_changes(session: &dyn Session, files: &empack_core::files::FilePlan) -> Result<()> {
    use empack_core::files::{FileChange, ObservedPath};
    for change in files.changes() {
        let path = crate::engine::layout::ProjectLayout::path(change.target())?;
        let (action, detail) = match change {
            FileChange::Replace {
                before: ObservedPath::Absent,
                after,
                ..
            } => ("create", format!("{} bytes", after.bytes)),
            FileChange::Replace {
                before: ObservedPath::File(before),
                after,
                ..
            } => (
                "replace",
                format!("{} -> {} bytes", before.bytes, after.bytes),
            ),
            FileChange::Remove { before, .. } => ("remove", format!("{} bytes", before.bytes)),
            FileChange::Replace {
                before: ObservedPath::Directory,
                ..
            } => anyhow::bail!("Recovery cannot replace a directory"),
        };
        session
            .display()
            .status()
            .info(&format!("{action} {} ({detail})", path.as_str()));
    }
    Ok(())
}
async fn recover_with_engine(
    session: &dyn Session,
    engine: &Engine,
    target: ProjectTarget,
    action: CliRecoveryAction,
    operation: Option<String>,
) -> Result<()> {
    session.process().check_cancelled()?;
    let status = cancellable(session, engine.inspect_recovery(target.clone())).await?;
    let Some(status) = status else {
        ensure!(
            operation.is_none(),
            "Requested recovery operation is no longer pending"
        );
        session
            .display()
            .status()
            .info("No interrupted engine operation to recover");
        return Ok(());
    };
    ensure!(
        operation
            .as_ref()
            .is_none_or(|value| value == &status.operation),
        "Pending recovery operation differs from --operation"
    );
    session.display().status().info(&format!(
        "Interrupted operation: {} ({:?})",
        status.operation, status.kind
    ));
    if status.restoring {
        session
            .display()
            .status()
            .info("This operation is already restoring its prior files");
    }
    let action = match action {
        CliRecoveryAction::Inspect => return Ok(()),
        CliRecoveryAction::Finish => RecoveryAction::Finish,
        CliRecoveryAction::Restore => RecoveryAction::Restore,
    };
    let prepared = match cancellable(
        session,
        engine.prepare(
            target,
            RecoverRequest {
                operation: status.operation,
                action,
            },
        ),
    )
    .await?
    {
        Preparation::Ready(prepared) => prepared,
        Preparation::NeedsInput(_) => anyhow::bail!("Recovery unexpectedly requires acquisition"),
    };
    let view = prepared
        .view()
        .recovery()
        .context("Missing recovery preview")?;
    session.display().status().info(&format!(
        "{:?}: {} managed file changes",
        view.action,
        view.files.changes().len()
    ));
    show_changes(session, &view.files)?;
    apply(session, engine, prepared, "Recovery", |receipt| {
        let ExecutionReceipt::Recovery(receipt) = receipt else {
            anyhow::bail!("Unexpected recovery receipt");
        };
        Ok(format!(
            "Recovery {:?}: {} managed files",
            receipt.publication.disposition, receipt.publication.changed_files
        ))
    })
    .await
}

/// Callers display their exact preview before this shared approval and owned execution boundary.
async fn apply(
    session: &dyn Session,
    engine: &Engine,
    prepared: PreparedOperation,
    label: &str,
    completed: impl FnOnce(&ExecutionReceipt) -> Result<String>,
) -> Result<()> {
    if !approve(session, label)? {
        return Ok(());
    }
    execute_approved(session, engine, prepared, label, completed).await
}
fn approve(session: &dyn Session, label: &str) -> Result<bool> {
    if session.config().app_config().dry_run {
        session
            .display()
            .status()
            .complete("Dry run complete - no changes applied");
        return Ok(false);
    }
    ensure!(
        session.config().app_config().yes || session.interactive().can_choose(),
        super::cli::CommandInputRequired(
            "Noninteractive execution requires --yes; use --dry-run to inspect the plan"
        )
    );
    if !session.config().app_config().yes
        && !session.interactive().confirm(
            &format!("Apply this {} plan?", label.to_ascii_lowercase()),
            false,
        )?
    {
        session
            .display()
            .status()
            .info(&format!("{label} not applied"));
        return Ok(false);
    }
    Ok(true)
}
/// The composition root has displayed and approved this exact plan (or a union containing it).
async fn execute_approved(
    session: &dyn Session,
    engine: &Engine,
    prepared: PreparedOperation,
    label: &str,
    completed: impl FnOnce(&ExecutionReceipt) -> Result<String>,
) -> Result<()> {
    session.process().check_cancelled()?;
    let grant = ExecutionGrant {
        plan: prepared.view().plan(),
        replacement: prepared.view().replacement(),
        network: if prepared.view().needs_network() {
            NetworkPermission::Allow
        } else {
            NetworkPermission::Offline
        },
        run_installer: prepared.view().runs_installer(),
        run_runtime: matches!(
            prepared.view(),
            crate::engine::api::OperationPreview::Launch(_)
        ),
    };
    let mut handle = engine.start(prepared.authorize(grant)?)?;
    let mut ticks = tokio::time::interval(Duration::from_millis(50));
    let outcome = loop {
        tokio::select! {
            outcome = handle.wait() => break outcome,
            _ = ticks.tick() => if session.process().check_cancelled().is_err() {
                handle.cancel();
                break handle.wait().await;
            }
        }
    };
    let result = match &*outcome {
        OperationOutcome::Completed(ExecutionOutcome::Completed(receipt)) => {
            completed(receipt).map(|message| session.display().status().complete(&message))
        }
        OperationOutcome::Completed(ExecutionOutcome::InterruptedBeforePublication)
        | OperationOutcome::Failed(RuntimeError::Cancelled) => {
            Err(super::process_runtime::Interrupted.into())
        }
        OperationOutcome::Completed(ExecutionOutcome::RecoveryRequired { operation, .. }) => {
            Err(retained_failure(
                outcome.clone(),
                format!("{label} requires recovery for {operation}; run empack recover"),
            ))
        }
        OperationOutcome::Completed(ExecutionOutcome::FailedBeforePublication(_)) => Err(
            retained_failure(outcome.clone(), format!("{label} was not applied")),
        ),
        OperationOutcome::Completed(ExecutionOutcome::ExecutionUncertain(_)) => {
            Err(retained_failure(
                outcome.clone(),
                format!("{label} outcome is uncertain; inspect recovery before retrying"),
            ))
        }
        OperationOutcome::Completed(ExecutionOutcome::PartiallyCompleted { receipt, .. }) => {
            if let Ok(message) = completed(receipt) {
                session.display().status().warning(&message);
            }
            Err(retained_failure(
                outcome.clone(),
                format!("{label} completed only part of its displayed effects"),
            ))
        }
        OperationOutcome::Completed(ExecutionOutcome::NeedsInput(requirements)) => {
            build::save_execution_input(session, engine, requirements).await
        }
        OperationOutcome::Failed(_) => Err(retained_failure(
            outcome.clone(),
            format!("{label} worker failed; inspect recovery before retrying"),
        )),
    };
    engine.release_completed(handle.id());
    result
}

// The operation registry can release its entry while the reported error still owns
// the original typed cause and its retained outcome.
struct RetainedExecutionError {
    message: String,
    outcome: std::sync::Arc<OperationOutcome<ExecutionOutcome>>,
}
impl std::fmt::Debug for RetainedExecutionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("RetainedExecutionError")
            .field(&self.message)
            .finish()
    }
}
impl std::fmt::Display for RetainedExecutionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for RetainedExecutionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &*self.outcome {
            OperationOutcome::Completed(
                ExecutionOutcome::FailedBeforePublication(cause)
                | ExecutionOutcome::ExecutionUncertain(cause)
                | ExecutionOutcome::RecoveryRequired { cause, .. }
                | ExecutionOutcome::PartiallyCompleted { cause, .. },
            ) => Some(cause.as_ref()),
            OperationOutcome::Failed(cause) => Some(cause),
            _ => None,
        }
    }
}
fn retained_failure(
    outcome: std::sync::Arc<OperationOutcome<ExecutionOutcome>>,
    message: String,
) -> anyhow::Error {
    let diagnostic = match &*outcome {
        OperationOutcome::Completed(outcome) => outcome.diagnostic(),
        OperationOutcome::Failed(_) => {
            use crate::engine::diagnostics::*;
            let mut diagnostic = Diagnostic::new(
                DiagnosticCode::ExecutionUncertain,
                DiagnosticPhase::Execution,
            );
            diagnostic.recovery = RecoveryClassification::InspectRecovery;
            Some(diagnostic)
        }
    };
    let error = anyhow::Error::new(RetainedExecutionError { message, outcome });
    match diagnostic {
        Some(diagnostic) => error.context(diagnostic),
        None => error,
    }
}

#[cfg(test)]
pub(super) mod tests;

mod initialize;
pub use initialize::initialize;

mod build;
pub use build::{BuildDecisions, build, build_with_local_files, continue_build};

mod dependencies;
pub use dependencies::{
    AddHostInput, add, add_providers, adopt_observed, remove, synchronize, update,
};

mod cleanup;
pub use cleanup::clean;

mod import;
pub use import::{ImportHostRequest, ImportSource, import};

/// Keep service workers and their retained results under the host's shared admission budget.
async fn scoped<T, F, Fut>(session: &dyn Session, governor: ResourceGovernor, work: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce(crate::engine::runtime::WorkScope) -> Fut + Send + 'static,
    Fut: Future<Output = Result<T>> + Send + 'static,
{
    let runtime = crate::engine::runtime::OperationRuntime::new(governor, 1);
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let mut handle = runtime.start(move |scope| async move {
        let _ = sender.send(work(scope).await);
        Ok(())
    })?;
    let result = cancellable(session, async {
        match &*handle.wait().await {
            OperationOutcome::Completed(()) => {
                receiver.await.context("Host service result was lost")?
            }
            OperationOutcome::Failed(error) => Err(error.clone().into()),
        }
    })
    .await;
    runtime.shutdown().await;
    result
}

mod files;
pub use files::add_files;

pub mod cli;

pub(super) mod instance;
pub(super) mod release;
