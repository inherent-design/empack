//! Literal manual release evidence; all continuation actions use public CLI output.
//! These bytes establish installation/recovery behavior, not runnable mod content.
use empack_tests::harness::process::TestProject;
use predicates::str::contains;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs, path::PathBuf};

#[path = "support/files.rs"]
mod files;
#[path = "support/pending.rs"]
mod public_pending;
use files::snapshot;

const ONE: &[u8] = b"first";
const TWO: &[u8] = b"second";
fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
struct Fixture {
    project: TestProject,
    host: tempfile::TempDir,
    target: PathBuf,
    state: PathBuf,
    release: PathBuf,
    payload: Vec<u8>,
}
impl Fixture {
    fn new() -> Self {
        let project = TestProject::new();
        let host = tempfile::tempdir().unwrap();
        let target = project.dir().join("instance");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("player-notes.txt"), b"keep player notes").unwrap();
        let state = host.path().join("state");
        let release = project.dir().join("release.json");
        let files: Vec<_> = [("first-role", "mods/first.jar", ONE), ("second-role", "mods/second.jar", TWO)]
            .into_iter().map(|(key, destination, bytes)| json!({
                "key":key,"destination":destination,"layer":"common","policy":"managed",
                "client":{"kind":"required"},"server":{"kind":"unsupported"},
                "sha256":hash(bytes),"bytes":bytes.len(),"readonly":false,"executable":false,
                "assertions":[],"source":{"kind":"manual","instructions":"Supply the exact selected bytes","selection":null}
            })).collect();
        let payload = serde_json::to_vec(&json!({
            "schema":1,"pack":"pending.instance.fixture","version":"1","minimum_engine":">=0.8.0-beta",
            "runtime":{"minecraft":"1.21.1","loader":{"kind":"vanilla"},"java_major":21},
            "choices":[],"files":files
        })).unwrap();
        fs::write(&release, &payload).unwrap();
        Self {
            project,
            host,
            target,
            state,
            release,
            payload,
        }
    }
    fn command(&self) -> assert_cmd::Command {
        public_pending::command(self.project.dir(), &self.target, &self.state)
    }
    fn prepare(&self) -> assert_cmd::Command {
        let mut command = self.command();
        command
            .args(["instance", "prepare"])
            .arg(&self.release)
            .args([
                "--sha256",
                &hash(&self.payload),
                "--layout",
                "game",
                "--side",
                "client",
            ]);
        command
    }
    fn pending(&self) -> Value {
        public_pending::single(self.project.dir(), &self.target, &self.state, "instance")
    }
    fn list(&self) -> Value {
        public_pending::report(
            self.project.dir(),
            &self.target,
            &self.state,
            &["pending", "list", "--json"],
        )
    }
    fn suspend(&self, first: bool) {
        let before = snapshot(self.project.dir());
        let mut command = self.prepare();
        if first {
            let supplied = self.host.path().join("first renamed.bin");
            fs::write(&supplied, ONE).unwrap();
            command.args(["--file", &format!("first-role={}", supplied.display())]);
        }
        command
            .assert()
            .failure()
            .stderr(contains("exact manual inputs remain pending"));
        assert_eq!(snapshot(self.project.dir()), before);
    }
}

