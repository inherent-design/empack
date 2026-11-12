//! Discovery observations and a native proposal are different facts in one report.
use super::*;
use crate::engine::{
    api::UpdatePreview,
    diagnostics::{Diagnostic, DiagnosticPhase},
    providers::{
        AssessmentScope, AssessmentSummary, DeclaredIncompatibility, DependencyAssessment,
        ObservedVersion, UpdateAssessment, UpdateReason,
    },
};
use empack_core::files::{FileChange, FileContent, ObservedPath};
use serde::{Serialize, Serializer};

#[derive(Clone, Copy, Default)]
pub(in crate::application::engine_host) struct UpdateOutput {
    pub json: bool,
    pub details: bool,
    pub quiet: bool,
}
#[derive(Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum ProposalState {
    Ready,
    Unchanged,
    Blocked,
    Incomplete,
    PartiallyReady,
}
#[derive(Serialize)]
struct Content {
    sha256: String,
    bytes: u64,
    readonly: bool,
    executable: bool,
}
impl From<&FileContent> for Content {
    fn from(value: &FileContent) -> Self {
        Self {
            sha256: empack_core::digest::ExpectedDigest::Sha256(*value.content.bytes()).hex(),
            bytes: value.bytes,
            readonly: value.permissions.readonly,
            executable: value.permissions.executable,
        }
    }
}
#[derive(Serialize)]
struct Change {
    path: String,
    action: &'static str,
    before: Option<Content>,
    after: Option<Content>,
}
#[derive(Serialize)]
struct PinChange {
    dependency: String,
    selection: String,
}
#[derive(Serialize)]
struct BlockedGroup {
    dependencies: Vec<String>,
    diagnostic: Diagnostic,
}

