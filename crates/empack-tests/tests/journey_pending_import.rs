//! An unsupported transport creates a real saved import without contacting a provider.
//! This proves the manual-input workflow, not a live provider's restricted-file policy.
use empack_tests::{
    fixtures::write_zip,
    harness::{
        archive::zip_members,
        process::{TestProject, assert_dist_artifact_suffix},
    },
};
use serde_json::{Value, json};
use std::{fs, path::PathBuf};

#[path = "support/approval.rs"]
mod approval;
#[path = "support/files.rs"]
mod files;
#[path = "support/pending.rs"]
mod public_pending;
#[path = "support/retained.rs"]
mod retained;
use files::snapshot;

struct ImportFixture {
    project: TestProject,
    host: tempfile::TempDir,
    archive: PathBuf,
    target: PathBuf,
    state: PathBuf,
}
impl ImportFixture {
    fn new() -> Self {
        Self::with_files(Self::default_files())
    }
    fn default_files() -> Value {
        json!([{
            "path":"resourcepacks/declared.zip", "downloads":["http://example.invalid/input.bin"],
            "fileSize":7, "hashes":{"sha256":"239f59ed55e737c77147cf55ad0c1b030b6d7ee748a7426952f9b852d5a935e5"},
            "env":{"client":"required", "server":"required"}
        }])
    }
    fn with_files(files: Value) -> Self {
        Self::with_storage(files, TestProject::new(), tempfile::tempdir().unwrap())
    }
    #[cfg(target_os = "macos")]
    fn new_in(parent: &std::path::Path) -> Self {
        Self::with_storage(
            Self::default_files(),
            TestProject::new_in(parent),
            tempfile::tempdir_in(parent).unwrap(),
        )
    }
    fn with_storage(files: Value, project: TestProject, host: tempfile::TempDir) -> Self {
        let archive = project.dir().join("input.mrpack");
        let target = project.dir().join("restored");
        let state = host.path().join("state");
        let manifest = json!({
            "formatVersion":1, "game":"minecraft", "name":"pending-import", "versionId":"1.0.0",
            "dependencies":{"minecraft":"1.21.1"}, "files":files
        });
        write_zip(
            &archive,
            &[
                (
                    "modrinth.index.json",
                    &serde_json::to_vec(&manifest).unwrap(),
                ),
                ("overrides/config/settings.toml", b"enabled = true\n"),
            ],
        )
        .unwrap();
        Self {
            project,
            host,
            archive,
            target,
            state,
        }
    }
    fn command(&self) -> assert_cmd::Command {
        public_pending::command(self.project.dir(), &self.target, &self.state)
    }
    fn pending(&self) -> Value {
        public_pending::single(self.project.dir(), &self.target, &self.state, "import")
    }
    fn list(&self) -> Value {
        public_pending::report(
            self.project.dir(),
            &self.target,
            &self.state,
            &["pending", "list", "--json"],
        )
    }
    fn suspend(&self) -> String {
        let selectors = self.suspend_inputs();
        assert_eq!(selectors.len(), 1);
        selectors.into_iter().next().unwrap()
    }
    fn suspend_inputs(&self) -> Vec<String> {
        let before = snapshot(self.project.dir());
        let mut command =
            public_pending::command(self.project.dir(), self.project.dir(), &self.state);
        let output = command
            .args(["init", "--from"])
            .arg(&self.archive)
            .arg(&self.target)
            .assert()
            .failure()
            .stderr(predicates::str::contains(
                "content obligations need explicit input",
            ))
            .get_output()
            .clone();
        assert_eq!(snapshot(self.project.dir()), before);
        assert!(!self.target.exists());
        import_selectors(&output)
    }
    fn resume(&self, operation: &Value, legacy: bool, association: &str) -> assert_cmd::Command {
        let mut command = self.command();
        if legacy {
            command.args(["init", "--continue", "--import-file", association]);
        } else {
            command.args([
                "pending",
                "resume",
                operation["id"].as_str().unwrap(),
                "--file",
                association,
            ]);
        }
        command.arg("--import-local-files");
        command
    }
}

