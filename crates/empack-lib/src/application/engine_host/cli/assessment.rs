//! Read-only author assessments use the same provider policy and captured documents as updates.
use super::*;
use crate::engine::providers::{ClosureLimits, ObservedVersion, UpdateAssessment};

pub async fn outdated(
    session: &dyn Session,
    options: crate::application::cli::OutdatedArgs,
) -> Result<()> {
    use crate::engine::providers::{AssessmentProgress, AssessmentRequest};
    let project = super::current(session).await?;
    let request = AssessmentRequest {
        keys: options
            .dependencies
            .iter()
            .map(|key| empack_core::model::DependencyKey::parse(key))
            .collect::<std::result::Result<_, _>>()?,
        releases: options.release_policy.map(Into::into),
    };
    let selected = request.scope(&project)?;
    let catalog = dependencies::configured_services(session)?.catalog;
    let mut limits = ClosureLimits::default();
    limits.selection.catalog.deadline =
        Duration::from_secs(session.config().app_config().net_timeout);
    let (sender, mut receiver) = tokio::sync::watch::channel(None::<AssessmentProgress>);
    let work = initialize::discover(session, move |mut scope| async move {
        catalog
            .assess_updates(&mut scope, &project, request, limits, |observation| {
                sender.send_replace(Some(observation));
            })
            .await
    });
    let mut progress = super::super::progress::ProgressDisplay::new(session, options.quiet);
    progress.update(&format!(
        "Assessing {} of {} author roots",
        selected.roots.len(),
        selected.total_roots
    ));
    tokio::pin!(work);
    let mut observing = true;
    let report = loop {
        tokio::select! {
            biased;
            result = &mut work => break result?,
            changed = receiver.changed(), if observing => {
                if changed.is_err() { observing = false; }
                else if let Some(message) = receiver.borrow_and_update().as_ref() { observe(&mut progress, message); }
            }
        }
    };
    drop(progress);
    present(
        session,
        &report,
        options.json,
        options.strict,
        options.details,
    )
}

fn observe(
    progress: &mut super::super::progress::ProgressDisplay,
    observation: &crate::engine::providers::AssessmentProgress,
) {
    use crate::engine::providers::AssessmentProgress;
    let message = match observation {
        AssessmentProgress::Catalog {
            key,
            index,
            total,
            records,
        } => format!("Catalog {index}/{total}: {key} ({records} records observed)"),
        AssessmentProgress::RequiredClosure { roots } => {
            format!("Checking required provider closure for {roots} roots")
        }
    };
    progress.update(&message);
}

fn present(
    session: &dyn Session,
    report: &UpdateAssessment,
    json: bool,
    strict: bool,
    details: bool,
) -> Result<()> {
    let output = if json {
        serde_json::to_string_pretty(report)?
    } else {
        text(report, details)?
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
fn text(report: &UpdateAssessment, details: bool) -> Result<String> {
    let mut lines = vec![format!(
        "Dependency assessment at {} ({})",
        report.observed_at, report.release_policy
    )];
    lines.push(format!(
        "Scope: {} of {} author roots ({}); {} locked selections retained as context",
        report.scope.roots.len(),
        report.scope.total_roots,
        report.scope.kind,
        report.scope.retained_inventory
    ));
    lines.push(format!("Assessment complete: {}", report.is_complete()));
    let summary = report.summary();
    lines.push(format!(
        "Change available: {}; held: {}; unknown: {}; manual: {}; unavailable: {}; conflicted: {}",
        summary.change_available,
        summary.held,
        summary.unknown,
        summary.manual,
        summary.unavailable,
        summary.conflicted
    ));
    if !details {
        lines.push("Use --details for held/manual rows and complete candidate evidence.".into());
    }
    for row in &report.dependencies {
        if !details
            && matches!(
                row.category(),
                crate::engine::providers::AssessmentCategory::Held
                    | crate::engine::providers::AssessmentCategory::Manual
            )
        {
            continue;
        }
        lines.push(format!(
            "{} [{}]: {} (policy: {})",
            row.name, row.key, row.state, row.release_policy
        ));
        if details {
            lines.push(format!(
                "  Release policy source: {}",
                row.release_policy_source
            ));
            for (label, value) in [
                ("current", &row.current),
                ("permitted", &row.permitted),
                ("Minecraft/loader-compatible candidate", &row.compatible),
                ("upstream", &row.upstream),
            ] {
                lines.push(format!("  {label}: {}", version(value.as_ref())));
            }
            lines.push(format!(
                "  Candidate matches automatic release-policy choice: {}",
                row.compatible_matches_release_policy
                    .map_or("unknown", |matches| if matches { "yes" } else { "no" })
            ));
        } else if row.category() == crate::engine::providers::AssessmentCategory::ChangeAvailable {
            lines.push(format!(
                "  {} -> {}",
                version(row.current.as_ref()),
                version(row.permitted.as_ref())
            ));
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
    if !report.joint.blocking_roots.is_empty() {
        lines.push(format!(
            "Joint assessment blocked by roots: {}",
            report.joint.blocking_roots.join(", ")
        ));
    }
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
    lines.push(format!(
        "Catalog validation: {} (other runtime payloads are not installation evidence)",
        UpdateAssessment::CATALOG_VALIDATION_SCOPE
    ));
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
            schema: 2,
            scope: crate::engine::providers::AssessmentScope {
                kind: "all-author-roots",
                roots: vec![],
                total_roots: 0,
                retained_inventory: 0,
            },
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
                blocking_roots: Vec::new(),
                declared_incompatibilities: vec![DeclaredIncompatibility {
                    from: pin("ProjectA"),
                    target: pin("ProjectB"),
                }],
            },
        };
        let output = text(&report, true).unwrap();
        let session = crate::application::session_mocks::MockCommandSession::new();
        assert!(present(&session, &report, true, false, true).is_ok());
        assert!(present(&session, &report, true, true, true).is_err());
        report.joint.declarations_complete = true;
        assert!(present(&session, &report, true, true, true).is_ok());
        assert!(output.contains("Required provider closure: validated"));
        assert!(output.contains("Declared incompatibility list complete: false"));
        assert!(
            output.contains("Candidate compatibility (including retained inventory): not-assessed")
        );
        assert!(output.contains("modrinth:ProjectA@Version1 -> modrinth:ProjectB@Version1 (client/server coexistence not assessed)"));

        report.dependencies.push(DependencyAssessment {
            release_policy: "prefer-stable",
            release_policy_source: "project-default",
            compatible_matches_release_policy: None,
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
            provider_problem: None,
            reasons: Vec::new(),
        });
        let output = text(&report, true).unwrap();
        assert!(output.contains("Assessment complete: false"));
        assert!(output.contains("Invalid provider field date_published in menu"));
        assert!(present(&session, &report, false, false, true).is_ok());
        assert!(present(&session, &report, false, true, true).is_err());
        for json in [false, true] {
            let (session, messages) = crate::application::session_mocks::MockCommandSession::new()
                .record_status_messages();
            let error = present(&session, &report, json, true, true).unwrap_err();
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
