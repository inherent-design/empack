//! Read-only author assessments use the same provider policy and captured documents as updates.
use super::*;
use crate::engine::providers::{ClosureLimits, UpdateAssessment};

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
        exact: Default::default(),
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
    if !selected.roots.is_empty() {
        progress.update("catalog", "Checking dependencies");
    }
    tokio::pin!(work);
    let mut observing = true;
    let mut ticks = tokio::time::interval(Duration::from_millis(50));
    let report = loop {
        tokio::select! {
            biased;
            result = &mut work => break result?,
            changed = receiver.changed(), if observing => {
                if changed.is_err() { observing = false; }
                else if let Some(message) = receiver.borrow_and_update().as_ref() { observe(&mut progress, message); }
            }
            _ = ticks.tick() => progress.flush(),
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
    progress.observe_work(Some(observation.clone().into()));
}

pub(super) fn render_update_report(report: &UpdateAssessment, details: bool) -> String {
    text::render(report, details)
}

fn present(
    session: &dyn Session,
    report: &UpdateAssessment,
    json: bool,
    strict: bool,
    details: bool,
) -> Result<()> {
    let output = if json {
        crate::terminal::json_text(report)?
    } else {
        text::render(report, details)
    };
    session.display().status().message(&output);
    ensure!(
        !strict || report.is_complete(),
        "Some update checks could not finish; see the reported problems"
    );
    Ok(())
}
mod text;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::providers::{
        DeclaredIncompatibility, DependencyAssessment, JointAssessment, ObservedVersion,
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
        let output = text::render(&report, true);
        let session = crate::application::session_mocks::MockCommandSession::new();
        assert!(present(&session, &report, true, false, true).is_ok());
        assert!(present(&session, &report, true, true, true).is_err());
        report.joint.declarations_complete = true;
        assert!(present(&session, &report, true, true, true).is_ok());
        assert!(output.contains("Required dependencies: checked"));
        assert!(output.contains("Reported conflicts: check incomplete"));
        assert!(output.contains("These checks use mod metadata; test changes in Minecraft."));
        assert!(output.contains(
            "Reported mod conflict: modrinth:ProjectA@Version1 and modrinth:ProjectB@Version1"
        ));

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
        let output = text::render(&report, true);
        assert!(output.contains("Some checks could not finish; these results are incomplete."));
        assert!(output.contains("Invalid provider field date_published in menu"));
        assert!(present(&session, &report, false, false, true).is_ok());
        assert!(present(&session, &report, false, true, true).is_err());
        for json in [false, true] {
            let (session, messages) = crate::application::session_mocks::MockCommandSession::new()
                .record_status_messages();
            let error = present(&session, &report, json, true, true).unwrap_err();
            assert_eq!(
                error.to_string(),
                "Some update checks could not finish; see the reported problems"
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
