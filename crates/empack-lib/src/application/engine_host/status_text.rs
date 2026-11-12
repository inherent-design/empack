//! Human status presentation. The engine report retains complete inspection facts.
use crate::engine::status::{RecordState, WorkspaceContext, WorkspaceStatus};
use crate::terminal::display_line;

fn state(value: RecordState) -> &'static str {
    match value {
        RecordState::Missing => "not found",
        RecordState::Valid => "current",
        RecordState::Stale => "needs sync",
        RecordState::Invalid => "invalid",
        RecordState::Unavailable => "not verified",
    }
}

pub(super) fn render(report: &WorkspaceStatus) -> String {
    let mut lines = Vec::new();
    for context in &report.contexts {
        if report
            .selected
            .is_some_and(|selected| selected != context.context)
        {
            continue;
        }
        if !lines.is_empty() {
            lines.push(String::new());
        }
        let label = match context.context {
            WorkspaceContext::Author => "Project",
            WorkspaceContext::Instance => "Instance",
            WorkspaceContext::Publisher => "Publisher",
        };
        let description = match (&context.name, &context.version) {
            (Some(name), Some(version)) => {
                format!("{} {}", display_line(name), display_line(version))
            }
            (Some(name), None) => display_line(name).into_owned(),
            _ => state(context.state).into(),
        };
        lines.push(format!("{label}: {description}"));
        if context.name.is_some() && context.state != RecordState::Valid {
            lines.push(format!("  Records: {}", state(context.state)));
        }
        if let Some(mc) = &context.minecraft {
            lines.push(format!(
                "  Minecraft {} ({})",
                display_line(mc),
                display_line(context.loader.as_deref().unwrap_or("unknown loader"))
            ));
        }
        if let Some(lock) = context.lock {
            lines.push(format!("  Lock: {}", state(lock)));
        }
        if let Some(game) = &context.game_directory {
            lines.push(format!("  Game directory: {}", display_line(game)));
        }
        if let Some(side) = &context.side {
            lines.push(format!("  Side: {}", display_line(side)));
        }
        if context.context == WorkspaceContext::Instance {
            if let Some(exact) = &context.exact {
                lines.push(format!("  Release: {}", display_line(exact)));
            }
            let owner = if context.update_owner
                == "snapshot: install an explicitly selected exact release"
            {
                "choose a release to install"
            } else {
                &context.update_owner
            };
            if owner != "unknown" {
                lines.push(format!("  Updates: {}", display_line(owner)));
            }
        } else if context.context == WorkspaceContext::Publisher
            && context.state == RecordState::Unavailable
        {
            lines.push("  Release signatures have not been checked.".into());
        }
    }
    for finding in &report.findings {
        // These facts already appear beside their affected records above.
        if !matches!(
            finding.code.as_str(),
            "context-not-ready" | "lock-not-current"
        ) {
            if !lines.is_empty() {
                lines.push(String::new());
            }
            lines.push(display_line(&finding.message).into_owned());
        }
        if !finding.next.is_empty()
            && let Some(args) = crate::application::shell::arguments(&finding.next)
        {
            lines.push(format!("Next: empack {args}"));
        }
    }
    if report.contexts.iter().any(|context| {
        context.state == RecordState::Valid
            && report
                .selected
                .is_none_or(|selected| selected == context.context)
    }) && !report.content_verified
    {
        lines.push(String::new());
        lines.push("File contents have not been checked.".into());
    }
    lines.push(format!(
        "Directory: {}",
        display_line(&report.root.display().to_string())
    ));
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::status::{ContextStatus, StatusFinding};

    fn project() -> WorkspaceStatus {
        WorkspaceStatus {
            schema: 1,
            root: "/packs/workshop".into(),
            selected: Some(WorkspaceContext::Author),
            contexts: vec![ContextStatus {
                context: WorkspaceContext::Author,
                state: RecordState::Valid,
                name: Some("Workshop 日本".into()),
                version: Some("1.2".into()),
                minecraft: Some("1.21.1".into()),
                loader: Some("Fabric".into()),
                exact: Some("a".repeat(64)),
                lock: Some(RecordState::Valid),
                game_directory: None,
                side: None,
                update_owner: "author: update selects dependencies; sync applies recorded intent"
                    .into(),
            }],
            findings: vec![],
            content_verified: false,
        }
    }

    #[test]
    fn project_status_leads_with_identity_and_relevant_checks() {
        assert_eq!(
            render(&project()),
            "Project: Workshop 日本 1.2\n  Minecraft 1.21.1 (Fabric)\n  Lock: current\n\nFile contents have not been checked.\nDirectory: /packs/workshop"
        );
    }

    #[test]
    fn stale_lock_keeps_next_action_without_duplicate_internal_findings() {
        let mut report = project();
        report.contexts[0].lock = Some(RecordState::Stale);
        report.findings.push(StatusFinding {
            code: "lock-not-current".into(),
            message: "Author intent has no valid matching resolution; preview synchronization"
                .into(),
            next: vec!["--dry-run".into(), "sync".into()],
        });
        let text = render(&report);
        assert!(text.contains("  Lock: needs sync\nNext: empack --dry-run sync"));
        assert!(!text.contains("lock-not-current"));
        assert!(!text.contains("Author intent"));
    }

    #[test]
    fn unsafe_names_and_guidance_cannot_insert_terminal_lines_or_shell_commands() {
        let mut report = project();
        report.contexts[0].name = Some("Pack\nFAKE\u{202e}".into());
        report.findings.push(StatusFinding {
            code: "future-finding".into(),
            message: "Problem\nFAKE".into(),
            next: vec!["bad\ncommand".into()],
        });
        let text = render(&report);
        assert!(!text.lines().any(|line| line == "FAKE"));
        assert!(!text.contains('\u{202e}'));
        assert!(!text.contains("Next:"));
        assert!(text.contains("Problem\\nFAKE"));
    }

    #[test]
    fn a_missing_selected_context_does_not_borrow_checks_or_unknown_values_from_another() {
        let mut report = project();
        let mut absent = report.contexts[0].clone();
        absent.context = WorkspaceContext::Instance;
        absent.state = RecordState::Missing;
        absent.name = None;
        absent.version = None;
        absent.minecraft = None;
        absent.loader = None;
        absent.exact = None;
        absent.lock = None;
        absent.update_owner = "unknown".into();
        report.contexts.push(absent);
        report.selected = Some(WorkspaceContext::Instance);
        assert_eq!(
            render(&report),
            "Instance: not found\nDirectory: /packs/workshop"
        );
    }
}
