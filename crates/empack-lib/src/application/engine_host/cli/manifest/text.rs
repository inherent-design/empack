//! Local inspection answers; complete normalized settings remain available as JSON.
use crate::engine::documents::{AuthoringInspection, DependencyInspection};
use crate::terminal::display_line;
use anyhow::Result;
use serde_json::Value;

fn editing(report: &AuthoringInspection, dependency: Option<&str>, lines: &mut Vec<String>) {
    let limitations: Vec<_> = report
        .editing_limitations
        .iter()
        .filter(|item| {
            dependency.is_none_or(|key| {
                item.path.is_empty()
                    || (matches!(
                        item.path.first().map(String::as_str),
                        Some("dependencies" | "bindings")
                    ) && item.path.get(1).is_none_or(|candidate| candidate == key))
            })
        })
        .collect();
    if limitations.is_empty() {
        return;
    }
    lines.push(String::new());
    let count = limitations.len();
    lines.push(format!(
        "{count} {} simpler YAML for automatic edits (--json lists them).",
        if count == 1 {
            "setting needs"
        } else {
            "settings need"
        }
    ));
    if dependency.is_some() {
        for item in limitations {
            lines.push(format!(
                "  {} / {}",
                display_line(item.document),
                display_line(&item.path.join("."))
            ));
        }
    }
}

pub(super) fn diff(report: &AuthoringInspection) -> String {
    let mut lines = vec![match report.lock_status {
        "missing" => "No lock to compare with.".into(),
        "current" => "Dependency settings match the lock.".into(),
        _ if report.runtime_satisfied == Some(true)
            && report
                .dependencies
                .iter()
                .all(|row| !row.explicit || row.selection_satisfied == Some(true)) =>
        {
            "Author files changed; recorded dependency choices still match.".into()
        }
        _ => "Author files changed; the lock needs sync.".into(),
    }];
    if report.runtime_satisfied == Some(false) {
        lines.push("Minecraft, loader or Java settings need to be resolved again.".into());
    }
    // Without a prior lock, every row's missing-selection error adds no new information.
    if report.lock_status != "missing" {
        for row in &report.dependencies {
            if let Some(reason) = &row.reason {
                let reason = if row.explicit {
                    display_line(reason)
                } else {
                    "kept in the lock; not a direct dependency in the author files".into()
                };
                lines.push(format!("{}: {reason}", display_line(&row.key)));
            }
        }
    }
    if report.lock_status != "current" {
        lines.push("Next: empack sync".into());
    }
    editing(report, None, &mut lines);
    lines.join("\n")
}

fn identity(value: &Value, field: &str) -> String {
    let provider = match value["provider"].as_str() {
        Some("modrinth") => "Modrinth",
        Some("curseforge") => "CurseForge",
        Some(value) => value,
        None => "unknown provider",
    };
    format!(
        "{} {}",
        display_line(provider),
        display_line(value[field].as_str().unwrap_or("unknown"))
    )
}

