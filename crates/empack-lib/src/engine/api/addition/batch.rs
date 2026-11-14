//! Verify connected groups separately and publish one coherent successful candidate.
use super::*;
use crate::engine::{
    addition::independent::{BatchPartition, independent_components},
    project::ProjectRevision,
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BatchPolicy {
    #[default]
    AllRequested,
    ContinueIndependent,
}
#[derive(Clone)]
pub enum DependencyBatchChange {
    Add(ExistingDependencyPolicy),
    Update {
        pins: BTreeMap<DependencyKey, empack_core::identity::PinSelector>,
    },
}
#[derive(Clone)]
pub struct DependencyBatchItem {
    pub group: AdditionGroup,
    pub content: DependencyContents,
}
pub struct DependencyBatchRequest {
    pub source_revision: Option<ProjectRevision>,
    pub change: DependencyBatchChange,
    pub policy: BatchPolicy,
    pub items: NonEmpty<DependencyBatchItem>,
}
#[derive(Debug, Clone)]
pub struct BlockedBatchGroup {
    /// Zero-based original request positions; a component retires as one unit.
    pub requests: Vec<usize>,
    pub cause: String,
    pub diagnostic: crate::engine::diagnostics::Diagnostic,
}
#[derive(Debug, Clone)]
pub struct DependencyBatchReport {
    /// Logical roots at each original request position.
    pub requested: Vec<Vec<DependencyKey>>,
    pub policy: BatchPolicy,
    /// Incomplete declared dependency evidence that prevents splitting the request.
    pub independence_unverified: Vec<DependencyKey>,
    /// Groups actually admitted to the one verified publication candidate.
    pub successful: Vec<Vec<usize>>,
    pub blocked: Vec<BlockedBatchGroup>,
}
#[derive(Debug)]
pub struct DependencyBatchIncomplete(pub DependencyBatchReport);
impl std::fmt::Display for DependencyBatchIncomplete {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Dependency batch incomplete: {} successful groups, {} blocked groups",
            self.0.successful.len(),
            self.0.blocked.len()
        )?;
        if !self.0.independence_unverified.is_empty() {
            f.write_str("; dependency information is incomplete, so these changes cannot be applied separately")?;
        }
        Ok(())
    }
}
impl std::error::Error for DependencyBatchIncomplete {}

