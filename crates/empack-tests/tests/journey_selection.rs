//! Selection edits and removals over a fixed, offline provider resolution.
use empack_core::{
    digest::{DigestSet, ExpectedDigest},
    identity::{ModrinthProjectId, ModrinthVersionId, PinSelector, ProviderProjectId},
    model::*,
    path::{InstallDestination, PathSyntax, PortableRelPath},
    requirements::{Requirement, Requirements},
};
use empack_lib::engine::documents::DocumentCodec;
use empack_tests::harness::process::{TestProject, offline_command, read_project};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs, path::Path};

#[path = "support/files.rs"]
mod files;

const AUTHOR: &str = r#"# Keep this author's selection notes.
schema: 5
pack:
  id: selection.fixture
  name: &title Selection fixture
  description: *title
  version: '1'
  authors: [Fixture]
runtime: {minecraft: '1.21.1', loader: vanilla, java: 21}
dependency-policy: {releases: stable-only}
dependencies:
  renderer-alias:
    source: modrinth:Project1 # Keep the canonical source and logical alias.
    type: resource-pack
    releases: any
  local:
    path: vendor/local.zip
    type: resource-pack
build: {default: [], profiles: {}}
"#;
const BINDINGS: &str = r#"# Keep the custom destination independent of provider filenames.
schema: 1
bindings:
  renderer-alias:
    placement:
      - destination: resourcepacks/custom-name.zip
        layer: common
        environment: {client: required, server: required}
"#;

