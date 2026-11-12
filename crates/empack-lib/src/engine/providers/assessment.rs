//! Read-only update evidence; catalog rows and joint dependency validation are separate.
use super::*;
use anyhow::Context;
use empack_core::model::{
    DependencySelection, ProviderIncompatibility, ResolvedProject, SourceIntent,
};
use std::collections::{BTreeMap, BTreeSet};
use tokio::time::Instant;

mod report;
mod update;
pub use report::*;
pub use update::{
    ProviderUpdateAnalysis, UpdateAnalysisOptions, UpdateVersionChoices, UpdateVersionSelector,
};

struct ObservedUpdate {
    report: UpdateAssessment,
    selected_pins: BTreeMap<empack_core::model::DependencyKey, PinSelector>,
    closure: Option<ProviderClosure>,
    failure: Option<anyhow::Error>,
}

pub(super) fn reason(error: &anyhow::Error) -> UpdateReason {
    if error.is::<empack_core::identity::IdentityError>() {
        return UpdateReason::InvalidEvidence;
    }
    match error.downcast_ref::<CatalogError>() {
        Some(CatalogError::Limit) => UpdateReason::CatalogLimit,
        Some(CatalogError::Deadline) => UpdateReason::Deadline,
        Some(CatalogError::Unauthorized) => UpdateReason::Unauthorized,
        Some(CatalogError::NotFound) => UpdateReason::NotFound,
        Some(CatalogError::RateLimited) => UpdateReason::RateLimited,
        Some(CatalogError::NoCompatibleSelection) => UpdateReason::NoCompatibleSelection,
        Some(
            CatalogError::InvalidRecord
            | CatalogError::Identity
            | CatalogError::ContentKindMismatch
            | CatalogError::UnsupportedKind
            | CatalogError::IncompleteLookup,
        ) => UpdateReason::InvalidEvidence,
        _ => UpdateReason::ProviderUnavailable,
    }
}
impl ProviderCatalog {
    /// Assess requested logical dependencies, defaulting to explicit author roots.
    /// Every root and its required closure shares one transport allowance and deadline.
    /// Root-local errors remain unknown while independent queries share the original budget.
    /// The report is not an execution grant or a proof that alternative solutions do not exist.
    pub async fn assess_updates(
        &self,
        scope: &mut WorkScope,
        project: &ResolvedProject,
        request: AssessmentRequest,
        limits: ClosureLimits,
        progress: impl Fn(AssessmentProgress) + Send + Sync,
    ) -> Result<UpdateAssessment> {
        Ok(self
            .observe_updates(scope, project, request, limits, None, progress)
            .await?
            .report)
    }

