//! Import uses the same engine ownership, grants and retained terminal outcomes as builds.
use super::*;
use crate::engine::{
    import::{
        ImportCandidate, ImportReplacementPolicy, PreparedImportReplacement,
        prepare_import_replacement,
    },
    publication::{Publisher, RecoveryRequired},
    runtime::WorkScope,
};
use empack_core::{
    files::{FileChange, FileContent, FilePlan, ObservedPath},
    model::PackMetadata,
};
use sha2::{Digest, Sha256};

pub struct ImportRequest {
    /// Acquired, interpreted bytes; source resolution itself has no project write authority.
    pub candidate: ImportCandidate,
    pub replacement: ImportReplacementPolicy,
}
/// A stable digest of this exact native file-change summary, not a reusable deletion capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReplacementSummary([u8; 32]);
#[derive(Clone)]
pub struct ImportPreview {
    pub plan: PlanId,
    pub metadata: PackMetadata,
    pub runtime: RuntimeResolution,
    pub files: FilePlan,
    pub replacement: Option<ReplacementSummary>,
}
pub struct ImportReceipt {
    pub plan: PlanId,
    pub publication: PublicationReceipt,
    pub project: empack_core::model::ResolvedProject,
}
pub(super) struct PreparedImport {
    pub(super) view: ImportPreview,
    replacement: PreparedImportReplacement,
}
fn resources(bytes: u64, config: &EngineConfig) -> Result<(ResourceRequest, ResourceRequest)> {
    let mut retained = config.resources.prepared;
    retained.scratch_bytes = bytes;
    let mut resources = config.resources.assembly;
    resources.scratch_bytes = resources.scratch_bytes.max(
        bytes
            .checked_mul(2)
            .ok_or_else(|| anyhow::anyhow!("Import staging size overflow"))?,
    );
    // Frozen packed storage retains one descriptor plus the captured root handle.
    retained.open_files = retained.open_files.max(2);
    Ok((resources, retained))
}
pub(super) async fn prepare(
    project: PathBuf,
    request: ImportRequest,
    config: &EngineConfig,
    scope: &mut WorkScope,
) -> Result<RetainedOutput<PreparedImport>> {
    ensure!(project.is_absolute(), "Project selection must be absolute");
    let host_state = config.state_root.clone();
    let limits = config.snapshot;
    let work = scope.spawn_blocking(
        config.resources.capture,
        config.resources.prepared,
        move |cancel| {
            ProjectReader::new(RecoveryReader::new(host_state))
                .capture_replacement(&project, limits, &cancel)
        },
    )?;
    let snapshot = scope.accept(work.wait().await?)?.transpose()?;
    let bytes = request
        .candidate
        .publication_bytes()
        .checked_add(
            snapshot
                .preserved_policy()
                .map_or(0, |(bytes, _)| bytes.len() as u64),
        )
        .context("Import staging size overflow")?;
    let (resources, retained) = resources(bytes, config)?;
    let work = scope.spawn_blocking(resources, retained, move |cancel| {
        let (snapshot, _reservation) = snapshot.into_parts();
        let replacement =
            prepare_import_replacement(snapshot, request.candidate, request.replacement, &cancel)?;
        describe(replacement)
    })?;
    scope.accept(work.wait().await?)?.transpose()
}
fn describe(replacement: PreparedImportReplacement) -> Result<PreparedImport> {
    let files = replacement.plan().clone();
    let replaces = files.changes().iter().any(|change| {
        matches!(
            change,
            FileChange::Remove { .. }
                | FileChange::Replace {
                    before: ObservedPath::File(_),
                    ..
                }
        )
    });
    let view = ImportPreview {
        plan: PlanId(
            NEXT_PLAN
                .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
                .map_err(|_| anyhow::anyhow!("Plan identifier exhausted"))?,
        ),
        metadata: replacement.project().intent().metadata.clone(),
        runtime: replacement.project().lock().runtime.clone(),
        replacement: replaces.then(|| summary(&files)).transpose()?,
        files,
    };
    Ok(PreparedImport { view, replacement })
}
fn summary(plan: &FilePlan) -> Result<ReplacementSummary> {
    fn content(hash: &mut Sha256, value: &FileContent) {
        hash.update(value.content.bytes());
        hash.update(value.bytes.to_le_bytes());
        hash.update([
            u8::from(value.permissions.readonly),
            u8::from(value.permissions.executable),
        ]);
    }
    let mut hash = Sha256::new();
    hash.update(b"empack.import-replacement.v1");
    for change in plan.changes() {
        let path = crate::engine::layout::ProjectLayout::path(change.target())?;
        hash.update((path.as_str().len() as u64).to_le_bytes());
        hash.update(path.as_str().as_bytes());
        match change {
            FileChange::Replace { before, after, .. } => {
                hash.update([0]);
                match before {
                    ObservedPath::Absent => hash.update([0]),
                    ObservedPath::File(value) => {
                        hash.update([1]);
                        content(&mut hash, value);
                    }
                    ObservedPath::Directory => anyhow::bail!("Import cannot replace a directory"),
                }
                content(&mut hash, after);
            }
            FileChange::Remove { before, .. } => {
                hash.update([1]);
                content(&mut hash, before);
            }
        }
    }
    Ok(ReplacementSummary(hash.finalize().into()))
}
pub(super) async fn run(
    prepared: RetainedOutput<PreparedImport>,
    config: EngineConfig,
    mut scope: WorkScope,
) -> Result<ExecutionOutcome, RuntimeError> {
    let cancel = scope.cancellation();
    let result = execute(prepared, config, &mut scope).await;
    Ok(match result {
        Ok(receipt) => ExecutionOutcome::Completed(ExecutionReceipt::Import(Box::new(receipt))),
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
    prepared: RetainedOutput<PreparedImport>,
    config: EngineConfig,
    scope: &mut WorkScope,
) -> Result<RetainedOutput<ImportReceipt>> {
    let work = scope.spawn_blocking(
        config.resources.assembly,
        config.resources.receipt,
        move |cancel| {
            cancel.check()?;
            let (prepared, _reservation) = prepared.into_parts();
            let receipt = prepared
                .replacement
                .publish(&Publisher::open(&config.state_root)?, &cancel)?;
            Ok::<_, anyhow::Error>(ImportReceipt {
                plan: prepared.view.plan,
                publication: receipt.publication,
                project: receipt.project,
            })
        },
    )?;
    let result = work.wait().await.map_err(PublicationWorkerFailed)?;
    scope.accept_publication(result)?.transpose()
}
