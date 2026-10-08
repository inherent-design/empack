//! Semantic operation lifecycle. Preparation has read-only project authority; only an approved,
//! engine-bound plan can admit acquisition, trusted tools and verified publication.
use super::diagnostics::{Diagnostic, DiagnosticCode, DiagnosticPhase};
use super::{
    acquisition::{HttpAcquisition, TransferLimits},
    artifacts::ArchiveLimits,
    build::{
        BuildAcquisitions,
        acquisition::{
            AcquisitionKey, BuildAcquisitionPlan, BuildAcquisitionResult, BuildContentSource,
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
    model::{
        DistributionArchive, ExpectedContent, LoaderKind, NonEmpty, RuntimeResolution,
        SemanticRevision,
    },
    path::{PathSyntax, PortableRelPath},
    projection::BuildTarget,
};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};
use tokio::sync::oneshot;

#[derive(Debug, thiserror::Error)]
#[error("Publication worker failed; inspect recovery before retrying")]
struct PublicationWorkerFailed(#[source] RuntimeError);
pub use super::dependency_content::{DependencyContent, DependencyContents};
mod addition;
mod cache_cleanup;
mod cleanup;
pub use cache_cleanup::{CacheCleanPreview, CacheCleanReceipt, CacheCleanRequest};
pub use cleanup::{CleanPreview, CleanReceipt, CleanRequest};
mod recovery;
pub use addition::{
    AddPreview, AddReceipt, AddRequest, AdoptObservedPreview, AdoptObservedReceipt,
    AdoptObservedRequest, AdoptionResolution, AdoptionSelection, BatchPolicy, BlockedBatchGroup,
    DependencyBatchChange, DependencyBatchIncomplete, DependencyBatchItem, DependencyBatchReport,
    DependencyBatchRequest, ExistingDependencyPolicy, ReplacementSelection, UpdatePreview,
    UpdateReceipt, UpdateRequest,
};
pub use recovery::{
    RecoverPreview, RecoverRequest, RecoveryAction, RecoveryKind, RecoveryReceipt, RecoveryStatus,
};
mod execution;
mod project_change;
mod removal;
mod synchronization;
pub use project_change::{
    ImportRequest, InitializeRequest, ProjectChangePreview, ProjectChangeReceipt,
    ReplacementSummary,
};
pub use removal::{
    ObservedRemovalSelection, RemovalSelection, RemovalSelector, RemovePreview, RemoveReceipt,
    RemoveRequest,
};
pub use synchronization::{SyncPreview, SyncReceipt, SyncRequest};
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
    /// Retire other captured dist files only when every requested artifact verifies.
    pub clean: bool,
    pub outputs: NonEmpty<BuildOutput>,
    pub archive: DistributionArchive,
    pub optional: OptionalPolicy,
    pub mrpack_optional: OptionalConversion,
    pub templates: TemplateOptions,
    pub evidence: SourceEvidencePolicy,
    pub interaction: InstallerInteraction,
}
/// Explicitly supplied bytes stay private to preparation, never in a display-only preview.
pub struct BuildPreparationRequest {
    pub request: BuildRequest,
    pub supplied: BuildAcquisitions,
    local_files: BTreeMap<AcquisitionKey, PathBuf>,
    prior: Option<Box<PreparationContinuation>>,
    expected_intent: Option<SemanticRevision>,
}
impl BuildRequest {
    pub fn with_content(self, supplied: BuildAcquisitions) -> BuildPreparationRequest {
        BuildPreparationRequest {
            request: self,
            supplied,
            local_files: BTreeMap::new(),
            prior: None,
            expected_intent: None,
        }
    }
}
impl BuildPreparationRequest {
    /// Explicit host files are verified against captured obligations in private preparation.
    /// Paths must be absolute; they never become durable source or destination authority.
    pub fn with_local_files(mut self, files: BTreeMap<AcquisitionKey, PathBuf>) -> Self {
        self.local_files = files;
        self
    }
    /// Bind host-derived filenames, target defaults and format choices to the intent that
    /// selected them. This is a precondition, not authority to publish that intent.
    pub fn require_intent(mut self, revision: SemanticRevision) -> Self {
        self.expected_intent = Some(revision);
        self
    }
}
/// Host estimates used for admission, separate from actual stream and snapshot limits.
#[derive(Clone, Copy)]
pub struct OperationResources {
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
    pub resources: OperationResources,
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
    /// Exact public identity for separately authorized provider/browser assistance.
    pub provider: Option<empack_core::model::ResolvedPin>,
}
/// Read-only effect summary. No URLs, content leases, publisher or conversion into approval.
#[derive(Clone)]
pub struct BuildCleanup {
    /// Exact obsolete artifacts; requested outputs are replaced by their build recipes.
    pub files: empack_core::files::FilePlan,
    pub removed_bytes: u64,
    replacement: ReplacementSummary,
}
#[derive(Clone)]
pub struct BuildPreview {
    pub plan: PlanId,
    pub outputs: Vec<BuildOutput>,
    pub runtime: RuntimeResolution,
    pub content: Vec<ContentRequirement>,
    pub needs_network: bool,
    pub runs_installer: bool,
    pub unresolved: Vec<AcquisitionKey>,
    pub cleanup: Option<BuildCleanup>,
    options: BuildRequest,
    file_names: BTreeMap<AcquisitionKey, std::collections::BTreeSet<String>>,
}
impl BuildPreview {
    /// Captured destination basenames are selectors only; original assertions still verify bytes.
    pub fn file_names(&self) -> &BTreeMap<AcquisitionKey, std::collections::BTreeSet<String>> {
        &self.file_names
    }
    /// All requested options, including conversions and template values, for host presentation.
    /// These values are deliberately not included in an automatic Debug rendering.
    pub fn request(&self) -> &BuildRequest {
        &self.options
    }
}
struct PreparedBuild {
    project: PathBuf,
    view: BuildPreview,
    workspace: WorkspaceSnapshot,
    request: BuildRequest,
    acquisition: BuildAcquisitionResult,
    acquired_permit: Option<super::resources::AdmissionPermit>,
}
pub enum Request {
    Clean(CleanRequest),
    Recover(RecoverRequest),
    Build(Box<BuildPreparationRequest>),
    Import(Box<ImportRequest>),
    Initialize(Box<InitializeRequest>),
    Remove(RemoveRequest),
    Add(AddRequest),
    DependencyBatch(Box<DependencyBatchRequest>),
    Update(UpdateRequest),
    AdoptObserved(AdoptObservedRequest),
    Sync(Box<SyncRequest>),
}
/// A selected existing root or one absent child of an existing selected parent.
#[derive(Clone)]
pub enum ProjectTarget {
    Existing(PathBuf),
    New(PathBuf),
}
impl From<PathBuf> for ProjectTarget {
    fn from(path: PathBuf) -> Self {
        Self::Existing(path)
    }
}
impl From<CleanRequest> for Request {
    fn from(request: CleanRequest) -> Self {
        Self::Clean(request)
    }
}
impl From<BuildRequest> for Request {
    fn from(request: BuildRequest) -> Self {
        Self::Build(Box::new(request.with_content(BuildAcquisitions::default())))
    }
}
impl From<BuildPreparationRequest> for Request {
    fn from(request: BuildPreparationRequest) -> Self {
        Self::Build(Box::new(request))
    }
}
impl From<InitializeRequest> for Request {
    fn from(request: InitializeRequest) -> Self {
        Self::Initialize(Box::new(request))
    }
}
impl From<SyncRequest> for Request {
    fn from(request: SyncRequest) -> Self {
        Self::Sync(Box::new(request))
    }
}
impl From<DependencyBatchRequest> for Request {
    fn from(request: DependencyBatchRequest) -> Self {
        Self::DependencyBatch(Box::new(request))
    }
}
impl From<AddRequest> for Request {
    fn from(request: AddRequest) -> Self {
        Self::Add(request)
    }
}
impl From<RecoverRequest> for Request {
    fn from(request: RecoverRequest) -> Self {
        Self::Recover(request)
    }
}
impl From<AdoptObservedRequest> for Request {
    fn from(request: AdoptObservedRequest) -> Self {
        Self::AdoptObserved(request)
    }
}
impl From<UpdateRequest> for Request {
    fn from(request: UpdateRequest) -> Self {
        Self::Update(request)
    }
}
impl From<RemoveRequest> for Request {
    fn from(request: RemoveRequest) -> Self {
        Self::Remove(request)
    }
}
impl From<ImportRequest> for Request {
    fn from(request: ImportRequest) -> Self {
        Self::Import(Box::new(request))
    }
}
#[derive(Clone)]
pub enum OperationPreview {
    CacheClean(CacheCleanPreview),
    Clean(CleanPreview),
    Recovery(RecoverPreview),
    Build(BuildPreview),
    Import(ProjectChangePreview),
    Initialize(ProjectChangePreview),
    Remove(RemovePreview),
    Add(AddPreview),
    Update(UpdatePreview),
    AdoptObserved(AdoptObservedPreview),
    Sync(SyncPreview),
}
impl OperationPreview {
    pub fn plan(&self) -> PlanId {
        match self {
            Self::CacheClean(view) => view.plan,
            Self::Clean(view) => view.plan,
            Self::Recovery(view) => view.plan,
            Self::Build(view) => view.plan,
            Self::Remove(view) => view.plan,
            Self::Add(view) => view.plan,
            Self::Update(view) => view.plan,
            Self::AdoptObserved(view) => view.plan,
            Self::Sync(view) => view.plan,
            Self::Import(view) | Self::Initialize(view) => view.plan,
        }
    }
    pub fn cache_clean(&self) -> Option<&CacheCleanPreview> {
        match self {
            Self::CacheClean(view) => Some(view),
            _ => None,
        }
    }
    pub fn clean(&self) -> Option<&CleanPreview> {
        match self {
            Self::Clean(view) => Some(view),
            _ => None,
        }
    }
    pub fn recovery(&self) -> Option<&RecoverPreview> {
        match self {
            Self::Recovery(view) => Some(view),
            _ => None,
        }
    }
    pub fn build(&self) -> Option<&BuildPreview> {
        match self {
            Self::Build(view) => Some(view),
            _ => None,
        }
    }
    pub fn import(&self) -> Option<&ProjectChangePreview> {
        match self {
            Self::Import(view) => Some(view),
            _ => None,
        }
    }
    pub fn initialize(&self) -> Option<&ProjectChangePreview> {
        match self {
            Self::Initialize(view) => Some(view),
            _ => None,
        }
    }
    pub fn sync(&self) -> Option<&SyncPreview> {
        match self {
            Self::Sync(view) => Some(view),
            _ => None,
        }
    }
    pub fn add(&self) -> Option<&AddPreview> {
        match self {
            Self::Add(view) => Some(view),
            _ => None,
        }
    }
    pub fn adoption(&self) -> Option<&AdoptObservedPreview> {
        match self {
            Self::AdoptObserved(view) => Some(view),
            _ => None,
        }
    }
    pub fn update(&self) -> Option<&UpdatePreview> {
        match self {
            Self::Update(view) => Some(view),
            _ => None,
        }
    }
    pub fn remove(&self) -> Option<&RemovePreview> {
        match self {
            Self::Remove(view) => Some(view),
            _ => None,
        }
    }
    pub fn replacement(&self) -> Option<ReplacementSummary> {
        match self {
            Self::Import(view) | Self::Initialize(view) => view.replacement,
            Self::CacheClean(view) => Some(view.replacement),
            Self::Clean(view) => Some(view.replacement),
            Self::Recovery(view) => Some(view.replacement),
            Self::Remove(view) => Some(view.replacement),
            Self::Add(view) => Some(view.replacement),
            Self::Update(view) => Some(view.replacement),
            Self::AdoptObserved(view) => Some(view.replacement),
            Self::Sync(view) => Some(view.replacement),
            Self::Build(view) => view.cleanup.as_ref().map(|cleanup| cleanup.replacement),
        }
    }
    pub fn needs_network(&self) -> bool {
        self.build().is_some_and(|view| view.needs_network)
    }
    pub fn runs_installer(&self) -> bool {
        self.build().is_some_and(|view| view.runs_installer)
    }
}
enum PreparedKind {
    CacheClean(Box<cache_cleanup::PreparedCacheCleanup>),
    Clean(Box<cleanup::PreparedCleanup>),
    Recovery(Box<recovery::PreparedRecoveryOperation>),
    Build(Box<PreparedBuild>),
    ProjectChange(Box<project_change::PreparedProjectChange>),
    Remove(Box<removal::PreparedRemovalOperation>),
    Add(Box<addition::PreparedAdditionOperation>),
    Update(Box<addition::PreparedAdditionOperation>),
    AdoptObserved(Box<addition::PreparedAdditionOperation>),
    Sync(Box<synchronization::PreparedSynchronizationOperation>),
}
impl PreparedKind {
    fn view(&self) -> OperationPreview {
        match self {
            Self::CacheClean(value) => OperationPreview::CacheClean(value.view.clone()),
            Self::Clean(value) => OperationPreview::Clean(value.view.clone()),
            Self::Recovery(value) => OperationPreview::Recovery(value.view.clone()),
            Self::Build(value) => OperationPreview::Build(value.view.clone()),
            Self::Remove(value) => OperationPreview::Remove(value.view.clone()),
            Self::Add(value) => OperationPreview::Add(value.view.clone()),
            Self::Update(value) => OperationPreview::Update((&value.view).into()),
            Self::AdoptObserved(value) => OperationPreview::AdoptObserved(value.adoption_preview()),
            Self::Sync(value) => OperationPreview::Sync(value.view.clone()),
            Self::ProjectChange(value) => {
                if value.initialize {
                    OperationPreview::Initialize(value.view.clone())
                } else {
                    OperationPreview::Import(value.view.clone())
                }
            }
        }
    }
}
pub struct PreparedOperation {
    owner: Arc<()>,
    view: Box<OperationPreview>,
    data: Box<RetainedOutput<PreparedKind>>,
}
pub struct ApprovedOperation {
    prepared: PreparedOperation,
}
pub enum Preparation {
    Ready(PreparedOperation),
    NeedsInput(Box<PreparationContinuation>),
}
/// In-memory suspension retains supplied leases and captured inputs, but cannot authorize effects.
pub struct PreparationContinuation {
    prepared: PreparedOperation,
}
impl PreparationContinuation {
    pub fn view(&self) -> &OperationPreview {
        self.prepared.view()
    }
}
impl std::ops::Deref for PreparationContinuation {
    type Target = OperationPreview;
    fn deref(&self) -> &Self::Target {
        self.view()
    }
}
#[derive(Debug, Clone, Copy)]
pub struct ExecutionGrant {
    pub plan: PlanId,
    pub network: NetworkPermission,
    pub run_installer: bool,
    /// Exact managed replacement footprint displayed by a project-change preview.
    pub replacement: Option<ReplacementSummary>,
}
impl PreparedOperation {
    pub fn view(&self) -> &OperationPreview {
        &self.view
    }
    pub fn authorize(self, grant: ExecutionGrant) -> Result<ApprovedOperation> {
        ensure!(
            grant.plan == self.view.plan(),
            anyhow::Error::new(Diagnostic::new(
                DiagnosticCode::AuthorizationDenied,
                DiagnosticPhase::Authorization
            ))
            .context("Execution grant belongs to another plan")
        );
        ensure!(
            !self.view.needs_network() || grant.network == NetworkPermission::Allow,
            anyhow::Error::new(Diagnostic::new(
                DiagnosticCode::AuthorizationDenied,
                DiagnosticPhase::Authorization
            ))
            .context("Operation requires network authorization")
        );
        ensure!(
            !self.view.runs_installer() || grant.run_installer,
            anyhow::Error::new(Diagnostic::new(
                DiagnosticCode::AuthorizationDenied,
                DiagnosticPhase::Authorization
            ))
            .context("Operation requires trusted installer authorization")
        );
        let replacement = self.view.replacement();
        ensure!(
            grant.replacement == replacement,
            anyhow::Error::new(Diagnostic::new(
                DiagnosticCode::AuthorizationDenied,
                DiagnosticPhase::Authorization
            ))
            .context("Execution grant must acknowledge the exact replacement footprint")
        );
        Ok(ApprovedOperation { prepared: self })
    }
}
pub enum ExecutionReceipt {
    CacheClean(Box<CacheCleanReceipt>),
    Clean(Box<RetainedOutput<CleanReceipt>>),
    Recovery(Box<RetainedOutput<RecoveryReceipt>>),
    Build(Box<RetainedOutput<BuildReceipt>>),
    Import(Box<RetainedOutput<ProjectChangeReceipt>>),
    Initialize(Box<RetainedOutput<ProjectChangeReceipt>>),
    Remove(Box<RetainedOutput<RemoveReceipt>>),
    Add(Box<RetainedOutput<AddReceipt>>),
    Update(Box<RetainedOutput<UpdateReceipt>>),
    AdoptObserved(Box<RetainedOutput<AdoptObservedReceipt>>),
    Sync(Box<RetainedOutput<SyncReceipt>>),
}
/// Accurate publication outcome: a failed preparation and a hot durable journal are distinct.
pub enum ExecutionOutcome {
    Completed(ExecutionReceipt),
    /// Only the effects described in this receipt completed; dependency batches retain blocked groups.
    PartiallyCompleted {
        receipt: ExecutionReceipt,
        cause: anyhow::Error,
    },
    NeedsInput(ExecutionInput),
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
impl ExecutionOutcome {
    fn failed(error: anyhow::Error, cancelled: bool) -> Self {
        if let Some(recovery) = error.downcast_ref::<crate::engine::publication::RecoveryRequired>()
        {
            Self::RecoveryRequired {
                operation: recovery.operation.clone(),
                cause: error,
            }
        } else if error.is::<PublicationWorkerFailed>() {
            Self::ExecutionUncertain(error)
        } else if error.is::<crate::application::process_runtime::Interrupted>()
            || matches!(
                error.downcast_ref::<RuntimeError>(),
                Some(
                    RuntimeError::Cancelled
                        | RuntimeError::Admission(
                            crate::engine::resources::AdmissionError::Cancelled
                        )
                )
            )
            || matches!(
                error.downcast_ref::<crate::engine::resources::AdmissionError>(),
                Some(crate::engine::resources::AdmissionError::Cancelled)
            )
            || (cancelled
                && matches!(
                    error.downcast_ref::<RuntimeError>(),
                    Some(RuntimeError::StaleResult)
                ))
        {
            Self::InterruptedBeforePublication
        } else {
            Self::FailedBeforePublication(error)
        }
    }
}
/// Missing execution inputs retain their recipe, captured project and verified leases.
/// Taking the continuation is single-consumer and grants no execution or durable-write authority.
pub struct ExecutionInput {
    requirements: Vec<ContentRequirement>,
    continuation: std::sync::Mutex<Option<PreparationContinuation>>,
}
impl ExecutionInput {
    pub fn requirements(&self) -> &[ContentRequirement] {
        &self.requirements
    }
    pub fn take_continuation(&self) -> Option<PreparationContinuation> {
        self.continuation
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take()
    }
}
impl std::ops::Deref for ExecutionInput {
    type Target = [ContentRequirement];
    fn deref(&self) -> &Self::Target {
        self.requirements()
    }
}

pub struct BuildReceipt {
    pub plan: PlanId,
    pub removed_artifacts: std::collections::BTreeSet<PortableRelPath>,
    pub publication: PublicationReceipt,
    pub artifacts: Vec<super::build::batch::BuiltDistribution>,
}
/// Owned operations over captured projects. The host authorizes the displayed plan.
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
///     plan: prepared.view().plan(),
///     network: NetworkPermission::Allow,
///     run_installer: true,
///     replacement: None,
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
    content_store: Option<super::content::store::FileContentStore>,
    content_cache: Option<super::content::cache::ContentCache>,
    preparations: OperationRuntime<()>,
    operations: OperationRuntime<ExecutionOutcome>,
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
            content_store: None,
            content_cache: None,
            preparations: OperationRuntime::new(governor.clone(), config.retained_operations),
            operations: OperationRuntime::new(governor, config.retained_operations),
            config,
        })
    }
    /// Select disposable host storage without creating it. Preparation may only read;
    /// approved execution may retain verified bytes. Cache failure does not grant cleanup.
    pub fn with_content_cache(mut self, cache: super::content::cache::ContentCache) -> Self {
        self.content_cache = Some(cache);
        self
    }
    /// Attach read-only provider resolution for authorized execution. Preparation receives only
    /// its availability description, never the network client or credentials.
    pub fn with_provider_catalog(
        mut self,
        catalog: ProviderCatalog,
        limits: CatalogLimits,
    ) -> Self {
        self.transport = catalog.configure_acquisition(self.transport);
        self.catalog = Some((catalog, limits));
        self
    }
    /// Capture and plan only. No persistent cache writer, downloader or process runner enters
    /// this worker. Dropping the future cancels its engine-owned preparation.
    pub async fn prepare(
        &self,
        project: impl Into<ProjectTarget>,
        request: impl Into<Request>,
    ) -> Result<Preparation> {
        let request = request.into();
        let project = project.into();
        let config = self.config.clone();
        let provider_access = self
            .catalog
            .as_ref()
            .map(|(catalog, _)| catalog.availability())
            .unwrap_or_default();
        let owner = self.owner.clone();
        let content_cache = self.content_cache.clone();
        let (sender, receiver) = oneshot::channel();
        let mut handle = self
            .preparations
            .start_ephemeral(move |mut scope| async move {
                let prepared: Result<RetainedOutput<PreparedKind>> = async {
                    match request {
                        Request::Clean(request) => {
                            Ok(cleanup::prepare(project, request, &config, &mut scope)
                                .await?
                                .map(|value| PreparedKind::Clean(Box::new(value))))
                        }
                        Request::Recover(request) => {
                            Ok(recovery::prepare(project, request, &config, &mut scope)
                                .await?
                                .map(|value| PreparedKind::Recovery(Box::new(value))))
                        }
                        Request::Build(request) => {
                            if let Some(prior) = &request.prior {
                                ensure!(
                                    Arc::ptr_eq(&owner, &prior.prepared.owner),
                                    "Continuation belongs to another engine"
                                );
                            }
                            let ProjectTarget::Existing(project) = project else {
                                anyhow::bail!("Build requires an existing project");
                            };
                            Ok(prepare_build(
                                project,
                                *request,
                                config,
                                provider_access,
                                content_cache,
                                &mut scope,
                            )
                            .await?
                            .map(|value| PreparedKind::Build(Box::new(value))))
                        }
                        Request::Sync(request) => Ok(synchronization::prepare(
                            project, *request, &config, &mut scope,
                        )
                        .await?
                        .map(|value| PreparedKind::Sync(Box::new(value)))),
                        Request::AdoptObserved(request) => Ok(addition::prepare_adoption(
                            project, request, &config, &mut scope,
                        )
                        .await?
                        .map(|value| PreparedKind::AdoptObserved(Box::new(value)))),
                        Request::Update(request) => Ok(addition::prepare_update(
                            project, request, &config, &mut scope,
                        )
                        .await?
                        .map(|value| PreparedKind::Update(Box::new(value)))),
                        Request::DependencyBatch(request) => {
                            let update = matches!(request.change, DependencyBatchChange::Update);
                            Ok(
                                addition::prepare_batch(project, *request, &config, &mut scope)
                                    .await?
                                    .map(|value| {
                                        if update {
                                            PreparedKind::Update(Box::new(value))
                                        } else {
                                            PreparedKind::Add(Box::new(value))
                                        }
                                    }),
                            )
                        }
                        Request::Add(request) => {
                            Ok(addition::prepare(project, request, &config, &mut scope)
                                .await?
                                .map(|value| PreparedKind::Add(Box::new(value))))
                        }
                        Request::Remove(request) => {
                            Ok(removal::prepare(project, request, &config, &mut scope)
                                .await?
                                .map(|value| PreparedKind::Remove(Box::new(value))))
                        }
                        Request::Initialize(request) => Ok(project_change::prepare(
                            project,
                            request.candidate.into(),
                            request.replacement,
                            &config,
                            &mut scope,
                        )
                        .await?
                        .map(|value| PreparedKind::ProjectChange(Box::new(value)))),
                        Request::Import(request) => Ok(project_change::prepare(
                            project,
                            request.candidate.into(),
                            request.replacement,
                            &config,
                            &mut scope,
                        )
                        .await?
                        .map(|value| PreparedKind::ProjectChange(Box::new(value)))),
                    }
                }
                .await;
                let prepared = prepared.map(|data| PreparedOperation {
                    owner,
                    view: Box::new(data.view()),
                    data: Box::new(data),
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
        if prepared
            .view()
            .build()
            .is_none_or(|view| view.unresolved.is_empty())
        {
            Ok(Preparation::Ready(prepared))
        } else {
            Ok(Preparation::NeedsInput(Box::new(PreparationContinuation {
                prepared,
            })))
        }
    }
    /// Revalidate a suspended build and merge additional explicit inputs before making a new plan.
    pub async fn resume(
        &self,
        pending: PreparationContinuation,
        supplied: BuildAcquisitions,
    ) -> Result<Preparation> {
        self.resume_inputs(pending, supplied, BTreeMap::new()).await
    }
    /// Resume using explicitly associated host files, retaining prior verified inputs.
    pub async fn resume_with_local_files(
        &self,
        pending: PreparationContinuation,
        files: BTreeMap<AcquisitionKey, PathBuf>,
    ) -> Result<Preparation> {
        self.resume_inputs(pending, BuildAcquisitions::default(), files)
            .await
    }
    async fn resume_inputs(
        &self,
        pending: PreparationContinuation,
        supplied: BuildAcquisitions,
        local_files: BTreeMap<AcquisitionKey, PathBuf>,
    ) -> Result<Preparation> {
        ensure!(
            Arc::ptr_eq(&self.owner, &pending.prepared.owner),
            "Continuation belongs to another engine"
        );
        let PreparedKind::Build(build) = &**pending.prepared.data else {
            anyhow::bail!("Continuation is not a build preparation");
        };
        let project = build.project.clone();
        let request = BuildPreparationRequest {
            request: build.request.clone(),
            supplied,
            local_files,
            prior: Some(Box::new(pending)),
            expected_intent: None,
        };
        self.prepare(project, request).await
    }
    /// Preview has no authority-bearing output even when its plan requires no additional input.
    pub async fn preview(
        &self,
        project: impl Into<ProjectTarget>,
        request: impl Into<Request>,
    ) -> Result<OperationPreview> {
        Ok(match self.prepare(project, request).await? {
            Preparation::Ready(prepared) => prepared.view().clone(),
            Preparation::NeedsInput(pending) => pending.view().clone(),
        })
    }
    pub fn start(&self, approved: ApprovedOperation) -> Result<OperationHandle<ExecutionOutcome>> {
        ensure!(
            Arc::ptr_eq(&self.owner, &approved.prepared.owner),
            anyhow::Error::new(Diagnostic::new(
                DiagnosticCode::AuthorizationDenied,
                DiagnosticPhase::Authorization
            ))
            .context("Prepared operation belongs to another engine")
        );
        let config = self.config.clone();
        let transport = self.transport.clone();
        let catalog = self.catalog.clone();
        let content_store = self.content_store.clone();
        let content_cache = self.content_cache.clone();
        let owner = self.owner.clone();
        Ok(self.operations.start(move |scope| async move {
            let data = *approved.prepared.data;
            match &*data {
                PreparedKind::CacheClean(_) => {
                    let prepared = data.map(|kind| match kind {
                        PreparedKind::CacheClean(value) => *value,
                        _ => unreachable!(),
                    });
                    cache_cleanup::run(
                        prepared,
                        content_store.expect("engine-bound cache preparation"),
                        scope,
                    )
                    .await
                }
                PreparedKind::Clean(_) => {
                    let prepared = data.map(|kind| match kind {
                        PreparedKind::Clean(value) => *value,
                        _ => unreachable!(),
                    });
                    cleanup::run(prepared, config, scope).await
                }
                PreparedKind::Recovery(_) => {
                    let prepared = data.map(|kind| match kind {
                        PreparedKind::Recovery(value) => *value,
                        _ => unreachable!(),
                    });
                    recovery::run(prepared, config, scope).await
                }
                PreparedKind::Build(_) => {
                    let prepared = data.map(|kind| match kind {
                        PreparedKind::Build(value) => *value,
                        _ => unreachable!(),
                    });
                    execution::run(
                        prepared,
                        config,
                        transport,
                        catalog,
                        content_cache,
                        owner,
                        scope,
                    )
                    .await
                }
                PreparedKind::ProjectChange(_) => {
                    let prepared = data.map(|kind| match kind {
                        PreparedKind::ProjectChange(value) => *value,
                        _ => unreachable!(),
                    });
                    project_change::run(prepared, config, content_cache, scope).await
                }
                PreparedKind::Sync(_) => {
                    let prepared = data.map(|kind| match kind {
                        PreparedKind::Sync(value) => *value,
                        _ => unreachable!(),
                    });
                    synchronization::run(prepared, config, scope).await
                }
                PreparedKind::AdoptObserved(_) => {
                    let prepared = data.map(|kind| match kind {
                        PreparedKind::AdoptObserved(value) => *value,
                        _ => unreachable!(),
                    });
                    addition::run_adoption(prepared, config, scope).await
                }
                PreparedKind::Update(_) => {
                    let prepared = data.map(|kind| match kind {
                        PreparedKind::Update(value) => *value,
                        _ => unreachable!(),
                    });
                    addition::run_update(prepared, config, content_cache, scope).await
                }
                PreparedKind::Add(_) => {
                    let prepared = data.map(|kind| match kind {
                        PreparedKind::Add(value) => *value,
                        _ => unreachable!(),
                    });
                    addition::run(prepared, config, content_cache, scope).await
                }
                PreparedKind::Remove(_) => {
                    let prepared = data.map(|kind| match kind {
                        PreparedKind::Remove(value) => *value,
                        _ => unreachable!(),
                    });
                    removal::run(prepared, config, scope).await
                }
            }
        })?)
    }
    pub fn observe(&self, id: OperationId) -> Option<OperationHandle<ExecutionOutcome>> {
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
        provider: match &need.source {
            BuildContentSource::Provider { pin, .. } => Some(pin.clone()),
            BuildContentSource::ProviderArchiveMember { archive, .. } => Some(archive.pin.clone()),
            BuildContentSource::Manual { pin } => pin.clone(),
            _ => None,
        },
        kind: match need.source {
            BuildContentSource::Download(_) => ContentRequirementKind::Download,
            BuildContentSource::Provider { .. }
            | BuildContentSource::ProviderArchiveMember { .. } => {
                ContentRequirementKind::ProviderLookup
            }
            BuildContentSource::Embedded { .. } => ContentRequirementKind::Embedded,
            BuildContentSource::Manual { .. } => ContentRequirementKind::Manual,
        },
    }
}
async fn prepare_build(
    project: PathBuf,
    mut input: BuildPreparationRequest,
    config: EngineConfig,
    provider_access: ProviderAvailability,
    cache: Option<super::content::cache::ContentCache>,
    scope: &mut super::runtime::WorkScope,
) -> Result<RetainedOutput<PreparedBuild>> {
    let local_files = std::mem::take(&mut input.local_files);
    let transfer = config.transfer;
    let file_limit = config.snapshot.entries;
    let prepared = if let Some(prior) = input.prior.take() {
        let retained = *prior.prepared.data;
        let held = retained.reserved();
        let required = config.resources.capture;
        // Keep the original reservation charged throughout recapture. Admit only its top-up;
        // the worker returns its replacement under that same retained preparation permit.
        ensure!(
            held == config.resources.prepared,
            "Continuation reservation changed"
        );
        let additional = ResourceRequest {
            jobs: required
                .jobs
                .checked_sub(held.jobs)
                .context("Capture reservation is smaller than retained preparation")?,
            memory_bytes: required
                .memory_bytes
                .checked_sub(held.memory_bytes)
                .context("Capture reservation is smaller than retained preparation")?,
            scratch_bytes: required
                .scratch_bytes
                .checked_sub(held.scratch_bytes)
                .context("Capture reservation is smaller than retained preparation")?,
            open_files: required
                .open_files
                .checked_sub(held.open_files)
                .context("Capture reservation is smaller than retained preparation")?,
        };
        let work = scope.spawn_blocking(additional, ResourceRequest::default(), move |cancel| {
            retained.map(|prior| {
                let PreparedKind::Build(prior) = prior else {
                    anyhow::bail!("Continuation is not a build preparation");
                };
                capture(
                    project,
                    input,
                    Some(*prior),
                    &config,
                    provider_access,
                    &cancel,
                )
            })
        })?;
        scope.accept(work.wait().await?)?.into_parts().0.transpose()
    } else {
        let work = scope.spawn_blocking(
            config.resources.capture,
            config.resources.prepared,
            move |cancel| capture(project, input, None, &config, provider_access, &cancel),
        )?;
        scope.accept(work.wait().await?)?.transpose()
    }?;
    let prepared = local_inputs::supply(prepared, local_files, transfer, file_limit, scope).await?;
    build_cache::supply(prepared, cache, transfer, scope).await
}
fn capture(
    project: PathBuf,
    input: BuildPreparationRequest,
    prior: Option<PreparedBuild>,
    config: &EngineConfig,
    provider_access: ProviderAvailability,
    cancel: &crate::application::process_runtime::Cancellation,
) -> Result<PreparedBuild> {
    let BuildPreparationRequest {
        request,
        mut supplied,
        local_files: _,
        prior: _,
        expected_intent,
    } = input;
    let mut prior_root = None;
    let mut acquired_permit = None;
    if let Some(prior) = prior {
        ensure!(
            prior.project == project,
            "Continuation selected another project"
        );
        prior
            .workspace
            .root()
            .revalidate(prior.workspace.observations(), cancel)?;
        let binding = prior.workspace.root().binding;
        ensure!(
            super::snapshot::ProjectReadRoot::open(&project)?.binding == binding,
            "Continuation project selection now refers to another root"
        );
        prior_root = Some(binding);
        acquired_permit = prior.acquired_permit;
        for (key, file) in prior.acquisition.acquired.locked {
            ensure!(
                supplied.locked.insert(key, file).is_none(),
                "Continuation repeats a supplied locked file"
            );
        }
        for (path, file) in prior.acquisition.acquired.observed {
            ensure!(
                supplied.observed.insert(path, file).is_none(),
                "Continuation repeats a supplied observed file"
            );
        }
    }
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
        .capture_build_selection(
            &project,
            &outputs,
            request.clean,
            config.snapshot,
            SnapshotLimits {
                file_bytes: config.archive.compressed_bytes,
                total_bytes: config
                    .snapshot
                    .total_bytes
                    .max(config.archive.compressed_bytes),
                entries: config.snapshot.entries,
                depth: config.snapshot.depth,
            },
            cancel,
        )?;
    ensure!(
        expected_intent.is_none_or(|revision| workspace.intent().semantic_revision() == revision),
        "Build choices belong to changed project intent; prepare a fresh build plan"
    );
    let cleanup = request
        .clean
        .then(|| build_cleanup(&workspace, &outputs))
        .transpose()?;
    ensure!(
        prior_root.is_none_or(|binding| workspace.root().binding == binding),
        "Continuation project selection changed during capture"
    );
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
    let acquisition =
        BuildAcquisitionPlan::combine(plans)?.supply(supplied, request.evidence, cancel)?;
    let unresolved = acquisition
        .pending
        .iter()
        .filter(|need| match &need.source {
            BuildContentSource::Provider {
                pin, alternatives, ..
            } => alternatives.is_empty() && !provider_access.supports(&pin.project),
            BuildContentSource::ProviderArchiveMember { archive, .. } => {
                archive.alternatives.is_empty() && !provider_access.supports(&archive.pin.project)
            }
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
    let resolved = workspace.require_resolved()?;
    let records = workspace.backend_files(cancel)?;
    let mut file_names = BTreeMap::new();
    for need in &acquisition.pending {
        let names = match &need.key {
            AcquisitionKey::Locked(key) => resolved
                .lock()
                .dependencies
                .get(&key.dependency)
                .and_then(|dependency| {
                    dependency
                        .files
                        .as_slice()
                        .iter()
                        .find(|file| file.slot == key.slot)
                })
                .context("Build obligation has no locked file")?
                .placements
                .as_slice()
                .iter()
                .map(|placement| {
                    placement
                        .destination
                        .relative()
                        .as_str()
                        .rsplit('/')
                        .next()
                        .unwrap()
                        .to_owned()
                })
                .collect(),
            AcquisitionKey::Observed(path) => std::collections::BTreeSet::from([records
                .iter()
                .find(|record| &record.metadata_path == path)
                .context("Build obligation has no observed metadata")?
                .destination
                .relative()
                .as_str()
                .rsplit('/')
                .next()
                .unwrap()
                .to_owned()]),
        };
        file_names.insert(need.key.clone(), names);
    }
    let view = BuildPreview {
        plan: PlanId(
            NEXT_PLAN
                .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
                .map_err(|_| anyhow::anyhow!("Plan identifier exhausted"))?,
        ),
        outputs: request.outputs.as_slice().to_vec(),
        runtime: runtime.clone(),
        content: acquisition.pending.iter().map(describe).collect(),
        needs_network: server
            || bootstrap
            || acquisition.pending.iter().any(|need| {
                matches!(
                    need.source,
                    BuildContentSource::Download(_)
                        | BuildContentSource::Provider { .. }
                        | BuildContentSource::ProviderArchiveMember { .. }
                )
            }),
        runs_installer: server
            && matches!(runtime.loader, LoaderKind::Forge | LoaderKind::NeoForge),
        unresolved,
        cleanup,
        options: request.clone(),
        file_names,
    };
    Ok(PreparedBuild {
        project,
        view,
        workspace,
        request,
        acquisition,
        acquired_permit,
    })
}

mod build_cache;
mod local_inputs;
mod mutation_cache;

fn build_cleanup(
    workspace: &WorkspaceSnapshot,
    outputs: &[PortableRelPath],
) -> Result<BuildCleanup> {
    use empack_core::files::{FileChange, ManagedPath};
    use std::collections::{BTreeMap, BTreeSet};
    let obsolete: BTreeSet<_> = workspace
        .observations()
        .entries()
        .iter()
        .filter(|(_, observation)| matches!(observation, Observation::File(_)))
        .filter_map(|(path, _)| path.as_str().strip_prefix("dist/"))
        .map(|path| PortableRelPath::parse(path, PathSyntax::ProjectContent))
        .collect::<Result<BTreeSet<_>, _>>()?
        .into_iter()
        .filter(|path| !outputs.contains(path))
        .map(ManagedPath::Artifact)
        .collect();
    let observed = super::verification::observed_artifacts_for(
        workspace.observations(),
        obsolete.iter().cloned(),
    )?;
    let files = super::verification::plan_files(&observed, &BTreeMap::new(), &obsolete)?;
    let removed_bytes = files.changes().iter().try_fold(0u64, |total, change| {
        let FileChange::Remove { before, .. } = change else {
            anyhow::bail!("Build cleanup may only remove artifacts");
        };
        total
            .checked_add(before.bytes)
            .context("Build cleanup size overflow")
    })?;
    Ok(BuildCleanup {
        replacement: project_change::summary(&files)?,
        files,
        removed_bytes,
    })
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod initialize_tests;

#[cfg(test)]
mod removal_tests;

#[cfg(test)]
mod addition_tests;

#[cfg(test)]
mod recovery_tests;

#[cfg(test)]
mod build_input_tests;

#[cfg(test)]
mod recorded_sync_tests;

mod suspension;
pub use suspension::{ResumedBuild, SavedBuildRecord, SavedBuildResume, SuspendedBuildReceipt};