#[test]
fn journey_pending_import_replays_retained_archive_using_public_selector() {
    import_roundtrip(ImportFixture::new(), |_, _| {}, |_| {});
}

fn import_roundtrip(
    fixture: ImportFixture,
    after_suspend: impl FnOnce(&ImportFixture, &Value),
    after_import: impl FnOnce(&ImportFixture),
) {
    let selector = fixture.suspend();
    let operation = fixture.pending();
    after_suspend(&fixture, &operation);
    assert_eq!(operation["state"], "unverified");
    assert_eq!(operation["requirements_complete"], false);
    assert_eq!(operation["inputs"], json!([]));
    assert!(
        operation["diagnostic"]
            .as_str()
            .unwrap()
            .contains("original exact selectors")
    );
    let id = operation["id"].as_str().unwrap();
    let supplied = fixture.project.dir().join("renamed download.bin");
    fs::write(&supplied, b"INVALID").unwrap();
    let association = format!("{selector}={}", supplied.display());
    let original = snapshot(fixture.project.dir());
    let retained = snapshot(&fixture.state);
    for associations in [
        vec![association.as_str()],
        vec!["declared:999=missing"],
        vec![association.as_str(), association.as_str()],
    ] {
        let mut command = fixture.command();
        command.args(["pending", "resume", id]);
        for value in associations {
            command.args(["--file", value]);
        }
        command.assert().failure();
        assert_eq!(snapshot(fixture.project.dir()), original);
        assert_eq!(snapshot(&fixture.state), retained);
        assert_eq!(fixture.pending(), operation);
    }
    fs::write(&supplied, b"payload").unwrap();
    fs::remove_file(&fixture.archive).unwrap();
    // The original source is gone. Normal continuation must recover the retained
    // archive, verify the selected bytes, and apply an explicit local-source choice.
    let next: Vec<_> = operation["next"]
        .as_array()
        .unwrap()
        .iter()
        .map(|arg| arg.as_str().unwrap())
        .collect();
    fixture
        .command()
        .args(next)
        .args(["--file", &association, "--import-local-files"])
        .assert()
        .success();
    after_import(&fixture);
    assert_eq!(fixture.list()["operations"], json!([]));
    assert!(!fixture.archive.exists());
    assert_eq!(fs::read(&supplied).unwrap(), b"payload");
    let author = fs::read(fixture.target.join("empack.yml")).unwrap();
    let lock = fs::read(fixture.target.join("empack.lock")).unwrap();
    for _ in 0..2 {
        fixture.command().arg("sync").assert().success();
        assert_eq!(fs::read(fixture.target.join("empack.yml")).unwrap(), author);
        assert_eq!(fs::read(fixture.target.join("empack.lock")).unwrap(), lock);
    }
    fixture
        .command()
        .args(["build", "modrinth"])
        .assert()
        .success();
    assert_imported_archive(&fixture);
}

#[test]
fn journey_pending_import_rejects_changed_destination_and_discards_without_overwrite() {
    let fixture = ImportFixture::new();
    fixture.suspend();
    let operation = fixture.pending();
    let id = operation["id"].as_str().unwrap();
    fs::create_dir(&fixture.target).unwrap();
    fs::write(fixture.target.join("user.txt"), b"created after capture").unwrap();
    let original = snapshot(fixture.project.dir());
    let retained = snapshot(&fixture.state);
    let stale = fixture.pending();
    assert_eq!(stale["state"], "stale");
    assert_eq!(stale["id"], id);
    fixture
        .command()
        .args(["pending", "resume", id])
        .assert()
        .failure()
        .stderr(predicates::str::contains("stale"));
    fixture
        .command()
        .args(["--dry-run", "pending", "discard", id])
        .assert()
        .success();
    assert_eq!(snapshot(fixture.project.dir()), original);
    assert_eq!(snapshot(&fixture.state), retained);
    fixture
        .command()
        .args(["pending", "discard", id])
        .assert()
        .success();
    assert_eq!(fixture.list()["operations"], json!([]));
    assert_eq!(snapshot(fixture.project.dir()), original);
}