pub(in crate::application::engine_host) struct UpdatePreviewReport {
    requested: Vec<String>,
    source_kinds: BTreeMap<String, &'static str>,
    observations: Option<UpdateAssessment>,
    state: ProposalState,
    selected: Vec<String>,
    changed: Vec<String>,
    pins: Vec<PinChange>,
    files: Vec<Change>,
    references: usize,
    blocked: Vec<BlockedGroup>,
    diagnostic: Option<Diagnostic>,
}
impl UpdatePreviewReport {
    pub fn new(
        mut requested: Vec<String>,
        source_kinds: BTreeMap<String, &'static str>,
        observations: Option<UpdateAssessment>,
    ) -> Self {
        requested.sort();
        requested.dedup();
        Self {
            requested,
            source_kinds,
            observations,
            state: ProposalState::Incomplete,
            selected: vec![],
            changed: vec![],
            pins: vec![],
            files: vec![],
            references: 0,
            blocked: vec![],
            diagnostic: None,
        }
    }
    pub fn empty() -> Self {
        let mut report = Self::new(vec![], Default::default(), None);
        report.state = ProposalState::Unchanged;
        report
    }
    pub fn failed(&mut self, error: &anyhow::Error) {
        self.state = if self
            .observations
            .as_ref()
            .is_some_and(|report| !report.is_complete())
        {
            ProposalState::Incomplete
        } else {
            ProposalState::Blocked
        };
        self.diagnostic = Some(Diagnostic::from_error(error, DiagnosticPhase::Preparation));
    }
    pub fn prepared(&mut self, view: &UpdatePreview) -> Result<()> {
        self.selected = view
            .selected
            .iter()
            .map(|key| key.as_str().to_owned())
            .collect();
        self.changed = view
            .changed_selections
            .iter()
            .map(|key| key.as_str().to_owned())
            .collect();
        self.references = view.references.len();
        self.pins = view
            .author_pin_changes
            .iter()
            .map(|(key, pin)| PinChange {
                dependency: key.as_str().to_owned(),
                selection: match pin {
                    empack_core::identity::PinSelector::ModrinthVersion(id) => id.to_string(),
                    empack_core::identity::PinSelector::CurseForgeFile(id) => id.to_string(),
                },
            })
            .collect();
        self.files = view
            .files
            .changes()
            .iter()
            .map(|change| {
                let path = crate::engine::layout::ProjectLayout::path(change.target())?
                    .as_str()
                    .to_owned();
                Ok(match change {
                    FileChange::Replace { before, after, .. } => Change {
                        path,
                        action: if matches!(before, ObservedPath::Absent) {
                            "create"
                        } else {
                            "replace"
                        },
                        before: match before {
                            ObservedPath::File(value) => Some(value.into()),
                            _ => None,
                        },
                        after: Some(after.into()),
                    },
                    FileChange::Remove { before, .. } => Change {
                        path,
                        action: "remove",
                        before: Some(before.into()),
                        after: None,
                    },
                })
            })
            .collect::<Result<_>>()?;
        if let Some(batch) = &view.batch {
            self.blocked = batch
                .blocked
                .iter()
                .map(|group| BlockedGroup {
                    dependencies: group
                        .requests
                        .iter()
                        .flat_map(|index| {
                            batch.requested[*index]
                                .iter()
                                .map(|key| key.as_str().to_owned())
                        })
                        .collect(),
                    diagnostic: group.diagnostic.clone(),
                })
                .collect();
        }
        self.state = if !self.blocked.is_empty() {
            ProposalState::PartiallyReady
        } else if self.files.is_empty() {
            ProposalState::Unchanged
        } else {
            ProposalState::Ready
        };
        Ok(())
    }
    pub fn finish(&self, session: &dyn Session, output: UpdateOutput) {
        if !output.json && !output.details && self.observations.is_some() {
            session
                .display()
                .status()
                .message("Call with --details for more information");
        }
    }
    pub fn present(&self, session: &dyn Session, output: UpdateOutput) -> Result<()> {
        if output.json {
            session
                .display()
                .status()
                .message(&crate::terminal::json_text(self)?);
            return Ok(());
        }
        if self.requested.is_empty() {
            session
                .display()
                .status()
                .message("No dependencies to update.");
            return Ok(());
        }
        session.display().status().message(match self.state {
            ProposalState::Ready => "Proposal: ready",
            ProposalState::Unchanged => "Proposal: no changes needed",
            ProposalState::Blocked => "Proposal: blocked; no changes applied",
            ProposalState::Incomplete => "Proposal: incomplete; no changes applied",
            ProposalState::PartiallyReady => "Proposal: some independent groups are blocked",
        });
        if let Some(observations) = &self.observations {
            session
                .display()
                .status()
                .message(&cli::render_update_report(
                    observations,
                    output.details || self.diagnostic.is_some(),
                ));
        }
        Ok(())
    }
}
impl Serialize for UpdatePreviewReport {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Discovery<'a> {
            observed_at: &'a str,
            catalog_validation: &'static str,
            release_policy: &'a str,
            scope: &'a AssessmentScope,
            complete: bool,
            summary: AssessmentSummary,
            dependencies: &'a [DependencyAssessment],
            required_dependencies: &'a str,
            required_selections: &'a [ObservedVersion],
            declared_incompatibilities: &'a [DeclaredIncompatibility],
            declarations_complete: bool,
            reasons: &'a [UpdateReason],
            blocking_dependencies: &'a [String],
        }
        #[derive(Serialize)]
        struct Report<'a> {
            schema: u32,
            requested: &'a [String],
            source_kinds: &'a BTreeMap<String, &'static str>,
            discovery: Option<Discovery<'a>>,
            proposal: ProposalState,
            selected: &'a [String],
            changed_selections: &'a [String],
            author_pin_changes: &'a [PinChange],
            file_changes: &'a [Change],
            references: usize,
            blocked_groups: &'a [BlockedGroup],
            diagnostic: &'a Option<Diagnostic>,
        }
        Report {
            schema: 1,
            requested: &self.requested,
            source_kinds: &self.source_kinds,
            discovery: self.observations.as_ref().map(|report| Discovery {
                observed_at: &report.observed_at,
                catalog_validation: UpdateAssessment::CATALOG_VALIDATION_SCOPE,
                release_policy: report.release_policy,
                scope: &report.scope,
                complete: report.is_complete(),
                summary: report.summary(),
                dependencies: &report.dependencies,
                required_dependencies: report.joint.state,
                required_selections: &report.joint.selections,
                declared_incompatibilities: &report.joint.declared_incompatibilities,
                declarations_complete: report.joint.declarations_complete,
                reasons: &report.joint.reasons,
                blocking_dependencies: &report.joint.blocking_roots,
            }),
            proposal: self.state,
            selected: &self.selected,
            changed_selections: &self.changed,
            author_pin_changes: &self.pins,
            file_changes: &self.files,
            references: self.references,
            blocked_groups: &self.blocked,
            diagnostic: &self.diagnostic,
        }
        .serialize(serializer)
    }
}
