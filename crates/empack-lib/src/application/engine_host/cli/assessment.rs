//! Read-only author assessments use the same provider policy and captured documents as updates.
use super::*;
use crate::engine::providers::{ClosureLimits, ObservedVersion, UpdateAssessment};

pub async fn outdated(
    session: &dyn Session,
    json: bool,
    releases: Option<ReleasePolicy>,
    strict: bool,
) -> Result<()> {
    let project = super::current(session).await?;
    let catalog = dependencies::configured_services(session)?.catalog;
    let mut limits = ClosureLimits::default();
    limits.selection.catalog.deadline =
        Duration::from_secs(session.config().app_config().net_timeout);
    let report = initialize::discover(session, move |mut scope| async move {
        catalog
            .assess_updates(&mut scope, &project, releases, limits)
            .await
    })
    .await?;
    present(session, &report, json, strict)
}
fn present(
    session: &dyn Session,
    report: &UpdateAssessment,
    json: bool,
    strict: bool,
) -> Result<()> {
    let output = if json {
        serde_json::to_string_pretty(report)?
    } else {
        text(report)?
    };
    session.display().status().message(&output);
    ensure!(
        !strict || report.is_complete(),
        "Dependency assessment is incomplete; inspect the reported reasons"
    );
    Ok(())
}
fn version(value: Option<&ObservedVersion>) -> String {
    value.map_or_else(
        || "unknown".into(),
        |value| {
            format!(
                "{}:{}@{}{}",
                value.provider,
                value.project,
                value.selection,
                value
                    .version
                    .as_ref()
                    .map_or_else(String::new, |label| format!(" ({label})"))
            )
        },
    )
}
fn text(report: &UpdateAssessment) -> Result<String> {
    let mut lines = vec![format!(
        "Dependency assessment at {} ({})",
        report.observed_at, report.release_policy
    )];
    lines.push(format!("Assessment complete: {}", report.is_complete()));
    for row in &report.dependencies {
        lines.push(format!(
            "{} [{}]: {} (policy: {})",
            row.name, row.key, row.state, row.release_policy
        ));
        for (label, value) in [
            ("current", &row.current),
            ("permitted", &row.permitted),
            ("compatible", &row.compatible),
            ("upstream", &row.upstream),
        ] {
            lines.push(format!("  {label}: {}", version(value.as_ref())));
        }
        if let Some(diagnostic) = &row.diagnostic {
            lines.push(format!("  {diagnostic}"));
        }
        lines.push(format!(
            "  Catalog complete: {}; reasons: {}",
            row.catalog_complete,
            serde_json::to_string(&row.reasons)?
        ));
    }
    lines.push(format!(
        "Required provider closure: {} ({})",
        report.joint.state,
        serde_json::to_string(&report.joint.reasons)?
    ));
    lines.push(format!(
        "Candidate compatibility (including retained inventory): {}",
        report.joint.candidate_compatibility
    ));
    lines.push(format!(
        "Declared incompatibility list complete: {}",
        report.joint.declarations_complete
    ));
    for declaration in &report.joint.declared_incompatibilities {
        lines.push(format!(
            "Declared incompatibility: {} -> {} (client/server coexistence not assessed)",
            version(Some(&declaration.from)),
            version(Some(&declaration.target))
        ));
    }
    lines.push("Available catalog observations are bounded and time-specific. A different permitted selection need not be newer. Required-closure validation does not verify complete retained-inventory compatibility, file bindings, startup, configuration or worlds; conflicts do not prove no alternative solution exists.".into());
    Ok(lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::providers::{
        DeclaredIncompatibility, DependencyAssessment, JointAssessment,
    };

    #[test]
    fn text_separates_required_closure_from_candidate_compatibility() {
        let pin = |project: &str| ObservedVersion {
            provider: "modrinth",
            project: project.into(),
            selection: "Version1".into(),
            version: None,
            published_at: None,
            channel: None,
        };
        let mut report = UpdateAssessment {
            schema: 1,
            observed_at: "now".into(),
            release_policy: "prefer-stable",
            dependencies: Vec::new(),
            joint: JointAssessment {
                scope: "provider-dependency-closure",
                state: "validated",
                candidate_compatibility: "not-assessed",
                declarations_complete: false,
                selections: Vec::new(),
                reasons: Vec::new(),
                declared_incompatibilities: vec![DeclaredIncompatibility {
                    from: pin("ProjectA"),
                    target: pin("ProjectB"),
                }],
            },
        };
        let output = text(&report).unwrap();
        let session = crate::application::session_mocks::MockCommandSession::new();
        assert!(present(&session, &report, true, false).is_ok());
        assert!(present(&session, &report, true, true).is_err());
        report.joint.declarations_complete = true;
        assert!(present(&session, &report, true, true).is_ok());
        assert!(output.contains("Required provider closure: validated"));
        assert!(output.contains("Declared incompatibility list complete: false"));
        assert!(
            output.contains("Candidate compatibility (including retained inventory): not-assessed")
        );
        assert!(output.contains("modrinth:ProjectA@Version1 -> modrinth:ProjectB@Version1 (client/server coexistence not assessed)"));

        report.dependencies.push(DependencyAssessment {
            release_policy: "prefer-stable",
            key: "menu".into(),
            name: "Menu".into(),
            current: Some(pin("ProjectA")),
            permitted: None,
            compatible: None,
            upstream: None,
            state: "invalid-evidence",
            catalog_complete: false,
            observed_records: None,
            diagnostic: Some("Invalid provider field date_published in menu".into()),
            reasons: Vec::new(),
        });
        let output = text(&report).unwrap();
        assert!(output.contains("Assessment complete: false"));
        assert!(output.contains("Invalid provider field date_published in menu"));
        assert!(present(&session, &report, false, false).is_ok());
        assert!(present(&session, &report, false, true).is_err());
        for json in [false, true] {
            let (session, messages) = crate::application::session_mocks::MockCommandSession::new()
                .record_status_messages();
            let error = present(&session, &report, json, true).unwrap_err();
            assert_eq!(
                error.to_string(),
                "Dependency assessment is incomplete; inspect the reported reasons"
            );
            let messages = messages.lock().unwrap();
            assert_eq!(
                messages.len(),
                1,
                "strict failure must retain exactly one report"
            );
            if json {
                let actual: serde_json::Value = serde_json::from_str(&messages[0]).unwrap();
                assert_eq!(actual["dependencies"][0]["key"], "menu");
                assert_eq!(actual["dependencies"][0]["state"], "invalid-evidence");
                assert_eq!(
                    actual["dependencies"][0]["diagnostic"],
                    "Invalid provider field date_published in menu"
                );
                assert_eq!(actual["joint"]["candidate_compatibility"], "not-assessed");
            } else {
                assert_eq!(messages[0], output);
            }
        }
    }
}