pub(in crate::engine::api) async fn prepare_batch(
    project: ProjectTarget,
    request: DependencyBatchRequest,
    config: &EngineConfig,
    scope: &mut WorkScope,
) -> Result<RetainedOutput<PreparedAdditionOperation>> {
    ensure!(
        request.items.as_slice().len() <= 128,
        "Dependency batch exceeds 128 requests"
    );
    let ProjectTarget::Existing(path) = &project else {
        anyhow::bail!("Dependency batch requires an existing project");
    };
    let path = path.clone();
    let state = config.state_root.clone();
    let limits = config.snapshot;
    let expected = request.source_revision;
    let capture = scope.spawn_blocking(
        config.resources.capture,
        config.resources.prepared,
        move |cancel| {
            let snapshot = ProjectReader::new(RecoveryReader::new(state)).capture(
                &path,
                &[],
                limits,
                &cancel,
            )?;
            ensure!(
                expected.is_none_or(|expected| expected == snapshot.revision()),
                "Project documents changed during batch resolution"
            );
            let current = snapshot.require_resolved()?;
            let count = request
                .items
                .as_slice()
                .iter()
                .try_fold(0usize, |total, item| {
                    item.group
                        .dependencies()
                        .values()
                        .try_fold(total, |total, dependency| {
                            total
                                .checked_add(dependency.files.as_slice().len())
                                .context("Batch file count overflow")
                        })
                })?;
            ensure!(
                count <= limits.entries.min(8192),
                "Dependency batch exceeds the file inventory limit"
            );
            let groups: Vec<_> = request
                .items
                .as_slice()
                .iter()
                .map(|item| item.group.clone())
                .collect();
            if let DependencyBatchChange::Add(ExistingDependencyPolicy::ReplaceSelected(
                selection,
            )) = &request.change
            {
                ensure!(
                    selection.keys.as_slice().iter().all(|key| groups
                        .iter()
                        .any(|group| group.dependencies().contains_key(key))),
                    "Batch replacement selection contains an unrequested dependency"
                );
            }
            if let DependencyBatchChange::Update { pins } = &request.change {
                ensure!(
                    pins.keys()
                        .all(|key| current.intent().roots.contains_key(key)
                            && groups.iter().any(|group| group.roots().contains_key(key))),
                    "Exact update pins must name requested provider author roots"
                );
            }
            let components = match request.policy {
                BatchPolicy::AllRequested => BatchPartition {
                    components: vec![(0..groups.len()).collect()],
                    incomplete_dependencies: BTreeSet::new(),
                },
                BatchPolicy::ContinueIndependent => independent_components(&current, &groups)?,
            };
            cancel.check()?;
            Ok::<_, anyhow::Error>((current, snapshot.revision(), components, request))
        },
    )?;
    let captured = scope.accept(capture.wait().await?)?.transpose()?;
    let ((current, revision, components, request), _captured) = captured.into_parts();
    let mut report = DependencyBatchReport {
        requested: request
            .items
            .as_slice()
            .iter()
            .map(|item| item.group.roots().keys().cloned().collect())
            .collect(),
        policy: request.policy,
        independence_unverified: components.incomplete_dependencies.into_iter().collect(),
        successful: vec![],
        blocked: vec![],
    };
    let (kind, existing) = match request.change {
        DependencyBatchChange::Add(policy) => (DependencyChange::Add, policy),
        DependencyBatchChange::Update { pins } => (
            DependencyChange::Update(pins),
            ExistingDependencyPolicy::UpdateSameIdentity,
        ),
    };
    let components = components.components;
    let single_component = components.len() == 1;
    // Combining and native staging are read-only. No component owns a publisher.
    for component in components {
        scope.cancellation().check()?;
        let result = async {
            let merged = merge(
                &current,
                request.items.as_slice(),
                &component,
                revision,
                &existing,
            )?;
            let selected_kind = scoped_change(&kind, &merged.group);
            prepare_change(project.clone(), merged, selected_kind, config, scope).await
        }
        .await;
        match result {
            Ok(prepared) => {
                report.successful.push(component);
                if single_component {
                    return Ok(prepared.map(|mut prepared| {
                        prepared.view.batch = Some(report);
                        prepared
                    }));
                }
                drop(prepared);
            }
            Err(error) => {
                // Cancellation stops new groups without replacing the failure just observed.
                // Resource/lifecycle failure is not evidence against a requested dependency.
                if scope.cancellation().is_cancelled()
                    || error.is::<crate::application::process_runtime::Interrupted>()
                    || error.downcast_ref::<RuntimeError>().is_some()
                    || request.policy == BatchPolicy::AllRequested
                {
                    return Err(error);
                }
                report.blocked.push(BlockedBatchGroup {
                    requests: component,
                    cause: format!("{error:#}").chars().take(8192).collect(),
                    diagnostic: crate::engine::diagnostics::Diagnostic::from_error(
                        &error,
                        crate::engine::diagnostics::DiagnosticPhase::Preparation,
                    ),
                });
            }
        }
    }
    if report.successful.is_empty() {
        return Err(DependencyBatchIncomplete(report).into());
    }
    let selected: Vec<_> = report.successful.iter().flatten().copied().collect();
    let request = merge(
        &current,
        request.items.as_slice(),
        &selected,
        revision,
        &existing,
    )?;
    // Recompute once against the same document generation. This catches cross-component
    // invariants and retains failed groups' existing intent, placements and exact selections.
    let kind = scoped_change(&kind, &request.group);
    let prepared = prepare_change(project, request, kind, config, scope).await?;
    Ok(prepared.map(|mut prepared| {
        prepared.view.batch = Some(report);
        prepared
    }))
}
fn scoped_change(kind: &DependencyChange, group: &AdditionGroup) -> DependencyChange {
    match kind {
        DependencyChange::Update(pins) => DependencyChange::Update(
            pins.iter()
                .filter(|(key, _)| group.roots().contains_key(*key))
                .map(|(key, pin)| (key.clone(), pin.clone()))
                .collect(),
        ),
        other => other.clone(),
    }
}
fn merge(
    current: &ResolvedProject,
    items: &[DependencyBatchItem],
    selected: &[usize],
    revision: ProjectRevision,
    policy: &ExistingDependencyPolicy,
) -> Result<AddRequest> {
    let group = AdditionGroup::combine(
        current,
        &selected
            .iter()
            .map(|&i| &items[i].group)
            .collect::<Vec<_>>(),
    )?;
    let mut content = BTreeMap::new();
    for &index in selected {
        for (key, supplied) in &items[index].content {
            if let Some(previous) = content.get(key) {
                let same = match (previous, supplied) {
                    (DependencyContent::Reference, DependencyContent::Reference) => true,
                    (DependencyContent::Materialized(a), DependencyContent::Materialized(b)) => {
                        a.content.lease().id() == b.content.lease().id()
                            && a.permissions == b.permissions
                    }
                    _ => false,
                };
                ensure!(
                    same,
                    "Batch requests disagree about materialization of {key:?}"
                );
            } else {
                content.insert(key.clone(), supplied.clone());
            }
        }
    }
    let existing = match policy {
        ExistingDependencyPolicy::ReplaceSelected(selection) => {
            let keys: Vec<_> = selection
                .keys
                .as_slice()
                .iter()
                .filter(|key| group.dependencies().contains_key(*key))
                .cloned()
                .collect();
            if keys.is_empty() {
                ExistingDependencyPolicy::RejectExisting
            } else {
                ExistingDependencyPolicy::ReplaceSelected(ReplacementSelection {
                    keys: NonEmpty::new(keys)?,
                    evidence: selection.evidence,
                })
            }
        }
        policy => policy.clone(),
    };
    Ok(AddRequest {
        source_revision: Some(revision),
        group,
        content,
        existing,
    })
}
