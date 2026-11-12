//! Shared assessment facts and derived summaries, independent of CLI formatting.
use crate::engine::providers::{ReleaseChannel, ReleasePolicy, ResolvedPin, SelectionObservation};
use empack_core::identity::{PinSelector, ProviderProjectId};
use serde::Serialize;

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
    IncompleteRootAssessment,
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
    pub(super) fn observation(value: &SelectionObservation) -> Self {
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
    pub release_policy_source: &'static str,
    /// Whether the runtime-compatible observation matches the automatic policy choice, before pins.
    pub compatible_matches_release_policy: Option<bool>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_problem: Option<crate::engine::providers::ProviderRecordProblem>,
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
    pub declared_incompatibilities: Vec<DeclaredIncompatibility>,
    /// False if the declaration scan did not run, timed out or exhausted its allowance.
    pub declarations_complete: bool,
    pub state: &'static str,
    pub selections: Vec<ObservedVersion>,
    pub reasons: Vec<UpdateReason>,
    pub blocking_roots: Vec<String>,
}
#[derive(Debug)]
pub struct UpdateAssessment {
    pub schema: u32,
    pub observed_at: String,
    pub release_policy: &'static str,
    pub scope: AssessmentScope,
    pub dependencies: Vec<DependencyAssessment>,
    pub joint: JointAssessment,
}
impl UpdateAssessment {
    pub const CATALOG_VALIDATION_SCOPE: &'static str = "runtime-compatible-payloads";
    /// Completeness applies to provider assessment, not installation or runtime verification.
    pub fn is_complete(&self) -> bool {
        self.dependencies
            .iter()
            .all(|row| row.catalog_complete || row.reasons == [UpdateReason::ManualSource])
            && (self.joint.state == "not-applicable"
                || (self.joint.state == "validated" && self.joint.declarations_complete))
    }
}

/// Exact existing logical keys; an empty selection assesses every explicit author root.
#[derive(Default)]
pub struct AssessmentRequest {
    pub keys: Vec<empack_core::model::DependencyKey>,
    /// Explicit exact selections for this analysis; changing author policy needs a separate request.
    pub exact: std::collections::BTreeMap<
        empack_core::model::DependencyKey,
        empack_core::identity::PinSelector,
    >,
    pub releases: Option<ReleasePolicy>,
}

#[derive(Debug, Serialize)]
pub struct AssessmentScope {
    /// All author roots, selected author roots or selected retained dependencies; never a gameplay claim.
    pub kind: &'static str,
    pub roots: Vec<String>,
    pub total_roots: usize,
    /// Locked selections outside the selected roots, not independent catalog scans.
    pub retained_inventory: usize,
}

/// Read-only observations for a host's progress display, never an execution grant.
#[derive(Debug, Clone)]
pub enum AssessmentProgress {
    Catalog {
        key: String,
        index: usize,
        total: usize,
        records: usize,
    },
    RequiredClosure {
        roots: usize,
    },
    RequiredDependency {
        project: String,
    },
    DeclaredConflicts,
}

impl From<AssessmentProgress> for crate::engine::runtime::WorkProgress {
    fn from(value: AssessmentProgress) -> Self {
        match value {
            AssessmentProgress::Catalog {
                key,
                index,
                total,
                records,
            } => Self::CheckingDependency {
                key,
                index,
                total,
                versions: records,
            },
            AssessmentProgress::RequiredClosure { .. } => {
                Self::CheckingRequirements { dependency: None }
            }
            AssessmentProgress::RequiredDependency { project } => Self::CheckingRequirements {
                dependency: Some(project),
            },
            AssessmentProgress::DeclaredConflicts => Self::CheckingConflicts,
        }
    }
}

