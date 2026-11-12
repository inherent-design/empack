//! Human saved-work summaries; complete recorded facts remain in the engine's JSON report.
use crate::application::shell;
use crate::engine::pending::{PendingKind, PendingReport};
use crate::terminal::display_line;

pub(super) fn kind(value: PendingKind) -> &'static str {
    match value {
        PendingKind::Build => "build",
        PendingKind::Import => "import",
        PendingKind::Sync => "sync",
        PendingKind::Instance => "installation",
    }
}

pub(super) fn render(report: &PendingReport, details: bool) -> String {
    if report.operations.is_empty() {
        return "No saved work.".into();
    }
    let mut lines = vec![format!("Saved work: {}", report.operations.len())];
    for op in &report.operations {
        lines.push(String::new());
        let id = op.id.get(..12).unwrap_or(&op.id);
        let state = match op.state.as_str() {
            "unverified" => "saved",
            "stale" => "inputs changed; cannot resume",
            "unavailable" => "cannot read saved work",
            other => other,
        };
        lines.push(format!(
            "{} {}: {}",
            kind(op.kind),
            display_line(id),
            display_line(state)
        ));
        if let Some(message) = &op.diagnostic
            && (details || op.state != "unverified")
        {
            lines.push(format!("  {}", display_line(message)));
        }
        if !op.inputs.is_empty() {
            lines.push(format!("  Recorded files: {}", op.inputs.len()));
        }
        if op.state == "unverified" && !op.requirements_complete {
            lines.push("  File requirements will be checked on resume.".into());
        }
        if details {
            for input in &op.inputs {
                lines.push(String::new());
                lines.push(format!("  File: {}", display_line(&input.key)));
                for (label, value) in [("Name", &input.name), ("Filename", &input.filename)] {
                    if let Some(value) = value {
                        lines.push(format!("    {label}: {}", display_line(value)));
                    }
                }
                if input.needed == Some(true) {
                    lines.push("    Required file".into());
                }
                let verification = match input.verification.as_str() {
                    "retained; verification required" => "Saved file; checked again on resume",
                    "not supplied; verification required" => "Not supplied yet",
                    value => value,
                };
                lines.push(format!("    {}", display_line(verification)));
                if let Some(bytes) = input.bytes {
                    lines.push(format!("    Size: {bytes} bytes"));
                }
                if let Some(selection) = &input.provider_selection {
                    lines.push(format!(
                        "    {} project {}, version {}",
                        display_line(selection.provider),
                        display_line(&selection.project),
                        display_line(&selection.selection)
                    ));
                }
                for (algorithm, digest) in &input.digests {
                    lines.push(format!(
                        "    {}: {}",
                        display_line(algorithm),
                        display_line(digest)
                    ));
                }
                if let Some(path) = &input.supplied_candidate {
                    lines.push(format!(
                        "    Candidate file: {}",
                        display_line(&path.display().to_string())
                    ));
                }
                // A download address may contain credentials or signed query parameters.
                if input.download_page.is_some() {
                    lines.push("    Download page recorded; use --json for the address.".into());
                }
            }
        }
        if op.state == "unverified" {
            if let Some(args) = shell::arguments(&op.next) {
                lines.push(format!("Next: empack {args}"));
            }
        } else if !details {
            if let Some(id) = shell::quote(&op.id) {
                lines.push(format!("Inspect: empack pending show {id}"));
            }
        } else {
            lines.push("Keep these records while you review the problem.".into());
        }
    }
    if !details {
        lines.push(String::new());
        lines.push("Use empack pending show ID for files and details.".into());
    }
    if report.operations.iter().any(|op| op.state == "unverified") {
        lines.push("Continuing checks the files again and asks for approval.".into());
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
    use crate::engine::pending::{PendingInput, PendingOperation};

    fn report() -> PendingReport {
        PendingReport {
            schema: 1,
            root: "/pack".into(),
            operations: vec![PendingOperation {
                id: "a".repeat(64),
                kind: PendingKind::Build,
                root: "/pack".into(),
                state: "unverified".into(),
                requirements_complete: false,
                diagnostic: Some("More files may be needed.".into()),
                next: vec!["pending".into(), "resume".into(), "a".repeat(64)],
                inputs: vec![PendingInput {
                    key: "example/main".into(),
                    name: Some("Example 日本".into()),
                    filename: Some("example.jar".into()),
                    provider_selection: None,
                    digests: vec![("sha256".into(), "b".repeat(64))],
                    bytes: Some(123),
                    download_page: Some("https://example.invalid/file?token=PRIVATE".into()),
                    supplied_candidate: None,
                    verification: "retained; verification required".into(),
                    needed: None,
                    expected: None,
                    pin: None,
                }],
            }],
        }
    }

    #[test]
    fn empty_list_is_a_result_without_routine_metadata_boilerplate() {
        let mut report = report();
        report.operations.clear();
        assert_eq!(render(&report, false), "No saved work.");
    }

    #[test]
    fn list_summarizes_and_show_retains_useful_exact_file_evidence() {
        let report = report();
        let list = render(&report, false);
        let detail = render(&report, true);
        assert!(
            list.starts_with("Saved work: 1\n\nbuild aaaaaaaaaaaa: saved\n  Recorded files: 1")
        );
        assert!(list.contains("File requirements will be checked on resume."));
        assert!(!list.contains(&"b".repeat(64)));
        assert!(detail.contains("Name: Example 日本"));
        assert!(detail.contains("Size: 123 bytes"));
        assert!(detail.contains(&format!("sha256: {}", "b".repeat(64))));
        assert!(!detail.contains("unknown"));
        assert!(!detail.contains("PRIVATE"));
        assert!(detail.contains("Continuing checks the files again and asks for approval."));
        assert!(serde_json::to_string(&report).unwrap().contains("PRIVATE"));
    }

    #[test]
    fn changed_or_unreadable_work_keeps_the_diagnostic_and_does_not_offer_resume() {
        for state in ["stale", "unavailable"] {
            let mut report = report();
            report.operations[0].state = state.into();
            report.operations[0].diagnostic = Some("A saved file is missing.\nFAKE\u{202e}".into());
            for details in [false, true] {
                let text = render(&report, details);
                assert!(text.contains("A saved file is missing.\\nFAKE"));
                assert!(!text.contains("Next: empack pending resume"));
                assert!(!text.contains('\u{202e}'));
            }
        }
    }
}
