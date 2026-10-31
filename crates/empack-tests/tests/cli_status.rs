//! Status reports bounded metadata; it neither repairs nor qualifies a workspace.
#[path = "support/files.rs"]
mod files;

use empack_tests::harness::process::{TestProject, offline_command};
use serde_json::Value;
use std::{fs, path::Path};

const AUTHOR: &str = include_str!("../../../tests/fixtures/authoring/empack.yml");

fn inspect(root: &Path, context: Option<&str>) -> (Value, String) {
    let before = files::snapshot(root);
    let entries = || -> std::collections::BTreeSet<_> {
        fs::read_dir(root)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect()
    };
    let before_entries = entries();
    let mut outputs = Vec::new();
    for json in [true, false] {
        let mut command = offline_command(root);
        command
            .env("EMPACK_STATE_DIR", root.join("unused-status-state"))
            .env("EMPACK_CACHE_DIR", root.join("unused-status-cache"))
            .arg("status");
        if let Some(context) = context {
            command.args(["--context", context]);
        }
        if json {
            command.arg("--json");
        }
        let output = command.output().unwrap();
        assert_eq!(output.status.code(), Some(0), "{output:?}");
        assert!(output.stderr.is_empty(), "{output:?}");
        outputs.push(String::from_utf8(output.stdout).unwrap());
        assert_eq!(files::snapshot(root), before);
        assert_eq!(entries(), before_entries);
        assert!(!root.join("unused-status-state").exists());
        assert!(!root.join("unused-status-cache").exists());
    }
    let report: Value = serde_json::from_str(&outputs[0]).unwrap();
    assert_eq!(report["schema"], 1);
    assert_eq!(report["content_verified"], false);
    assert_eq!(
        Path::new(report["root"].as_str().unwrap())
            .canonicalize()
            .unwrap(),
        root.canonicalize().unwrap()
    );
    assert!(outputs[1].contains("Metadata inspection only; content bytes have not been verified."));
    // Both renderings must retain findings and actionable argument sequences.
    for finding in report["findings"].as_array().unwrap() {
        assert!(outputs[1].contains(finding["code"].as_str().unwrap()));
        assert!(outputs[1].contains(finding["message"].as_str().unwrap()));
        let next: Vec<_> = finding["next"]
            .as_array()
            .unwrap()
            .iter()
            .map(|part| part.as_str().unwrap())
            .collect();
        if !next.is_empty() {
            assert!(outputs[1].contains(&format!(
                "Next in this workspace: empack {}",
                next.join(" ")
            )));
        }
    }
    (report, outputs.remove(1))
}

fn finding(report: &Value, code: &str) -> bool {
    report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|row| row["code"] == code)
}