#[test]
fn journey_pending_instance_retains_verified_inputs_and_uses_public_resume_keys() {
    for legacy in [false, true] {
        let fixture = Fixture::new();
        let before = snapshot(fixture.project.dir());
        fixture.prepare().arg("--dry-run").assert().success();
        assert_eq!(snapshot(fixture.project.dir()), before);
        assert!(!fixture.state.exists());
        fixture.suspend(legacy);
        if !legacy {
            let initial = fixture.pending();
            let inputs = initial["inputs"].as_array().unwrap();
            assert!(inputs.iter().all(|input| input["needed"] == true));
            let key = inputs
                .iter()
                .find(|input| input["filename"] == "first.jar")
                .unwrap()["key"]
                .as_str()
                .unwrap();
            let first = fixture.host.path().join("first renamed.bin");
            fs::write(&first, ONE).unwrap();
            fixture
                .command()
                .args([
                    "pending",
                    "resume",
                    initial["id"].as_str().unwrap(),
                    "--file",
                    &format!("{key}={}", first.display()),
                ])
                .assert()
                .failure()
                .stderr(contains("exact manual inputs remain pending"));
            assert_eq!(snapshot(fixture.project.dir()), before);
            assert_ne!(fixture.pending()["id"], initial["id"]);
        }
        let operation = fixture.pending();
        assert_eq!(operation["state"], "unverified");
        assert_eq!(operation["requirements_complete"], true);
        let inputs = operation["inputs"].as_array().unwrap();
        assert_eq!(inputs.len(), 2);
        for (key, bytes, verification, needed) in [
            ("first-role", ONE, "retained; verification required", false),
            (
                "second-role",
                TWO,
                "not supplied; verification required",
                true,
            ),
        ] {
            let row = inputs.iter().find(|r| r["key"] == key).unwrap();
            assert_eq!(row["digests"], json!([["selected-sha256", hash(bytes)]]));
            assert_eq!(row["bytes"], bytes.len());
            assert_eq!(row["verification"], verification);
            assert_eq!(row["needed"], needed);
            assert_eq!(row["provider_selection"], Value::Null);
        }
        let id = operation["id"].as_str().unwrap();
        let key = inputs.iter().find(|r| r["needed"] == true).unwrap()["key"]
            .as_str()
            .unwrap();
        let supplied = fixture.host.path().join("second renamed.bin");
        fs::write(&supplied, b"WRONG!").unwrap();
        let association = format!("{key}={}", supplied.display());
        let retained = snapshot(&fixture.state);
        for (args, diagnostic) in [
            (
                vec!["pending", "resume", id, "--file", &association],
                "Content differs from the accepted observation",
            ),
            (
                vec!["pending", "resume", id, "--file", "unknown=missing"],
                "exactly one displayed input",
            ),
            (
                vec![
                    "pending",
                    "resume",
                    id,
                    "--file",
                    &association,
                    "--file",
                    &association,
                ],
                "more than once",
            ),
        ] {
            fixture
                .command()
                .args(args)
                .assert()
                .failure()
                .stderr(contains(diagnostic));
            assert_eq!(snapshot(fixture.project.dir()), before);
            assert_eq!(snapshot(&fixture.state), retained);
            assert_eq!(fixture.pending(), operation);
        }
        fs::remove_file(fixture.host.path().join("first renamed.bin")).unwrap();
        fs::remove_file(&fixture.release).unwrap();
        // Retention must survive ordinary cache eviction, not rely on a cache hit.
        fixture
            .command()
            .args(["clean", "cache"])
            .assert()
            .success();
        let cache = fixture.state.with_extension("cache");
        assert!(!cache.exists() || snapshot(&cache).values().all(|bytes| bytes != ONE));
        let protected = snapshot(&fixture.state);
        fixture
            .command()
            .args(["clean", "retained"])
            .assert()
            .success();
        assert_eq!(snapshot(&fixture.state), protected);
        assert_eq!(fixture.pending(), operation);
        fs::write(&supplied, TWO).unwrap();
        let mut command = fixture.command();
        if legacy {
            command.args(["instance", "continue"]);
        } else {
            command.args(
                operation["next"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_str().unwrap()),
            );
        }
        command
            .args(["--file", &association])
            .assert()
            .success()
            .stdout(contains(hash(&fixture.payload)));
        assert_eq!(fixture.list()["operations"], json!([]));
        assert_eq!(
            snapshot(&fixture.target.join("game")),
            BTreeMap::from([
                (
                    PathBuf::from(".empack-layout"),
                    b"empack-game-layout-v1\n".to_vec()
                ),
                (PathBuf::from("mods/first.jar"), ONE.to_vec()),
                (PathBuf::from("mods/second.jar"), TWO.to_vec()),
            ])
        );
        assert_eq!(
            fs::read(fixture.target.join("player-notes.txt")).unwrap(),
            b"keep player notes"
        );
        assert_eq!(fs::read(&supplied).unwrap(), TWO);
        fs::write(&fixture.release, &fixture.payload).unwrap();
        let completed = snapshot(fixture.project.dir());
        for _ in 0..2 {
            fixture.prepare().assert().success();
            assert_eq!(snapshot(fixture.project.dir()), completed);
        }
    }
}

#[test]
fn journey_pending_instance_rejects_changed_installation_and_discards_only_selected_work() {
    for legacy in [false, true] {
        let fixture = Fixture::new();
        fixture.suspend(false);
        let original = fixture.pending();
        // Another ordinary installation changes the base; never edit a private record.
        let mut other: Value = serde_json::from_slice(&fixture.payload).unwrap();
        other["version"] = json!("2");
        other["files"] = json!([]);
        let bytes = serde_json::to_vec(&other).unwrap();
        let descriptor = fixture.project.dir().join("other.json");
        fs::write(&descriptor, &bytes).unwrap();
        fixture
            .command()
            .args(["instance", "prepare"])
            .arg(&descriptor)
            .args([
                "--sha256",
                &hash(&bytes),
                "--layout",
                "game",
                "--side",
                "client",
            ])
            .assert()
            .success();
        let stale = fixture.pending();
        assert_eq!(stale["id"], original["id"]);
        assert_eq!(stale["state"], "stale");
        let id = stale["id"].as_str().unwrap();
        let before = snapshot(fixture.project.dir());
        let retained = snapshot(&fixture.state);
        fixture
            .command()
            .args(["pending", "resume", id])
            .assert()
            .failure()
            .stderr(contains("stale"));
        assert_eq!(snapshot(&fixture.state), retained);
        assert_eq!(snapshot(fixture.project.dir()), before);
        let args = if legacy {
            vec!["instance", "discard-pending"]
        } else {
            vec!["pending", "discard", id]
        };
        fixture
            .command()
            .arg("--dry-run")
            .args(&args)
            .assert()
            .success();
        assert_eq!(snapshot(&fixture.state), retained);
        assert_eq!(snapshot(fixture.project.dir()), before);
        fixture.command().args(&args).assert().success();
        assert_eq!(fixture.list()["operations"], json!([]));
        assert_eq!(snapshot(fixture.project.dir()), before);
        let after = snapshot(&fixture.state);
        assert_eq!(retained.len(), after.len() + 1);
        for (path, bytes) in after {
            assert_eq!(retained[&path], bytes);
        }
        fixture
            .command()
            .args(["pending", "resume", id])
            .assert()
            .failure();
        assert_eq!(snapshot(fixture.project.dir()), before);
    }
}
