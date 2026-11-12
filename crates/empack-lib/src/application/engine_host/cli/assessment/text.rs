//! Human update reports. Machine data remains in the shared assessment report.
use crate::engine::providers::{
    AssessmentCategory, DependencyAssessment, ObservedVersion, UpdateAssessment, UpdateReason,
};
use crate::terminal::display_line;

pub(super) fn render(report: &UpdateAssessment, details: bool) -> String {
    render_with_hint(report, details, true)
}

pub(super) fn render_findings(report: &UpdateAssessment, details: bool) -> String {
    render_with_hint(report, details, false)
}

fn render_with_hint(report: &UpdateAssessment, details: bool, hint: bool) -> String {
    if report.dependencies.is_empty()
        && report.scope.roots.is_empty()
        && report.is_complete()
        && report.joint.declared_incompatibilities.is_empty()
    {
        return "No dependencies to check.".into();
    }
    let mut lines = vec![if report.scope.roots.len() == report.scope.total_roots {
        let count = report.scope.roots.len();
        format!(
            "Checked {count} {}",
            if count == 1 {
                "dependency"
            } else {
                "dependencies"
            }
        )
    } else {
        format!(
            "Checked {} of {} dependencies",
            report.scope.roots.len(),
            report.scope.total_roots
        )
    }];
    if !report.is_complete() {
        lines.push("Some checks could not finish; these results are incomplete.".into());
    }
    let summary = report.summary();
    lines.extend([String::new(), "Updates:".into()]);
    for (label, count) in [
        ("Available", summary.change_available),
        ("Held", summary.held),
        ("Unknown", summary.unknown),
        ("Manual", summary.manual),
        ("Conflicted", summary.conflicted),
        ("Unavailable", summary.unavailable),
    ] {
        lines.push(format!("    {label:<12} {count}"));
    }
    let specific = report.scope.kind != "all-author-roots" || report.dependencies.len() == 1;
    for row in &report.dependencies {
        if !details
            && !specific
            && matches!(
                row.category(),
                AssessmentCategory::Held | AssessmentCategory::Manual
            )
        {
            continue;
        }
        lines.push(String::new());
        lines.push(result(row));
        if details {
            row_details(row, report.release_policy, &mut lines);
        }
    }
    if !report.joint.declared_incompatibilities.is_empty() {
        lines.push(String::new());
        for conflict in &report.joint.declared_incompatibilities {
            lines.push(format!(
                "Reported mod conflict: {} and {}",
                version(Some(&conflict.from)),
                version(Some(&conflict.target))
            ));
        }
        lines.push("These conflicts come from mod metadata; review them before updating.".into());
    } else if summary.change_available > 0 && report.joint.candidate_compatibility == "not-assessed"
    {
        lines.extend([
            String::new(),
            "Test updates in Minecraft before using them.".into(),
        ]);
    }
    if details {
        lines.extend([
            String::new(),
            "Check details:".into(),
            format!(
                "    Required dependencies: {}",
                match report.joint.state {
                    "validated" => "checked",
                    "not-applicable" => "not needed for these files",
                    _ => "could not finish",
                }
            ),
            format!(
                "    Reported conflicts: {}",
                if report.joint.declarations_complete {
                    if report.joint.declared_incompatibilities.is_empty() {
                        "none found"
                    } else {
                        "see conflicts above"
                    }
                } else if report.joint.state == "not-applicable" {
                    "not checked for manual files"
                } else {
                    "check incomplete"
                }
            ),
            format!(
                "    Other saved dependencies used: {}",
                report.scope.retained_inventory
            ),
            format!(
                "    Version preference: {}",
                preference(report.release_policy)
            ),
            format!("    Checked at: {}", report.observed_at),
        ]);
        if !report.joint.blocking_roots.is_empty() {
            lines.push(format!(
                "    Dependencies needing attention: {}",
                report.joint.blocking_roots.join(", ")
            ));
        }
        lines.push("    These checks use mod metadata; test changes in Minecraft.".into());
    } else if hint {
        lines.extend([
            String::new(),
            "Call with --details for more information".into(),
        ]);
    }
    lines.join("\n")
}