#[test]
fn journey_pending_import_corruption_is_visible_and_explicitly_discardable() {
    let fixture = ImportFixture::new();
    fixture.suspend();
    public_pending::corrupt_record(
        fixture.project.dir(),
        &fixture.target,
        &fixture.state,
        "import",
    );
}

#[test]
fn journey_pending_import_resume_requires_fresh_approval() {
    let fixture = ImportFixture::new();
    let selector = fixture.suspend();
    let operation = fixture.pending();
    let input = fixture.project.dir().join("renamed.bin");
    fs::write(&input, b"payload").unwrap();
    fs::write(
        fixture.project.dir().join("prior.zip"),
        b"keep prior artifact",
    )
    .unwrap();
    let association = format!("{selector}={}", input.display());
    fs::remove_file(&fixture.archive).unwrap();
    approval::decline_resume(
        fixture.project.dir(),
        &fixture.target,
        &fixture.state,
        &[
            "pending",
            "resume",
            operation["id"].as_str().unwrap(),
            "--file",
            &association,
            "--import-local-files",
        ],
        "(?i)apply this import plan",
    );
    assert_eq!(fixture.pending(), operation);
}

#[test]
fn journey_pending_import_rejects_corrupt_and_missing_retained_archive() {
    let fixture = ImportFixture::new();
    let archive = fs::read(&fixture.archive).unwrap();
    let selector = fixture.suspend();
    fs::remove_file(&fixture.archive).unwrap();
    fixture
        .command()
        .args(["clean", "cache"])
        .assert()
        .success();
    let original = snapshot(fixture.project.dir());
    let operation = fixture.pending();
    let backing = retained::backing(&fixture.state, &archive);
    let supplied = fixture.host.path().join("renamed download.bin");
    fs::write(&supplied, b"payload").unwrap();
    let association = format!("{selector}={}", supplied.display());
    for missing in [false, true] {
        if missing {
            fs::remove_file(&backing).unwrap();
        } else {
            let mut changed = archive.clone();
            changed[0] ^= 1;
            fs::write(&backing, changed).unwrap();
        }
        let retained = snapshot(&fixture.state);
        fixture
            .command()
            .args([
                "pending",
                "resume",
                operation["id"].as_str().unwrap(),
                "--file",
                &association,
                "--import-local-files",
            ])
            .assert()
            .failure()
            .stderr(predicates::str::contains(if missing {
                "Saved import archive is missing"
            } else {
                "Content differs from the accepted observation"
            }));
        assert_eq!(snapshot(&fixture.state), retained);
        assert_eq!(snapshot(fixture.project.dir()), original);
        assert!(!fixture.target.exists());
        assert_eq!(fixture.pending(), operation);
    }
    // Restore only the exact captured backing. Resumption still re-verifies it.
    fs::write(&backing, &archive).unwrap();
    fixture
        .command()
        .args([
            "pending",
            "resume",
            operation["id"].as_str().unwrap(),
            "--file",
            &association,
            "--import-local-files",
        ])
        .assert()
        .success();
    assert_eq!(fixture.list()["operations"], json!([]));
    fixture
        .command()
        .args(["build", "modrinth"])
        .assert()
        .success();
    assert_imported_archive(&fixture);
    for (path, bytes) in original {
        assert_eq!(fs::read(fixture.project.dir().join(path)).unwrap(), bytes);
    }
    assert_eq!(fs::read(&supplied).unwrap(), b"payload");
    assert!(!fixture.archive.exists());
}

fn assert_imported_archive(fixture: &ImportFixture) {
    let members = zip_members(&assert_dist_artifact_suffix(&fixture.target, ".mrpack"));
    assert_eq!(
        members.keys().map(String::as_str).collect::<Vec<_>>(),
        [
            "modrinth.index.json",
            "overrides/config/settings.toml",
            "overrides/resourcepacks/declared.zip",
        ]
    );
    assert_eq!(
        members["overrides/config/settings.toml"],
        b"enabled = true\n"
    );
    assert_eq!(members["overrides/resourcepacks/declared.zip"], b"payload");
    assert_eq!(
        serde_json::from_slice::<Value>(&members["modrinth.index.json"]).unwrap(),
        json!({
            "formatVersion":1,"game":"minecraft","name":"pending-import","versionId":"1.0.0",
            "files":[],"dependencies":{"minecraft":"1.21.1"}
        })
    );
}