    async fn observe_updates(
        &self,
        scope: &mut WorkScope,
        project: &ResolvedProject,
        mut request: AssessmentRequest,
        limits: ClosureLimits,
        mut selector: Option<&mut dyn UpdateVersionSelector>,
        progress: impl Fn(AssessmentProgress) + Send + Sync,
    ) -> Result<ObservedUpdate> {
        let policy = request.releases;
        let assessment_scope = request.scope(project)?;
        let selected: BTreeSet<_> = request.keys.iter().cloned().collect();
        ensure!(
            assessment_scope.roots.len() <= limits.projects,
            CatalogError::Limit
        );
        let initial_budget = transport::RequestBudget::new(limits.selection.catalog)?;
        let deadline = initial_budget.deadline();
        let mut budget = Some(initial_budget);
        let mut games = vec![project.intent().runtime.minecraft.clone()];
        games.extend(project.intent().runtime.acceptable_versions.clone());
        let games = NonEmpty::new(games)?;
        let mut report = UpdateAssessment {
            schema: 2,
            scope: assessment_scope,
            observed_at: time::OffsetDateTime::now_utc()
                .format(&time::format_description::well_known::Rfc3339)?,
            release_policy: policy
                .unwrap_or(project.intent().dependency_policy)
                .as_str(),
            dependencies: Vec::new(),
            joint: JointAssessment {
                scope: "provider-dependency-closure",
                declared_incompatibilities: Vec::new(),
                declarations_complete: false,
                state: "not-assessed",
                selections: Vec::new(),
                reasons: Vec::new(),
                blocking_roots: Vec::new(),
            },
        };
        let mut roots = Vec::new();
        let mut selected_pins = BTreeMap::new();
        let mut retained_closure = None;
        let mut failure = None;
        let mut complete = true;
        for (key, dependency) in &project.lock().dependencies {
            let intent = project.intent().roots.get(key);
            if (selected.is_empty() && intent.is_none())
                || (!selected.is_empty() && !selected.contains(key))
            {
                continue;
            }
            scope.cancellation().check()?;
            progress(AssessmentProgress::Catalog {
                key: key.as_str().into(),
                index: report.dependencies.len() + 1,
                total: report.scope.roots.len(),
                records: 0,
            });
            let locked = project.lock().dependencies.get(key);
            let current = locked.and_then(|dependency| dependency.selected.as_ref());
            let mut row = DependencyAssessment {
                release_policy: project.intent().release_policy(Some(key), policy).as_str(),
                release_policy_source: if policy.is_some() {
                    "operation-override"
                } else if intent.is_some_and(|intent| intent.releases.is_some()) {
                    "dependency"
                } else {
                    "project-default"
                },
                compatible_matches_release_policy: None,
                key: key.as_str().into(),
                name: locked.map_or_else(|| key.as_str().into(), |d| d.title.clone()),
                current: current.map(ObservedVersion::from_pin),
                permitted: None,
                compatible: None,
                upstream: None,
                state: "unknown",
                catalog_complete: false,
                observed_records: None,
                diagnostic: None,
                provider_problem: None,
                reasons: Vec::new(),
            };
            let Some(identity) = intent
                .and_then(|intent| intent.source.canonical_project(locked))
                .or(match &dependency.identity {
                    empack_core::model::ResolvedIdentity::Provider(id) => Some(id),
                    _ => None,
                })
            else {
                row.state = "manual";
                row.reasons.push(
                    if matches!(
                        intent.map(|intent| &intent.source),
                        Some(SourceIntent::Search { .. })
                    ) {
                        complete = false;
                        UpdateReason::UnresolvedSource
                    } else {
                        UpdateReason::ManualSource
                    },
                );
                report.dependencies.push(row);
                continue;
            };
            let Some(remaining) = budget.take() else {
                row.reasons.push(UpdateReason::NotAssessedAfterFailure);
                complete = false;
                report.dependencies.push(row);
                continue;
            };
            let query = CompatibleRequest {
                project: identity.clone(),
                kind: dependency.kind,
                game_versions: games.clone(),
                loader: project.intent().runtime.loader,
                releases: project.intent().release_policy(Some(key), policy),
            };
            let retained_budget = remaining.clone();
            let observe = |records| {
                progress(AssessmentProgress::Catalog {
                    key: key.as_str().into(),
                    index: report.dependencies.len() + 1,
                    total: report.scope.roots.len(),
                    records,
                })
            };
            match self
                .assess_versions_budget(
                    scope,
                    query,
                    limits.selection,
                    remaining,
                    Some(&observe),
                    if selector.is_some() {
                        super::compatible::CatalogScanMode::Choose
                    } else {
                        super::compatible::CatalogScanMode::Observe
                    },
                )
                .await
            {
                Ok((assessment, remaining)) => {
                    budget = Some(remaining);
                    row.catalog_complete = true;
                    row.observed_records = Some(assessment.observed_records);
                    row.compatible_matches_release_policy =
                        assessment.compatible.as_ref().map(|candidate| {
                            assessment
                                .permitted
                                .as_ref()
                                .is_some_and(|policy| candidate.pin == policy.pin)
                        });
                    row.compatible = assessment
                        .compatible
                        .as_ref()
                        .map(ObservedVersion::observation);
                    row.upstream = assessment
                        .upstream
                        .as_ref()
                        .map(ObservedVersion::observation);
                    let choices = assessment.choices();
                    if let Some(ref mut selector) = selector {
                        if choices.is_empty() {
                            row.state = "unavailable";
                            row.reasons.push(UpdateReason::NoCompatibleSelection);
                            complete = false;
                            failure = Some(CatalogError::NoCompatibleSelection.into());
                            report.dependencies.push(row);
                            continue;
                        }
                        let offered = UpdateVersionChoices {
                            key: key.clone(),
                            name: row.name.clone(),
                            current: current
                                .cloned()
                                .context("Provider dependency lacks an exact selection")?,
                            versions: choices.clone(),
                        };
                        let selected = selector.choose(&offered).await?;
                        scope.cancellation().check()?;
                        ensure!(
                            choices.iter().any(|value| value.pin.selection == selected
                                && &value.pin.project == identity),
                            "Selected version was not offered for {}",
                            key.as_str()
                        );
                        request.exact.insert(key.clone(), selected.clone());
                        selected_pins.insert(key.clone(), selected);
                        if let Err(error) = budget
                            .as_ref()
                            .context("Missing shared catalog budget")?
                            .check_deadline()
                        {
                            report.joint.reasons.push(UpdateReason::Deadline);
                            failure = Some(error);
                            complete = false;
                            budget = None;
                        }
                    }
                    let held = intent.and_then(|intent| match &intent.selection {
                        DependencySelection::ProviderPin(pin) => Some(pin),
                        _ => None,
                    });
                    let requested = request.exact.get(key);
                    let permitted = if let Some(pin) = requested.or(held) {
                        if requested.is_none() || requested == held {
                            row.state = "pinned";
                            row.reasons.push(UpdateReason::Pinned);
                        }
                        Some(ResolvedPin {
                            project: identity.clone(),
                            selection: pin.clone(),
                        })
                    } else {
                        assessment.permitted.as_ref().map(|value| value.pin.clone())
                    };
                    row.permitted = permitted.as_ref().map(|pin| {
                        choices
                            .iter()
                            .find(|value| &value.pin == pin)
                            .or_else(|| {
                                assessment
                                    .permitted
                                    .as_ref()
                                    .filter(|value| &value.pin == pin)
                            })
                            .map_or_else(
                                || ObservedVersion::from_pin(pin),
                                ObservedVersion::observation,
                            )
                    });
                    if let Some(pin) = permitted {
                        if row.state != "pinned" {
                            row.state = if current == Some(&pin) {
                                "unchanged"
                            } else {
                                "change-available"
                            };
                        }
                        roots.push(ClosureRoot {
                            pin,
                            kind: dependency.kind,
                        });
                    } else {
                        complete = false;
                        row.state = "unavailable";
                        row.reasons.push(if row.compatible.is_none() {
                            UpdateReason::NoCompatibleSelection
                        } else {
                            UpdateReason::ReleasePolicy
                        });
                    }
                }
                Err(error) => {
                    scope.cancellation().check()?;
                    let cause = reason(&error);
                    let local = matches!(
                        cause,
                        UpdateReason::InvalidEvidence
                            | UpdateReason::NotFound
                            | UpdateReason::NoCompatibleSelection
                    );
                    if local {
                        budget = Some(retained_budget);
                    }
                    row.provider_problem = error.downcast_ref::<ProviderRecordProblem>().cloned();
                    row.diagnostic = Some(format!(
                        "Dependency {} ({}): {error:#}",
                        key.as_str(),
                        identity
                    ));
                    row.reasons.push(cause);
                    if failure.is_none() || !local {
                        failure = Some(error);
                    }
                    complete = false;
                }
            }
            report.dependencies.push(row);
        }
        if !complete {
            report
                .joint
                .reasons
                .push(UpdateReason::IncompleteRootAssessment);
            report.joint.blocking_roots = report
                .dependencies
                .iter()
                .filter(|row| {
                    row.reasons != [UpdateReason::ManualSource]
                        && (!row.catalog_complete || row.permitted.is_none())
                })
                .map(|row| row.key.clone())
                .collect();
        }
        if complete && roots.is_empty() {
            report.joint.state = "not-applicable";
        } else if complete {
            progress(AssessmentProgress::RequiredClosure { roots: roots.len() });
            let closure = self
                .resolve_required_closure_observed(
                    scope,
                    ClosureRequest {
                        roots: NonEmpty::new(roots)?,
                        game_versions: games,
                        loader: project.intent().runtime.loader,
                        releases: policy.unwrap_or(project.intent().dependency_policy),
                    },
                    limits,
                    budget.expect("successful scans return their shared budget"),
                    (Some(project), false),
                    |identity| {
                        progress(AssessmentProgress::RequiredDependency {
                            project: project
                                .lock()
                                .dependencies
                                .iter()
                                .find(|(_, value)| {
                                    value
                                        .selected
                                        .as_ref()
                                        .is_some_and(|pin| &pin.project == identity)
                                })
                                .map_or_else(
                                    || identity.to_string(),
                                    |(key, _)| key.as_str().to_owned(),
                                ),
                        })
                    },
                )
                .await;
            match closure {
                Ok(closure) => {
                    progress(AssessmentProgress::DeclaredConflicts);
                    let (declarations, incomplete) = declared_incompatibilities(
                        scope,
                        project,
                        &closure,
                        limits.edges,
                        deadline,
                        Instant::now,
                    )?;
                    report.joint.declared_incompatibilities = declarations;
                    report.joint.declarations_complete = incomplete.is_none();
                    report.joint.reasons.extend(incomplete);
                    report.joint.selections = closure
                        .selections
                        .values()
                        .map(|selected| ObservedVersion::from_pin(&selected.resolution.pin))
                        .collect();
                    report.joint.state = if closure.complete_for_required() {
                        "validated"
                    } else {
                        "blocked"
                    };
                    let mut codes = BTreeSet::new();
                    for issue in &closure.issues {
                        let (key, reason) = match issue.kind {
                            ClosureIssueKind::SelectionConflict { .. }
                            | ClosureIssueKind::IncompatibleRequirement { .. } => {
                                ("conflict", UpdateReason::DependencyConflict)
                            }
                            _ => ("evidence", UpdateReason::IncompleteDependencyEvidence),
                        };
                        if codes.insert(key) {
                            report.joint.reasons.push(reason);
                        }
                    }
                    retained_closure = Some(closure);
                }
                Err(error) => {
                    scope.cancellation().check()?;
                    report.joint.state = "unknown";
                    report.joint.reasons.push(reason(&error));
                    failure = Some(error);
                }
            }
        }
        scope.cancellation().check()?;
        Ok(ObservedUpdate {
            report,
            selected_pins,
            closure: retained_closure,
            failure,
        })
    }
}

