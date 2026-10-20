//! Read-only author assessments use the same provider policy and captured documents as updates.
use super::*;
use crate::engine::providers::{ClosureLimits, ObservedVersion, UpdateAssessment};

pub async fn outdated(
    session: &dyn Session,
    json: bool,
    releases: Option<ReleasePolicy>,
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
    let output = if json {
        serde_json::to_string_pretty(&report)?
    } else {
        text(&report)?
    };
    session.display().status().message(&output);
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
    use crate::engine::providers::{DeclaredIncompatibility, JointAssessment};

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
        let report = UpdateAssessment {
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
        assert!(output.contains("Required provider closure: validated"));
        assert!(output.contains("Declared incompatibility list complete: false"));
        assert!(
            output.contains("Candidate compatibility (including retained inventory): not-assessed")
        );
        assert!(output.contains("modrinth:ProjectA@Version1 -> modrinth:ProjectB@Version1 (client/server coexistence not assessed)"));
    }
}
