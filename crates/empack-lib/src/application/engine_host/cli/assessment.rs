//! Human provider findings; command execution and proposal readiness have one update owner.
use crate::engine::providers::UpdateAssessment;

pub(super) fn render_update_report(report: &UpdateAssessment, details: bool) -> String {
    text::render_findings(report, details)
}
mod text;
