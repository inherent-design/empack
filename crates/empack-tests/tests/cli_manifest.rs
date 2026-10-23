//! Author-visible YAML conformance, without acquiring or rewriting project data.
#[path = "support/files.rs"]
mod files;

use empack_tests::harness::process::{TestProject, offline_command};
use serde_json::Value;
use std::{collections::BTreeSet, fs, path::Path, process::Output};

const HEADER: &str = "# Author note: retain this text exactly.\nschema: 5\npack:\n  id: literal.pack\n  name: Literal Pack\n  version: Autumn Update\nruntime:\n  minecraft: '1.21.1'\n  loader: vanilla\n  java: 21\n";

fn inspect(root: &Path, args: &[&str]) -> Output {
    let before = files::snapshot(root);
    let entries = || -> BTreeSet<_> {
        fs::read_dir(root)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect()
    };
    let before_entries = entries();
    let output = offline_command(root)
        .env("EMPACK_STATE_DIR", root.join("unused-manifest-state"))
        .env("EMPACK_CACHE_DIR", root.join("unused-manifest-cache"))
        .arg("manifest")
        .args(args)
        .output()
        .unwrap();
    assert_eq!(files::snapshot(root), before);
    assert_eq!(entries(), before_entries);
    assert!(!root.join("unused-manifest-state").exists());
    assert!(!root.join("unused-manifest-cache").exists());
    assert!(!output.stdout.contains(&0x1b), "{output:?}");
    assert!(!output.stderr.contains(&0x1b), "{output:?}");
    output
}

fn report(root: &Path) -> Value {
    let output = inspect(root, &["check", "--json"]);
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["schema"], 1);
    assert_eq!(report["author_schema"], 5);
    assert_eq!(report["pack"], "Literal Pack");
    assert_eq!(report["lock_status"], "missing");
    assert!(report["runtime_satisfied"].is_null());
    report
}

#[test]
fn cli_manifest_equivalent_source_forms_preserve_literal_authoring() {
    let project = TestProject::new();
    let root = project.dir();
    let digest = "a".repeat(64);
    for (short, expanded, expected_source, policy) in [
        (
            "  example: modrinth:Example1\n".to_owned(),
            "  example:\n    source:\n      kind: provider-selector\n      request:\n        provider: modrinth\n        selector: Example1\n".to_owned(),
            serde_json::json!({"kind": "provider-selector", "request": {"provider": "modrinth", "selector": "Example1"}}),
            None,
        ),
        (
            format!("  example:\n    url: https://example.invalid/custom.jar\n    sha256: '{digest}'\n"),
            format!("  example:\n    source:\n      kind: url\n      downloads: [https://example.invalid/custom.jar]\n    sha256: '{digest}'\n"),
            serde_json::json!({"kind": "url", "downloads": ["https://example.invalid/custom.jar"]}),
            Some("authored-assertions"),
        ),
        (
            "  example:\n    path: absent-private.jar\n".to_owned(),
            "  example:\n    source:\n      kind: local\n      path: absent-private.jar\n".to_owned(),
            serde_json::json!({"kind": "local", "path": "absent-private.jar"}),
            None,
        ),
    ] {
        let mut rows = Vec::new();
        for source in [short, expanded] {
            fs::write(root.join("empack.yml"), format!("{HEADER}dependencies:\n{source}")).unwrap();
            let observed = report(root);
            assert_eq!(observed["dependencies"].as_array().unwrap().len(), 1);
            let row = &observed["dependencies"][0];
            assert_eq!(row["key"], "example");
            assert_eq!(row["explicit"], true);
            assert_eq!(row["effective"]["source"], expected_source);
            assert_eq!(row["evidence_policy"].as_str(), policy);
            let expected_selection = if policy.is_some() {
                serde_json::json!({"mode": "content-assertions", "digests": {"sha256": digest}})
            } else {
                serde_json::json!({"mode": "automatic"})
            };
            assert_eq!(row["effective"]["selection"], expected_selection);
            assert_eq!(row["selection_satisfied"], false);
            assert_eq!(row["reason"], "No recorded selection");
            assert!(row["locked_selection"].is_null());
            rows.push(row.clone());
        }
        assert_eq!(rows[0], rows[1]);
    }
}

