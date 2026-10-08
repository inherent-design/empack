//! Recorded synchronization uses captured plans, explicit replacement and retained engine outcomes.
use super::*;
use crate::engine::{
    mrpack::LockedFileKey,
    publication::{Publisher, RecoveryRequired},
    runtime::WorkScope,
    synchronization::{self as native_sync, PreparedSynchronization},
};
use empack_core::{
    files::{FileChange, FilePlan, ObservedPath},
    model::{DependencyKey, ResolvedProject},
};
use std::collections::BTreeSet;

/// Exact per-slot materialization, optionally paired with resolution for changed authoring intent.
pub enum SyncRequest {
    /// Use explicit per-slot content choices, retaining their acquisition evidence.
    Supplied {
        resolution: Option<ResolvedProject>,
        content: DependencyContents,
    },
    /// Combine captured local/member sources with externally verified remote bytes.
    /// These bytes cannot replace a local source or an unselected slot.
    AcquiredReferences {
        source_revision: Option<crate::engine::project::ProjectRevision>,
        resolution: Option<ResolvedProject>,
        evidence: SourceEvidencePolicy,
        content: BTreeMap<LockedFileKey, crate::engine::mrpack::AcquiredBuildFile>,
    },
    /// Restore captured local/member sources and preserve exact deferred references.
    /// Changed semantic intent still requires explicit fresh resolution.
    Recorded {
        resolution: Option<ResolvedProject>,
        evidence: SourceEvidencePolicy,
    },
}
#[derive(Clone)]
pub struct SyncPreview {
    pub plan: PlanId,
    pub selected: Vec<DependencyKey>,
    pub rebinds_lock: bool,
    pub files: FilePlan,
    pub replacement: ReplacementSummary,
    pub references: BTreeSet<LockedFileKey>,
}
pub struct SyncReceipt {
    pub plan: PlanId,
    pub publication: PublicationReceipt,
    pub project: ResolvedProject,
    pub references: BTreeSet<LockedFileKey>,
}
pub(super) struct PreparedSynchronizationOperation {
    pub(super) view: SyncPreview,
    synchronization: PreparedSynchronization,
}
pub(super) async fn prepare(
    project: ProjectTarget,
    request: SyncRequest,
    config: &EngineConfig,
    scope: &mut WorkScope,
) -> Result<RetainedOutput<PreparedSynchronizationOperation>> {
    let ProjectTarget::Existing(project) = project else {
        anyhow::bail!("Synchronization requires an existing project")
    };
    ensure!(project.is_absolute(), "Project selection must be absolute");
    let (request, acquired, source_revision) = match request {
        SyncRequest::AcquiredReferences {
            source_revision,
            resolution,
            evidence,
            content,
        } => (
            SyncRequest::Recorded {
                resolution,
                evidence,
            },
            content,
            source_revision,
        ),
        request => (request, BTreeMap::new(), None),
    };
    let planned = match request {
        SyncRequest::AcquiredReferences { .. } => unreachable!("normalized above"),
        SyncRequest::Supplied {
            resolution,
            content,
        } => {
            let state = config.state_root.clone();
            let limits = config.snapshot;
            let work = scope.spawn_blocking(
                config.resources.capture,
                config.resources.prepared,
                move |cancel| {
                    let snapshot = ProjectReader::new(RecoveryReader::new(state))
                        .capture_synchronization_with_resolution(
                            &project,
                            resolution.as_ref(),
                            limits,
                            &cancel,
                        )?;
                    native_sync::plan_synchronization_with_resolution(
                        snapshot,
                        content,
                        resolution.as_ref(),
                        &cancel,
                    )
                },
            )?;
            scope.accept(work.wait().await?)?.transpose()?
        }
        SyncRequest::Recorded {
            resolution,
            evidence,
        } => {
            let state = config.state_root.clone();
            let limits = config.snapshot;
            let archive = config.archive;
            let work = scope.spawn_blocking(
                config.resources.capture,
                config.resources.prepared,
                move |cancel| {
                    let snapshot = ProjectReader::new(RecoveryReader::new(state))
                        .capture_recorded_synchronization(
                            &project,
                            resolution.as_ref(),
                            limits,
                            &cancel,
                        )?;
                    snapshot.require_revision(source_revision)?;
                    let memory = native_sync::recorded::RecordedInputs::metadata_memory(
                        &snapshot,
                        resolution.as_ref(),
                        archive,
                    )?;
                    Ok::<_, anyhow::Error>((snapshot, resolution, memory))
                },
            )?;
            let captured = scope.accept(work.wait().await?)?.transpose()?;
            let mut resources = config.resources.capture;
            resources.memory_bytes = resources.memory_bytes.max(captured.2);
            let work =
                scope.spawn_blocking(resources, config.resources.prepared, move |cancel| {
                    let ((snapshot, resolution, _), _reservation) = captured.into_parts();
                    native_sync::recorded::RecordedInputs::new(
                        snapshot, resolution, archive, &cancel,
                    )
                })?;
            let inputs = scope.accept(work.wait().await?)?.transpose()?;
            let mut work_resources = config.resources.local_acquisition;
            work_resources.scratch_bytes = work_resources.scratch_bytes.max(inputs.bytes());
            work_resources.memory_bytes = work_resources.memory_bytes.max(inputs.memory()?);
            work_resources.open_files = work_resources.open_files.max(inputs.open_files());
            let mut retained = config.resources.prepared;
            retained.scratch_bytes = inputs.retained_bytes();
            retained.open_files = retained.open_files.max(inputs.open_files());
            let work = scope.spawn_blocking(work_resources, retained, move |cancel| {
                let (inputs, _reservation) = inputs.into_parts();
                inputs.prepare_with_references(evidence, acquired, &cancel)
            })?;
            scope.accept(work.wait().await?)?.transpose()?
        }
    };
    let (resources, retained) = project_change::resources(planned.bytes()?, config)?;
    let work = scope.spawn_blocking(resources, retained, move |cancel| {
        let (planned, _reservation) = planned.into_parts();
        let synchronization = planned.stage(&cancel)?;
        let files = synchronization.files().clone();
        let view = SyncPreview {
            plan: PlanId(
                NEXT_PLAN
                    .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
                    .map_err(|_| anyhow::anyhow!("Plan identifier exhausted"))?,
            ),
            selected: synchronization
                .candidate()
                .project()
                .lock()
                .dependencies
                .keys()
                .cloned()
                .collect(),
            references: synchronization.references().clone(),
            rebinds_lock: !synchronization.candidate().preserves_lock_document(),
            replacement: project_change::summary(&files)?,
            files,
        };
        Ok::<_, anyhow::Error>(PreparedSynchronizationOperation {
            view,
            synchronization,
        })
    })?;
    scope.accept(work.wait().await?)?.transpose()
}
pub(super) async fn run(
    prepared: RetainedOutput<PreparedSynchronizationOperation>,
    config: EngineConfig,
    mut scope: WorkScope,
) -> Result<ExecutionOutcome, RuntimeError> {
    let cancel = scope.cancellation();
    let result = execute(prepared, config, &mut scope).await;
    Ok(match result {
        Ok(receipt) => ExecutionOutcome::Completed(ExecutionReceipt::Sync(Box::new(receipt))),
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
    prepared: RetainedOutput<PreparedSynchronizationOperation>,
    config: EngineConfig,
    scope: &mut WorkScope,
) -> Result<RetainedOutput<SyncReceipt>> {
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
            .context("Synchronization publication size overflow")?;
            sum.checked_add(bytes)
                .context("Synchronization publication size overflow")
        })?;
    let mut resources = config.resources.assembly;
    resources.scratch_bytes = resources.scratch_bytes.max(bytes);
    let work = scope.spawn_blocking(resources, config.resources.receipt, move |cancel| {
        cancel.check()?;
        let (prepared, _reservation) = prepared.into_parts();
        let receipt = prepared
            .synchronization
            .publish(&Publisher::open(&config.state_root)?, &cancel)?;
        Ok::<_, anyhow::Error>(SyncReceipt {
            plan: prepared.view.plan,
            publication: receipt.publication,
            project: receipt.project,
            references: prepared.view.references,
        })
    })?;
    scope
        .accept_publication(work.wait().await.map_err(PublicationWorkerFailed)?)?
        .transpose()
}
