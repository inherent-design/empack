//! Resolved additions use captured plans, explicit replacement and retained engine outcomes.
use super::*;
use crate::engine::{
    addition::{self as native_addition, PreparedAddition},
    mrpack::LockedFileKey,
    publication::{Publisher, RecoveryRequired},
    runtime::WorkScope,
};
use empack_core::{
    addition::AdditionGroup,
    files::{FileChange, FilePlan, ObservedPath},
    model::{Coverage, DependencyKey, LockedDependency, ResolutionLock, ResolvedProject},
};
use std::collections::{BTreeMap, BTreeSet};

pub use empack_core::addition::ReplacementSelection;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum ExistingDependencyPolicy {
    #[default]
    RejectExisting,
    UpdateSameIdentity,
    ReplaceSelected(ReplacementSelection),
}
/// Resolved request and explicit per-slot materialization. Provider/local/URL hosts resolve before
/// this boundary; neither the group nor its references grant project publication authority.
pub struct AddRequest {
    /// Bind host resolution to the native document generation from which it was derived.
    pub source_revision: Option<crate::engine::project::ProjectRevision>,
    pub group: AdditionGroup,
    pub content: DependencyContents,
    pub existing: ExistingDependencyPolicy,
}
#[derive(Clone)]
pub struct AddPreview {
    pub plan: PlanId,
    pub bindings: BTreeMap<DependencyKey, DependencyKey>,
    pub existing_roots: BTreeSet<DependencyKey>,
    pub existing: ExistingDependencyPolicy,
    pub replaced: BTreeMap<DependencyKey, LockedDependency>,
    pub incomplete_evidence: Vec<DependencyKey>,
    pub files: FilePlan,
    pub replacement: ReplacementSummary,
    /// Canonical slots explicitly recorded as references, not claimed as newly acquired bytes.
    pub references: BTreeSet<LockedFileKey>,
}
pub struct AddReceipt {
    pub plan: PlanId,
    pub publication: PublicationReceipt,
    pub replaced: BTreeMap<DependencyKey, LockedDependency>,
    pub incomplete_evidence: Vec<DependencyKey>,
    pub project: ResolvedProject,
    pub bindings: BTreeMap<DependencyKey, DependencyKey>,
    /// Canonical slots explicitly recorded as references, not claimed as newly acquired bytes.
    pub references: BTreeSet<LockedFileKey>,
}
/// Exact replacement selections for explicitly requested installed identities. Resolution hosts
/// supply the group; the engine preserves current authoring intent, including explicit pins.
pub struct UpdateRequest {
    pub source_revision: Option<crate::engine::project::ProjectRevision>,
    pub group: AdditionGroup,
    pub content: DependencyContents,
}
#[derive(Clone)]
pub struct UpdatePreview {
    pub plan: PlanId,
    pub bindings: BTreeMap<DependencyKey, DependencyKey>,
    pub selected: BTreeSet<DependencyKey>,
    pub files: FilePlan,
    pub replacement: ReplacementSummary,
    /// Canonical slots explicitly recorded as references, not claimed as newly acquired bytes.
    pub references: BTreeSet<LockedFileKey>,
}
impl From<&AddPreview> for UpdatePreview {
    fn from(value: &AddPreview) -> Self {
        Self {
            plan: value.plan,
            bindings: value.bindings.clone(),
            selected: value.existing_roots.clone(),
            files: value.files.clone(),
            references: value.references.clone(),
            replacement: value.replacement,
        }
    }
}
pub struct UpdateReceipt {
    pub plan: PlanId,
    pub selected: BTreeSet<DependencyKey>,
    pub publication: PublicationReceipt,
    pub project: ResolvedProject,
    pub bindings: BTreeMap<DependencyKey, DependencyKey>,
    /// Canonical slots explicitly recorded as references, not claimed as newly acquired bytes.
    pub references: BTreeSet<LockedFileKey>,
}
/// Selected observed content and its proposed durable description. Every selected payload must
/// already exist and satisfy the supplied evidence. Adoption does not install missing bytes.
pub struct AdoptObservedRequest {
    pub group: AdditionGroup,
}
#[derive(Clone)]
pub struct AdoptObservedPreview {
    pub plan: PlanId,
    pub bindings: BTreeMap<DependencyKey, DependencyKey>,
    pub files: FilePlan,
    pub replacement: ReplacementSummary,
    /// Captured prior resolution and verified proposed resolution for each changed selection.
    pub selections: Vec<AdoptionSelection>,
}
#[derive(Clone, PartialEq, Eq)]
pub struct AdoptionResolution {
    pub dependency: LockedDependency,
    pub required: BTreeSet<DependencyKey>,
    pub coverage: Coverage,
}
#[derive(Clone, PartialEq, Eq)]
pub struct AdoptionSelection {
    pub key: DependencyKey,
    pub before: Option<AdoptionResolution>,
    pub after: AdoptionResolution,
}
pub struct AdoptObservedReceipt {
    pub plan: PlanId,
    pub publication: PublicationReceipt,
    pub project: ResolvedProject,
    pub bindings: BTreeMap<DependencyKey, DependencyKey>,
}
#[derive(Clone, Copy)]
enum DependencyChange {
    Add,
    Update,
    Adopt,
}
pub(super) struct PreparedAdditionOperation {
    pub(super) view: AddPreview,
    addition: PreparedAddition,
    adoption: Vec<AdoptionSelection>,
}
impl PreparedAdditionOperation {
    pub(super) fn adoption_preview(&self) -> AdoptObservedPreview {
        AdoptObservedPreview {
            plan: self.view.plan,
            bindings: self.view.bindings.clone(),
            files: self.view.files.clone(),
            replacement: self.view.replacement,
            selections: self.adoption.clone(),
        }
    }
}
pub(super) async fn prepare(
    project: ProjectTarget,
    request: AddRequest,
    config: &EngineConfig,
    scope: &mut WorkScope,
) -> Result<RetainedOutput<PreparedAdditionOperation>> {
    prepare_change(project, request, DependencyChange::Add, config, scope).await
}
pub(super) async fn prepare_update(
    project: ProjectTarget,
    request: UpdateRequest,
    config: &EngineConfig,
    scope: &mut WorkScope,
) -> Result<RetainedOutput<PreparedAdditionOperation>> {
    prepare_change(
        project,
        AddRequest {
            source_revision: request.source_revision,
            group: request.group,
            content: request.content,
            existing: ExistingDependencyPolicy::UpdateSameIdentity,
        },
        DependencyChange::Update,
        config,
        scope,
    )
    .await
}
pub(super) async fn prepare_adoption(
    project: ProjectTarget,
    request: AdoptObservedRequest,
    config: &EngineConfig,
    scope: &mut WorkScope,
) -> Result<RetainedOutput<PreparedAdditionOperation>> {
    prepare_change(
        project,
        AddRequest {
            source_revision: None,
            group: request.group,
            content: BTreeMap::new(),
            existing: ExistingDependencyPolicy::UpdateSameIdentity,
        },
        DependencyChange::Adopt,
        config,
        scope,
    )
    .await
}
async fn prepare_change(
    project: ProjectTarget,
    request: AddRequest,
    kind: DependencyChange,
    config: &EngineConfig,
    scope: &mut WorkScope,
) -> Result<RetainedOutput<PreparedAdditionOperation>> {
    let ProjectTarget::Existing(project) = project else {
        anyhow::bail!("Addition requires an existing project")
    };
    ensure!(project.is_absolute(), "Project selection must be absolute");
    let state = config.state_root.clone();
    let limits = config.snapshot;
    let policy = request.existing.clone();
    let work = scope.spawn_blocking(config.resources.capture, config.resources.prepared, move |cancel| {
        let reader = ProjectReader::new(RecoveryReader::new(state));
        let snapshot = match kind {
            DependencyChange::Adopt => reader.capture_adoption(&project, &request.group, limits, &cancel)?,
            DependencyChange::Add if matches!(&request.existing, ExistingDependencyPolicy::ReplaceSelected(_)) => {
                let ExistingDependencyPolicy::ReplaceSelected(selection) = &request.existing else { unreachable!() };
                reader.capture_dependency_replacement(&project, &request.group, selection, limits, &cancel)?
            }
            _ => reader.capture_addition(&project, &request.group, limits, &cancel)?,
        };
        snapshot.require_revision(request.source_revision)?;
        let planned = match kind {
            DependencyChange::Add => match &request.existing {
                ExistingDependencyPolicy::ReplaceSelected(selection) => native_addition::plan_replacement(snapshot, &request.group, request.content, selection, &cancel)?,
                _ => native_addition::plan_addition(snapshot, &request.group, request.content, &cancel)?,
            },
            DependencyChange::Update => native_addition::plan_update(snapshot, &request.group, request.content, &cancel)?,
            DependencyChange::Adopt => native_addition::plan_adoption(snapshot, &request.group, &cancel)?,
        };
        ensure!(request.existing != ExistingDependencyPolicy::RejectExisting || planned.candidate().plan().existing_roots().is_empty(), "Requested dependency already exists; updating the same identity requires explicit authorization");
        Ok::<_, anyhow::Error>(planned)
    })?;
    let planned = scope.accept(work.wait().await?)?.transpose()?;
    let (resources, retained) = project_change::resources(planned.bytes()?, config)?;
    let work = scope.spawn_blocking(resources, retained, move |cancel| {
        let (planned, _reservation) = planned.into_parts();
        let adoption = if matches!(kind, DependencyChange::Adopt) {
            let next = planned.candidate().project().lock();
            planned
                .candidate()
                .plan()
                .bindings()
                .values()
                .filter_map(|key| {
                    let summarize = |lock: &ResolutionLock| {
                        lock.dependencies
                            .get(key)
                            .map(|dependency| AdoptionResolution {
                                dependency: dependency.clone(),
                                required: lock.required_edges.get(key).cloned().unwrap_or_default(),
                                coverage: lock
                                    .coverage
                                    .get(key)
                                    .cloned()
                                    .unwrap_or(Coverage::Unknown),
                            })
                    };
                    let before = planned.prior_lock().and_then(summarize);
                    let after = summarize(next).expect("planned binding has a verified dependency");
                    (before.as_ref() != Some(&after)).then(|| AdoptionSelection {
                        key: key.clone(),
                        before,
                        after,
                    })
                })
                .collect()
        } else {
            Vec::new()
        };
        let addition = planned.stage(&cancel)?;
        let files = addition.files().clone();
        let view = AddPreview {
            plan: PlanId(
                NEXT_PLAN
                    .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
                    .map_err(|_| anyhow::anyhow!("Plan identifier exhausted"))?,
            ),
            bindings: addition.candidate().plan().bindings().clone(),
            existing_roots: addition.candidate().plan().existing_roots().clone(),
            replaced: addition.candidate().plan().replaced().clone(),
            incomplete_evidence: addition.candidate().plan().incomplete_evidence().to_vec(),
            existing: policy,
            references: addition.references().clone(),
            replacement: project_change::summary(&files)?,
            files,
        };
        Ok::<_, anyhow::Error>(PreparedAdditionOperation {
            view,
            addition,
            adoption,
        })
    })?;
    scope.accept(work.wait().await?)?.transpose()
}
impl super::mutation_cache::StagedMutation for PreparedAdditionOperation {
    fn cache_parts(&mut self) -> (&ResolvedProject, &mut crate::engine::staging::FrozenStage) {
        self.addition.cache_parts()
    }
}
pub(super) async fn run(
    prepared: RetainedOutput<PreparedAdditionOperation>,
    config: EngineConfig,
    cache: Option<crate::engine::content::cache::ContentCache>,
    mut scope: WorkScope,
) -> Result<ExecutionOutcome, RuntimeError> {
    let cancel = scope.cancellation();
    let result = async {
        let prepared = super::mutation_cache::publish(prepared, cache, &mut scope).await?;
        execute(prepared, config, &mut scope).await
    }
    .await;
    Ok(match result {
        Ok(receipt) => ExecutionOutcome::Completed(ExecutionReceipt::Add(Box::new(receipt))),
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
pub(super) async fn run_update(
    prepared: RetainedOutput<PreparedAdditionOperation>,
    config: EngineConfig,
    cache: Option<crate::engine::content::cache::ContentCache>,
    scope: WorkScope,
) -> Result<ExecutionOutcome, RuntimeError> {
    let selected = prepared.view.existing_roots.clone();
    Ok(match run(prepared, config, cache, scope).await? {
        ExecutionOutcome::Completed(ExecutionReceipt::Add(receipt)) => {
            ExecutionOutcome::Completed(ExecutionReceipt::Update(Box::new(receipt.map(|value| {
                UpdateReceipt {
                    plan: value.plan,
                    selected,
                    publication: value.publication,
                    project: value.project,
                    bindings: value.bindings,
                    references: value.references,
                }
            }))))
        }
        other => other,
    })
}
pub(super) async fn run_adoption(
    prepared: RetainedOutput<PreparedAdditionOperation>,
    config: EngineConfig,
    scope: WorkScope,
) -> Result<ExecutionOutcome, RuntimeError> {
    Ok(match run(prepared, config, None, scope).await? {
        ExecutionOutcome::Completed(ExecutionReceipt::Add(receipt)) => ExecutionOutcome::Completed(
            ExecutionReceipt::AdoptObserved(Box::new(receipt.map(|value| AdoptObservedReceipt {
                plan: value.plan,
                publication: value.publication,
                project: value.project,
                bindings: value.bindings,
            }))),
        ),
        other => other,
    })
}
async fn execute(
    prepared: RetainedOutput<PreparedAdditionOperation>,
    config: EngineConfig,
    scope: &mut WorkScope,
) -> Result<RetainedOutput<AddReceipt>> {
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
            .context("Addition publication size overflow")?;
            sum.checked_add(bytes)
                .context("Addition publication size overflow")
        })?;
    let mut resources = config.resources.assembly;
    resources.scratch_bytes = resources.scratch_bytes.max(bytes);
    let work = scope.spawn_blocking(resources, config.resources.receipt, move |cancel| {
        cancel.check()?;
        let (prepared, _reservation) = prepared.into_parts();
        let receipt = prepared
            .addition
            .publish(&Publisher::open(&config.state_root)?, &cancel)?;
        Ok::<_, anyhow::Error>(AddReceipt {
            plan: prepared.view.plan,
            publication: receipt.publication,
            project: receipt.project,
            bindings: prepared.view.bindings,
            references: prepared.view.references,
            replaced: prepared.view.replaced,
            incomplete_evidence: prepared.view.incomplete_evidence,
        })
    })?;
    scope
        .accept_publication(work.wait().await.map_err(PublicationWorkerFailed)?)?
        .transpose()
}