#[test]
fn cli_manifest_rejects_ambiguous_authoring_and_bindings_without_rewriting() {
    let project = TestProject::new();
    let root = project.dir();
    for suffix in [
        "schema: 5\n",
        "surprise: true\n",
        "dependencies:\n  example: modrinth:Example1\n  example: modrinth:OtherPrj\n",
        "dependencies:\n  example:\n    path: private.jar\n    path: other.jar\n",
        "dependencies:\n  example: {path: private.jar, pin: Version1}\n",
        "dependencies:\n  example: {source: 'modrinth:Example1', pin: Version1, releases: any}\n",
        "dependencies:\n  example: {source: 'modrinth:Example1', path: private.jar}\n",
        "dependencies:\n  example: {url: 'https://example.invalid/custom.jar'}\n",
        "dependencies:\n  example: {url: 'https://example.invalid/custom.jar', accept-observed: false}\n",
        "dependencies:\n  example: {path: private.jar, unknown: true}\n",
        "dependencies:\n  example: {url: 'https://example.invalid/custom.jar', accept-observed: true, sha256: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa}\n",
    ] {
        fs::write(root.join("empack.yml"), format!("{HEADER}{suffix}")).unwrap();
        let output = inspect(root, &["check", "--json"]);
        assert!(!output.status.success(), "accepted {suffix}: {output:?}");
        assert!(output.stdout.is_empty(), "{output:?}");
        assert!(!output.stderr.is_empty(), "{output:?}");
    }
    fs::write(
        root.join("empack.yml"),
        format!("{HEADER}dependencies:\n  example: modrinth:Example1\n"),
    )
    .unwrap();
    for binding in [
        "schema: 1\nschema: 1\nbindings: {}\n",
        "schema: 1\nbindings:\n  example: {}\n",
        "schema: 1\nbindings:\n  example: {source: 'modrinth:OtherPrj'}\n",
        "schema: 1\nbindings:\n  missing: {placement: automatic}\n",
        "schema: 1\nbindings: {}\npolicies:\n  icon.png: seed\n  icon.png: managed\n",
    ] {
        fs::write(root.join("empack.bindings.yml"), binding).unwrap();
        let output = inspect(root, &["check", "--json"]);
        assert!(!output.status.success(), "accepted {binding}: {output:?}");
        assert!(output.stdout.is_empty(), "{output:?}");
        assert!(!output.stderr.is_empty(), "{output:?}");
    }
}

#[test]
fn cli_manifest_composes_bindings_and_rejects_competing_placement_authority() {
    let project = TestProject::new();
    let root = project.dir();
    let source = format!("{HEADER}dependencies:\n  example:\n    path: absent-private.jar\n");
    fs::write(root.join("empack.yml"), &source).unwrap();
    fs::write(root.join("empack.bindings.yml"), "# User placement, not provider identity.\nschema: 1\nbindings:\n  example:\n    placement:\n      - destination: mods/custom.jar\n        layer: client\n        environment: {client: required, server: unsupported}\n").unwrap();
    let observed = report(root);
    assert_eq!(
        observed["dependencies"][0]["effective"]["placement"],
        serde_json::json!([
            {"destination": "mods/custom.jar", "layer": "client",
             "environment": {"client": "required", "server": "unsupported"}}
        ])
    );
    fs::write(root.join("empack.yml"), format!("{source}    placement:\n      - destination: mods/competing.jar\n        layer: common\n        environment: {{client: required, server: required}}\n")).unwrap();
    let output = inspect(root, &["check", "--json"]);
    assert!(!output.status.success(), "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("Conflicting placement in empack.yml and empack.bindings.yml")
    );
}

#[test]
fn cli_manifest_reports_explicit_url_observation_and_editing_limits() {
    let project = TestProject::new();
    let root = project.dir();
    fs::write(root.join("empack.yml"), format!("{HEADER}dependencies:\n  custom: {{url: 'https://example.invalid/custom.jar', accept-observed: true}} # keep inline note\n")).unwrap();
    let observed = report(root);
    assert_eq!(
        observed["dependencies"][0]["evidence_policy"],
        "accept-first-observation"
    );
    assert!(
        observed["editing_limitations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| {
                item["document"] == "empack.yml"
                    && item["path"] == serde_json::json!(["dependencies", "custom"])
                    && item["reason"].as_str().unwrap().contains("flow")
            })
    );
    let output = inspect(root, &["explain", "custom"]);
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(
        text.contains(
            "URL evidence: accept-first-observation; retained observations remain binding"
        )
    );
    assert!(text.contains("Editing limitations (input remains readable):"));
    assert!(text.contains("Local metadata only: no provider queries or file verification."));
}
