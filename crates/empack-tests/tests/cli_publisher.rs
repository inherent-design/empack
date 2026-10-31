//! Publisher commands over literal inputs; HTTPS qualification is explicitly opt-in.
use ed25519_dalek::SigningKey;
use empack_tests::harness::{
    process::{TestProject, offline_command},
    workspace::snapshot,
};
use predicates::str::contains;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    net::TcpListener,
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[path = "support/publisher.rs"]
mod publisher_checks;
use publisher_checks::{hex, unhex, verify_signed_payload, verify_staged};

#[path = "support/approval.rs"]
mod approval;

const FIXTURE_COMMIT: &str = "dd677b62f894030bf149bef7147d14231cdb2a36";

struct Publisher {
    project: TestProject,
    inputs: tempfile::TempDir,
    manifest: Value,
}

impl Publisher {
    fn new() -> Self {
        assert_eq!(
            unhex(FIXTURE_COMMIT).len(),
            20,
            "fixture source must be an immutable Git commit"
        );
        let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/hosted-publication");
        let manifest: Value =
            serde_json::from_slice(&fs::read(fixtures.join("manifest.json")).unwrap()).unwrap();
        assert_eq!(manifest["schema"], 1);
        assert_eq!(manifest["license"], "Apache-2.0");
        let files = manifest["files"].as_object().unwrap();
        assert_eq!(files.len(), 16);
        let actual = snapshot(&fixtures)
            .files
            .into_keys()
            .filter(|path| {
                path != std::path::Path::new("README.md")
                    && path != std::path::Path::new("manifest.json")
            })
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(actual, files.keys().map(PathBuf::from).collect());
        for (relative, digest) in files {
            assert!(
                std::path::Path::new(relative)
                    .components()
                    .all(|part| matches!(part, std::path::Component::Normal(_)))
            );
            assert_eq!(
                hex(&Sha256::digest(fs::read(fixtures.join(relative)).unwrap())),
                digest.as_str().unwrap(),
                "{relative}"
            );
        }
        let fixture = Self {
            project: TestProject::new(),
            inputs: tempfile::tempdir().unwrap(),
            manifest,
        };
        fs::write(
            fixture.project.dir().join("publisher notes.txt"),
            b"preserve publisher work\n",
        )
        .unwrap();
        for label in ["a", "b"] {
            let release = fixture.release(label);
            let source = fixtures.join(release["input"].as_str().unwrap());
            let input = fixture.input(label);
            fs::create_dir_all(input.join("assets")).unwrap();
            let raw = fs::read(source.join("release.json")).unwrap();
            assert_eq!(hex(&Sha256::digest(&raw)), release["id"].as_str().unwrap());
            let document: Value = serde_json::from_slice(&raw).unwrap();
            assert_eq!(document["pack"], fixture.manifest["pack"]);
            assert_eq!(document["version"], release["version"]);
            assert_eq!(document["files"].as_array().unwrap().len(), 1);
            assert_eq!(
                document["files"][0]["sha256"],
                fixture.manifest["asset"]["sha256"]
            );
            assert_eq!(
                document["files"][0]["bytes"],
                fixture.manifest["asset"]["bytes"]
            );
            assert_eq!(
                document["files"][0]["destination"],
                fixture.manifest["asset"]["destination"]
            );
            assert_eq!(
                hex(&Sha256::digest(
                    fixture.signer(label).verifying_key().as_bytes()
                )),
                release["key_id"].as_str().unwrap()
            );
            fs::write(input.join("release.json"), raw).unwrap();
            fs::copy(
                source.join("assets/config.toml"),
                input.join("assets/config.toml"),
            )
            .unwrap();
            fs::write(
                fixture.key(label),
                format!("{}\n", release["seed_hex"].as_str().unwrap()),
            )
            .unwrap();
            let before = snapshot(fixture.inputs.path());
            offline_command(fixture.project.dir())
                .args(["--yes", "release", "stage"])
                .arg(&input)
                .arg("--key-file")
                .arg(fixture.key(label))
                .assert()
                .success()
                .stdout(contains("upload this release directory and all assets"));
            assert_eq!(snapshot(fixture.inputs.path()), before);
            let staged = fixture
                .project
                .dir()
                .join("dist/releases")
                .join(release["id"].as_str().unwrap());
            let encoded = fs::read(staged.join("release.json")).unwrap();
            assert_eq!(
                encoded,
                fs::read(
                    fixtures
                        .join("good/releases")
                        .join(release["id"].as_str().unwrap())
                        .join("release.json")
                )
                .unwrap()
            );
            assert_eq!(
                encoded.len() as u64,
                release["envelope_bytes"].as_u64().unwrap()
            );
            assert_eq!(
                hex(&Sha256::digest(&encoded)),
                release["envelope_sha256"].as_str().unwrap()
            );
            verify_staged(
                &staged.join("release.json"),
                &fixture.signer(label).verifying_key(),
                release["id"].as_str().unwrap(),
                &input.join("release.json"),
            );
            assert_eq!(
                fs::read(staged.join("assets/config.toml")).unwrap(),
                fs::read(input.join("assets/config.toml")).unwrap()
            );
        }
        fixture
    }