fn current(row: &DependencyAssessment) -> Option<&ObservedVersion> {
    let value = row.current.as_ref()?;
    if value.version.is_some() {
        return Some(value);
    }
    [&row.permitted, &row.compatible, &row.upstream]
        .into_iter()
        .flatten()
        .find(|candidate| candidate.same_selection(value) && candidate.version.is_some())
        .or(Some(value))
}
fn short(value: Option<&ObservedVersion>) -> std::borrow::Cow<'_, str> {
    display_line(value.map_or("unknown", |value| {
        value.version.as_deref().unwrap_or(&value.selection)
    }))
}
fn version(value: Option<&ObservedVersion>) -> String {
    let label = value.map_or_else(
        || "unknown".into(),
        |value| {
            let identity = format!("{}:{}@{}", value.provider, value.project, value.selection);
            value
                .version
                .as_ref()
                .map_or_else(|| identity.clone(), |label| format!("{label} ({identity})"))
        },
    );
    display_line(&label).into_owned()
}
fn result(row: &DependencyAssessment) -> String {
    let (outcome, fallback) = match row.category() {
        AssessmentCategory::Held if row.reasons.contains(&UpdateReason::Pinned) => {
            ("Held", format!("pinned at {}", short(current(row))))
        }
        AssessmentCategory::Held => ("Held", format!("keeping {}", short(current(row)))),
        AssessmentCategory::ChangeAvailable => (
            "Available",
            format!(
                "{} -> {}",
                short(current(row)),
                short(row.permitted.as_ref())
            ),
        ),
        AssessmentCategory::Unknown
            if row.reasons.contains(&UpdateReason::NotAssessedAfterFailure) =>
        {
            ("Unknown", "not checked after an earlier failure".into())
        }
        AssessmentCategory::Unknown => ("Unknown", "could not check for updates".into()),
        AssessmentCategory::Manual => ("Manual", "updates must be checked manually".into()),
        AssessmentCategory::Unavailable => (
            "Unavailable",
            "no version matches this pack's settings".into(),
        ),
        AssessmentCategory::Conflicted => (
            "Conflicted",
            "the selected update conflicts with another dependency".into(),
        ),
    };
    let additional = row
        .diagnostic
        .as_ref()
        .map(|value| display_line(value).into_owned())
        .or_else(|| {
            row.reasons
                .iter()
                .find(|reason| !matches!(reason, UpdateReason::Pinned | UpdateReason::ManualSource))
                .map(|reason| explanation(reason).to_owned())
        });
    let mut text = format!("{}: {outcome}\n    Reason: ", display_line(&row.key));
    if matches!(
        row.category(),
        AssessmentCategory::Held | AssessmentCategory::ChangeAvailable
    ) {
        text.push_str(&fallback);
        if let Some(additional) = additional {
            text.push_str(&format!("\n    {additional}"));
        }
    } else {
        text.push_str(additional.as_deref().unwrap_or(&fallback));
    }
    text
}
fn row_details(row: &DependencyAssessment, default_preference: &str, lines: &mut Vec<String>) {
    if row.name != row.key {
        lines.push(format!("    Name: {}", display_line(&row.name)));
    }
    lines.push(format!("    Current: {}", version(current(row))));
    if let Some(value) = &row.permitted
        && row
            .current
            .as_ref()
            .is_none_or(|old| !old.same_selection(value))
    {
        lines.push(format!(
            "    Selected by your settings: {}",
            version(Some(value))
        ));
    }
    if let Some(value) = &row.compatible
        && row
            .permitted
            .as_ref()
            .is_none_or(|preferred| !preferred.same_selection(value))
    {
        lines.push(format!(
            "    Minecraft/loader match: {}",
            version(Some(value))
        ));
    }
    if let Some(value) = &row.upstream
        && row
            .compatible
            .as_ref()
            .is_none_or(|compatible| !compatible.same_selection(value))
    {
        lines.push(format!("    Latest listed: {}", version(Some(value))));
    }
    if row.release_policy != default_preference {
        lines.push(format!(
            "    Version preference: {} ({})",
            preference(row.release_policy),
            match row.release_policy_source {
                "project-default" => "pack default",
                "operation-override" => "command option",
                "root-preference" => "dependency setting",
                value => value,
            }
        ));
    }
    if row.compatible_matches_release_policy == Some(false) {
        lines.push("    The Minecraft/loader match differs from the preferred release.".into());
    }
    if !row.catalog_complete {
        lines.push("    The available-version check did not finish.".into());
    }
    for reason in row
        .reasons
        .iter()
        .filter(|reason| !matches!(reason, UpdateReason::Pinned | UpdateReason::ManualSource))
        .skip(usize::from(row.diagnostic.is_none()))
    {
        lines.push(format!("    {}", explanation(reason)));
    }
}
fn preference(value: &str) -> &str {
    match value {
        "prefer-stable" => "prefer stable releases",
        "stable-only" => "stable releases only",
        "any" => "include beta and alpha releases",
        value => value,
    }
}
fn explanation(reason: &UpdateReason) -> &'static str {
    match reason {
        UpdateReason::Pinned => "The version is pinned in your pack.",
        UpdateReason::ManualSource => "This file has no provider update check.",
        UpdateReason::UnresolvedSource => "The source of this dependency is unresolved.",
        UpdateReason::NoCompatibleSelection => {
            "No version was found for this Minecraft and loader."
        }
        UpdateReason::ReleasePolicy => "No version matches your release preference.",
        UpdateReason::CatalogLimit => "The available-version check reached its limit.",
        UpdateReason::Deadline => "The update check ran out of time.",
        UpdateReason::Unauthorized => "The provider denied access; check its access requirements.",
        UpdateReason::NotFound => "The provider could not find the requested project or version.",
        UpdateReason::RateLimited => "The provider is limiting requests; try again later.",
        UpdateReason::InvalidEvidence => "The provider returned invalid version information.",
        UpdateReason::ProviderUnavailable => "The provider could not be reached.",
        UpdateReason::NotAssessedAfterFailure => "An earlier failure stopped the remaining checks.",
        UpdateReason::IncompleteDependencyEvidence => {
            "Some required-dependency information could not be checked."
        }
        UpdateReason::DependencyConflict => "The selected versions have conflicting requirements.",
        UpdateReason::IncompleteRootAssessment => "Some dependency checks are incomplete.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::providers::{AssessmentScope, JointAssessment};

    fn observed(selection: &str, label: Option<&str>) -> ObservedVersion {
        ObservedVersion {
            provider: "modrinth",
            project: "ProjectA".into(),
            selection: selection.into(),
            version: label.map(str::to_owned),
            published_at: None,
            channel: None,
        }
    }
    fn row() -> DependencyAssessment {
        DependencyAssessment {
            release_policy: "prefer-stable",
            release_policy_source: "project-default",
            compatible_matches_release_policy: Some(true),
            key: "c2me-fabric".into(),
            name: "C2ME".into(),
            current: Some(observed("VersionA", None)),
            permitted: Some(observed("VersionA", Some("0.4.0-alpha.0.30+1.21.1"))),
            compatible: Some(observed("VersionA", Some("0.4.0-alpha.0.30+1.21.1"))),
            upstream: None,
            state: "pinned",
            catalog_complete: true,
            observed_records: Some(12),
            diagnostic: None,
            provider_problem: None,
            reasons: vec![UpdateReason::Pinned],
        }
    }
    fn report(rows: Vec<DependencyAssessment>) -> UpdateAssessment {
        UpdateAssessment {
            schema: 2,
            observed_at: "2026-11-12T08:32:58.816491Z".into(),
            release_policy: "prefer-stable",
            scope: AssessmentScope {
                kind: "selected-author-roots",
                roots: rows.iter().map(|r| r.key.clone()).collect(),
                total_roots: 272,
                retained_inventory: 275,
            },
            dependencies: rows,
            joint: JointAssessment {
                scope: "provider-dependency-closure",
                state: "validated",
                candidate_compatibility: "not-assessed",
                declarations_complete: true,
                selections: vec![],
                reasons: vec![],
                blocking_roots: vec![],
                declared_incompatibilities: vec![],
            },
        }
    }
    #[test]
    fn selected_pin_has_a_concise_complete_answer() {
        let report = report(vec![row()]);
        assert_eq!(
            render(&report, false),
            "Checked 1 of 272 dependencies\n\nUpdates:\n    Available    0\n    Held         1\n    Unknown      0\n    Manual       0\n    Conflicted   0\n    Unavailable  0\n\nc2me-fabric: Held\n    Reason: pinned at 0.4.0-alpha.0.30+1.21.1\n\nCall with --details for more information"
        );
        let machine = serde_json::to_value(&report).unwrap();
        assert_eq!(machine["schema"], 2);
        assert_eq!(machine["complete"], true);
        assert_eq!(machine["scope"]["kind"], "selected-author-roots");
        assert_eq!(machine["summary"]["held"], 1);
        assert_eq!(machine["joint"]["candidate_compatibility"], "not-assessed");
    }
    #[test]
    fn partial_results_keep_available_updates_and_actionable_failures_visible() {
        let mut update = row();
        update.key = "menu".into();
        update.reasons.clear();
        update.state = "available";
        update.current = Some(observed("VersionA", Some("1.0")));
        update.permitted = Some(observed("VersionB", Some("2.0")));
        let mut broken = row();
        broken.key = "broken".into();
        broken.catalog_complete = false;
        broken.state = "invalid-evidence";
        broken.reasons = vec![UpdateReason::InvalidEvidence];
        broken.diagnostic = Some("Invalid provider field files in broken".into());
        let output = render(&report(vec![update, broken]), false);
        assert!(output.starts_with("Checked 2 of 272 dependencies\nSome checks could not finish; these results are incomplete."));
        assert!(output.contains("Available    1"));
        assert!(output.contains("Unknown      1"));
        assert!(output.contains("menu: Available\n    Reason: 1.0 -> 2.0"));
        assert!(
            output.contains("broken: Unknown\n    Reason: Invalid provider field files in broken")
        );
        assert!(output.contains("Test updates in Minecraft before using them."));
        assert!(output.ends_with("Call with --details for more information"));
    }
    #[test]
    fn details_retain_exact_identity_without_repeating_default_bookkeeping() {
        let report = report(vec![row()]);
        let output = render(&report, true);
        assert!(output.contains("Current: 0.4.0-alpha.0.30+1.21.1 (modrinth:ProjectA@VersionA)"));
        assert!(output.contains("Required dependencies: checked"));
        assert!(output.contains("Reported conflicts: none found"));
        assert_eq!(output.matches("Version preference:").count(), 1);
        assert!(output.ends_with("These checks use mod metadata; test changes in Minecraft."));
        assert!(!output.contains("Call with --details"));
        assert!(!render(&report, false).contains(&report.observed_at));
    }
    #[test]
    fn no_dependencies_is_a_short_noop() {
        let mut report = report(vec![]);
        report.scope.total_roots = 0;
        assert_eq!(render(&report, false), "No dependencies to check.");
    }

    #[test]
    fn external_labels_cannot_add_report_lines_or_terminal_controls() {
        let mut dependency = row();
        dependency.name = "日本\nfalse heading".into();
        let label = "1.0\nfalse update\u{202e}";
        dependency.permitted.as_mut().unwrap().version = Some(label.into());
        dependency.diagnostic = Some("provider note\nfalse action\u{1b}[2J".into());
        let report = report(vec![dependency]);
        let before = serde_json::to_value(&report).unwrap();
        for details in [false, true] {
            let output = render(&report, details);
            assert!(output.contains("1.0\\nfalse update\\u{202e}"), "{output}");
            assert!(
                output.contains("provider note\\nfalse action\\u{1b}[2J"),
                "{output}"
            );
            assert!(!output.contains("\nfalse"));
            assert!(!output.contains('\u{1b}'));
            assert!(!output.contains('\u{202e}'));
            if details {
                assert!(output.contains("Name: 日本\\nfalse heading"));
            }
        }
        assert_eq!(serde_json::to_value(&report).unwrap(), before);
        assert_eq!(before["dependencies"][0]["permitted"]["version"], label);
    }
}
