//! Artifact cleanup owns only the inspected dist namespace and uses ordinary publication.
use super::*;
use crate::engine::{
    publication::{Publisher, RecoveryRequired},
    runtime::WorkScope,
    snapshot::ProjectReadRoot,
    staging::MutableStage,
    verification::{self, VerifiedFileChange},
};
use empack_core::files::{FileChange, FilePlan, ManagedPath};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy)]
pub enum CleanRequest {
    /// Remove regular files in the managed artifact namespace, retaining empty directories.
    Artifacts,
}
#[derive(Clone)]
pub struct CleanPreview {
    pub plan: PlanId,
    pub files: FilePlan,
    /// Logical bytes removed from dist; recovery preimages have separate retention.
    pub removed_bytes: u64,
    pub replacement: ReplacementSummary,
}
pub struct CleanReceipt {
    pub plan: PlanId,
    pub publication: PublicationReceipt,
    pub removed_bytes: u64,
}
pub(super) struct PreparedCleanup {
    pub(super) view: CleanPreview,
    root: ProjectReadRoot,
    verified: VerifiedFileChange,
}
pub(super) async fn prepare(
    target: ProjectTarget,
    request: CleanRequest,
    config: &EngineConfig,
    scope: &mut WorkScope,
) -> Result<RetainedOutput<PreparedCleanup>> {
    let ProjectTarget::Existing(project) = target else {
        anyhow::bail!("Artifact cleanup requires an existing selected root");
    };
    ensure!(project.is_absolute(), "Project selection must be absolute");
    let state = config.state_root.clone();
    let limits = config.snapshot;
    let work = scope.spawn_blocking(
        config.resources.capture,
        config.resources.prepared,
        move |cancel| {
            cancel.check()?;
            let root = ProjectReadRoot::open(&project)?;
            let _guard = RecoveryReader::new(state).enter(&root)?;
            let selected = match request {
                CleanRequest::Artifacts => {
                    PortableRelPath::parse("dist", PathSyntax::ProjectContent)?
                }
            };
            let captured = root.capture(&[selected], limits, &cancel)?;
            let observed = verification::observed_files(&captured)?;
            ensure!(
                observed
                    .keys()
                    .all(|path| matches!(path, ManagedPath::Artifact(_))),
                "Cleanup escaped the artifact namespace"
            );
            let removals = observed.keys().cloned().collect();
            let files = verification::plan_files(&observed, &BTreeMap::new(), &removals)?;
            let removed_bytes = files.changes().iter().try_fold(0u64, |total, change| {
                let FileChange::Remove { before, .. } = change else {
                    anyhow::bail!("Cleanup may only remove artifact files");
                };
                total
                    .checked_add(before.bytes)
                    .context("Artifact cleanup size overflow")
            })?;
            let stage = MutableStage::empty()?.freeze(limits, &cancel)?;
            let verified = VerifiedFileChange::verify_artifacts(captured, files.clone(), stage)?;
            let view = CleanPreview {
                plan: PlanId(
                    NEXT_PLAN
                        .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
                        .map_err(|_| anyhow::anyhow!("Plan identifier exhausted"))?,
                ),
                replacement: project_change::summary(&files)?,
                files,
                removed_bytes,
            };
            Ok::<_, anyhow::Error>(PreparedCleanup {
                view,
                root,
                verified,
            })
        },
    )?;
    scope.accept(work.wait().await?)?.transpose()
}
pub(super) async fn run(
    prepared: RetainedOutput<PreparedCleanup>,
    config: EngineConfig,
    scope: WorkScope,
) -> Result<ExecutionOutcome, RuntimeError> {
    let cancel = scope.cancellation();
    let result: Result<RetainedOutput<CleanReceipt>> = async {
        // Retain before-images for crash recovery. Do not claim these bytes were globally reclaimed.
        let mut resources = config.resources.assembly;
        resources.scratch_bytes = resources.scratch_bytes.max(prepared.view.removed_bytes);
        let work = scope.spawn_blocking(resources, config.resources.receipt, move |cancel| {
            cancel.check()?;
            let (prepared, _reservation) = prepared.into_parts();
            let publication = Publisher::open(&config.state_root)?.publish(
                &prepared.root,
                prepared.verified,
                &cancel,
            )?;
            Ok::<_, anyhow::Error>(CleanReceipt {
                plan: prepared.view.plan,
                publication,
                removed_bytes: prepared.view.removed_bytes,
            })
        })?;
        scope
            .accept_publication(work.wait().await.map_err(PublicationWorkerFailed)?)?
            .transpose()
    }
    .await;
    Ok(match result {
        Ok(receipt) => ExecutionOutcome::Completed(ExecutionReceipt::Clean(Box::new(receipt))),
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

#[cfg(test)]
mod tests;
