//! Semantic build lifecycle. Preparation has read-only project authority; only an approved,
//! engine-bound plan can admit acquisition, trusted tools and verified publication.
use super::{
    acquisition::{HttpAcquisition, TransferLimits},
    artifacts::ArchiveLimits,
    build::{
        BuildAcquisitions,
        acquisition::{
            AcquisitionKey, BuildAcquisitionPlan, BuildContentSource,
            plan_target_build_acquisitions,
        },
    },
    content::SourceEvidencePolicy,
    layout::CollisionIndex,
    mrpack::OptionalConversion,
    packwiz::InstallerInteraction,
    project::{ProjectReader, WorkspaceSnapshot},
    providers::{CatalogLimits, ProviderAvailability, ProviderCatalog},
    publication::{PublicationReceipt, RecoveryReader},
    resources::{ResourceGovernor, ResourceRequest},
    runtime::{
        OperationHandle, OperationId, OperationOutcome, OperationRuntime, RetainedOutput,
        RuntimeError,
    },
    server_runtime::installer::InstallerExecution,
    snapshot::{Observation, SnapshotLimits},
    templates::TemplateOptions,
};
use anyhow::{Context, Result, ensure};
use empack_core::{
    inventory::OptionalPolicy,
    model::{DistributionArchive, ExpectedContent, LoaderKind, NonEmpty, RuntimeResolution},
    path::{PathSyntax, PortableRelPath},
    projection::BuildTarget,
};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};
use tokio::sync::oneshot;

mod execution;
static NEXT_PLAN: AtomicU64 = AtomicU64::new(1);

