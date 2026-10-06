//! Import uses the same engine ownership, grants and retained terminal outcomes as builds.
use super::*;
use crate::engine::{
    import::{
        ImportCandidate, ImportReplacementPolicy, PreparedImportCreation,
        PreparedImportReplacement, prepare_import_creation, prepare_import_replacement,
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
    replacement: NativeImport,
}
enum NativeImport {
    Existing(PreparedImportReplacement),
    New(PreparedImportCreation),
}
impl NativeImport {
    fn plan(&self) -> &FilePlan {
        match self {
            Self::Existing(value) => value.plan(),
            Self::New(value) => value.plan(),
        }
    }
    fn project(&self) -> &empack_core::model::ResolvedProject {
        match self {
            Self::Existing(value) => value.project(),
            Self::New(value) => value.project(),
        }
    }
    fn publish(
        self,
        publisher: &Publisher,
        cancel: &crate::application::process_runtime::Cancellation,
    ) -> Result<crate::engine::import::ImportReplacementReceipt> {
        match self {
            Self::Existing(value) => value.publish(publisher, cancel),
            Self::New(value) => value.publish(publisher, cancel),
        }
    }
}
enum CapturedImport {
    Existing(crate::engine::project::ReplacementSnapshot),
    New(crate::engine::project::NewProjectSnapshot),
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
    project: ProjectTarget,
    request: ImportRequest,
    config: &EngineConfig,
    scope: &mut WorkScope,
) -> Result<RetainedOutput<PreparedImport>> {
    let host_state = config.state_root.clone();
    let limits = config.snapshot;
    let work = scope.spawn_blocking(
        config.resources.capture,
        config.resources.prepared,
        move |cancel| {
            let reader = ProjectReader::new(RecoveryReader::new(host_state));
            match project {
                ProjectTarget::Existing(project) => {
                    ensure!(project.is_absolute(), "Project selection must be absolute");
                    reader
                        .capture_replacement(&project, limits, &cancel)
                        .map(CapturedImport::Existing)
                }
                ProjectTarget::New(project) => reader
                    .capture_new(&project, &cancel)
                    .map(CapturedImport::New),
            }
        },
    )?;
    let snapshot = scope.accept(work.wait().await?)?.transpose()?;
    let policy_bytes = match &*snapshot {
        CapturedImport::Existing(value) => value
            .preserved_policy()
            .map_or(0, |(bytes, _)| bytes.len() as u64),
        CapturedImport::New(_) => 0,
    };
    let bytes = request
        .candidate
        .publication_bytes()
        .checked_add(policy_bytes)
        .context("Import staging size overflow")?;
    let (resources, retained) = resources(bytes, config)?;
    let work = scope.spawn_blocking(resources, retained, move |cancel| {
        let (snapshot, _reservation) = snapshot.into_parts();
        let replacement = match snapshot {
            CapturedImport::Existing(snapshot) => {
                NativeImport::Existing(prepare_import_replacement(
                    snapshot,
                    request.candidate,
                    request.replacement,
                    &cancel,
                )?)
            }
            CapturedImport::New(snapshot) => NativeImport::New(prepare_import_creation(
                snapshot,
                request.candidate,
                limits,
                &cancel,
            )?),
        };
        describe(replacement)
    })?;
    scope.accept(work.wait().await?)?.transpose()
}
fn describe(replacement: NativeImport) -> Result<PreparedImport> {
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
    let publication_bytes =
        prepared
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
                .context("Publication size overflow")?;
                sum.checked_add(bytes).context("Publication size overflow")
            })?;
    let mut publication_resources = config.resources.assembly;
    publication_resources.scratch_bytes =
        publication_resources.scratch_bytes.max(publication_bytes);
    let work = scope.spawn_blocking(
        publication_resources,
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
