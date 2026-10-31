//! Author-visible YAML conformance, without acquiring or rewriting project data.
#[path = "support/files.rs"]
mod files;

use empack_tests::harness::process::{TestProject, offline_command};
use serde_json::Value;
use std::{fs, path::Path, process::Output};

const HEADER: &str = "# Author note: retain this text exactly.\nschema: 5\npack:\n  id: literal.pack\n  name: Literal Pack\n  version: Autumn Update\nruntime:\n  minecraft: '1.21.1'\n  loader: vanilla\n  java: 21\n";

fn inspect(root: &Path, args: &[&str]) -> Output {
    let before = empack_tests::harness::workspace::snapshot(root);
    let output = offline_command(root)
        .env("EMPACK_STATE_DIR", root.join("unused-manifest-state"))
        .env("EMPACK_CACHE_DIR", root.join("unused-manifest-cache"))
        .arg("manifest")
        .args(args)
        .output()
        .unwrap();
    assert_eq!(empack_tests::harness::workspace::snapshot(root), before);
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
fn cli_manifest_documented_projects_preserve_literal_yaml_and_report_exact_requests() {
    // Read the published examples themselves. A copied test fixture would keep
    // passing when a documentation edit silently makes the shown YAML invalid.
    fn yaml_block(text: &str) -> (&str, &str) {
        let (_, tail) = text.split_once("```yaml\n").expect("documented YAML block");
        tail.split_once("\n```").expect("closed YAML block")
    }
    // Git may check Markdown out with CRLF on Windows. Normalize only line
    // endings before locating fences; the YAML fields still come from the docs.
    let manifest_document = include_str!("../../../docs/manifest.md").replace("\r\n", "\n");
    let usage_document = include_str!("../../../docs/usage.md").replace("\r\n", "\n");
    let (manifest, _) = yaml_block(&manifest_document);
    let (_, native_section) = usage_document
        .split_once("## Native release example\n")
        .expect("documented native release example");
    let (native, remainder) = yaml_block(native_section);
    let (bindings, _) = yaml_block(remainder);
    for (author, binding, pack) in [
        (manifest, None, "Workshop"),
        (native, Some(bindings), "My Pack"),
    ] {
        let project = TestProject::new();
        let root = project.dir();
        fs::write(root.join("empack.yml"), author).unwrap();
        if let Some(binding) = binding {
            fs::write(root.join("empack.bindings.yml"), binding).unwrap();
        }
        let output = inspect(root, &["check", "--json"]);
        assert!(output.status.success(), "{pack}: {output:?}");
        assert!(output.stderr.is_empty(), "{pack}: {output:?}");
        let observed: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(observed["schema"], 1);
        assert_eq!(observed["author_schema"], 5);
        assert_eq!(observed["pack"], pack);
        assert_eq!(observed["lock_status"], "missing");
        assert!(observed["runtime_satisfied"].is_null());
        let rows = observed["dependencies"].as_array().unwrap();
        if pack == "Workshop" {
            assert_eq!(
                rows.iter()
                    .map(|row| row["key"].as_str().unwrap())
                    .collect::<Vec<_>>(),
                ["house-rules", "menu", "openloader", "textures"]
            );
            for (key, source, content, environment) in [
                (
                    "house-rules",
                    serde_json::json!({"kind": "local", "path": "vendor/house-rules.jar"}),
                    "mod",
                    serde_json::json!({"client": "required", "server": "required"}),
                ),
                (
                    "menu",
                    serde_json::json!({"kind": "provider-selector", "request": {"provider": "modrinth", "selector": "fancymenu"}}),
                    "mod",
                    serde_json::json!({"client": "required", "server": "unsupported"}),
                ),
                (
                    "openloader",
                    serde_json::json!({"kind": "provider-selector", "request": {"provider": "modrinth", "selector": "open-loader"}}),
                    "mod",
                    serde_json::json!({"client": "required", "server": "required"}),
                ),
                (
                    "textures",
                    serde_json::json!({"kind": "local", "path": "vendor/textures.zip"}),
                    "resource-pack",
                    serde_json::json!({"client": {"optional": "textures", "default-enabled": false, "description": null}, "server": "unsupported"}),
                ),
            ] {
                let row = rows.iter().find(|row| row["key"] == key).unwrap();
                assert_eq!(row["explicit"], true);
                assert_eq!(
                    row["effective"],
                    serde_json::json!({
                        "source": source, "content": content, "environment": environment,
                        "selection": {"mode": "automatic"}, "placement": "automatic", "releases": null
                    })
                );
                assert_eq!(row["selection_satisfied"], false);
                assert_eq!(row["reason"], "No recorded selection");
                assert!(row["canonical_project"].is_null());
                assert!(row["locked_selection"].is_null());
            }
        } else {
            assert!(rows.is_empty());
        }
        // The shared inspection helper already checks the complete author
        // snapshot and absence of host storage after each public invocation.
        assert_eq!(
            fs::read(root.join("empack.yml")).unwrap(),
            author.as_bytes()
        );
        if let Some(binding) = binding {
            assert_eq!(
                fs::read(root.join("empack.bindings.yml")).unwrap(),
                binding.as_bytes()
            );
        }
    }
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
            "  example:\n    url: https://example.invalid/custom.jar\n    accept-observed: true\n".to_owned(),
            "  example:\n    source:\n      kind: url\n      downloads: [https://example.invalid/custom.jar]\n    accept-observed: true\n".to_owned(),
            serde_json::json!({"kind": "url", "downloads": ["https://example.invalid/custom.jar"]}),
            Some("accept-first-observation"),
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
            let expected_selection = if policy == Some("authored-assertions") {
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
        "dependencies:\n  example: {source: {kind: url, downloads: ['https://example.invalid/custom.jar']}}\n",
        "dependencies:\n  example: {source: {kind: url, downloads: ['https://example.invalid/custom.jar']}, accept-observed: false}\n",
        "dependencies:\n  example: {source: {kind: url, downloads: ['https://example.invalid/custom.jar']}, accept-observed: true, sha256: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa}\n",
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

fn fixture_jar() -> Vec<u8> {
    use std::io::{Cursor, Write};
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    archive
        .start_file("fabric.mod.json", zip::write::SimpleFileOptions::default())
        .unwrap();
    archive
        .write_all(br#"{"schemaVersion":1,"id":"fixture","version":"1"}"#)
        .unwrap();
    archive.finish().unwrap().into_inner()
}

#[test]
fn cli_manifest_explanation_matches_local_synchronization_and_preserves_inputs() {
    let jar = fixture_jar();
    for change in [
        "unchanged",
        "display",
        "source",
        "missing-source",
        "layout",
        "binding",
    ] {
        let project = TestProject::new();
        let root = project.dir();
        // Runtime journals may change even for converged publication. Compare
        // author/project bytes independently from the host's operation storage.
        let storage = tempfile::tempdir().unwrap();
        let sync = || {
            offline_command(root)
                .env("EMPACK_STATE_DIR", storage.path().join("state"))
                .env("EMPACK_CACHE_DIR", storage.path().join("cache"))
                .args(["--yes", "sync", "--materialize"])
                .assert()
        };
        fs::create_dir(root.join("vendor")).unwrap();
        fs::write(root.join("vendor/original.jar"), &jar).unwrap();
        fs::write(root.join("vendor/alternate.jar"), &jar).unwrap();
        let author = format!("{HEADER}dependencies:\n  example:\n    path: vendor/original.jar\n");
        fs::write(root.join("empack.yml"), &author).unwrap();
        sync().success();
        let original_lock = fs::read(root.join("empack.lock")).unwrap();
        assert_eq!(fs::read(root.join("pack/mods/original.jar")).unwrap(), jar);
        let changed_author = match change {
            "display" => author.replace("name: Literal Pack", "name: Renamed Pack"),
            "source" => author.replace("vendor/original.jar", "vendor/alternate.jar"),
            "missing-source" => author.replace("vendor/original.jar", "vendor/missing.jar"),
            "layout" => format!("{author}layout:\n  mod: custom-mods\n"),
            _ => author.clone(),
        };
        fs::write(root.join("empack.yml"), &changed_author).unwrap();
        let binding = "# Preserve this author-owned binding.\nschema: 1\nbindings:\n  example:\n    placement:\n      - destination: mods/renamed.jar\n        layer: common\n        environment: {client: required, server: required}\n";
        if change == "binding" {
            fs::write(root.join("empack.bindings.yml"), binding).unwrap();
        }
        let satisfied = matches!(change, "unchanged" | "display");
        let output = inspect(root, &["diff", "--json"]);
        assert!(output.status.success(), "{change}: {output:?}");
        assert!(output.stderr.is_empty(), "{output:?}");
        let diff: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(
            diff["lock_status"],
            if change == "unchanged" {
                "current"
            } else {
                "stale"
            },
            "{change}: {diff}"
        );
        // A display edit invalidates the recorded document revision without
        // invalidating the exact dependency selection; sync may rebind it.
        assert_eq!(diff["lock_reason"].is_null(), change == "unchanged");
        assert_eq!(diff["runtime_satisfied"], true);
        assert_eq!(diff["dependencies"].as_array().unwrap().len(), 1);
        let expected_row = &diff["dependencies"][0];
        assert_eq!(expected_row["key"], "example");
        assert_eq!(
            expected_row["selection_satisfied"], satisfied,
            "{change}: {diff}"
        );
        assert_eq!(expected_row["reason"].is_null(), satisfied);
        let output = inspect(root, &["explain", "example", "--json"]);
        assert!(output.status.success(), "{output:?}");
        assert!(output.stderr.is_empty(), "{output:?}");
        assert_eq!(
            serde_json::from_slice::<Value>(&output.stdout).unwrap(),
            *expected_row
        );
        for args in [&["diff"][..], &["explain", "example"][..]] {
            let output = inspect(root, args);
            assert!(output.status.success(), "{output:?}");
            assert!(output.stderr.is_empty(), "{output:?}");
            let text = String::from_utf8(output.stdout).unwrap();
            assert!(
                text.contains("Local metadata only: no provider queries or file verification.")
            );
            if let Some(reason) = expected_row["reason"].as_str() {
                assert!(text.contains(reason), "{change}: {text}");
            }
            if args == ["diff"]
                && let Some(reason) = diff["lock_reason"].as_str()
            {
                assert!(text.contains(reason), "{change}: {text}");
            }
        }
        let output = inspect(root, &["explain", "absent-key", "--json"]);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(
            String::from_utf8(output.stderr)
                .unwrap()
                .contains("Unknown dependency key")
        );
        if change == "missing-source" {
            let before = files::snapshot(root);
            let failed = sync().failure();
            assert!(
                String::from_utf8_lossy(&failed.get_output().stderr).contains("missing.jar"),
                "{failed:?}"
            );
            assert_eq!(
                files::snapshot(root),
                before,
                "failed resolution changed project bytes"
            );
            continue;
        }
        sync().success();
        assert_eq!(
            fs::read_to_string(root.join("empack.yml")).unwrap(),
            changed_author
        );
        if change == "binding" {
            assert_eq!(
                fs::read_to_string(root.join("empack.bindings.yml")).unwrap(),
                binding
            );
        }
        let destination = match change {
            "source" => "pack/mods/alternate.jar",
            "layout" => "pack/custom-mods/original.jar",
            "binding" => "pack/mods/renamed.jar",
            _ => "pack/mods/original.jar",
        };
        assert_eq!(fs::read(root.join(destination)).unwrap(), jar, "{change}");
        if !satisfied {
            assert!(!root.join("pack/mods/original.jar").exists(), "{change}");
        }
        if change == "unchanged" {
            assert_eq!(
                fs::read(root.join("empack.lock")).unwrap(),
                original_lock,
                "{change}"
            );
        } else {
            assert_ne!(
                fs::read(root.join("empack.lock")).unwrap(),
                original_lock,
                "{change}"
            );
        }
        let output = inspect(root, &["diff", "--json"]);
        assert!(output.status.success(), "{output:?}");
        let diff: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(diff["lock_status"], "current", "{change}: {diff}");
        assert_eq!(diff["dependencies"][0]["selection_satisfied"], true);
        let stable = files::snapshot(root);
        sync().success();
        assert_eq!(
            files::snapshot(root),
            stable,
            "{change}: repeated sync changed bytes"
        );
    }
}

#[test]
fn cli_sync_distinguishes_lock_states_and_preserves_populated_author_work() {
    use empack_core::{digest::ContentId, model::DependencyKey};
    use empack_tests::harness::process::read_project;
    use sha2::{Digest, Sha256};
    const INSTALLED: &str = "pack/mods/custom.jar";
    const BINDING: &str = "# Placement remains author-owned.\nschema: 1\nbindings:\n  example:\n    placement:\n      - destination: mods/custom.jar\n        layer: common\n        environment: {client: required, server: required}\n";
    let jar = fixture_jar();
    for materialize in [false, true] {
        for state in [
            "current",
            "missing",
            "missing-occupied-identical",
            "missing-occupied-different",
            "stale-display",
            "corrupt-syntax",
            "unsupported-schema",
            "missing-installed",
            "modified-installed",
            "missing-source-and-installed",
        ] {
            let project = TestProject::new();
            let root = project.dir();
            let host = tempfile::tempdir().unwrap();
            let command = |preview| {
                let mut cmd = offline_command(root);
                cmd.env("EMPACK_STATE_DIR", host.path().join("state"))
                    .env("EMPACK_CACHE_DIR", host.path().join("cache"))
                    .env("TMPDIR", host.path())
                    .env("TMP", host.path())
                    .env("TEMP", host.path());
                cmd.args(["--yes", "sync"]);
                if preview {
                    cmd.arg("--dry-run");
                }
                if materialize {
                    cmd.arg("--materialize");
                }
                cmd
            };
            let author = format!(
                "{HEADER}dependencies:\n  example:\n    path: vendor/original.jar # retained source\n"
            );
            for (name, bytes) in [
                ("empack.yml", author.as_bytes()),
                ("empack.bindings.yml", BINDING.as_bytes()),
                ("vendor/original.jar", jar.as_slice()),
                (
                    "pack/config/user.properties",
                    b"author choice=true\n".as_slice(),
                ),
                ("pack/mods/untracked.jar", b"untracked payload".as_slice()),
                ("templates/client/note.txt", b"author template".as_slice()),
                ("notes/decision.md", b"retain this decision".as_slice()),
                ("dist/prior.zip", b"prior artifact".as_slice()),
            ] {
                let path = root.join(name);
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(path, bytes).unwrap();
            }
            let pristine = files::snapshot(root);
            // First resolve from literal documents with no lock and no occupied
            // tracked destination; all other populated author work already exists.
            command(true).assert().success();
            assert_eq!(files::snapshot(root), pristine);
            command(false).assert().success();
            let resolved = read_project(root);
            let key = DependencyKey::parse("example").unwrap();
            assert_eq!(resolved.lock().dependencies.len(), 1);
            let selected = &resolved.lock().dependencies[&key];
            assert!(selected.selected.is_none());
            let file = &selected.files.as_slice()[0];
            assert_eq!(selected.files.as_slice().len(), 1);
            assert_eq!(file.expected.size, Some(jar.len() as u64));
            assert!(
                file.expected.digests.is_none(),
                "local observation is not a source declaration"
            );
            assert_eq!(
                file.expected.accepted_observation,
                Some(ContentId::from_sha256(Sha256::digest(&jar).into()))
            );
            assert_eq!(
                file.placements.as_slice()[0]
                    .destination
                    .relative()
                    .as_str(),
                "mods/custom.jar"
            );
            let lock = fs::read(root.join("empack.lock")).unwrap();
            let mut expected_files = pristine.clone();
            expected_files.insert("empack.lock".into(), lock.clone());
            expected_files.insert(INSTALLED.into(), jar.clone());
            assert_eq!(
                files::snapshot(root),
                expected_files,
                "{state}: initial resolution"
            );
            match state {
                "missing" => {
                    fs::remove_file(root.join("empack.lock")).unwrap();
                    fs::remove_file(root.join(INSTALLED)).unwrap();
                }
                "missing-occupied-identical" | "missing-occupied-different" => {
                    fs::remove_file(root.join("empack.lock")).unwrap();
                    if state.ends_with("different") {
                        fs::write(root.join(INSTALLED), b"unowned different bytes").unwrap();
                    }
                }
                "stale-display" => fs::write(
                    root.join("empack.yml"),
                    author.replace("name: Literal Pack", "name: Renamed Pack"),
                )
                .unwrap(),
                "corrupt-syntax" => fs::write(root.join("empack.lock"), b"schema: [\n").unwrap(),
                "unsupported-schema" => {
                    fs::write(root.join("empack.lock"), b"schema: 999\n").unwrap()
                }
                "missing-installed" | "missing-source-and-installed" => {
                    fs::remove_file(root.join(INSTALLED)).unwrap();
                    if state == "missing-source-and-installed" {
                        fs::remove_file(root.join("vendor/original.jar")).unwrap();
                        // A retained cache is not evidence that the missing source
                        // can be reacquired. Require the actual unavailable-input path.
                        let cache = host.path().join("cache");
                        if cache.exists() {
                            fs::remove_dir_all(cache).unwrap();
                        }
                    }
                }
                "modified-installed" => {
                    fs::write(root.join(INSTALLED), b"modified installed bytes").unwrap()
                }
                "current" => {}
                _ => unreachable!(),
            }
            let before = files::snapshot(root);
            let refusal = match state {
                "missing-occupied-identical" | "missing-occupied-different" => {
                    Some("untracked content")
                }
                "corrupt-syntax" => Some("Invalid lock document"),
                "unsupported-schema" => Some("Unsupported lock schema"),
                "missing-source-and-installed" => Some("original.jar"),
                _ => None,
            };
            for preview in [true, false] {
                let output = command(preview).output().unwrap();
                let diagnostic = String::from_utf8_lossy(&output.stderr);
                if let Some(reason) = refusal {
                    assert!(
                        !output.status.success(),
                        "{state}, materialize={materialize}, preview={preview}"
                    );
                    assert!(diagnostic.contains(reason), "{state}: {diagnostic}");
                } else {
                    assert!(output.status.success(), "{state}: {diagnostic}");
                }
                if preview || refusal.is_some() {
                    assert_eq!(
                        files::snapshot(root),
                        before,
                        "{state}: unexpected publication"
                    );
                }
            }
            if refusal.is_some() {
                continue;
            }
            let current = read_project(root);
            let mut expected_lock = resolved.lock().clone();
            expected_lock.intent_revision = current.lock().intent_revision;
            assert_eq!(
                current.lock(),
                &expected_lock,
                "{state}: changed retained resolution"
            );
            if state == "stale-display" {
                assert_ne!(
                    current.lock().intent_revision,
                    resolved.lock().intent_revision
                );
                expected_files.insert(
                    "empack.yml".into(),
                    author
                        .replace("name: Literal Pack", "name: Renamed Pack")
                        .into_bytes(),
                );
                expected_files.insert(
                    "empack.lock".into(),
                    fs::read(root.join("empack.lock")).unwrap(),
                );
            }
            assert_eq!(
                files::snapshot(root),
                expected_files,
                "{state}: exact restored project"
            );
            for _ in 0..2 {
                command(false).assert().success();
                assert_eq!(
                    files::snapshot(root),
                    expected_files,
                    "{state}: repeated sync"
                );
            }
        }
    }
}
