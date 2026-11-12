//! Status reports bounded metadata; it neither repairs nor qualifies a workspace.
use empack_tests::harness::{
    process::{TestProject, offline_command},
    workspace::snapshot,
};
use serde_json::Value;
use std::{fs, path::Path};

const AUTHOR: &str = include_str!("../../../tests/fixtures/authoring/empack.yml");

fn inspect(root: &Path, context: Option<&str>) -> (Value, String) {
    let before = snapshot(root);
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
        assert_eq!(snapshot(root), before);
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
    // Human output keeps actionable findings; machine-only codes remain in JSON.
    for finding in report["findings"].as_array().unwrap() {
        let code = finding["code"].as_str().unwrap();
        if !matches!(code, "context-not-ready" | "lock-not-current") {
            assert!(outputs[1].contains(finding["message"].as_str().unwrap()));
        }
        assert!(!outputs[1].contains(&format!("{code}:")));
        let next: Vec<_> = finding["next"]
            .as_array()
            .unwrap()
            .iter()
            .map(|part| part.as_str().unwrap())
            .collect();
        if !next.is_empty() {
            assert!(outputs[1].contains(&format!("Next: empack {}", next.join(" "))));
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
            assert!(text.contains("Next: empack init --help"));
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
            assert!(text.contains("Project: invalid"));
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
        assert!(text.starts_with("Project: Verification Pack 1.0.0\n"));
        let label = match state {
            "missing" => "not found",
            "invalid" => "invalid",
            "stale" => "needs sync",
            "valid" => "current",
            _ => unreachable!(),
        };
        assert!(text.contains(&format!("Lock: {label}")));
        assert_eq!(finding(&report, "lock-not-current"), state != "valid");
        if state == "valid" {
            let digest: String = Sha256::digest(fs::read(root.join("empack.lock")).unwrap())
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect();
            assert_eq!(author["exact"], digest);
            assert!(!text.contains(&digest));
            assert!(text.contains("File contents have not been checked."));
        } else {
            assert!(author["exact"].is_null());
            assert!(text.contains("Next: empack --dry-run sync"));
        }
    }
}

#[test]
fn cli_status_requires_explicit_context_for_mixed_roots() {
    let project = TestProject::new();
    let root = project.dir();
    // Explicit selection distinguishes missing contexts from an undetected workspace.
    for (context, label) in [
        ("author", "Project"),
        ("instance", "Instance"),
        ("publisher", "Publisher"),
    ] {
        let (report, text) = inspect(root, Some(context));
        assert_eq!(report["selected"], context);
        assert_eq!(report["contexts"].as_array().unwrap().len(), 1);
        assert_eq!(report["contexts"][0]["state"], "missing");
        assert!(finding(&report, "context-not-ready"));
        assert!(!finding(&report, "workspace-empty"));
        assert!(text.contains(&format!("{label}: not found")));
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
        ("author", "Project", "invalid", "invalid"),
        ("instance", "Instance", "invalid", "invalid"),
        ("publisher", "Publisher", "unavailable", "not verified"),
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

#[test]
fn cli_instance_check_verifies_managed_content_without_repairing_player_data() {
    use empack_lib::engine::release::*;
    use sha2::{Digest, Sha256};
    let project = TestProject::new();
    let root = project.dir();
    fs::create_dir(root.join("assets")).unwrap();
    let files = [
        (
            "mod",
            "mods/check.jar",
            FilePolicy::Managed,
            &b"mod bytes"[..],
            None,
        ),
        (
            "options",
            "options.txt",
            FilePolicy::Seed,
            &b"defaults"[..],
            None,
        ),
        (
            "world",
            "saves/played/level.dat",
            FilePolicy::Seed,
            &b"initial level"[..],
            Some("saves/played"),
        ),
    ]
    .into_iter()
    .map(|(key, destination, policy, bytes, world)| {
        fs::write(root.join("assets").join(key), bytes).unwrap();
        ReleaseFile {
            world_root: world.map(str::to_owned),
            key: key.into(),
            destination: destination.into(),
            layer: ReleaseLayer::Common,
            policy,
            client: Participation::Required,
            server: Participation::Unsupported,
            sha256: Sha256::digest(bytes)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect(),
            bytes: bytes.len() as u64,
            readonly: false,
            executable: false,
            assertions: vec![],
            asset: None,
            source: ReleaseSource::Asset {
                path: format!("assets/{key}"),
            },
        }
    })
    .collect();
    let release = DecodedRelease::encode(ReleaseDocument {
        schema: RELEASE_SCHEMA,
        pack: "check.fixture".into(),
        version: "1".into(),
        minimum_engine: ">=0.8.0-beta".into(),
        require_subscription: false,
        server_launch: None,
        runtime: ReleaseRuntime {
            minecraft: "1.21.1".into(),
            loader: ReleaseLoader::Vanilla,
            java: ReleaseJava::for_game("1.21.1", 21).unwrap(),
        },
        choices: vec![],
        files,
    })
    .unwrap();
    fs::write(root.join("release.json"), release.bytes()).unwrap();
    offline_command(root)
        .args([
            "--yes",
            "instance",
            "install",
            "release.json",
            "--sha256",
            release.id(),
            "--layout",
            "prism",
        ])
        .assert()
        .success();
    fs::write(root.join(".minecraft/options.txt"), b"player choices").unwrap();
    fs::remove_file(root.join(".minecraft/saves/played/level.dat")).unwrap();
    let before = snapshot(root);
    for (json, details) in [(false, false), (true, false), (false, true)] {
        let mut command = offline_command(root);
        command.args(["instance", "check", "--quiet"]);
        if json {
            command.arg("--json");
        }
        if details {
            command.arg("--details");
        }
        let output = command.output().unwrap();
        assert!(output.status.success(), "{output:?}");
        assert!(output.stderr.is_empty(), "{output:?}");
        if json {
            let report: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(report["scope"], "instance");
            assert_eq!(report["complete"], true);
            assert_eq!(report["verified_files"], 1);
            assert_eq!(report["player_owned_entries"], 2);
        } else {
            assert!(String::from_utf8_lossy(&output.stdout).contains("Managed files verified: 1"));
        }
        assert_eq!(snapshot(root), before);
    }
    fs::write(root.join(".minecraft/mods/check.jar"), b"changed").unwrap();
    let before = snapshot(root);
    let output = offline_command(root)
        .args(["instance", "check", "--json", "--quiet"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["complete"], false);
    assert!(!output.stderr.is_empty());
    assert_eq!(snapshot(root), before);
    fs::write(root.join(".empack/instance.json"), b"{broken").unwrap();
    let before = snapshot(root);
    let output = offline_command(root)
        .args(["instance", "check", "--json", "--quiet"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["complete"], false);
    assert_eq!(snapshot(root), before);
}
