//! Required dependency evidence, independent of placement, installation and garbage collection.
use super::*;
use crate::engine::resources::AdmissionPermit;
use empack_core::model::{GameVersion, LoaderKind};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone)]
pub struct ClosureRoot {
    pub pin: ResolvedPin,
    pub kind: ContentKind,
}
#[derive(Clone)]
pub struct ClosureRequest {
    pub roots: NonEmpty<ClosureRoot>,
    pub game_versions: NonEmpty<GameVersion>,
    pub loader: LoaderKind,
    pub releases: ReleasePolicy,
}
#[derive(Clone, Copy)]
pub struct ClosureLimits {
    /// One transport allowance and deadline across every root, edge and retry.
    pub selection: SelectionLimits,
    pub projects: usize,
    pub edges: usize,
}
impl Default for ClosureLimits {
    fn default() -> Self {
        Self {
            selection: SelectionLimits {
                catalog: CatalogLimits {
                    transfer_bytes: 256 << 20,
                    deadline: Duration::from_secs(300),
                    ..Default::default()
                },
                ..Default::default()
            },
            projects: 1024,
            edges: 8192,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClosureIssueKind {
    IncompleteEvidence(Coverage),
    MissingIdentity,
    AmbiguousContentKind,
    /// The observed selection conflicts; this is not a proof that no solution exists.
    SelectionConflict {
        selected: ResolvedPin,
        required: PinSelector,
    },
    IncompatibleSelection {
        selected: ResolvedPin,
    },
    IncompatibleRequirement {
        required: ResolvedPin,
    },
    UninterpretedRelation,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClosureIssue {
    pub from: ResolvedPin,
    pub dependency: Option<ProviderDependency>,
    pub kind: ClosureIssueKind,
}
pub struct ClosureSelection {
    pub kind: ContentKind,
    pub resolution: RetainedOutput<ProviderResolution>,
}
pub struct ProviderClosure {
    pub roots: BTreeSet<ProviderProjectId>,
    pub selections: BTreeMap<ProviderProjectId, ClosureSelection>,
    pub required_edges: BTreeMap<ResolvedPin, BTreeSet<ResolvedPin>>,
    pub issues: Vec<ClosureIssue>,
    _graph: Vec<AdmissionPermit>,
}
impl ProviderClosure {
    /// Complete only for these exact selections. Never authorizes deletion of unlisted content.
    pub fn complete_for_required(&self) -> bool {
        self.issues.is_empty()
    }
}
impl ProviderCatalog {
    /// Expand required records under one budget. Cycles are valid; conflicting observed pins
    /// and incomplete evidence remain explicit issues, never silently omitted dependencies.
    pub async fn resolve_required_closure(
        &self,
        scope: &mut WorkScope,
        request: ClosureRequest,
        limits: ClosureLimits,
    ) -> Result<ProviderClosure> {
        ensure!(
            limits.projects > 0
                && limits.edges > 0
                && request.game_versions.as_slice().len() <= 16
                && request
                    .game_versions
                    .as_slice()
                    .iter()
                    .all(|v| v.as_str().len() <= 128),
            CatalogError::Limit
        );
        let mut result = ProviderClosure {
            roots: BTreeSet::new(),
            selections: BTreeMap::new(),
            required_edges: BTreeMap::new(),
            issues: Vec::new(),
            _graph: Vec::new(),
        };
        let mut budget = transport::RequestBudget::new(limits.selection.catalog)?;
        let mut queue = Vec::new();
        // Resolve explicit roots first so a transitive choice cannot displace a requested pin.
        for root in request.roots.as_slice() {
            scope.cancellation().check()?;
            budget.check_deadline()?;
            root.pin.validate()?;
            if let Some(previous) = result.selections.get(&root.pin.project) {
                ensure!(
                    previous.resolution.pin == root.pin,
                    "Explicit roots select conflicting pins for {}",
                    root.pin.project
                );
                check_selection(&previous.resolution, root.kind, &request)?;
                continue;
            }
            ensure!(
                result.selections.len() < limits.projects,
                CatalogError::Limit
            );
            result._graph.push(graph_reservation(scope, 4096)?);
            let (selected, next) = self
                .resolve_exact_budget(scope, root.pin.clone(), limits.selection.catalog, budget)
                .await?;
            budget = next;
            check_selection(&selected, root.kind, &request)?;
            result.roots.insert(root.pin.project.clone());
            queue.push(root.pin.project.clone());
            result.selections.insert(
                root.pin.project.clone(),
                ClosureSelection {
                    kind: root.kind,
                    resolution: selected,
                },
            );
        }
        let mut cursor = 0;
        let mut edge_count = 0usize;
        while cursor < queue.len() {
            scope.cancellation().check()?;
            budget.check_deadline()?;
            let parent = &result.selections[&queue[cursor]];
            let from = parent.resolution.pin.clone();
            let parent_kind = parent.kind;
            let coverage = parent.resolution.coverage;
            edge_count = edge_count
                .checked_add(parent.resolution.dependencies.len())
                .ok_or(CatalogError::Limit)?;
            ensure!(edge_count <= limits.edges, CatalogError::Limit);
            let charge = (parent.resolution.dependencies.len() as u64)
                .checked_mul(1024)
                .ok_or(CatalogError::Limit)?;
            result._graph.push(graph_reservation(scope, charge)?);
            let dependencies = parent.resolution.dependencies.clone();
            if coverage != Coverage::CompleteForSelection {
                result.issues.push(ClosureIssue {
                    from: from.clone(),
                    dependency: None,
                    kind: ClosureIssueKind::IncompleteEvidence(coverage),
                });
            }
            for dependency in dependencies {
                scope.cancellation().check()?;
                budget.check_deadline()?;
                match dependency.relation {
                    DependencyRelation::Required => {}
                    DependencyRelation::Include | DependencyRelation::Tool => {
                        result.issues.push(ClosureIssue {
                            from: from.clone(),
                            dependency: Some(dependency),
                            kind: ClosureIssueKind::UninterpretedRelation,
                        });
                        continue;
                    }
                    _ => continue,
                }
                let existing = dependency
                    .project
                    .as_ref()
                    .and_then(|id| result.selections.get(id))
                    .or_else(|| {
                        dependency.pin.as_ref().and_then(|pin| {
                            result
                                .selections
                                .values()
                                .find(|node| &node.resolution.pin.selection == pin)
                        })
                    });
                let selected_pin = if let Some(existing) = existing {
                    if let Some(required) = &dependency.pin
                        && *required != existing.resolution.pin.selection
                    {
                        result.issues.push(ClosureIssue {
                            from: from.clone(),
                            dependency: Some(dependency.clone()),
                            kind: ClosureIssueKind::SelectionConflict {
                                selected: existing.resolution.pin.clone(),
                                required: required.clone(),
                            },
                        });
                        continue;
                    }
                    if let Some(owner) = &dependency.project {
                        ensure!(
                            *owner == existing.resolution.pin.project,
                            CatalogError::Identity
                        );
                    }
                    existing.resolution.pin.clone()
                } else {
                    if dependency
                        .project
                        .as_ref()
                        .is_some_and(|owner| !result.selections.contains_key(owner))
                    {
                        ensure!(
                            result.selections.len() < limits.projects,
                            CatalogError::Limit
                        );
                    }
                    let selected = match (&dependency.project, &dependency.pin) {
                        (Some(project), Some(pin)) => {
                            let (value, next) = self
                                .resolve_exact_budget(
                                    scope,
                                    ResolvedPin {
                                        project: project.clone(),
                                        selection: pin.clone(),
                                    },
                                    limits.selection.catalog,
                                    budget,
                                )
                                .await?;
                            budget = next;
                            value
                        }
                        (None, Some(pin)) => {
                            let (value, next) = self
                                .resolve_pin_budget(
                                    scope,
                                    pin.clone(),
                                    limits.selection.catalog,
                                    budget,
                                )
                                .await?;
                            budget = next;
                            value
                        }
                        (Some(project), None) => {
                            let (project_record, next) = self
                                .resolve_selector_budget(
                                    scope,
                                    ProjectSelector::canonical(project.clone()),
                                    limits.selection.catalog,
                                    budget,
                                )
                                .await?;
                            budget = next;
                            let Some(kind) =
                                choose_kind(project_record.kinds.as_slice(), parent_kind)
                            else {
                                result.issues.push(ClosureIssue {
                                    from: from.clone(),
                                    dependency: Some(dependency.clone()),
                                    kind: ClosureIssueKind::AmbiguousContentKind,
                                });
                                continue;
                            };
                            drop(project_record);
                            let (value, next) = self
                                .resolve_compatible_budget(
                                    scope,
                                    CompatibleRequest {
                                        project: project.clone(),
                                        kind,
                                        game_versions: request.game_versions.clone(),
                                        loader: request.loader,
                                        releases: request.releases,
                                    },
                                    limits.selection,
                                    budget,
                                )
                                .await?;
                            budget = next;
                            value.map(|selected| selected.resolution)
                        }
                        (None, None) => {
                            result.issues.push(ClosureIssue {
                                from: from.clone(),
                                dependency: Some(dependency.clone()),
                                kind: ClosureIssueKind::MissingIdentity,
                            });
                            continue;
                        }
                    };
                    if let Some(previous) = result.selections.get(&selected.pin.project) {
                        if previous.resolution.pin != selected.pin {
                            result.issues.push(ClosureIssue {
                                from: from.clone(),
                                dependency: Some(dependency.clone()),
                                kind: ClosureIssueKind::SelectionConflict {
                                    selected: previous.resolution.pin.clone(),
                                    required: selected.pin.selection.clone(),
                                },
                            });
                            continue;
                        }
                        previous.resolution.pin.clone()
                    } else {
                        let Some(kind) = choose_kind(selected.kinds.as_slice(), parent_kind) else {
                            result.issues.push(ClosureIssue {
                                from: from.clone(),
                                dependency: Some(dependency.clone()),
                                kind: ClosureIssueKind::AmbiguousContentKind,
                            });
                            continue;
                        };
                        if check_selection(&selected, kind, &request).is_err() {
                            result.issues.push(ClosureIssue {
                                from: from.clone(),
                                dependency: Some(dependency.clone()),
                                kind: ClosureIssueKind::IncompatibleRequirement {
                                    required: selected.pin.clone(),
                                },
                            });
                            continue;
                        }
                        ensure!(
                            result.selections.len() < limits.projects,
                            CatalogError::Limit
                        );
                        result._graph.push(graph_reservation(scope, 4096)?);
                        let pin = selected.pin.clone();
                        queue.push(pin.project.clone());
                        result.selections.insert(
                            pin.project.clone(),
                            ClosureSelection {
                                kind,
                                resolution: selected,
                            },
                        );
                        pin
                    }
                };
                result
                    .required_edges
                    .entry(from.clone())
                    .or_default()
                    .insert(selected_pin);
            }
            cursor += 1;
        }
        // Incompatibilities may reference a selection discovered after their source was scanned.
        for node in result.selections.values() {
            scope.cancellation().check()?;
            budget.check_deadline()?;
            for dependency in &node.resolution.dependencies {
                if dependency.relation != DependencyRelation::Incompatible {
                    continue;
                }
                let conflicting = result.selections.values().find(|target| {
                    (dependency.project.is_some() || dependency.pin.is_some())
                        && dependency
                            .project
                            .as_ref()
                            .is_none_or(|id| id == &target.resolution.pin.project)
                        && dependency
                            .pin
                            .as_ref()
                            .is_none_or(|pin| pin == &target.resolution.pin.selection)
                });
                if let Some(conflicting) = conflicting {
                    result.issues.push(ClosureIssue {
                        from: node.resolution.pin.clone(),
                        dependency: Some(dependency.clone()),
                        kind: ClosureIssueKind::IncompatibleSelection {
                            selected: conflicting.resolution.pin.clone(),
                        },
                    });
                }
            }
        }
        scope.cancellation().check()?;
        budget.check_deadline()?;
        Ok(result)
    }
}
fn graph_reservation(scope: &WorkScope, bytes: u64) -> Result<AdmissionPermit> {
    Ok(scope.reserve_storage(ResourceRequest {
        memory_bytes: bytes,
        ..Default::default()
    })?)
}
fn choose_kind(kinds: &[ContentKind], parent: ContentKind) -> Option<ContentKind> {
    if kinds.contains(&parent) {
        Some(parent)
    } else if kinds.len() == 1 {
        Some(kinds[0])
    } else {
        None
    }
}
fn check_selection(
    selected: &ProviderResolution,
    kind: ContentKind,
    request: &ClosureRequest,
) -> Result<()> {
    ensure!(
        selected.kinds.as_slice().contains(&kind),
        CatalogError::ContentKindMismatch
    );
    ensure!(
        request
            .game_versions
            .as_slice()
            .iter()
            .any(|game| selected.game_versions.iter().any(|v| v == game.as_str()))
            && compatible::loader_matches(
                selected,
                &CompatibleRequest {
                    project: selected.pin.project.clone(),
                    kind,
                    game_versions: request.game_versions.clone(),
                    loader: request.loader,
                    releases: request.releases
                }
            ),
        CatalogError::NoCompatibleSelection
    );
    Ok(())
}
#[cfg(test)]
mod tests;