/// Read exact selectors from the command's diagnostic, not a private record.
fn import_selectors(output: &std::process::Output) -> Vec<String> {
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(text.contains("Import continuation was saved"), "{text}");
    let selectors: Vec<String> = text
        .lines()
        .filter_map(|line| {
            let (_, displayed) = line.split_once("Import input ")?;
            let quoted = displayed.strip_suffix(": UnsupportedTransport")?;
            Some(serde_json::from_str(quoted).unwrap())
        })
        .collect();
    assert!(!selectors.is_empty(), "{text}");
    selectors
}

#[test]
fn journey_pending_import_reverifies_retained_manual_payloads_across_both_resume_commands() {
    for legacy in [false, true] {
        let fixture = ImportFixture::with_files(json!([
            {"path":"resourcepacks/first.zip","downloads":["http://example.invalid/first"],"fileSize":5,
             "hashes":{"sha512":"7fdd80dbdded156323d36c459e5fd133a4d888c227320cfb7042be9feb35d7f07201e535697af914e69d6f46b2a88655c86c2371288052ccd4fa92058b01d3fd"}},
            {"path":"resourcepacks/second.zip","downloads":["http://example.invalid/second"],"fileSize":6,
             "hashes":{"sha512":"9381e9a67aa361751cea90178c094ad6133742163cbd14f146be5c3ee6606d4e8ab4bdd839e7c672baa6eb87e06f59b2d3a68ad0533f2a13ef6c0c5d8769216a"}}
        ]));
        fs::write(
            fixture.project.dir().join("author-notes.txt"),
            b"keep author notes",
        )
        .unwrap();
        fs::write(
            fixture.project.dir().join("prior.mrpack"),
            b"keep prior artifact",
        )
        .unwrap();
        let selectors = fixture.suspend_inputs();
        assert_eq!(selectors, ["declared:0", "declared:1"]);
        let first = fixture.host.path().join("first renamed.bin");
        let second = fixture.host.path().join("second renamed.bin");
        fs::write(&first, b"first").unwrap();
        fs::write(&second, b"second").unwrap();
        let operation = fixture.pending();
        let output = fixture
            .resume(
                &operation,
                legacy,
                &format!("{}={}", selectors[0], first.display()),
            )
            .assert()
            .failure()
            .get_output()
            .clone();
        assert_eq!(import_selectors(&output), [selectors[1].clone()]);
        fs::remove_file(&first).unwrap();
        fs::remove_file(&fixture.archive).unwrap();
        fixture
            .command()
            .args(["clean", "cache"])
            .assert()
            .success();
        let original = snapshot(fixture.project.dir());
        assert!(!fixture.target.exists());
        let partial = fixture.pending();
        let backing = retained::backing(&fixture.state, b"first");
        let association = format!("{}={}", selectors[1], second.display());
        fs::write(&backing, b"WRONG").unwrap();
        let corrupted = snapshot(&fixture.state);
        fixture
            .resume(&partial, legacy, &association)
            .assert()
            .failure()
            .stderr(predicates::str::contains("sha512 digest mismatch"));
        assert_eq!(snapshot(&fixture.state), corrupted);
        assert_eq!(snapshot(fixture.project.dir()), original);
        assert_eq!(fixture.pending(), partial);
        assert!(!fixture.target.exists());

        fs::remove_file(&backing).unwrap();
        let output = fixture
            .resume(&partial, legacy, &association)
            .assert()
            .failure()
            .get_output()
            .clone();
        assert_eq!(import_selectors(&output), [selectors[0].clone()]);
        assert_eq!(snapshot(fixture.project.dir()), original);
        assert!(!fixture.target.exists());
        let missing = fixture.pending();
        assert_ne!(missing["id"], partial["id"]);
        assert_eq!(missing["requirements_complete"], false);
        let inputs = missing["inputs"].as_array().unwrap();
        assert_eq!(inputs.len(), 1, "only the second payload is retained");
        assert_eq!(inputs[0]["key"], selectors[1]);
        assert_eq!(inputs[0]["verification"], "retained; verification required");
        assert_eq!(inputs[0]["needed"], Value::Null);
        assert_eq!(
            inputs[0]["digests"],
            json!([[
                "retained-sha256",
                "16367aacb67a4a017c8da8ab95682ccb390863780f7114dda0a0e0c55644c7c4"
            ]])
        );
        fs::remove_file(&second).unwrap();
        fixture
            .command()
            .args(["clean", "cache"])
            .assert()
            .success();
        fs::write(&first, b"first").unwrap();
        fixture
            .resume(
                &missing,
                legacy,
                &format!("{}={}", selectors[0], first.display()),
            )
            .assert()
            .success();
        assert_eq!(fixture.list()["operations"], json!([]));
        fixture
            .command()
            .args(["build", "modrinth"])
            .assert()
            .success();
        let members = zip_members(&assert_dist_artifact_suffix(&fixture.target, ".mrpack"));
        assert_eq!(
            members.keys().map(String::as_str).collect::<Vec<_>>(),
            [
                "modrinth.index.json",
                "overrides/config/settings.toml",
                "overrides/resourcepacks/first.zip",
                "overrides/resourcepacks/second.zip",
            ]
        );
        assert_eq!(
            members["overrides/config/settings.toml"],
            b"enabled = true\n"
        );
        assert_eq!(members["overrides/resourcepacks/first.zip"], b"first");
        assert_eq!(members["overrides/resourcepacks/second.zip"], b"second");
        assert_eq!(
            serde_json::from_slice::<Value>(&members["modrinth.index.json"]).unwrap(),
            json!({
                "formatVersion":1,"game":"minecraft","versionId":"1.0.0","name":"pending-import",
                "files":[],"dependencies":{"minecraft":"1.21.1"}
            })
        );
        for (path, bytes) in original {
            assert_eq!(fs::read(fixture.project.dir().join(path)).unwrap(), bytes);
        }
        assert_eq!(fs::read(&first).unwrap(), b"first");
        assert!(!fixture.archive.exists());
        assert!(!second.exists());
        let completed = snapshot(&fixture.target);
        for _ in 0..2 {
            fixture.command().arg("sync").assert().success();
            assert_eq!(snapshot(&fixture.target), completed);
        }
    }
}

