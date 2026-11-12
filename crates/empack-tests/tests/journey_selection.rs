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

use empack_tests::harness::workspace::snapshot;

const AUTHOR: &str = r#"# Keep this author's selection notes.
schema: 6
pack:
  id: selection.fixture
  name: &title Selection fixture
  description: *title
  version: '1'
  authors: [Fixture]
runtime: {minecraft: '1.21.1', loader: vanilla, java: {preferred: 21}}
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

#[test]
fn journey_cli_changed_author_pin_reports_sync_without_mutation() {
    let project = TestProject::new();
    let root = project.dir();
    fixture(root);
    let unchanged = snapshot(root).files;
    for pins in [
        vec!["renderer-alias=Version2", "renderer-alias=Version1"],
        vec!["local=Version2"],
        vec!["missing=Version2"],
    ] {
        let mut command = offline_command(root);
        command.args(["update", "renderer-alias", "local"]);
        for pin in pins {
            command.args(["--pin", pin]);
        }
        let result = command.output().unwrap();
        assert_eq!(
            result.status.code(),
            Some(empack_lib::application::exit::EmpackExitCode::Usage.as_i32())
        );
        assert_eq!(snapshot(root).files, unchanged);
    }

    let intent = AUTHOR.replace("    releases: any", "    pin: Version2");
    fs::write(root.join("empack.yml"), intent).unwrap();
    let before = snapshot(root).files;
    for args in [
        vec!["update", "renderer-alias"],
        vec!["update", "renderer-alias", "--json"],
    ] {
        let result = offline_command(root).args(args).output().unwrap();
        assert_eq!(
            result.status.code(),
            Some(empack_lib::application::exit::EmpackExitCode::Usage.as_i32())
        );
        let message = String::from_utf8_lossy(&result.stderr);
        assert!(
            message.contains("Authored project or bindings changed"),
            "{message}"
        );
        assert!(
            message.contains("empack sync") && message.contains("renderer-alias"),
            "{message}"
        );
        assert_eq!(snapshot(root).files, before);
    }
}

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
    use std::io::Write;
    let mut archive = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    archive
        .start_file("pack.mcmeta", zip::write::SimpleFileOptions::default())
        .unwrap();
    archive
        .write_all(br#"{"pack":{"pack_format":34,"description":"Literal local update fixture"}}"#)
        .unwrap();
    let local_pack = archive.finish().unwrap().into_inner();
    for (name, destination, bytes) in [
        (
            "renderer-alias",
            "resourcepacks/custom-name.zip",
            b"retained provider bytes".to_vec(),
        ),
        ("local", "resourcepacks/local.zip", local_pack),
    ] {
        let key = DependencyKey::parse(name).unwrap();
        let is_provider = name == "renderer-alias";
        let digests =
            DigestSet::new(vec![ExpectedDigest::Sha256(Sha256::digest(&bytes).into())]).unwrap();
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
        write(root, &format!("pack/{destination}"), &bytes);
        if !is_provider {
            write(root, "vendor/local.zip", &bytes);
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
    let original_files = snapshot(root).files;
    for action in ["pin", "unpin"] {
        for (invalid, diagnostic) in [
            ("missing", "explicit dependency"),
            ("renderer-alias", "Repeated pin key"),
            ("local", "resolved provider source"),
        ] {
            let before = snapshot(root).files;
            command()
                .args(["--yes", action, "renderer-alias", invalid])
                .assert()
                .failure()
                .stderr(predicates::str::contains(diagnostic));
            assert_eq!(
                snapshot(root).files,
                before,
                "{action} {invalid} published partial changes"
            );
        }
        let before = snapshot(root).files;
        command()
            .args(["--dry-run", action, "renderer-alias"])
            .assert()
            .success();
        assert_eq!(snapshot(root).files, before);
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
        assert_eq!(snapshot(root).files, expected_files);
        let after = snapshot(root).files;
        for _ in 0..2 {
            command().args(["--yes", "sync"]).assert().success();
            assert_eq!(
                snapshot(root).files,
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
        let before = snapshot(root).files;
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
            assert_eq!(snapshot(root).files, before);
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
            assert_eq!(snapshot(root).files, before);
        }
        if known_dependent {
            command()
                .args(["--yes", "remove", query, "--acknowledge-unknown"])
                .assert()
                .failure()
                .stderr(predicates::str::contains(
                    "Retained dependencies require selected content",
                ));
            assert_eq!(snapshot(root).files, before);
        }
        command()
            .args(["--dry-run", "remove", query, "--forget"])
            .assert()
            .success()
            .stdout(predicates::str::contains(
                "renderer-alias: renderer-alias (modrinth:Project1)",
            ));
        assert_eq!(snapshot(root).files, before);
        command()
            .args(["--yes", "remove", query, "--forget"])
            .assert()
            .success()
            .stdout(predicates::str::contains(
                "Removed 1 direct dependency from empack.yml; installed files were kept",
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
            snapshot(root).files,
            expected,
            "forget changed a non-document file"
        );
        command()
            .args(["--yes", "remove", query, "--forget"])
            .assert()
            .failure()
            .stderr(predicates::str::contains("not an explicit root"));
        assert_eq!(snapshot(root).files, expected);
        for _ in 0..2 {
            command().args(["--yes", "sync"]).assert().success();
            assert_eq!(snapshot(root).files, expected);
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
        assert_eq!(snapshot(root).files, expected);
        command()
            .args(args)
            .assert()
            .success()
            .stdout(predicates::str::contains(if known_dependent {
                "Removed 2 dependencies"
            } else {
                "Removed 1 dependency"
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
            snapshot(root).files,
            expected,
            "remove changed unselected author files"
        );
        assert!(root.join("pack/resourcepacks/unowned").is_dir());
        for _ in 0..2 {
            command().args(["--yes", "sync"]).assert().success();
            assert_eq!(snapshot(root).files, expected);
        }
    }
}

#[test]
fn journey_cli_update_reports_incomplete_provider_work_without_edits() {
    use serde_json::{Value, json};

    // Refuse the real client's CONNECT through its ordinary proxy configuration.
    // A closed port can take longer than the catalog deadline on Windows, which
    // tests deadline presentation instead of the intended transport-failure row.
    // This proxy supplies no TLS endpoint, provider response, or catalog records.
    fn refused_transport(command: &mut assert_cmd::Command) -> std::process::Output {
        use std::{
            io::{Read, Write},
            net::TcpListener,
            time::{Duration, Instant},
        };
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        for key in ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY"] {
            command.env(key, &address);
        }
        command.env("EMPACK_NET_TIMEOUT", "10");
        std::thread::scope(|scope| {
            let proxy = scope.spawn(|| {
                let deadline = Instant::now() + Duration::from_secs(5);
                let mut connection = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(Instant::now() < deadline, "catalog request did not start");
                            std::thread::sleep(Duration::from_millis(10));
                        }
                        Err(error) => panic!("{error}"),
                    }
                };
                // Accepted sockets inherit nonblocking mode on some Unix hosts.
                // Header reads must wait for the client, not race its first write.
                connection.set_nonblocking(false).unwrap();
                connection.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
                connection.set_write_timeout(Some(Duration::from_secs(2))).unwrap();
                let mut request = Vec::new();
                while !request.ends_with(b"\r\n\r\n") {
                    assert!(Instant::now() < deadline, "proxy headers exceeded deadline");
                    assert!(request.len() < 8192, "oversized proxy request");
                    let mut byte = [0];
                    connection.read_exact(&mut byte).unwrap();
                    request.push(byte[0]);
                }
                assert!(request.starts_with(b"CONNECT api.modrinth.com:443 HTTP/1.1\r\n"));
                std::thread::sleep(Duration::from_millis(250));
                connection.write_all(b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
            });
            let result = command.output().unwrap();
            proxy.join().unwrap();
            result
        })
    }

    for manual_only in [false, true] {
        let project = TestProject::new();
        let root = project.dir();
        let host = tempfile::tempdir().unwrap();
        fixture(root);
        let command = || isolated_command(root, host.path());
        if manual_only {
            // Retained provider context is checked from the lock, not independently upgraded.
            command()
                .args(["--yes", "remove", "renderer-alias", "--forget"])
                .assert()
                .success();
        }
        let before = snapshot(root).files;
        let host_before = snapshot(host.path()).files;
        for json_output in [true, false] {
            let mut cmd = command();
            cmd.args(["update", "--all", "--release-policy", "any"]);
            cmd.arg(if json_output { "--json" } else { "--details" });
            let output = if manual_only {
                cmd.output().unwrap()
            } else {
                refused_transport(&mut cmd)
            };
            assert_eq!(output.status.success(), manual_only, "{output:?}");
            let stderr = String::from_utf8(output.stderr).unwrap();
            assert_eq!(stderr.contains("Error:"), !manual_only, "{stderr}");
            if !manual_only {
                assert!(stderr.contains("Checking dependency"), "{stderr}");
            }
            let stdout = String::from_utf8(output.stdout).unwrap();
            if json_output {
                let report: Value = serde_json::from_str(&stdout).unwrap();
                assert_eq!(report["schema"], 1);
                assert_eq!(report["source_kinds"]["local"], "local");
                if manual_only {
                    assert_eq!(report["requested"], json!(["local"]));
                    assert_eq!(report["selected"], json!(["local"]));
                    assert!(
                        report["discovery"].is_null(),
                        "A local check must not invent fresh provider evidence"
                    );
                    let changes = report["file_changes"].as_array().unwrap();
                    assert!(changes.iter().all(|change| change["path"] == "empack.lock"));
                    assert_eq!(
                        report["proposal"],
                        if changes.is_empty() {
                            "unchanged"
                        } else {
                            "ready"
                        }
                    );
                } else {
                    assert_eq!(report["requested"], json!(["local", "renderer-alias"]));
                    assert_eq!(report["source_kinds"]["renderer-alias"], "provider");
                    assert_eq!(report["proposal"], "incomplete");
                    assert_eq!(report["selected"], json!([]));
                    assert_eq!(report["file_changes"], json!([]));
                    let discovery = &report["discovery"];
                    assert!(!discovery["observed_at"].as_str().unwrap().is_empty());
                    assert_eq!(discovery["release_policy"], "any");
                    assert_eq!(discovery["dependencies"].as_array().unwrap().len(), 1);
                    let row = &discovery["dependencies"][0];
                    assert_eq!(row["key"], "renderer-alias");
                    assert_eq!(row["current"]["project"], "Project1");
                    assert_eq!(row["current"]["selection"], "Version1");
                    assert_eq!(row["state"], "unknown");
                    assert_eq!(row["reasons"], json!(["provider-unavailable"]));
                    assert!(row["permitted"].is_null());
                    assert!(!row["catalog_complete"].as_bool().unwrap());
                    assert!(
                        row["diagnostic"]
                            .as_str()
                            .unwrap()
                            .contains("Dependency renderer-alias (Project1)")
                    );
                    assert_eq!(discovery["required_dependencies"], "not-assessed");
                    assert_eq!(discovery["required_selections"], json!([]));
                    assert!(discovery.get("candidate_compatibility").is_none());
                    assert!(report["diagnostic"].is_object());
                }
            } else {
                assert!(
                    stdout.contains(if manual_only {
                        "Update plan: 1 dependency"
                    } else {
                        "Proposal: incomplete; no changes applied"
                    }),
                    "{stdout}"
                );
                if !manual_only {
                    assert!(stdout.contains("renderer-alias: Unknown"), "{stdout}");
                    assert!(
                        stdout.contains("Current: modrinth:Project1@Version1"),
                        "{stdout}"
                    );
                }
                for internal in [
                    "author roots",
                    "Assessment complete:",
                    "provider-unavailable",
                ] {
                    assert!(!stdout.contains(internal), "{stdout}");
                }
            }
            assert_eq!(
                snapshot(root).files,
                before,
                "Preview changed author or installed bytes"
            );
            assert_eq!(
                snapshot(host.path()).files,
                host_before,
                "Preview changed operation state"
            );
        }
    }
}

#[test]
fn journey_cli_migration_refuses_invalid_replacements_and_stale_evidence_without_edits() {
    let project = TestProject::new();
    let root = project.dir();
    let host = tempfile::tempdir().unwrap();
    fixture(root);
    // Make policy loss observable: a redundant replacement must not unpin this root.
    isolated_command(root, host.path())
        .args(["--yes", "pin", "renderer-alias"])
        .assert()
        .success();

    let command = || isolated_command(root, host.path());
    let args = ["migrate", "--minecraft", "1.21.2", "--loader", "vanilla"];
    for (extra, diagnostic) in [
        (
            vec!["--replace", "missing=modrinth:Project2"],
            "not an explicit dependency",
        ),
        (
            vec!["--replace", "local=modrinth:Project2"],
            "only provider-owned roots",
        ),
        (
            vec!["--replace", "renderer-alias=modrinth:Project1"],
            "existing project",
        ),
        (
            vec!["--replace", "renderer-alias=Project2"],
            "provider and canonical project ID",
        ),
        (
            vec![
                "--replace",
                "renderer-alias=modrinth:Project2",
                "--replace",
                "renderer-alias=modrinth:Project3",
            ],
            "Duplicate replacement key",
        ),
        (
            vec!["--loader-version", "0.16.14"],
            "Vanilla has no loader version",
        ),
        (vec!["--json"], "--plan"),
    ] {
        for mode in ["--dry-run", "--yes"] {
            let before = snapshot(root).files;
            command()
                .args(args)
                .args(&extra)
                .arg(mode)
                .assert()
                .failure()
                .stderr(predicates::str::contains(diagnostic));
            assert_eq!(snapshot(root).files, before, "{extra:?}/{mode}");
        }
    }
    for invalid in ["missing", "stale"] {
        fixture(root);
        if invalid == "missing" {
            fs::remove_file(root.join("empack.lock")).unwrap();
        } else {
            fs::write(
                root.join("empack.yml"),
                AUTHOR.replace("version: '1'", "version: '2'"),
            )
            .unwrap();
        }
        let before = snapshot(root).files;
        for mode in ["--plan", "--dry-run", "--yes"] {
            let result = command().args(args).arg(mode).output().unwrap();
            assert!(!result.status.success());
            let message = String::from_utf8_lossy(&result.stderr);
            assert!(
                message.contains(if invalid == "missing" {
                    "current lock"
                } else {
                    "different intent revision"
                }),
                "{message}"
            );
            assert_eq!(snapshot(root).files, before, "{invalid}/{mode}");
        }
    }
}

#[test]
fn journey_cli_update_exact_filter_preserves_retained_scope_and_streams() {
    let project = TestProject::new();
    let root = project.dir();
    let initial = fixture(root);
    let before = snapshot(root).files;
    for quiet in [false, true] {
        let mut command = offline_command(root);
        command.args(["update", "local", "--json"]);
        if quiet {
            command.arg("--quiet");
        }
        let output = command.assert().success().get_output().clone();
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["schema"], 1);
        assert_eq!(report["requested"], serde_json::json!(["local"]));
        assert_eq!(report["selected"], serde_json::json!(["local"]));
        assert_eq!(report["source_kinds"], serde_json::json!({"local":"local"}));
        assert!(report["discovery"].is_null());
        let changes = report["file_changes"].as_array().unwrap();
        assert!(changes.iter().all(|change| change["path"] == "empack.lock"));
        assert_eq!(
            report["proposal"],
            if changes.is_empty() {
                "unchanged"
            } else {
                "ready"
            }
        );
        if quiet {
            assert!(output.stderr.is_empty(), "{:?}", output.stderr);
        }
        assert_eq!(snapshot(root).files, before);
    }
    let output = offline_command(root)
        .args(["update", "local", "--quiet"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(output).unwrap();
    assert!(text.contains("Update plan: 1 dependency"), "{text}");
    // This component fixture declares SHA-256 evidence. Explicitly refreshing
    // its local source accepts observed content instead; only the lock changes.
    assert!(text.contains("local: will change"), "{text}");
    assert!(text.contains("Preview only; use --apply"), "{text}");
    assert!(
        !text.contains("renderer-alias"),
        "An unselected provider must not be queried or reported as an independent update: {text}"
    );
    for (keys, code, diagnostic) in [
        (
            vec!["missing"],
            empack_lib::EmpackExitCode::NotFound.as_i32(),
            "Unknown dependency key: missing",
        ),
        (
            vec!["local", "local"],
            2,
            "Dependency listed more than once: local",
        ),
    ] {
        let output = offline_command(root)
            .arg("update")
            .args(keys)
            .assert()
            .code(code)
            .get_output()
            .clone();
        assert!(
            String::from_utf8(output.stderr)
                .unwrap()
                .contains(diagnostic)
        );
        assert!(output.stdout.is_empty());
        assert_eq!(snapshot(root).files, before);
    }
    offline_command(root)
        .args(["--yes", "update", "local", "--apply", "--quiet"])
        .assert()
        .success();
    let refreshed = read_project(root);
    let local = DependencyKey::parse("local").unwrap();
    let retained = DependencyKey::parse("renderer-alias").unwrap();
    assert_eq!(
        refreshed.lock().dependencies[&retained],
        initial.lock().dependencies[&retained]
    );
    let expected = &refreshed.lock().dependencies[&local].files.as_slice()[0].expected;
    assert!(expected.digests.is_none());
    assert!(expected.accepted_observation.is_some());
    let after = snapshot(root).files;
    for (path, bytes) in &before {
        if path != "empack.lock" {
            assert_eq!(
                after.get(path),
                Some(bytes),
                "Unexpected change to {path:?}"
            );
        }
    }
    let output = offline_command(root)
        .args(["update", "local", "--json", "--quiet"])
        .assert()
        .success()
        .get_output()
        .clone();
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["proposal"], "unchanged");
    assert_eq!(report["changed_selections"], serde_json::json!([]));
    assert_eq!(report["file_changes"], serde_json::json!([]));
    assert_eq!(snapshot(root).files, after);
}
