//! Present the engine's read-only check report before returning its original typed failure.
use super::*;
use crate::{
    application::cli::CheckArgs,
    engine::{
        checking::{CheckReport, CheckRequest, CheckScope, CheckState, check_project},
        providers::{ClosureLimits, ProviderCatalog},
    },
    networking::rate_budget::HostBudgetRegistry,
    terminal::display_line,
};
use std::sync::Arc;

pub(in crate::application) async fn check(session: &dyn Session, options: CheckArgs) -> Result<()> {
    let catalog = ProviderCatalog::new(
        session
            .config()
            .app_config()
            .curseforge_api_client_key
            .clone(),
        Arc::new(HostBudgetRegistry::new()),
    )?;
    check_with_catalog(session, options, catalog).await
}
async fn check_with_catalog(
    session: &dyn Session,
    options: CheckArgs,
    catalog: ProviderCatalog,
) -> Result<()> {
    ensure!(
        !(options.documents && options.online),
        "Choose --documents or --online, not both"
    );
    ensure!(
        !(options.json && options.details),
        "Choose --json or --details, not both"
    );
    let (invocation, project) = project_path(session)?;
    let mut providers = ClosureLimits::default();
    providers.selection.catalog.deadline =
        Duration::from_secs(session.config().app_config().net_timeout);
    let request = CheckRequest {
        project,
        state: state_root(session.config().app_config(), &invocation)?,
        scope: if options.documents {
            CheckScope::Documents
        } else if options.online {
            CheckScope::Online
        } else {
            CheckScope::Recorded
        },
        files: SnapshotLimits::default(),
        providers,
    };
    let outcome = scoped_with_feedback(
        session,
        ResourceGovernor::new(ResourceRequest {
            jobs: 1,
            memory_bytes: 512 << 20,
            open_files: 64,
            scratch_bytes: 0,
        }),
        options.quiet,
        move |mut scope| async move { check_project(&mut scope, request, &catalog).await },
    )
    .await?;
    session.display().status().message(&if options.json {
        crate::terminal::json_text(&outcome.report)?
    } else {
        render(&outcome.report, options.details)
    });
    outcome.result
}
fn render(report: &CheckReport, details: bool) -> String {
    let target = report.pack.as_deref().unwrap_or("Project");
    let mut lines = vec![
        format!(
            "{}: {}",
            display_line(target),
            if report.complete {
                "check passed"
            } else {
                "check failed"
            }
        ),
        format!(
            "Scope: {}",
            match report.scope {
                CheckScope::Documents => "author files and any present lock",
                CheckScope::Recorded => "recorded relationships and local content",
                CheckScope::Online => "recorded content and fresh exact provider metadata",
            }
        ),
    ];
    if report.verified_files > 0 || report.references > 0 {
        lines.push(format!(
            "Local files verified: {}; references not materialized: {}",
            report.verified_files, report.references
        ));
    }
    let mut hidden = 0;
    for finding in &report.findings {
        if !details
            && (finding.state == CheckState::Passed || finding.state == CheckState::Reference)
        {
            hidden += 1;
            continue;
        }
        let label = match finding.state {
            CheckState::Passed => "Checked",
            CheckState::Failed => "Failed",
            CheckState::Unverified => "Not verified",
            CheckState::Reference => "Reference",
        };
        lines.push(format!("  {label}: {}", display_line(&finding.subject)));
        for line in finding.message.lines() {
            lines.push(format!("    {}", display_line(line)));
        }
    }
    if !details && hidden > 0 {
        lines.push("Call with --details for more information".into());
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::checking::CheckFinding;
    #[test]
    fn check_output_leads_with_scope_and_failure_without_claiming_repairs() {
        let report = CheckReport {
            schema: 1,
            scope: CheckScope::Online,
            pack: Some("Pack 日本".into()),
            author_schema: Some(6),
            authoring: None,
            lock: "current",
            verified_files: 2,
            references: 1,
            complete: false,
            findings: vec![
                CheckFinding {
                    subject: "Authoring".into(),
                    state: CheckState::Passed,
                    required: true,
                    message: "Author files are valid".into(),
                    diagnostic: None,
                },
                CheckFinding {
                    subject: "menu".into(),
                    state: CheckState::Failed,
                    required: true,
                    message: "Required dependency is missing".into(),
                    diagnostic: None,
                },
            ],
        };
        assert_eq!(
            render(&report, false),
            "Pack 日本: check failed\nScope: recorded content and fresh exact provider metadata\nLocal files verified: 2; references not materialized: 1\n  Failed: menu\n    Required dependency is missing\nCall with --details for more information"
        );
        assert!(render(&report, true).contains("  Checked: Authoring\n    Author files are valid"));
        assert!(!render(&report, true).contains("Call with"));
    }
}