pub(super) fn explain(report: &AuthoringInspection, row: &DependencyInspection) -> Result<String> {
    let mut lines = vec![display_line(&row.key).into_owned()];
    if !row.explicit {
        lines.push("Kept in the lock; not a direct dependency in the author files.".into());
    }
    if let Some(project) = &row.canonical_project {
        lines.push(format!("Project: {}", identity(project, "project")));
    }
    if let Some(selected) = &row.locked_selection {
        lines.push(format!("Recorded version: {}", identity(selected, "id")));
    }
    if let Some(request) = &row.effective {
        let source = &request["source"];
        match source["kind"].as_str() {
            Some("local") => lines.push(format!(
                "Local file: {}",
                display_line(source["path"].as_str().unwrap_or("unknown"))
            )),
            Some("local-files") => {
                lines.push("Local files:".into());
                if let Some(members) = source["members"].as_object() {
                    for (slot, path) in members {
                        lines.push(format!(
                            "  {}: {}",
                            display_line(slot),
                            display_line(path.as_str().unwrap_or("unknown"))
                        ));
                    }
                }
            }
            // URLs may carry credentials or signed queries. Do not echo them as ordinary diagnostics.
            Some("url") => {
                let count = source["downloads"].as_array().map_or(0, Vec::len);
                lines.push(if count == 1 {
                    "Source: download URL (use --json for the full address)".into()
                } else {
                    format!("Source: {count} download URLs (use --json for full addresses)")
                });
            }
            Some("search") => lines.push(format!(
                "Search: {}",
                display_line(source["query"].as_str().unwrap_or("unknown"))
            )),
            Some("provider-selector") => {
                lines.push(format!(
                    "Source: {}",
                    identity(&source["request"], "selector")
                ));
                if row.source_binding_current == Some(false) {
                    lines.push("The project has not been resolved for this source.".into());
                }
            }
            _ => {}
        }
        match request["selection"]["mode"].as_str() {
            Some("provider-pin") => lines.push(format!(
                "Pinned version: {}",
                identity(&request["selection"]["pin"], "id")
            )),
            Some("automatic")
                if matches!(
                    source["kind"].as_str(),
                    Some("provider" | "provider-selector" | "search")
                ) =>
            {
                lines.push(if row.selection_satisfied == Some(true) {
                    "Version choice: keep the recorded version during sync".into()
                } else {
                    "Version choice: resolve during sync".into()
                })
            }
            Some("content-assertions") => {
                lines.push("Content must match the authored checksums.".into())
            }
            _ => {}
        }
        // These are supported author settings, rather than serialized internal identities.
        for (key, label) in [
            ("environment", "Client/server settings"),
            ("placement", "Placement"),
        ] {
            if let Some(value) = request.get(key) {
                let yaml = serde_saphyr::to_string(value)?;
                lines.push(format!("{label}:"));
                lines.extend(
                    yaml.trim_end_matches('\n')
                        .lines()
                        .map(|line| format!("  {}", display_line(line))),
                );
            }
        }
    }
    if let Some(policy) = row.evidence_policy {
        lines.push(match policy {
            "accept-first-observation" => "Download policy: accept the first downloaded bytes, then require the recorded content on later syncs.".into(),
            "authored-assertions" => "Download policy: require the authored checksums.".into(),
            _ => format!("Download policy: {}", display_line(policy)),
        });
    }
    if let Some(policy) = row.release_policy {
        let policy = match policy {
            "prefer-stable" => "prefer stable versions",
            "stable-only" => "stable versions only",
            "any" => "stable and prerelease versions",
            value => value,
        };
        let origin = match row.policy_origin {
            Some("dependency") => "this dependency",
            Some("project") => "project settings",
            _ => "default",
        };
        lines.push(format!(
            "Version policy: {} ({origin})",
            display_line(policy)
        ));
    }
    if row.explicit
        && report.lock_status != "missing"
        && let Some(reason) = &row.reason
    {
        lines.push(display_line(reason).into_owned());
    }
    if report.lock_status != "current" {
        lines.push("Next: empack sync".into());
    }
    editing(report, Some(&row.key), &mut lines);
    Ok(lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn report() -> AuthoringInspection {
        AuthoringInspection {
            schema: 1,
            author_schema: 6,
            pack: "Workshop 日本".into(),
            lock_status: "current",
            lock_reason: None,
            runtime_satisfied: Some(true),
            dependencies: vec![],
            editing_limitations: vec![],
        }
    }
    #[test]
    fn unchanged_diff_answers_the_question_without_a_report_footer() {
        let report = report();
        assert_eq!(diff(&report), "Dependency settings match the lock.");
    }
    #[test]
    fn missing_lock_is_one_action_not_a_missing_selection_message_per_dependency() {
        let mut report = report();
        report.lock_status = "missing";
        report.runtime_satisfied = None;
        assert_eq!(diff(&report), "No lock to compare with.\nNext: empack sync");
    }

    #[test]
    fn editing_warnings_are_summarized_and_explanation_only_names_affected_settings() {
        use crate::engine::documents::DocumentCodec;
        let source = DocumentCodec.decode_intent(b"schema: 6\npack:\n  id: fixture\n  name: Workshop\n  version: alpha\n  authors: [Author]\nruntime:\n  minecraft: '1.21.1'\n  loader: vanilla\n  java: {preferred: 21}\ndependencies:\n  custom: {path: vendor/custom.jar}\n", "fixture").unwrap();
        let report = DocumentCodec.inspect_authoring(&source, None);
        assert_eq!(report.editing_limitations.len(), 3);
        let checked = diff(&report);
        assert!(
            checked
                .contains("3 settings need simpler YAML for automatic edits (--json lists them).")
        );
        assert!(!checked.contains("Cannot safely edit"));
        let explanation = explain(&report, &report.dependencies[0]).unwrap();
        assert!(explanation.contains("1 setting needs simpler YAML for automatic edits (--json lists them).\n  empack.yml / dependencies.custom"));
        assert!(!explanation.contains("pack.authors"));
        assert!(!explanation.contains("runtime.java"));
        assert!(!explanation.contains("No recorded selection"));
        let machine = serde_json::to_value(&report).unwrap();
        assert_eq!(machine["editing_limitations"].as_array().unwrap().len(), 3);
        assert!(
            machine["editing_limitations"]
                .as_array()
                .unwrap()
                .iter()
                .all(|item| item["reason"].as_str().unwrap().contains("flow"))
        );
    }
}
