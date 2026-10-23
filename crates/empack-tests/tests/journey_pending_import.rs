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

#[path = "support/files.rs"]
mod files;
#[path = "support/pending.rs"]
mod public_pending;
use files::snapshot;

struct ImportFixture {
    project: TestProject,
    _host: tempfile::TempDir,
    archive: PathBuf,
    target: PathBuf,
    state: PathBuf,
}
impl ImportFixture {
    fn new() -> Self {
        let project = TestProject::new();
        let host = tempfile::tempdir().unwrap();
        let archive = project.dir().join("input.mrpack");
        let target = project.dir().join("restored");
        let state = host.path().join("state");
        let manifest = json!({
            "formatVersion":1, "game":"minecraft", "name":"pending-import", "versionId":"1.0.0",
            "dependencies":{"minecraft":"1.21.1"},
            "files":[{"path":"resourcepacks/declared.zip", "downloads":["http://example.invalid/input.bin"],
                "fileSize":7, "hashes":{"sha256":"239f59ed55e737c77147cf55ad0c1b030b6d7ee748a7426952f9b852d5a935e5"},
                "env":{"client":"required", "server":"required"}}]
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
            _host: host,
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
        assert_eq!(selectors.len(), 1, "{text}");
        selectors.into_iter().next().unwrap()
    }
}

#[test]
fn journey_pending_import_replays_retained_archive_using_public_selector() {
    let fixture = ImportFixture::new();
    let selector = fixture.suspend();
    let operation = fixture.pending();
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
    let members = zip_members(&assert_dist_artifact_suffix(&fixture.target, ".mrpack"));
    assert_eq!(
        members.keys().map(String::as_str).collect::<Vec<_>>(),
        vec![
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
    let index: Value = serde_json::from_slice(&members["modrinth.index.json"]).unwrap();
    assert_eq!(index["name"], "pending-import");
    assert_eq!(index["versionId"], "1.0.0");
    assert_eq!(index["dependencies"], json!({"minecraft":"1.21.1"}));
    assert_eq!(index["files"], json!([]));
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