fn write(root: &Path, name: &str, bytes: &[u8]) {
    let path = root.join(name);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

// The lock is component setup, not evidence of provider resolution. The author
// inputs and expected file inventory are literal and independent of the writer.
fn fixture(root: &Path) -> ResolvedProject {
    let source = DocumentCodec
        .decode_authoring(
            AUTHOR.as_bytes(),
            Some(BINDINGS.as_bytes()),
            "selection fixture",
        )
        .unwrap();
    let provider = ProviderProjectId::Modrinth(ModrinthProjectId::parse("Project1").unwrap());
    let selected = ResolvedPin {
        project: provider.clone(),
        selection: PinSelector::ModrinthVersion(ModrinthVersionId::parse("Version1").unwrap()),
    };
    let mut dependencies = BTreeMap::new();
    let mut coverage = BTreeMap::new();
    for (name, destination, bytes) in [
        (
            "renderer-alias",
            "resourcepacks/custom-name.zip",
            b"retained provider bytes".as_slice(),
        ),
        (
            "local",
            "resourcepacks/local.zip",
            b"retained local bytes".as_slice(),
        ),
    ] {
        let key = DependencyKey::parse(name).unwrap();
        let is_provider = name == "renderer-alias";
        let digests =
            DigestSet::new(vec![ExpectedDigest::Sha256(Sha256::digest(bytes).into())]).unwrap();
        dependencies.insert(
            key.clone(),
            LockedDependency {
                source_selector: source.intent().roots[&key].source.selector().cloned(),
                title: name.into(),
                kind: ContentKind::ResourcePack,
                identity: if is_provider {
                    ResolvedIdentity::Provider(provider.clone())
                } else {
                    ResolvedIdentity::Local(key.clone())
                },
                selected: is_provider.then(|| selected.clone()),
                incompatibilities: vec![],
                files: NonEmpty::new(vec![ResolvedFile {
                    slot: FileSlot::parse("primary").unwrap(),
                    acquisition: if is_provider {
                        AcquisitionSpec::Provider {
                            pin: selected.clone(),
                            slot: FileSlot::parse("original.zip").unwrap(),
                            alternatives: vec!["https://example.invalid/original.zip".into()],
                        }
                    } else {
                        AcquisitionSpec::Local(
                            PortableRelPath::parse("vendor/local.zip", PathSyntax::ProjectContent)
                                .unwrap(),
                        )
                    },
                    expected: ExpectedContent {
                        digests: Some(digests.clone()),
                        size: Some(bytes.len() as u64),
                        accepted_observation: None,
                    },
                    provenance: Provenance {
                        source: "fixed selection fixture".into(),
                        location: None,
                        declared_digests: Some(digests),
                        conversions: vec![],
                    },
                    placements: NonEmpty::new(vec![Placement {
                        destination: InstallDestination::parse(destination).unwrap(),
                        layer: ContentLayer::Common,
                        requirements: Requirements {
                            client: Requirement::Required,
                            server: Requirement::Required,
                        },
                    }])
                    .unwrap(),
                }])
                .unwrap(),
            },
        );
        coverage.insert(key, Coverage::CompleteForSelection);
        write(root, &format!("pack/{destination}"), bytes);
        if !is_provider {
            write(root, "vendor/local.zip", bytes);
        }
    }
    let lock = ResolutionLock {
        acceptable_versions: vec![],
        intent_revision: source.semantic_revision(),
        resolver: "fixed-selection-fixture".into(),
        dependencies,
        required_edges: BTreeMap::new(),
        coverage,
        runtime: RuntimeResolution {
            minecraft: GameVersion::parse("1.21.1").unwrap(),
            loader: LoaderKind::Vanilla,
            loader_version: None,
        },
    };
    let project =
        ResolvedProject::validate(source.intent().clone(), lock, source.semantic_revision())
            .unwrap();
    write(root, "empack.yml", AUTHOR.as_bytes());
    write(root, "empack.bindings.yml", BINDINGS.as_bytes());
    write(
        root,
        "empack.lock",
        &DocumentCodec.encode_lock(&project).unwrap(),
    );
    write(root, "templates/note.txt", b"author template");
    write(root, "dist/prior.zip", b"previous artifact");
    project
}

fn isolated_command(root: &Path, host: &Path) -> assert_cmd::Command {
    let mut command = offline_command(root);
    command
        .env("EMPACK_STATE_DIR", host.join("state"))
        .env("EMPACK_CACHE_DIR", host.join("cache"))
        .env("TMPDIR", host)
        .env("TMP", host)
        .env("TEMP", host);
    command
}

#[test]
fn journey_cli_pin_unpin_retains_selection_and_refuses_grouped_invalid_edits() {
    let project = TestProject::new();
    let root = project.dir();
    let host = tempfile::tempdir().unwrap();
    let command = || isolated_command(root, host.path());
    let original = fixture(root);
    let key = DependencyKey::parse("renderer-alias").unwrap();
    let original_files = files::snapshot(root);
    for action in ["pin", "unpin"] {
        for (invalid, diagnostic) in [
            ("missing", "explicit dependency"),
            ("renderer-alias", "Repeated pin key"),
            ("local", "resolved provider source"),
        ] {
            let before = files::snapshot(root);
            command()
                .args(["--yes", action, "renderer-alias", invalid])
                .assert()
                .failure()
                .stderr(predicates::str::contains(diagnostic));
            assert_eq!(
                files::snapshot(root),
                before,
                "{action} {invalid} published partial changes"
            );
        }
        let before = files::snapshot(root);
        command()
            .args(["--dry-run", action, "renderer-alias"])
            .assert()
            .success();
        assert_eq!(files::snapshot(root), before);
        command()
            .args(["--yes", action, "renderer-alias"])
            .assert()
            .success();
        let current = read_project(root);
        let expected = if action == "pin" {
            DependencySelection::ProviderPin(
                original.lock().dependencies[&key]
                    .selected
                    .as_ref()
                    .unwrap()
                    .selection
                    .clone(),
            )
        } else {
            DependencySelection::Automatic
        };
        assert_eq!(current.intent().roots[&key].selection, expected);
        assert_eq!(current.intent().roots[&key].releases, None);
        assert_eq!(
            current.intent().dependency_policy,
            ReleasePolicy::StableOnly
        );
        let mut expected_lock = original.lock().clone();
        expected_lock.intent_revision = current.lock().intent_revision;
        assert_eq!(
            current.lock(),
            &expected_lock,
            "pin/unpin changed the retained resolution"
        );
        let author = fs::read_to_string(root.join("empack.yml")).unwrap();
        let stripped = author
            .lines()
            .filter(|line| !line.trim_start().starts_with("pin:"))
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        assert_eq!(stripped, AUTHOR.replace("    releases: any\n", ""));
        let mut expected_files = original_files.clone();
        expected_files.insert("empack.yml".into(), author.into_bytes());
        expected_files.insert(
            "empack.lock".into(),
            fs::read(root.join("empack.lock")).unwrap(),
        );
        assert_eq!(files::snapshot(root), expected_files);
        let after = files::snapshot(root);
        for _ in 0..2 {
            command().args(["--yes", "sync"]).assert().success();
            assert_eq!(
                files::snapshot(root),
                after,
                "sync changed an exact retained selection"
            );
        }
    }
}

#[test]
fn journey_cli_forget_and_remove_preserve_retained_content_and_enforce_dependency_evidence() {
    for known_dependent in [false, true] {
        let project = TestProject::new();
        let root = project.dir();
        let host = tempfile::tempdir().unwrap();
        let command = || isolated_command(root, host.path());
        let original = fixture(root);
        let selected = DependencyKey::parse("renderer-alias").unwrap();
        let retained = DependencyKey::parse("local").unwrap();
        let mut lock = original.lock().clone();
        lock.coverage.insert(retained.clone(), Coverage::Partial);
        if known_dependent {
            lock.required_edges
                .insert(retained.clone(), [selected.clone()].into());
        }
        let original = ResolvedProject::validate(
            original.intent().clone(),
            lock,
            original.lock().intent_revision,
        )
        .unwrap();
        write(
            root,
            "empack.lock",
            &DocumentCodec.encode_lock(&original).unwrap(),
        );
        for (name, bytes) in [
            ("pack/saves/player/level.dat", b"unrelated world".as_slice()),
            (
                "pack/resourcepacks/unowned/note.txt",
                b"unowned directory".as_slice(),
            ),
            (
                "overrides/server/server.properties",
                b"author settings".as_slice(),
            ),
        ] {
            write(root, name, bytes);
        }
        let before = files::snapshot(root);
        for (arguments, diagnostic) in [
            (
                vec!["remove", "renderer-alias", "missing"],
                "No tracked dependency matches",
            ),
            (
                vec!["remove", "renderer-alias", "--deps"],
                "Automatic orphan cleanup requires complete dependency evidence",
            ),
            (
                vec![
                    "remove",
                    "renderer-alias",
                    "--forget",
                    "--acknowledge-unknown",
                ],
                "cannot be used with",
            ),
        ] {
            command()
                .arg("--yes")
                .args(arguments)
                .assert()
                .failure()
                .stderr(predicates::str::contains(diagnostic));
            assert_eq!(files::snapshot(root), before);
        }
        // A canonical provider selector resolves the aliased author root.
        let query = "modrinth:Project1";
        for dry in [true, false] {
            let mut cmd = command();
            cmd.args(["--yes", "remove", query]);
            if dry {
                cmd.arg("--dry-run");
            }
            cmd.assert()
                .failure()
                .stderr(predicates::str::contains(if known_dependent {
                    "Retained dependencies require selected content"
                } else {
                    "Retained dependency evidence is incomplete"
                }));
            assert_eq!(files::snapshot(root), before);
        }
        if known_dependent {
            command()
                .args(["--yes", "remove", query, "--acknowledge-unknown"])
                .assert()
                .failure()
                .stderr(predicates::str::contains(
                    "Retained dependencies require selected content",
                ));
            assert_eq!(files::snapshot(root), before);
        }
        command()
            .args(["--dry-run", "remove", query, "--forget"])
            .assert()
            .success()
            .stdout(predicates::str::contains("Select renderer-alias:"));
        assert_eq!(files::snapshot(root), before);
        command()
            .args(["--yes", "remove", query, "--forget"])
            .assert()
            .success()
            .stdout(predicates::str::contains(
                "Applied ForgetRoots to 1 dependencies",
            ));
        let demoted = read_project(root);
        let mut expected_intent = original.intent().clone();
        expected_intent.roots.remove(&selected);
        assert_eq!(demoted.intent(), &expected_intent);
        let mut expected_lock = original.lock().clone();
        expected_lock.intent_revision = demoted.lock().intent_revision;
        assert_eq!(demoted.lock(), &expected_lock);
        assert!(
            fs::read_to_string(root.join("empack.yml"))
                .unwrap()
                .starts_with("# Keep this author's selection notes.\n")
        );
        let mut expected = before;
        for name in ["empack.yml", "empack.lock", "empack.bindings.yml"] {
            expected.insert(name.into(), fs::read(root.join(name)).unwrap());
        }
        assert_eq!(
            files::snapshot(root),
            expected,
            "forget changed a non-document file"
        );
        command()
            .args(["--yes", "remove", query, "--forget"])
            .assert()
            .failure()
            .stderr(predicates::str::contains("not an explicit root"));
        assert_eq!(files::snapshot(root), expected);
        for _ in 0..2 {
            command().args(["--yes", "sync"]).assert().success();
            assert_eq!(files::snapshot(root), expected);
        }
        let args = if known_dependent {
            // Removing the dependent is a separate explicit choice, never traversal.
            vec!["--yes", "rm", query, "local"]
        } else {
            vec!["--yes", "rm", query, "--acknowledge-unknown"]
        };
        let mut preview = args.clone();
        preview.push("--dry-run");
        command().args(preview).assert().success();
        assert_eq!(files::snapshot(root), expected);
        command()
            .args(args)
            .assert()
            .success()
            .stdout(predicates::str::contains(if known_dependent {
                "Applied RemoveContent to 2 dependencies"
            } else {
                "Applied RemoveContent to 1 dependencies"
            }));
        let removed = read_project(root);
        assert!(!removed.lock().dependencies.contains_key(&selected));
        if known_dependent {
            expected_intent.roots.remove(&retained);
        }
        assert_eq!(removed.intent(), &expected_intent);
        expected.remove(Path::new("pack/resourcepacks/custom-name.zip"));
        if known_dependent {
            assert!(removed.lock().dependencies.is_empty());
            expected.remove(Path::new("pack/resourcepacks/local.zip"));
        } else {
            assert_eq!(
                removed.lock().dependencies[&retained],
                original.lock().dependencies[&retained]
            );
            assert_eq!(removed.lock().coverage[&retained], Coverage::Partial);
        }
        for name in ["empack.yml", "empack.lock", "empack.bindings.yml"] {
            expected.insert(name.into(), fs::read(root.join(name)).unwrap());
        }
        assert_eq!(
            files::snapshot(root),
            expected,
            "remove changed unselected author files"
        );
        assert!(root.join("pack/resourcepacks/unowned").is_dir());
        for _ in 0..2 {
            command().args(["--yes", "sync"]).assert().success();
            assert_eq!(files::snapshot(root), expected);
        }
    }
}

#[test]
fn journey_cli_outdated_renders_incomplete_rows_before_strict_failure_without_edits() {
    use serde_json::{Value, json};

    for manual_only in [false, true] {
        let project = TestProject::new();
        let root = project.dir();
        let host = tempfile::tempdir().unwrap();
        fixture(root);
        let command = || isolated_command(root, host.path());
        if manual_only {
            // A retained, non-root provider record must not turn this manual-root
            // assessment into network work or a claim about retained compatibility.
            command()
                .args(["--yes", "remove", "renderer-alias", "--forget"])
                .assert()
                .success();
        }
        let before = files::snapshot(root);
        for strict in [false, true] {
            for json_output in [true, false] {
                let mut cmd = command();
                cmd.args(["outdated", "--release-policy", "any"]);
                if strict {
                    cmd.arg("--strict");
                }
                if json_output {
                    cmd.arg("--json");
                }
                let output = cmd.output().unwrap();
                let fails = strict && !manual_only;
                assert_eq!(output.status.code(), Some(i32::from(fails)), "{output:?}");
                let stderr = String::from_utf8(output.stderr).unwrap();
                if fails {
                    assert!(
                        stderr.contains("Dependency assessment is incomplete"),
                        "{stderr}"
                    );
                } else {
                    assert!(stderr.is_empty(), "{stderr}");
                }
                let stdout = String::from_utf8(output.stdout).unwrap();
                if json_output {
                    let mut report: Value = serde_json::from_str(&stdout).unwrap();
                    assert!(!report["observed_at"].as_str().unwrap().is_empty());
                    report.as_object_mut().unwrap().remove("observed_at");
                    let mut rows = vec![json!({
                        "release_policy": "any", "key": "local", "name": "local",
                        "current": null, "permitted": null, "compatible": null,
                        "upstream": null, "state": "manual", "catalog_complete": false,
                        "observed_records": null, "diagnostic": null, "reasons": ["manual-source"]
                    })];
                    if !manual_only {
                        let diagnostic = report["dependencies"][1]["diagnostic"].as_str().unwrap();
                        assert!(
                            diagnostic.contains("Dependency renderer-alias (Project1)"),
                            "{diagnostic}"
                        );
                        // The ordinary offline proxy deliberately refuses transport;
                        // it supplies no invented provider records or successful catalog.
                        report["dependencies"][1]["diagnostic"] = Value::Null;
                        rows.push(json!({
                            "release_policy": "any", "key": "renderer-alias", "name": "renderer-alias",
                            "current": {"provider": "modrinth", "project": "Project1", "selection": "Version1",
                                "version": null, "published_at": null, "channel": null},
                            "permitted": null, "compatible": null, "upstream": null, "state": "unknown",
                            "catalog_complete": false, "observed_records": null, "diagnostic": null,
                            "reasons": ["provider-unavailable"]
                        }));
                    }
                    assert_eq!(
                        report,
                        json!({
                            "schema": 1, "release_policy": "any", "dependencies": rows,
                            "joint": {"scope": "provider-dependency-closure", "candidate_compatibility": "not-assessed",
                                "declared_incompatibilities": [], "declarations_complete": false,
                                "state": if manual_only {"not-applicable"} else {"not-assessed"},
                                "selections": [], "reasons": []}
                        })
                    );
                } else {
                    assert!(
                        stdout.contains(&format!("Assessment complete: {manual_only}")),
                        "{stdout}"
                    );
                    assert!(stdout.contains("local [local]: manual (policy: any)"));
                    assert!(stdout.contains(
                        "Candidate compatibility (including retained inventory): not-assessed"
                    ));
                    assert!(stdout.contains("Declared incompatibility list complete: false"));
                    if !manual_only {
                        assert!(
                            stdout
                                .contains("renderer-alias [renderer-alias]: unknown (policy: any)")
                        );
                        assert!(stdout.contains("current: modrinth:Project1@Version1"));
                        for column in ["permitted", "compatible", "upstream"] {
                            assert!(stdout.contains(&format!("{column}: unknown")));
                        }
                        assert!(stdout.contains("provider-unavailable"));
                    }
                }
                assert_eq!(
                    files::snapshot(root),
                    before,
                    "assessment edited author files"
                );
            }
        }
    }
}