#[derive(Debug, Default, PartialEq, Eq, Serialize)]
pub struct AssessmentSummary {
    pub change_available: usize,
    pub held: usize,
    pub unknown: usize,
    pub manual: usize,
    pub unavailable: usize,
    pub conflicted: usize,
}
impl ObservedVersion {
    pub(crate) fn same_selection(&self, other: &Self) -> bool {
        self.provider == other.provider
            && self.project == other.project
            && self.selection == other.selection
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssessmentCategory {
    ChangeAvailable,
    Held,
    Unknown,
    Manual,
    Unavailable,
    Conflicted,
}
impl DependencyAssessment {
    pub fn category(&self) -> AssessmentCategory {
        if self.reasons == [UpdateReason::ManualSource] {
            AssessmentCategory::Manual
        } else if self.reasons.contains(&UpdateReason::DependencyConflict) {
            AssessmentCategory::Conflicted
        } else if !self.catalog_complete {
            AssessmentCategory::Unknown
        } else if self.permitted.is_none() {
            AssessmentCategory::Unavailable
        } else if self.reasons.contains(&UpdateReason::Pinned)
            || self
                .current
                .as_ref()
                .zip(self.permitted.as_ref())
                .is_some_and(|(a, b)| a.same_selection(b))
        {
            AssessmentCategory::Held
        } else {
            AssessmentCategory::ChangeAvailable
        }
    }
}
impl UpdateAssessment {
    /// Disjoint row counts; observation labels and timestamps do not change identity.
    pub fn summary(&self) -> AssessmentSummary {
        let mut summary = AssessmentSummary::default();
        for row in &self.dependencies {
            match row.category() {
                AssessmentCategory::ChangeAvailable => summary.change_available += 1,
                AssessmentCategory::Held => summary.held += 1,
                AssessmentCategory::Unknown => summary.unknown += 1,
                AssessmentCategory::Manual => summary.manual += 1,
                AssessmentCategory::Unavailable => summary.unavailable += 1,
                AssessmentCategory::Conflicted => summary.conflicted += 1,
            }
        }
        summary
    }
}
impl Serialize for UpdateAssessment {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut report = serializer.serialize_struct("UpdateAssessment", 9)?;
        report.serialize_field("schema", &self.schema)?;
        report.serialize_field("observed_at", &self.observed_at)?;
        report.serialize_field("release_policy", &self.release_policy)?;
        report.serialize_field("scope", &self.scope)?;
        report.serialize_field("catalog_validation", Self::CATALOG_VALIDATION_SCOPE)?;
        report.serialize_field("complete", &self.is_complete())?;
        report.serialize_field("summary", &self.summary())?;
        report.serialize_field("dependencies", &self.dependencies)?;
        report.serialize_field("joint", &self.joint)?;
        report.end()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DependencySelectionError {
    #[error("Unknown dependency key: {}", .0.as_str())]
    Missing(empack_core::model::DependencyKey),
    #[error("Dependency listed more than once: {}", .0.as_str())]
    Duplicate(empack_core::model::DependencyKey),
}
impl AssessmentRequest {
    /// Validate exact keys locally before opening any catalog request.
    pub fn scope(
        &self,
        project: &empack_core::model::ResolvedProject,
    ) -> anyhow::Result<AssessmentScope> {
        let mut selected = std::collections::BTreeSet::new();
        for key in &self.keys {
            if !project.lock().dependencies.contains_key(key) {
                return Err(DependencySelectionError::Missing(key.clone()).into());
            }
            if !selected.insert(key) {
                return Err(DependencySelectionError::Duplicate(key.clone()).into());
            }
        }
        for (key, selection) in &self.exact {
            anyhow::ensure!(
                project.lock().dependencies.contains_key(key)
                    && (if selected.is_empty() {
                        project.intent().roots.contains_key(key)
                    } else {
                        selected.contains(key)
                    }),
                "Exact selection must name a requested dependency: {}",
                key.as_str()
            );
            let identity = project.lock().dependencies[key]
                .selected
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Not a provider dependency: {}", key.as_str()))?;
            ResolvedPin {
                project: identity.project.clone(),
                selection: selection.clone(),
            }
            .validate()?;
        }
        let roots: Vec<_> = project
            .lock()
            .dependencies
            .keys()
            .filter(|key| {
                if selected.is_empty() {
                    project.intent().roots.contains_key(*key)
                } else {
                    selected.contains(key)
                }
            })
            .map(|key| key.as_str().to_owned())
            .collect();
        Ok(AssessmentScope {
            kind: if selected.is_empty() {
                "all-author-roots"
            } else if selected
                .iter()
                .all(|key| project.intent().roots.contains_key(*key))
            {
                "selected-author-roots"
            } else {
                "selected-dependencies"
            },
            total_roots: project.intent().roots.len()
                + selected
                    .iter()
                    .filter(|key| !project.intent().roots.contains_key(**key))
                    .count(),
            retained_inventory: project.lock().dependencies.len() - roots.len(),
            roots,
        })
    }
}