    fn release(&self, label: &str) -> &Value {
        self.manifest["releases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|release| release["label"] == label)
            .unwrap()
    }
    fn input(&self, label: &str) -> PathBuf {
        self.inputs.path().join(format!("export {label} Ω"))
    }
    fn key(&self, label: &str) -> PathBuf {
        self.inputs
            .path()
            .join(format!("public-fixture-{label}.key"))
    }
    fn signer(&self, label: &str) -> SigningKey {
        SigningKey::from_bytes(
            &unhex(self.release(label)["seed_hex"].as_str().unwrap())
                .try_into()
                .unwrap(),
        )
    }
    fn public(&self, label: &str) -> String {
        hex(self.signer(label).verifying_key().as_bytes())
    }
    fn base(variant: &str) -> String {
        format!(
            "https://raw.githubusercontent.com/inherent-design/empack/{FIXTURE_COMMIT}/tests/fixtures/hosted-publication/{variant}/"
        )
    }
    fn publish(&self, label: &str, base: &str, sequence: u64, expires: u64) -> assert_cmd::Command {
        let mut command = assert_cmd::Command::from_std(self.project.cmd());
        command
            .timeout(Duration::from_secs(90))
            .args([
                "release",
                "publish-channel",
                self.release(label)["id"].as_str().unwrap(),
                "--channel",
                "stable",
                "--base-url",
                base,
                "--sequence",
            ])
            .arg(sequence.to_string())
            .arg("--expires")
            .arg(expires.to_string())
            .arg("--key-file")
            .arg(self.key(label));
        command
    }
    fn pointer(&self, label: &str, sequence: u64, expires: u64) -> Vec<u8> {
        let file = self.project.dir().join("dist/channels/stable.json");
        let payload = verify_signed_payload(&file, "channel", &self.signer(label).verifying_key());
        let release = self.release(label);
        assert_eq!(
            serde_json::from_slice::<Value>(&payload).unwrap(),
            json!({
                "schema":1,"pack":"verification.hosted","channel":"stable","sequence":sequence,
                "expires":expires,"minimum_engine":">=0.8.0-beta",
                "release":{"id":release["id"],"url":format!("{}releases/{}/release.json",Self::base("good"),release["id"].as_str().unwrap()),"maximum_bytes":release["envelope_bytes"]}
            })
        );
        fs::read(file).unwrap()
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

#[test]
fn cli_pty_release_staging_requires_approval_and_preserves_preview_inputs() {
    let fixture = Publisher::new();
    let publisher = TestProject::new();
    fs::write(publisher.dir().join("publisher notes.txt"), b"retain notes").unwrap();
    fs::create_dir_all(publisher.dir().join("dist/releases/prior")).unwrap();
    fs::write(
        publisher.dir().join("dist/releases/prior/retained.txt"),
        b"retain earlier release",
    )
    .unwrap();
    let host = tempfile::tempdir().unwrap();
    let state = host.path().join("selected state");
    let input = fixture.input("a");
    let key = fixture.key("a");
    let args = [
        "release",
        "stage",
        input.to_str().unwrap(),
        "--key-file",
        key.to_str().unwrap(),
    ];
    let inputs_before = snapshot(fixture.inputs.path());
    let origin_before = snapshot(fixture.project.dir());
    let publisher_before = snapshot(publisher.dir());
    let host_before = snapshot(host.path());
    approval::preserve_without_approval(
        publisher.dir(),
        publisher.dir(),
        &state,
        &args,
        "(?i)apply this immutable release plan",
    );
    assert_eq!(snapshot(fixture.inputs.path()), inputs_before);
    assert_eq!(snapshot(fixture.project.dir()), origin_before);
    assert_eq!(snapshot(publisher.dir()), publisher_before);
    assert_eq!(snapshot(host.path()), host_before);

    // The identical selection succeeds with explicit approval, so refusal was
    // not an unrelated input, key, destination or prerequisite failure.
    let id = fixture.release("a")["id"].as_str().unwrap();
    offline_command(publisher.dir())
        .arg("--state-dir")
        .arg(&state)
        .arg("--cache-dir")
        .arg(state.with_extension("cache"))
        .arg("--yes")
        .args(args)
        .assert()
        .success()
        .stdout(contains(id));
    let staged = publisher.dir().join("dist/releases").join(id);
    verify_staged(
        &staged.join("release.json"),
        &fixture.signer("a").verifying_key(),
        id,
        &input.join("release.json"),
    );
    let output = snapshot(&staged);
    assert_eq!(output.files.len(), 2);
    assert_eq!(
        output.files[std::path::Path::new("assets/config.toml")],
        fs::read(input.join("assets/config.toml")).unwrap()
    );
    let after = snapshot(publisher.dir());
    for (path, bytes) in &publisher_before.files {
        assert_eq!(&after.files[path], bytes);
    }
    for (path, entry) in &publisher_before.entries {
        assert_eq!(&after.entries[path], entry);
    }
    assert_eq!(snapshot(fixture.inputs.path()), inputs_before);
    assert_eq!(snapshot(fixture.project.dir()), origin_before);
    assert!(!publisher.dir().join("dist/channels").exists());
}

#[test]
fn cli_hosted_channel_preview_and_refusals_preserve_publisher_work_without_network() {
    let fixture = Publisher::new();
    let before = snapshot(fixture.project.dir());
    let inputs = snapshot(fixture.inputs.path());
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let proxy = format!("http://{}", listener.local_addr().unwrap());
    let expires = now() + 3600;
    for (mode, base, sequence, expiry, diagnostic, success) in [
        (
            "preview",
            "https://publisher.invalid/",
            1,
            expires,
            "Dry run complete - no changes applied",
            true,
        ),
        (
            "unapproved",
            "https://publisher.invalid/",
            1,
            expires,
            "Noninteractive execution requires --yes",
            false,
        ),
        (
            "invalid",
            "http://publisher.invalid/",
            1,
            expires,
            "Release URLs require HTTPS",
            false,
        ),
        (
            "invalid",
            "https://publisher.invalid/",
            0,
            expires,
            "Invalid channel sequence or expiry",
            false,
        ),
        (
            "invalid",
            "https://publisher.invalid/",
            1,
            1,
            "Channel has expired",
            false,
        ),
        (
            "invalid",
            "https://publisher.invalid/",
            1,
            now() + 32 * 86400,
            "Channel expiry is beyond",
            false,
        ),
    ] {
        let mut command = fixture.publish("a", base, sequence, expiry);
        for name in ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY"] {
            command.env(name, &proxy);
        }
        command.env("NO_PROXY", "");
        if mode == "preview" {
            command.args(["--dry-run", "--yes"]);
        } else if mode == "invalid" {
            command.arg("--yes");
        }
        let result = command.assert();
        if success {
            result.success().stdout(contains(diagnostic));
        } else {
            result.failure().stderr(contains(diagnostic));
        }
        assert_eq!(
            snapshot(fixture.project.dir()),
            before,
            "{mode}: {diagnostic}"
        );
        assert_eq!(snapshot(fixture.inputs.path()), inputs);
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
    }
    assert!(!fixture.project.dir().join("dist/channels").exists());
}

#[test]
#[ignore = "requires immutable GitHub raw-content fixtures through actual HTTPS"]
fn live_cli_hosted_channel_verifies_inputs_before_local_publication() {
    let fixture = Publisher::new();
    let inputs = snapshot(fixture.inputs.path());
    let immutable = snapshot(&fixture.project.dir().join("dist/releases"));
    let expires = now() + 3600;
    fixture
        .publish("a", &Publisher::base("good"), 1, expires)
        .arg("--yes")
        .assert()
        .success()
        .stdout(contains("upload this channel file last"));
    let first = fixture.pointer("a", 1, expires);
    for (variant, previous, diagnostic) in [
        ("good", None, "PublisherAuthenticationFailed"),
        (
            "wrong-envelope",
            Some("a"),
            "Hosted release envelope differs from the staged immutable release",
        ),
        (
            "wrong-asset",
            Some("a"),
            "Source size differs from expected content",
        ),
        ("missing-asset", Some("a"), "Download was not found"),
    ] {
        let before = snapshot(fixture.project.dir());
        let mut command = fixture.publish("b", &Publisher::base(variant), 2, expires);
        command.arg("--yes");
        if let Some(label) = previous {
            command.arg("--previous-key").arg(fixture.public(label));
        }
        command.assert().failure().stderr(contains(diagnostic));
        assert_eq!(snapshot(fixture.project.dir()), before, "{variant}");
        assert_eq!(snapshot(fixture.inputs.path()), inputs);
        assert_eq!(
            fs::read(fixture.project.dir().join("dist/channels/stable.json")).unwrap(),
            first
        );
    }
    let before = snapshot(fixture.project.dir());
    fixture
        .publish("b", &Publisher::base("good"), 2, expires)
        .args(["--dry-run", "--yes", "--previous-key"])
        .arg(fixture.public("a"))
        .assert()
        .success()
        .stdout(contains("Dry run complete - no changes applied"));
    assert_eq!(snapshot(fixture.project.dir()), before);
    fixture
        .publish("b", &Publisher::base("good"), 2, expires)
        .args(["--yes", "--previous-key"])
        .arg(fixture.public("a"))
        .assert()
        .success()
        .stdout(contains("Published channel stable"));
    let second = fixture.pointer("b", 2, expires);
    assert_ne!(first, second);
    fixture
        .publish("b", &Publisher::base("good"), 2, expires)
        .arg("--yes")
        .assert()
        .success();
    assert_eq!(fixture.pointer("b", 2, expires), second);
    let before = snapshot(fixture.project.dir());
    // A previous signing key can authenticate history, but cannot regress its sequence.
    fixture
        .publish("a", &Publisher::base("good"), 1, expires)
        .args(["--yes", "--previous-key"])
        .arg(fixture.public("b"))
        .assert()
        .failure()
        .stderr(contains("Channel sequence must advance"));
    fixture
        .publish("b", &Publisher::base("good"), 2, expires + 1)
        .arg("--yes")
        .assert()
        .failure()
        .stderr(contains("Channel sequence must advance"));
    assert_eq!(snapshot(fixture.project.dir()), before);
    assert_eq!(
        snapshot(&fixture.project.dir().join("dist/releases")),
        immutable
    );
    assert_eq!(snapshot(fixture.inputs.path()), inputs);
    assert_eq!(
        fs::read(fixture.project.dir().join("publisher notes.txt")).unwrap(),
        b"preserve publisher work\n"
    );
}
