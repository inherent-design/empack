//! Native consumer export through the same owned preparation, approval and publication lifecycle.
use super::*;
use crate::engine::{
    build::{PreparedArtifact, prepare_archive_publication},
    release::producer,
    runtime::WorkScope,
    snapshot::Observation,
    staging::PrivateFile,
};

pub struct NativeExportRequest {
    pub artifact: PortableRelPath,
    pub archive: Option<DistributionArchive>,
}
#[derive(Clone)]
pub struct NativeExportPreview {
    pub replacement: ReplacementSummary,
    pub plan: PlanId,
    pub artifact: PortableRelPath,
    pub release: String,
    pub files: usize,
    pub bytes: u64,
}
pub struct NativeExportReceipt {
    pub preview: NativeExportPreview,
    pub publication: PublicationReceipt,
}
pub(super) struct PreparedNativeExport {
    pub(super) view: NativeExportPreview,
    artifact: PreparedArtifact,
}
pub(super) async fn prepare(
    target: ProjectTarget,
    request: NativeExportRequest,
    config: &EngineConfig,
    scope: &mut WorkScope,
) -> Result<RetainedOutput<PreparedNativeExport>> {
    let ProjectTarget::Existing(root) = target else {
        anyhow::bail!("Native export requires an existing author project");
    };
    let state = config.state_root.clone();
    let limits = config.snapshot;
    let artifact = request.artifact.clone();
    let work = scope.spawn_blocking(
        config.resources.capture,
        config.resources.prepared,
        move |cancel| {
            ProjectReader::new(RecoveryReader::new(state))
                .capture_native_release(&root, &artifact, limits, &cancel)
        },
    )?;
    let captured = scope.accept(work.wait().await?)?.transpose()?;
    let source_bytes = captured
        .observations()
        .entries()
        .values()
        .try_fold(0u64, |sum, item| {
            sum.checked_add(match item {
                Observation::File(file) => file.bytes,
                _ => 0,
            })
            .context("Native export capture size overflow")
        })?;
    // Content, staged assets, archive and publication copies overlap during assembly.
    let estimate = source_bytes
        .checked_add(
            config
                .archive
                .file_bytes
                .min(crate::engine::release::MAX_RELEASE_BYTES as u64),
        )
        .and_then(|n| n.checked_mul(4))
        .context("Native export scratch estimate overflow")?;
    let (resources, retained) = project_change::resources(estimate, config)?;
    let archive_limits = config.archive;
    let work = scope.spawn_blocking(resources, retained, move |cancel| {
        let (workspace, _reservation) = captured.into_parts();
        let project = workspace.require_resolved()?;
        let format = request
            .archive
            .unwrap_or(project.intent().distribution.archive);
        let plan = producer::capture(&workspace, &cancel)?;
        let mut output = PrivateFile::new()?;
        let verified = plan.write(output.file(), format, archive_limits, &cancel)?;
        let artifact = prepare_archive_publication(
            workspace,
            request.artifact.clone(),
            output,
            &verified,
            &cancel,
        )?;
        let view = NativeExportPreview {
            replacement: project_change::summary(artifact.plan())?,
            plan: PlanId(
                NEXT_PLAN
                    .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
                    .map_err(|_| anyhow::anyhow!("Plan identifier exhausted"))?,
            ),
            artifact: request.artifact.clone(),
            release: plan.release().id().into(),
            files: plan.release().document().files.len(),
            bytes: verified.len(),
        };
        Ok::<_, anyhow::Error>(PreparedNativeExport { view, artifact })
    })?;
    scope.accept(work.wait().await?)?.transpose()
}
pub(super) async fn run(
    prepared: RetainedOutput<PreparedNativeExport>,
    config: EngineConfig,
    scope: WorkScope,
) -> Result<ExecutionOutcome, RuntimeError> {
    let cancel = scope.cancellation();
    let result = async {
        let mut resources = config.resources.assembly;
        resources.scratch_bytes = resources.scratch_bytes.max(
            prepared
                .view
                .bytes
                .checked_mul(2)
                .context("Native publication size overflow")?,
        );
        let work = scope.spawn_blocking(resources, config.resources.receipt, move |cancel| {
            let (prepared, _reservation) = prepared.into_parts();
            let publication = prepared.artifact.publish(
                &crate::engine::publication::Publisher::open(&config.state_root)?,
                &cancel,
            )?;
            Ok::<_, anyhow::Error>(NativeExportReceipt {
                preview: prepared.view,
                publication,
            })
        })?;
        scope
            .accept_publication(work.wait().await.map_err(PublicationWorkerFailed)?)?
            .transpose()
    }
    .await;
    Ok(match result {
        Ok(receipt) => {
            ExecutionOutcome::Completed(ExecutionReceipt::NativeExport(Box::new(receipt)))
        }
        Err(error) => ExecutionOutcome::failed(error, cancel.is_cancelled()),
    })
}