#[cfg(target_os = "macos")]
#[test]
#[ignore = "opt-in native APFS images; no GUI, Java or normal projects"]
fn journey_pending_import_survives_native_remount_and_reopens_created_root() {
    let volume = empack_tests::harness::native_volume::ApfsFixture::new().unwrap();
    let fixture = ImportFixture::new_in(volume.root());
    import_roundtrip(
        fixture,
        |fixture, operation| {
            let project = snapshot(fixture.project.dir());
            let host = snapshot(fixture.host.path());
            volume.remount().unwrap();
            assert_eq!(snapshot(fixture.project.dir()), project);
            assert_eq!(snapshot(fixture.host.path()), host);
            assert_eq!(
                &fixture.pending(),
                operation,
                "saved identity must survive remount"
            );
        },
        |fixture| {
            let project = snapshot(fixture.project.dir());
            let host = snapshot(fixture.host.path());
            volume.remount().unwrap();
            let status = public_pending::report(
                fixture.project.dir(),
                &fixture.target,
                &fixture.state,
                &["status", "--json"],
            );
            assert_eq!(status["selected"], "author");
            let author = status["contexts"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["context"] == "author")
                .unwrap();
            assert_eq!(author["state"], "valid");
            assert_eq!(snapshot(fixture.project.dir()), project);
            assert_eq!(snapshot(fixture.host.path()), host);
        },
    );
    volume.finish().unwrap();
}
