//! Read-only update evidence; catalog rows and joint dependency validation are separate.
use super::*;
use empack_core::model::{
    DependencySelection, ProviderIncompatibility, ResolvedProject, SourceIntent,
};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use tokio::time::Instant;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum UpdateReason {
    Pinned,
    ManualSource,
    UnresolvedSource,
    NoCompatibleSelection,
    ReleasePolicy,
    CatalogLimit,
    Deadline,
    Unauthorized,
    NotFound,
    RateLimited,
    InvalidEvidence,
    ProviderUnavailable,
    NotAssessedAfterFailure,
    IncompleteDependencyEvidence,
    DependencyConflict,
}
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ObservedVersion {
    pub provider: &'static str,
    pub project: String,
    pub selection: String,
    pub version: Option<String>,
    pub published_at: Option<String>,
    pub channel: Option<&'static str>,
}
impl ObservedVersion {
    pub fn from_pin(pin: &ResolvedPin) -> Self {
        Self {
            provider: match pin.project {
                ProviderProjectId::Modrinth(_) => "modrinth",
                ProviderProjectId::CurseForge(_) => "curseforge",
            },
            project: pin.project.to_string(),
            selection: match &pin.selection {
                PinSelector::ModrinthVersion(id) => id.to_string(),
                PinSelector::CurseForgeFile(id) => id.to_string(),
            },
            version: None,
            published_at: None,
            channel: None,
        }
    }
    fn observation(value: &SelectionObservation) -> Self {
        Self {
            version: value.version.clone(),
            published_at: Some(value.published_at.clone()),
            channel: Some(match value.channel {
                ReleaseChannel::Release => "release",
                ReleaseChannel::Beta => "beta",
                ReleaseChannel::Alpha => "alpha",
            }),
            ..Self::from_pin(&value.pin)
        }
    }
}
#[derive(Debug, Serialize)]
pub struct DependencyAssessment {
    /// Effective automatic preference for this root, including operation override.
    pub release_policy: &'static str,
    pub key: String,
    pub name: String,
    pub current: Option<ObservedVersion>,
    pub permitted: Option<ObservedVersion>,
    pub compatible: Option<ObservedVersion>,
    pub upstream: Option<ObservedVersion>,
    pub state: &'static str,
    pub catalog_complete: bool,
    pub observed_records: Option<usize>,
    pub diagnostic: Option<String>,
    pub reasons: Vec<UpdateReason>,
}
/// Known provider declaration between candidate selections; participation is not assessed.
#[derive(Debug, Serialize)]
pub struct DeclaredIncompatibility {
    pub from: ObservedVersion,
    pub target: ObservedVersion,
}
#[derive(Debug, Serialize)]
pub struct JointAssessment {
    /// This validates required relationships, not complete inventory compatibility.
    pub scope: &'static str,
    /// Final placements and retained-inventory coexistence require update preparation.
    pub candidate_compatibility: &'static str,
    pub declared_incompatibilities: Vec<DeclaredIncompatibility>,
    /// False if the declaration scan did not run, timed out or exhausted its allowance.
    pub declarations_complete: bool,
    pub state: &'static str,
    pub selections: Vec<ObservedVersion>,
    pub reasons: Vec<UpdateReason>,
}
#[derive(Debug, Serialize)]
pub struct UpdateAssessment {
    pub schema: u32,
    pub observed_at: String,
    pub release_policy: &'static str,
    pub dependencies: Vec<DependencyAssessment>,
    pub joint: JointAssessment,
}
impl UpdateAssessment {
    /// Completeness applies to provider assessment, not installation or runtime verification.
    pub fn is_complete(&self) -> bool {
        self.dependencies
            .iter()
            .all(|row| row.catalog_complete || row.reasons == [UpdateReason::ManualSource])
            && (self.joint.state == "not-applicable"
                || (self.joint.state == "validated" && self.joint.declarations_complete))
    }
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
    /// Assess explicit author roots under one transport allowance and deadline.
    /// Root-local errors remain unknown while independent queries share the original budget.
    /// The report is not an execution grant or a proof that alternative solutions do not exist.
    pub async fn assess_updates(
        &self,
        scope: &mut WorkScope,
        project: &ResolvedProject,
        policy: impl Into<Option<ReleasePolicy>> + Send,
        limits: ClosureLimits,
    ) -> Result<UpdateAssessment> {
        let policy = policy.into();
        ensure!(
            project.intent().roots.len() <= limits.projects,
            CatalogError::Limit
        );
        let initial_budget = transport::RequestBudget::new(limits.selection.catalog)?;
        let deadline = initial_budget.deadline();
        let mut budget = Some(initial_budget);
        let mut games = vec![project.intent().runtime.minecraft.clone()];
        games.extend(project.intent().runtime.acceptable_versions.clone());
        let games = NonEmpty::new(games)?;
        let mut report = UpdateAssessment {
            schema: 1,
            observed_at: time::OffsetDateTime::now_utc()
                .format(&time::format_description::well_known::Rfc3339)?,
            release_policy: policy
                .unwrap_or(project.intent().dependency_policy)
                .as_str(),
            dependencies: Vec::new(),
            joint: JointAssessment {
                scope: "provider-dependency-closure",
                candidate_compatibility: "not-assessed",
                declared_incompatibilities: Vec::new(),
                declarations_complete: false,
                state: "not-assessed",
                selections: Vec::new(),
                reasons: Vec::new(),
            },
        };
        let mut roots = Vec::new();
        let mut complete = true;
        for (key, intent) in &project.intent().roots {
            scope.cancellation().check()?;
            let locked = project.lock().dependencies.get(key);
            let current = locked.and_then(|dependency| dependency.selected.as_ref());
            let mut row = DependencyAssessment {
                release_policy: project.intent().release_policy(Some(key), policy).as_str(),
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
                reasons: Vec::new(),
            };
            let Some(identity) = intent.source.canonical_project(locked) else {
                row.state = "manual";
                row.reasons
                    .push(if matches!(intent.source, SourceIntent::Search { .. }) {
                        complete = false;
                        UpdateReason::UnresolvedSource
                    } else {
                        UpdateReason::ManualSource
                    });
                report.dependencies.push(row);
                continue;
            };
            let Some(remaining) = budget.take() else {
                row.reasons.push(UpdateReason::NotAssessedAfterFailure);
                complete = false;
                report.dependencies.push(row);
                continue;
            };
            let request = CompatibleRequest {
                project: identity.clone(),
                kind: intent.kind,
                game_versions: games.clone(),
                loader: project.intent().runtime.loader,
                releases: project.intent().release_policy(Some(key), policy),
            };
            let retained_budget = remaining.clone();
            match self
                .assess_versions_budget(scope, request, limits.selection, remaining)
                .await
            {
                Ok((assessment, remaining)) => {
                    budget = Some(remaining);
                    row.catalog_complete = true;
                    row.observed_records = Some(assessment.observed_records);
                    row.compatible = assessment
                        .compatible
                        .as_ref()
                        .map(ObservedVersion::observation);
                    row.upstream = assessment
                        .upstream
                        .as_ref()
                        .map(ObservedVersion::observation);
                    let permitted = if let DependencySelection::ProviderPin(pin) = &intent.selection
                    {
                        row.state = "pinned";
                        row.reasons.push(UpdateReason::Pinned);
                        Some(ResolvedPin {
                            project: identity.clone(),
                            selection: pin.clone(),
                        })
                    } else {
                        assessment.permitted.as_ref().map(|value| value.pin.clone())
                    };
                    row.permitted = permitted.as_ref().map(|pin| {
                        assessment
                            .permitted
                            .as_ref()
                            .filter(|value| &value.pin == pin)
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
                            kind: intent.kind,
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
                    if matches!(
                        cause,
                        UpdateReason::InvalidEvidence
                            | UpdateReason::NotFound
                            | UpdateReason::NoCompatibleSelection
                    ) {
                        budget = Some(retained_budget);
                    }
                    row.diagnostic = Some(format!(
                        "Dependency {} ({}): {error:#}",
                        key.as_str(),
                        identity
                    ));
                    row.reasons.push(cause);
                    complete = false;
                }
            }
            report.dependencies.push(row);
        }
        if complete && roots.is_empty() {
            report.joint.state = "not-applicable";
        } else if complete {
            let closure = self
                .resolve_required_closure_budget(
                    scope,
                    ClosureRequest {
                        roots: NonEmpty::new(roots)?,
                        game_versions: games,
                        loader: project.intent().runtime.loader,
                        releases: policy.unwrap_or(project.intent().dependency_policy),
                    },
                    limits,
                    budget.expect("successful scans return their shared budget"),
                    Some(project),
                    false,
                )
                .await;
            match closure {
                Ok(closure) => {
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
                    for issue in closure.issues {
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
                }
                Err(error) => {
                    scope.cancellation().check()?;
                    report.joint.state = "unknown";
                    report.joint.reasons.push(reason(&error));
                }
            }
        }
        scope.cancellation().check()?;
        Ok(report)
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