#[test]
fn cli_status_distinguishes_author_and_lock_states_without_repair() {
    use sha2::{Digest, Sha256};
    for state in [
        "absent",
        "invalid-author",
        "missing",
        "invalid",
        "valid",
        "stale",
    ] {
        let project = TestProject::new();
        let root = project.dir();
        if state != "absent" {
            fs::write(
                root.join("empack.yml"),
                if state == "invalid-author" {
                    "schema: [broken author text\n"
                } else {
                    AUTHOR
                },
            )
            .unwrap();
        }
        match state {
            "invalid" => fs::write(root.join("empack.lock"), b"schema: [broken lock\n").unwrap(),
            "valid" | "stale" => {
                fs::create_dir_all(root.join("pack/config")).unwrap();
                fs::write(root.join("pack/config/local.txt"), b"recorded content\n").unwrap();
                offline_command(root)
                    .args(["--yes", "sync"])
                    .assert()
                    .success();
                if state == "stale" {
                    fs::write(root.join("empack.yml"), AUTHOR.replace("1.21.1", "1.21.2")).unwrap();
                }
                // A current metadata lock must not imply verification of these bytes.
                fs::write(
                    root.join("pack/config/local.txt"),
                    b"changed after synchronization\n",
                )
                .unwrap();
            }
            _ => {}
        }
        let (report, text) = inspect(root, None);
        let contexts = report["contexts"].as_array().unwrap();
        if state == "absent" {
            assert!(contexts.is_empty());
            assert!(report["selected"].is_null());
            assert!(finding(&report, "workspace-empty"));
            assert!(text.contains("Next in this workspace: empack init --help"));
            continue;
        }
        assert_eq!(report["selected"], "author");
        assert_eq!(contexts.len(), 1);
        let author = &contexts[0];
        assert_eq!(author["context"], "author");
        if state == "invalid-author" {
            assert_eq!(author["state"], "invalid");
            assert!(author["lock"].is_null());
            assert!(author["name"].is_null());
            assert!(finding(&report, "context-not-ready"));
            assert!(text.contains("Author: Invalid"));
            continue;
        }
        assert_eq!(author["state"], "valid");
        assert_eq!(author["lock"], state);
        assert_eq!(author["name"], "Verification Pack");
        assert_eq!(author["version"], "1.0.0");
        assert_eq!(
            author["minecraft"],
            if state == "stale" { "1.21.2" } else { "1.21.1" }
        );
        assert_eq!(author["loader"], "Vanilla");
        assert!(text.contains("Author: Valid"));
        assert!(text.contains("Pack: Verification Pack 1.0.0"));
        let label = match state {
            "missing" => "Missing",
            "invalid" => "Invalid",
            "stale" => "Stale",
            "valid" => "Valid",
            _ => unreachable!(),
        };
        assert!(text.contains(&format!("Resolution lock: {label}")));
        assert_eq!(finding(&report, "lock-not-current"), state != "valid");
        if state == "valid" {
            let digest: String = Sha256::digest(fs::read(root.join("empack.lock")).unwrap())
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect();
            assert_eq!(author["exact"], digest);
            assert!(text.contains(&format!("Lock document SHA-256: {digest}")));
        } else {
            assert!(author["exact"].is_null());
            assert!(text.contains("Next in this workspace: empack --dry-run sync"));
        }
    }
}

#[test]
fn cli_status_requires_explicit_context_for_mixed_roots() {
    let project = TestProject::new();
    let root = project.dir();
    // Explicit selection distinguishes missing contexts from an undetected workspace.
    for (context, label) in [
        ("author", "Author"),
        ("instance", "Instance"),
        ("publisher", "Publisher"),
    ] {
        let (report, text) = inspect(root, Some(context));
        assert_eq!(report["selected"], context);
        assert_eq!(report["contexts"].as_array().unwrap().len(), 1);
        assert_eq!(report["contexts"][0]["state"], "missing");
        assert!(finding(&report, "context-not-ready"));
        assert!(!finding(&report, "workspace-empty"));
        assert!(text.contains(&format!("{label}: Missing")));
    }
    fs::write(root.join("empack.yml"), b"schema: [invalid\n").unwrap();
    fs::create_dir(root.join(".empack")).unwrap();
    fs::write(root.join(".empack/instance.json"), b"{malformed instance\n").unwrap();
    fs::create_dir_all(root.join("dist/releases")).unwrap();
    fs::write(
        root.join("dist/releases/untrusted.json"),
        b"not authenticated\n",
    )
    .unwrap();
    let expected = [
        ("author", "Author", "invalid", "Invalid"),
        ("instance", "Instance", "invalid", "Invalid"),
        ("publisher", "Publisher", "unavailable", "Unavailable"),
    ];
    for selected in [None, Some("author"), Some("instance"), Some("publisher")] {
        let (report, text) = inspect(root, selected);
        assert_eq!(report["selected"].as_str(), selected);
        assert_eq!(report["contexts"].as_array().unwrap().len(), 3);
        assert_eq!(finding(&report, "context-ambiguous"), selected.is_none());
        for (context, label, state, display) in expected {
            let row = report["contexts"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["context"] == context)
                .unwrap();
            assert_eq!(row["state"], state);
            assert_eq!(
                text.lines()
                    .any(|line| line == format!("{label}: {display}")),
                selected.is_none() || selected == Some(context)
            );
        }
    }
}