/// In-process identity for one immutable captured plan; deliberately not deserializable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlanId(u64);
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildOutput {
    pub target: BuildTarget,
    pub artifact: PortableRelPath,
}
#[derive(Clone)]
pub struct BuildRequest {
    pub outputs: NonEmpty<BuildOutput>,
    pub archive: DistributionArchive,
    pub optional: OptionalPolicy,
    pub mrpack_optional: OptionalConversion,
    pub templates: TemplateOptions,
    pub evidence: SourceEvidencePolicy,
    pub interaction: InstallerInteraction,
}
/// Host estimates used for admission, separate from actual stream and snapshot limits.
#[derive(Clone, Copy)]
pub struct BuildResources {
    pub capture: ResourceRequest,
    pub prepared: ResourceRequest,
    pub local_acquisition: ResourceRequest,
    pub acquired: ResourceRequest,
    pub assembly: ResourceRequest,
    pub receipt: ResourceRequest,
}
#[derive(Clone)]
pub struct EngineConfig {
    pub state_root: PathBuf,
    pub resources: BuildResources,
    pub snapshot: SnapshotLimits,
    pub archive: ArchiveLimits,
    pub transfer: TransferLimits,
    pub installer: InstallerExecution,
    pub retained_operations: usize,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkPermission {
    Offline,
    Allow,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentRequirementKind {
    Download,
    ProviderLookup,
    Embedded,
    Manual,
}
#[derive(Debug, Clone)]
pub struct ContentRequirement {
    pub key: AcquisitionKey,
    pub expected: ExpectedContent,
    pub kind: ContentRequirementKind,
}
/// Read-only effect summary. No URLs, content leases, publisher or conversion into approval.
#[derive(Clone)]
pub struct BuildPreview {
    pub plan: PlanId,
    pub outputs: Vec<BuildOutput>,
    pub runtime: RuntimeResolution,
    pub content: Vec<ContentRequirement>,
    pub needs_network: bool,
    pub runs_installer: bool,
    pub unresolved: Vec<AcquisitionKey>,
    options: BuildRequest,
}
impl BuildPreview {
    /// All requested options, including conversions and template values, for host presentation.
    /// These values are deliberately not included in an automatic Debug rendering.
    pub fn request(&self) -> &BuildRequest {
        &self.options
    }
}
struct PreparedBuild {
    view: BuildPreview,
    workspace: WorkspaceSnapshot,
    request: BuildRequest,
    acquisition: BuildAcquisitionPlan,
}
pub struct PreparedOperation {
    owner: Arc<()>,
    data: Box<RetainedOutput<PreparedBuild>>,
}
pub struct ApprovedOperation {
    prepared: PreparedOperation,
}
pub enum Preparation {
    Ready(PreparedOperation),
    NeedsInput(Box<BuildPreview>),
}
#[derive(Debug, Clone, Copy)]
pub struct ExecutionGrant {
    pub plan: PlanId,
    pub network: NetworkPermission,
    pub run_installer: bool,
}
impl PreparedOperation {
    pub fn view(&self) -> &BuildPreview {
        &self.data.view
    }
    pub fn authorize(self, grant: ExecutionGrant) -> Result<ApprovedOperation> {
        ensure!(
            grant.plan == self.data.view.plan,
            "Execution grant belongs to another plan"
        );
        ensure!(
            !self.data.view.needs_network || grant.network == NetworkPermission::Allow,
            "Build requires network authorization"
        );
        ensure!(
            !self.data.view.runs_installer || grant.run_installer,
            "Build requires trusted installer authorization"
        );
        Ok(ApprovedOperation { prepared: self })
    }
}
/// Accurate publication outcome: a failed preparation and a hot durable journal are distinct.
pub enum BuildOutcome {
    Completed(RetainedOutput<BuildReceipt>),
    NeedsInput(Vec<ContentRequirement>),
    FailedBeforePublication(anyhow::Error),
    InterruptedBeforePublication,
    /// A publication worker failed without returning its durable-state classification.
    /// Inspect recovery before deciding whether the operation can be retried.
    ExecutionUncertain(anyhow::Error),
    RecoveryRequired {
        operation: String,
        cause: anyhow::Error,
    },
}
pub struct BuildReceipt {
    pub plan: PlanId,
    pub publication: PublicationReceipt,
    pub artifacts: Vec<super::build::batch::BuiltDistribution>,
}
/// Owned build operations over captured projects. The host authorizes the displayed plan.
///
/// ```no_run
/// # use empack_lib::engine::api::*;
/// # use std::path::PathBuf;
/// # async fn example(engine: &Engine, project: PathBuf, request: BuildRequest) -> anyhow::Result<()> {
/// let prepared = match engine.prepare(project, request).await? {
///     Preparation::Ready(value) => value,
///     Preparation::NeedsInput(_) => anyhow::bail!("Resolve the reported content obligations"),
/// };
/// // The host presents prepared.view() and explicitly permits these effects.
/// let grant = ExecutionGrant {
///     plan: prepared.view().plan,
///     network: NetworkPermission::Allow,
///     run_installer: true,
/// };
/// let mut operation = engine.start(prepared.authorize(grant)?)?;
/// let outcome = operation.wait().await;
/// // Inspect the retained outcome; starting an operation is not a completion receipt.
/// # drop(outcome);
/// # Ok(())
/// # }
/// ```
pub struct Engine {
    owner: Arc<()>,
    config: EngineConfig,
    transport: HttpAcquisition,
    catalog: Option<(ProviderCatalog, CatalogLimits)>,
    preparations: OperationRuntime<()>,
    operations: OperationRuntime<BuildOutcome>,
}
impl Engine {
    /// Construction neither creates host state nor bootstraps tooling.
    pub fn new(config: EngineConfig, governor: ResourceGovernor) -> Result<Self> {
        ensure!(
            config.state_root.is_absolute(),
            "Host state root must be absolute"
        );
        Ok(Self {
            owner: Arc::new(()),
            transport: HttpAcquisition::new()?,
            catalog: None,
            preparations: OperationRuntime::new(governor.clone(), config.retained_operations),
            operations: OperationRuntime::new(governor, config.retained_operations),
            config,
        })
    }
    /// Attach read-only provider resolution for authorized execution. Preparation receives only
    /// its availability description, never the network client or credentials.
    pub fn with_provider_catalog(
        mut self,
        catalog: ProviderCatalog,
        limits: CatalogLimits,
    ) -> Self {
        self.catalog = Some((catalog, limits));
        self
    }
    /// Capture and plan only. No persistent cache writer, downloader or process runner enters
    /// this worker. Dropping the future cancels its engine-owned preparation.
    pub async fn prepare(&self, project: PathBuf, request: BuildRequest) -> Result<Preparation> {
        let config = self.config.clone();
        let provider_access = self
            .catalog
            .as_ref()
            .map(|(catalog, _)| catalog.availability())
            .unwrap_or_default();
        let owner = self.owner.clone();
        let (sender, receiver) = oneshot::channel();
        let mut handle =
            self.preparations
                .start_ephemeral(move |mut scope| async move {
                    let work = scope.spawn_blocking(
                        config.resources.capture,
                        config.resources.prepared,
                        move |cancel| capture(project, request, &config, provider_access, &cancel),
                    )?;
                    let prepared = scope.accept(work.wait().await?)?.transpose().map(|data| {
                        PreparedOperation {
                            owner,
                            data: Box::new(data),
                        }
                    });
                    let _ = sender.send(prepared);
                    Ok(())
                })?;
        let outcome = handle.wait().await;
        if let OperationOutcome::Failed(error) = &*outcome {
            return Err(error.clone().into());
        }
        let prepared = receiver
            .await
            .context("Preparation result was not retained")??;
        if prepared.view().unresolved.is_empty() {
            Ok(Preparation::Ready(prepared))
        } else {
            Ok(Preparation::NeedsInput(Box::new(prepared.view().clone())))
        }
    }
    /// Preview has no authority-bearing output even when its plan requires no additional input.
    pub async fn preview(&self, project: PathBuf, request: BuildRequest) -> Result<BuildPreview> {
        Ok(match self.prepare(project, request).await? {
            Preparation::Ready(prepared) => prepared.view().clone(),
            Preparation::NeedsInput(report) => *report,
        })
    }
    pub fn start(&self, approved: ApprovedOperation) -> Result<OperationHandle<BuildOutcome>> {
        ensure!(
            Arc::ptr_eq(&self.owner, &approved.prepared.owner),
            "Prepared operation belongs to another engine"
        );
        let config = self.config.clone();
        let transport = self.transport.clone();
        let catalog = self.catalog.clone();
        Ok(self.operations.start(move |scope| async move {
            execution::run(*approved.prepared.data, config, transport, catalog, scope).await
        })?)
    }
    pub fn observe(&self, id: OperationId) -> Option<OperationHandle<BuildOutcome>> {
        self.operations.observe(id)
    }
    pub fn release_completed(&self, id: OperationId) -> bool {
        self.operations.release_completed(id)
    }
    pub async fn shutdown(&self) {
        tokio::join!(self.preparations.shutdown(), self.operations.shutdown());
    }
}
fn describe(need: &super::build::acquisition::AcquisitionNeed) -> ContentRequirement {
    ContentRequirement {
        key: need.key.clone(),
        expected: need.expected.clone(),
        kind: match need.source {
            BuildContentSource::Download(_) => ContentRequirementKind::Download,
            BuildContentSource::Provider { .. } => ContentRequirementKind::ProviderLookup,
            BuildContentSource::Embedded { .. } => ContentRequirementKind::Embedded,
            BuildContentSource::Manual { .. } => ContentRequirementKind::Manual,
        },
    }
}
fn capture(
    project: PathBuf,
    request: BuildRequest,
    config: &EngineConfig,
    provider_access: ProviderAvailability,
    cancel: &crate::application::process_runtime::Cancellation,
) -> Result<PreparedBuild> {
    ensure!(project.is_absolute(), "Project selection must be absolute");
    let mut collisions = CollisionIndex::default();
    for output in request.outputs.as_slice() {
        PortableRelPath::parse(output.artifact.as_str(), PathSyntax::ArtifactName)?;
        collisions.insert_file(&output.artifact)?;
        if output.target == BuildTarget::Mrpack {
            ensure!(
                output.artifact.as_str().ends_with(".mrpack"),
                "Mrpack output requires a .mrpack filename"
            );
        }
    }
    let outputs: Vec<_> = request
        .outputs
        .as_slice()
        .iter()
        .map(|output| output.artifact.clone())
        .collect();
    let workspace = ProjectReader::new(RecoveryReader::new(config.state_root.clone()))
        .capture_build(&project, &outputs, config.snapshot, cancel)?;
    let runtime = workspace.require_resolved()?.lock().runtime.clone();
    let mut plans = Vec::new();
    for output in request.outputs.as_slice() {
        let optional = if output.target == BuildTarget::Mrpack {
            &OptionalPolicy::Preserve
        } else {
            &request.optional
        };
        plans.push(plan_target_build_acquisitions(
            &workspace,
            &BuildAcquisitions::default(),
            output.target,
            optional,
            request.evidence,
            cancel,
        )?);
    }
    let acquisition = BuildAcquisitionPlan::combine(plans)?;
    let unresolved = acquisition
        .needs()
        .iter()
        .filter(|need| match &need.source {
            BuildContentSource::Provider { pin, .. } => !provider_access.supports(&pin.project),
            BuildContentSource::Manual { .. } => true,
            BuildContentSource::Embedded { archive, .. } => !matches!(
                workspace.observations().entries().get(archive),
                Some(Observation::File(_))
            ),
            BuildContentSource::Download(_) => false,
        })
        .map(|need| need.key.clone())
        .collect();
    let server = request
        .outputs
        .as_slice()
        .iter()
        .any(|output| matches!(output.target, BuildTarget::Server | BuildTarget::ServerFull));
    let bootstrap = request
        .outputs
        .as_slice()
        .iter()
        .any(|output| matches!(output.target, BuildTarget::Server | BuildTarget::Client));
    let view = BuildPreview {
        plan: PlanId(
            NEXT_PLAN
                .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
                .map_err(|_| anyhow::anyhow!("Plan identifier exhausted"))?,
        ),
        outputs: request.outputs.as_slice().to_vec(),
        runtime: runtime.clone(),
        content: acquisition.needs().iter().map(describe).collect(),
        needs_network: server
            || bootstrap
            || acquisition.needs().iter().any(|need| {
                matches!(
                    need.source,
                    BuildContentSource::Download(_) | BuildContentSource::Provider { .. }
                )
            }),
        runs_installer: server
            && matches!(runtime.loader, LoaderKind::Forge | LoaderKind::NeoForge),
        unresolved,
        options: request.clone(),
    };
    Ok(PreparedBuild {
        view,
        workspace,
        request,
        acquisition,
    })
}

#[cfg(test)]
mod tests;