fn declared_incompatibilities(
    scope: &WorkScope,
    project: &ResolvedProject,
    closure: &ProviderClosure,
    maximum: usize,
    deadline: Instant,
    mut now: impl FnMut() -> Instant,
) -> Result<(Vec<DeclaredIncompatibility>, Option<UpdateReason>)> {
    let mut within_deadline = || -> Result<bool> {
        scope.cancellation().check()?;
        Ok(now() < deadline)
    };
    let mut candidate: BTreeMap<_, (ResolvedPin, Vec<ProviderIncompatibility>)> = BTreeMap::new();
    for dependency in project.lock().dependencies.values() {
        if !within_deadline()? {
            return Ok((Vec::new(), Some(UpdateReason::Deadline)));
        }
        if let Some(pin) = &dependency.selected {
            candidate.insert(
                pin.project.clone(),
                (pin.clone(), dependency.incompatibilities.clone()),
            );
        }
    }
    for (id, node) in &closure.selections {
        if !within_deadline()? {
            return Ok((Vec::new(), Some(UpdateReason::Deadline)));
        }
        let mut assertions = node
            .resolution
            .dependencies
            .iter()
            .filter_map(ProviderDependency::incompatibility)
            .collect::<Vec<_>>();
        if let Some((prior, retained)) = candidate.get(id)
            && prior == &node.resolution.pin
        {
            for assertion in retained {
                if !within_deadline()? {
                    return Ok((Vec::new(), Some(UpdateReason::Deadline)));
                }
                if !assertions.contains(assertion) {
                    assertions.push(assertion.clone());
                }
            }
        }
        candidate.insert(id.clone(), (node.resolution.pin.clone(), assertions));
    }
    let mut declarations = Vec::new();
    for (from, assertions) in candidate.values() {
        if !within_deadline()? {
            return Ok((declarations, Some(UpdateReason::Deadline)));
        }
        if assertions.is_empty() {
            continue;
        }
        for (target, _) in candidate.values() {
            if !within_deadline()? {
                return Ok((declarations, Some(UpdateReason::Deadline)));
            }
            if assertions.iter().any(|assertion| assertion.matches(target)) {
                if declarations.len() == maximum {
                    return Ok((declarations, Some(UpdateReason::CatalogLimit)));
                }
                declarations.push(DeclaredIncompatibility {
                    from: ObservedVersion::from_pin(from),
                    target: ObservedVersion::from_pin(target),
                });
            }
        }
    }
    Ok((declarations, None))
}

#[cfg(test)]
mod tests;
