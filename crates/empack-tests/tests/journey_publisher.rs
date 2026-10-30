//! Offline publisher/player commands with real signatures and disposable keys.
//! Local envelopes exercise the documented CLI path, not HTTPS deployment.
use ed25519_dalek::{Signer, SigningKey};
use empack_tests::harness::process::{assert_dist_artifact_suffix, offline_command};
use empack_tests::harness::workspace::snapshot;
use predicates::{prelude::PredicateBooleanExt, str::contains};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{fs, path::Path, time::SystemTime};

#[path = "support/publisher.rs"]
mod publisher_checks;
use publisher_checks::{hex, unhex, verify_staged};

fn write(root: &Path, name: &str, bytes: &[u8]) {
    let path = root.join(name);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

fn private_directory(path: &Path) {
    fs::create_dir(path).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    }
    #[cfg(windows)]
    {
        // Use the same native private-directory prerequisite as a publisher.
        // Only this disposable fixture directory is changed.
        let account = std::process::Command::new("whoami").output().unwrap();
        assert!(account.status.success());
        let account = String::from_utf8(account.stdout).unwrap();
        let output = std::process::Command::new("icacls")
            .arg(path)
            .args(["/inheritance:r", "/grant:r"])
            .arg(format!("{}:(OI)(CI)F", account.trim()))
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
    }
}

fn build_and_stage(
    author: &Path,
    publisher: &Path,
    key: &Path,
    export: &Path,
    version: &str,
) -> String {
    write(
        author,
        "empack.yml",
        format!(
            r#"# Publisher-owned source note
schema: 5
pack:
  id: verification.publisher
  name: Publisher Fixture
  version: '{version}'
runtime:
  minecraft: '1.21.1'
  loader: vanilla
  java: 21
dependencies: {{}}
build:
  default: [native]
  profiles:
    native:
      target: empack
      delivery: bundled
      environment: client
      updates: empack
      format: zip
"#
        )
        .as_bytes(),
    );
    write(author, "pack/managed.txt", version.as_bytes());
    write(
        author,
        "pack/config/settings.txt",
        format!("publisher {version}").as_bytes(),
    );
    write(
        author,
        "pack/saves/fixture/level.dat",
        format!("world {version}").as_bytes(),
    );
    if version == "2.0.0" {
        write(
            author,
            "pack/saves/fixture/new-publisher-member.dat",
            b"new seed member",
        );
    }
    offline_command(author)
        .args(["--yes", "sync"])
        .assert()
        .success();
    let build = offline_command(author)
        .args(["--yes", "build"])
        .assert()
        .success();
    let archive = assert_dist_artifact_suffix(
        author,
        &format!("{version}-empack-bundled-client-subscribed.empack"),
    );
    fs::create_dir(export).unwrap();
    zip::ZipArchive::new(fs::File::open(archive).unwrap())
        .unwrap()
        .extract(export)
        .unwrap();
    let payload = fs::read(export.join("release.json")).unwrap();
    let id = hex(&Sha256::digest(&payload));
    build.stdout(contains(&id));
    let asset_name = format!("assets/{}", hex(&Sha256::digest(version.as_bytes())));
    let asset = export.join(&asset_name);
    assert_eq!(fs::read(&asset).unwrap(), version.as_bytes());
    fs::write(&asset, b"wrong").unwrap();
    offline_command(publisher)
        .args(["--yes", "release", "stage"])
        .arg(export)
        .arg("--key-file")
        .arg(key)
        .assert()
        .failure();
    assert!(!publisher.join(format!("dist/releases/{id}")).exists());
    assert_eq!(fs::read(export.join("release.json")).unwrap(), payload);
    fs::write(&asset, version.as_bytes()).unwrap();
    offline_command(publisher)
        .args(["--yes", "release", "stage"])
        .arg(export)
        .arg("--key-file")
        .arg(key)
        .assert()
        .success()
        .stdout(contains(&id));
    assert_eq!(fs::read(export.join("release.json")).unwrap(), payload);
    assert_eq!(
        fs::read(publisher.join(format!("dist/releases/{id}/{asset_name}"))).unwrap(),
        version.as_bytes()
    );
    assert!(!publisher.join("dist/channels").exists());
    id
}

fn channel(path: &Path, key: &SigningKey, id: &str, envelope: &Path, sequence: u64, expires: u64) {
    // Independent fixture encoding of the documented signed channel envelope.
    // No product planner, transport override or signature bypass supplies it.
    let payload = serde_json::to_vec(&json!({
        "schema": 1, "pack": "verification.publisher", "channel": "stable",
        "sequence": sequence, "expires": expires, "minimum_engine": ">=0.8.0-beta",
        "release": {"id": id, "url": format!("https://publisher.invalid/releases/{id}/release.json"),
                    "maximum_bytes": fs::metadata(envelope).unwrap().len()}
    })).unwrap();
    let message = [b"empack.channel.v1\0".as_slice(), &payload].concat();
    let signature = key.sign(&message);
    fs::write(path, serde_json::to_vec(&json!({
        "schema": 1, "kind": "channel", "payload": hex(&payload),
        "signatures": [{"algorithm": "ed25519", "key": hex(&Sha256::digest(key.verifying_key().as_bytes())),
                        "signature": hex(&signature.to_bytes())}]
    })).unwrap()).unwrap();
}

fn observe(instance: &Path, channel: &Path) -> assert_cmd::assert::Assert {
    offline_command(instance)
        .args(["--yes", "instance", "check-updates"])
        .arg(channel)
        .assert()
}

fn prepare_bootstrap(instance: &Path, export: &Path, digest: &str) -> assert_cmd::Command {
    let mut command = offline_command(instance);
    command
        .args(["--yes", "instance", "prepare"])
        .arg(export.join("release.json"))
        .args(["--sha256", digest, "--require-subscription"]);
    command
}

fn installed(instance: &Path, release: &str, managed: &[u8]) {
    assert!(
        !instance
            .join("game/saves/fixture/new-publisher-member.dat")
            .exists()
    );
    offline_command(instance)
        .args(["instance", "inspect"])
        .assert()
        .success()
        .stdout(contains(format!("release {release};")));
    assert_eq!(
        fs::read(instance.join("game/managed.txt")).unwrap(),
        managed
    );
    assert_eq!(
        fs::read(instance.join("game/config/settings.txt")).unwrap(),
        b"player settings"
    );
    assert_eq!(
        fs::read(instance.join("game/saves/fixture/level.dat")).unwrap(),
        b"player world"
    );
    assert_eq!(
        fs::read(instance.join("game/saves/fixture/player-added.dat")).unwrap(),
        b"new player data"
    );
}

fn runtime_defers_update(instance: &Path, envelope: &Path) {
    use empack_tests::harness::{
        process::{empack_cmd, file_sha256},
        publication::Driver,
    };
    use std::{
        process::Stdio,
        time::{Duration, Instant},
    };

    struct Runtime {
        host: std::process::Child,
        pid: Option<u32>,
        game: std::path::PathBuf,
    }
    impl Drop for Runtime {
        fn drop(&mut self) {
            let _ = fs::write(self.game.join("launch-exit"), b"exit");
            #[cfg(unix)]
            if let Some(pid) = self.pid {
                let _ = std::process::Command::new("/bin/kill")
                    .args(["-KILL", "--", &format!("-{pid}")])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
            }
            let _ = self.host.kill();
            let _ = self.host.wait();
        }
    }

    const CHILD: &str = "engine::api::launch::tests::runtime_fixture";
    let driver = Driver::native_fixture(CHILD);
    // Keep this large executable outside the deliberately small APFS images.
    let executable = tempfile::Builder::new()
        .prefix("empack runtime Ω ")
        .tempdir()
        .unwrap();
    let program = executable.path().join(if cfg!(windows) {
        "fixture runtime.exe"
    } else {
        "fixture runtime"
    });
    if fs::hard_link(driver.program(), &program).is_err() {
        fs::copy(driver.program(), &program).unwrap();
    }
    let digest = file_sha256(driver.program()).unwrap();
    assert_eq!(file_sha256(&program).unwrap(), digest);
    let arguments = [
        "--exact",
        CHILD,
        "--nocapture",
        "--test-threads=1",
        "--",
        "",
        "space and Ω",
        "literal'quote\"value",
        "$HOME",
        "%TEMP%",
        "$(touch must-not-run)",
        "`touch must-not-run`",
        "; touch must-not-run",
        "--looks-like-an-option",
        "line one\nline two",
        "trailing\\",
    ];
    let game = instance.join("game");
    write(&game, "launch-fixture", b"wait for explicit retirement");
    let stderr = tempfile::NamedTempFile::new().unwrap();
    let mut runtime = Runtime {
        host: empack_cmd(instance)
            .args(["--yes", "instance", "launch", "--"])
            .arg(&program)
            .args(arguments)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::from(stderr.reopen().unwrap()))
            .spawn()
            .unwrap(),
        pid: None,
        game: game.clone(),
    };
    let deadline = Instant::now() + Duration::from_secs(20);
    while !game.join("launch-started").exists() {
        assert!(
            runtime.host.try_wait().unwrap().is_none(),
            "{}",
            fs::read_to_string(stderr.path()).unwrap()
        );
        assert!(Instant::now() < deadline, "runtime readiness timed out");
        std::thread::sleep(Duration::from_millis(10));
    }
    let observed: serde_json::Value =
        serde_json::from_slice(&fs::read(game.join("launch-observed.json")).unwrap()).unwrap();
    runtime.pid = Some(u32::try_from(observed["pid"].as_u64().unwrap()).unwrap());
    assert!(runtime.pid.unwrap() > 1);
    assert_eq!(observed["arguments"], json!(arguments));
    assert_eq!(
        std::path::Path::new(observed["cwd"].as_str().unwrap())
            .canonicalize()
            .unwrap(),
        game.canonicalize().unwrap()
    );
    assert_eq!(
        file_sha256(Path::new(observed["program"].as_str().unwrap())).unwrap(),
        digest
    );
    assert!(!game.join("must-not-run").exists());
    let held = snapshot(instance);
    // Refused publication may retain verified acquisition. Permit only this exact
    // incoming blob; all installation, ownership and pre-existing cache bytes and
    // attributes must remain unchanged.
    let acquired = std::path::PathBuf::from(".empack-cache/content-v1")
        .join(format!("{}.blob", hex(&Sha256::digest(b"2.0.0"))));
    let assert_held = || {
        let mut current = snapshot(instance);
        if !held.files.contains_key(&acquired) && current.files.contains_key(&acquired) {
            assert_eq!(current.files.remove(&acquired).unwrap(), b"2.0.0");
            #[cfg(unix)]
            let mode = 0o600;
            #[cfg(not(unix))]
            let mode = 0;
            assert_eq!(current.entries.remove(&acquired), Some((false, mode)));
        }
        assert_eq!(current, held);
    };
    offline_command(instance)
        .args(["--yes", "instance", "update"])
        .arg(envelope)
        .assert()
        .failure()
        .stderr(contains("Project publication is busy"));
    assert_held();
    offline_command(instance)
        .args(["--yes", "instance", "repair"])
        .assert()
        .failure()
        .stderr(contains("Project publication is busy"));
    assert_held();
    offline_command(instance)
        .args(["--yes", "instance", "launch", "--"])
        .arg(&program)
        .args(["--exact", CHILD, "--nocapture"])
        .assert()
        .failure()
        .stderr(contains("Instance is already running"));
    assert_held();
    offline_command(instance)
        .args([
            "--yes",
            "instance",
            "recover-runtime",
            "--acknowledge-stopped",
        ])
        .assert()
        .failure()
        .stderr(contains("Instance is still running"));
    assert_held();

    write(&game, "launch-exit", b"normal stop");
    let deadline = Instant::now() + Duration::from_secs(20);
    let status = loop {
        if let Some(status) = runtime.host.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < deadline, "runtime did not retire");
        std::thread::sleep(Duration::from_millis(10));
    };
    assert!(
        status.success(),
        "{}",
        fs::read_to_string(stderr.path()).unwrap()
    );
    runtime.pid = None;
    drop(runtime);
    for name in [
        "launch-fixture",
        "launch-exit",
        "launch-started",
        "launch-observed.json",
    ] {
        fs::remove_file(game.join(name)).unwrap();
    }
    driver.verify();
    assert_eq!(file_sha256(&program).unwrap(), digest);
}

#[test]
fn journey_signed_local_channels_update_repair_rollback_and_retain_trust() {
    let root = tempfile::tempdir().unwrap();
    publisher_player_roundtrip(root.path(), |_| {});
}

fn publisher_player_roundtrip(root: &Path, after_install: impl FnOnce(&Path)) {
    let author = root.join("author");
    let publisher = root.join("publisher");
    let instance = root.join("instance");
    for path in [&author, &publisher, &instance] {
        fs::create_dir(path).unwrap();
    }
    let secrets = root.join("private keys");
    private_directory(&secrets);
    let key_file = secrets.join("publisher.key");
    offline_command(&publisher)
        .args(["--dry-run", "release", "init", "--key-file"])
        .arg(&key_file)
        .assert()
        .success();
    assert!(!key_file.exists());
    assert!(!publisher.join("dist").exists());
    let setup = offline_command(&publisher)
        .args(["--yes", "release", "init", "--key-file"])
        .arg(&key_file)
        .assert()
        .success();
    let secret = fs::read_to_string(&key_file).unwrap();
    let seed: [u8; 32] = (0..32)
        .map(|index| u8::from_str_radix(&secret[index * 2..index * 2 + 2], 16).unwrap())
        .collect::<Vec<_>>()
        .try_into()
        .unwrap();
    let key = SigningKey::from_bytes(&seed);
    setup
        .stdout(contains(hex(key.verifying_key().as_bytes())))
        .stdout(contains(secret.trim()).not())
        .stderr(contains(secret.trim()).not());
    offline_command(&publisher)
        .args(["--yes", "release", "init", "--key-file"])
        .arg(&key_file)
        .assert()
        .failure();
    assert_eq!(fs::read_to_string(&key_file).unwrap(), secret);
    offline_command(&author)
        .args([
            "--yes",
            "init",
            "--modloader",
            "none",
            "--mc-version",
            "1.21.1",
        ])
        .assert()
        .success();

    let export_a = root.join("export A");
    let a = build_and_stage(&author, &publisher, &key_file, &export_a, "1.0.0");
    let envelope_a = publisher.join(format!("dist/releases/{a}/release.json"));
    verify_staged(
        &envelope_a,
        &key.verifying_key(),
        &a,
        &export_a.join("release.json"),
    );
    let bootstrap = snapshot(&export_a);
    prepare_bootstrap(&instance, &export_a, &a)
        .assert()
        .failure()
        .stderr(contains("requires explicit publisher enrollment"));
    assert!(!instance.join("game").exists());
    assert_eq!(snapshot(&export_a), bootstrap);
    let channel_a = root.join("channel-a.json");
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    channel(&channel_a, &key, &a, &envelope_a, 1, now + 3600);
    offline_command(&instance)
        .args([
            "--yes",
            "instance",
            "subscribe",
            "--pack",
            "verification.publisher",
            "--channel",
            "stable",
            "https://publisher.invalid/channels/stable.json",
            "--key",
        ])
        .arg(hex(key.verifying_key().as_bytes()))
        .assert()
        .success();
    assert!(!instance.join("game").exists());
    // The generated snapshot bootstraps only after explicit enrollment. A later
    // signed update must remain active when the same startup command is reused.
    prepare_bootstrap(&instance, &export_a, &a)
        .assert()
        .success();
    assert_eq!(
        fs::read(instance.join("game/managed.txt")).unwrap(),
        b"1.0.0"
    );
    let initial_game = snapshot(&instance.join("game"));
    observe(&instance, &channel_a).success();
    assert_eq!(snapshot(&instance.join("game")), initial_game);
    offline_command(&instance)
        .args(["--yes", "instance", "update"])
        .arg(&envelope_a)
        .assert()
        .success();
    assert_eq!(
        fs::read(instance.join("game/managed.txt")).unwrap(),
        b"1.0.0"
    );
    assert_eq!(
        fs::read(instance.join("game/config/settings.txt")).unwrap(),
        b"publisher 1.0.0"
    );
    write(&instance, "game/config/settings.txt", b"player settings");
    write(&instance, "game/saves/fixture/level.dat", b"player world");
    write(
        &instance,
        "game/saves/fixture/player-added.dat",
        b"new player data",
    );

    after_install(&instance);

    let export_b = root.join("export B");
    let b = build_and_stage(&author, &publisher, &key_file, &export_b, "2.0.0");
    let envelope_b = publisher.join(format!("dist/releases/{b}/release.json"));
    verify_staged(
        &envelope_b,
        &key.verifying_key(),
        &b,
        &export_b.join("release.json"),
    );
    let channel_b = root.join("channel-b.json");
    channel(&channel_b, &key, &b, &envelope_b, 2, now + 3600);
    let wrong = root.join("wrong-key.json");
    channel(
        &wrong,
        &SigningKey::from_bytes(&[7; 32]),
        &b,
        &envelope_b,
        2,
        now + 3600,
    );
    observe(&instance, &wrong).failure();
    installed(&instance, &a, b"1.0.0");
    let expired = root.join("expired.json");
    channel(&expired, &key, &b, &envelope_b, 20, now - 1);
    observe(&instance, &expired).failure();
    installed(&instance, &a, b"1.0.0");
    // Change authenticated payload bytes without replacing the signature.
    let tampered = root.join("tampered.json");
    let mut altered: serde_json::Value =
        serde_json::from_slice(&fs::read(&channel_b).unwrap()).unwrap();
    let payload = String::from_utf8(unhex(altered["payload"].as_str().unwrap())).unwrap();
    assert!(payload.contains("stable"));
    altered["payload"] = json!(hex(payload.replace("stable", "unsafe").as_bytes()));
    fs::write(&tampered, serde_json::to_vec(&altered).unwrap()).unwrap();
    observe(&instance, &tampered).failure();
    installed(&instance, &a, b"1.0.0");
    // Refused high-sequence evidence must not poison the accepted replay floor.
    observe(&instance, &channel_b).success();
    installed(&instance, &a, b"1.0.0");
    runtime_defers_update(&instance, &envelope_b);
    installed(&instance, &a, b"1.0.0");
    write(&instance, "game/managed.txt", b"local conflict");
    let conflicted = snapshot(&instance);
    offline_command(&instance)
        .args(["--yes", "instance", "update"])
        .arg(&envelope_b)
        .assert()
        .failure()
        .stderr(contains("ModifiedManagedFile"));
    assert_eq!(snapshot(&instance), conflicted);
    installed(&instance, &a, b"local conflict");
    let signed_b = fs::read(&envelope_b).unwrap();
    let original_b = fs::read(export_b.join("release.json")).unwrap();
    offline_command(&instance)
        .args(["--yes", "instance", "update"])
        .arg(&envelope_b)
        .args(["--preserve", "managed.txt"])
        .assert()
        .success();
    installed(&instance, &b, b"local conflict");
    offline_command(&instance)
        .args(["instance", "inspect"])
        .assert()
        .success()
        .stdout(contains(format!(
            "Local override: managed.txt (accepted SHA-256 {})",
            hex(&Sha256::digest(b"local conflict"))
        )));
    // The accepted local bytes survive fresh-process repair and repeated update.
    for command in ["repair", "update"] {
        let mut invocation = offline_command(&instance);
        invocation.args(["--yes", "instance", command]);
        if command == "repair" {
            invocation.arg("--assets").arg(envelope_b.parent().unwrap());
        } else {
            invocation.arg(&envelope_b);
        }
        invocation.assert().success();
        installed(&instance, &b, b"local conflict");
    }
    write(&instance, "game/managed.txt", b"changed after acceptance");
    let changed_override = snapshot(&instance);
    offline_command(&instance)
        .args(["--yes", "instance", "repair", "--assets"])
        .arg(envelope_b.parent().unwrap())
        .assert()
        .failure()
        .stderr(contains("ModifiedManagedFile"));
    assert_eq!(snapshot(&instance), changed_override);
    let merge = root.join("merged result Ω.txt");
    fs::write(&merge, b"explicit merged result").unwrap();
    offline_command(&instance)
        .args(["--yes", "instance", "repair", "--assets"])
        .arg(envelope_b.parent().unwrap())
        .arg("--merge")
        .arg(format!("managed.txt={}", merge.display()))
        .assert()
        .success();
    installed(&instance, &b, b"explicit merged result");
    offline_command(&instance)
        .args(["instance", "inspect"])
        .assert()
        .success()
        .stdout(contains(format!(
            "Local override: managed.txt (accepted SHA-256 {})",
            hex(&Sha256::digest(b"explicit merged result"))
        )));
    fs::write(&merge, b"later source edit").unwrap();
    offline_command(&instance)
        .args(["--yes", "instance", "update"])
        .arg(&envelope_b)
        .assert()
        .success();
    installed(&instance, &b, b"explicit merged result");
    // Explicit replacement restores publisher bytes and retires local evidence.
    offline_command(&instance)
        .args([
            "--yes",
            "instance",
            "repair",
            "--replace",
            "managed.txt",
            "--assets",
        ])
        .arg(envelope_b.parent().unwrap())
        .assert()
        .success();
    installed(&instance, &b, b"2.0.0");
    offline_command(&instance)
        .args(["instance", "inspect"])
        .assert()
        .success()
        .stdout(contains("Local override:").not());
    assert_eq!(fs::read(&envelope_b).unwrap(), signed_b);
    assert_eq!(fs::read(export_b.join("release.json")).unwrap(), original_b);
    verify_staged(
        &envelope_b,
        &key.verifying_key(),
        &b,
        &export_b.join("release.json"),
    );
    let tampered_export = root.join("tampered bootstrap");
    fs::create_dir(&tampered_export).unwrap();
    let mut tampered = fs::read(export_a.join("release.json")).unwrap();
    tampered.push(b'\n'); // Still valid JSON, but no longer the selected exact bytes.
    fs::write(tampered_export.join("release.json"), &tampered).unwrap();
    let before_bad_selection = snapshot(&instance);
    for (offered, digest) in [(&export_a, "0".repeat(64)), (&tampered_export, a.clone())] {
        prepare_bootstrap(&instance, offered, &digest)
            .assert()
            .failure()
            .stderr(contains("Selected release payload identity mismatch"));
        assert_eq!(snapshot(&instance), before_bad_selection);
    }
    let active_game = snapshot(&instance.join("game"));
    for _ in 0..2 {
        prepare_bootstrap(&instance, &export_a, &a)
            .assert()
            .success();
        installed(&instance, &b, b"2.0.0");
        assert_eq!(snapshot(&instance.join("game")), active_game);
    }
    assert_eq!(snapshot(&export_a), bootstrap);
    fs::remove_file(instance.join("game/managed.txt")).unwrap();
    prepare_bootstrap(&instance, &export_a, &a)
        .assert()
        .success();
    installed(&instance, &b, b"2.0.0");
    // Explicit repair remains a separate public workflow after preparation has
    // already repaired the active release rather than replaying the bootstrap.
    fs::remove_file(instance.join("game/managed.txt")).unwrap();
    offline_command(&instance)
        .args(["--yes", "instance", "repair", "--assets"])
        .arg(envelope_b.parent().unwrap())
        .assert()
        .success();
    installed(&instance, &b, b"2.0.0");
    offline_command(&instance)
        .args(["--yes", "instance", "rollback"])
        .arg(&a)
        .arg("--assets")
        .arg(envelope_a.parent().unwrap())
        .assert()
        .success();
    installed(&instance, &a, b"1.0.0");
    observe(&instance, &channel_a).failure(); // Rollback did not reset the sequence floor.
    installed(&instance, &a, b"1.0.0");

    let replacement = SigningKey::from_bytes(&[8; 32]);
    offline_command(&instance)
        .args(["--yes", "instance", "trust", "--key"])
        .arg(hex(replacement.verifying_key().as_bytes()))
        .assert()
        .success();
    observe(&instance, &channel_b).failure();
    let stale_new_key = root.join("stale-new-key.json");
    channel(&stale_new_key, &replacement, &b, &envelope_b, 1, now + 3600);
    observe(&instance, &stale_new_key).failure();
    let channel_c = root.join("channel-c.json");
    channel(&channel_c, &replacement, &b, &envelope_b, 3, now + 3600);
    observe(&instance, &channel_c).success();
    offline_command(&instance)
        .args(["--yes", "instance", "trust", "--revoke-all"])
        .assert()
        .success();
    observe(&instance, &channel_c).failure();
    offline_command(&instance)
        .args(["--yes", "instance", "trust", "--key"])
        .arg(hex(replacement.verifying_key().as_bytes()))
        .assert()
        .success();
    channel(&stale_new_key, &replacement, &b, &envelope_b, 2, now + 3600);
    observe(&instance, &stale_new_key).failure(); // Revocation also retained the floor.
    installed(&instance, &a, b"1.0.0");
    assert_eq!(fs::read_to_string(&key_file).unwrap(), secret);
    assert!(
        fs::read_to_string(author.join("empack.yml"))
            .unwrap()
            .starts_with("# Publisher-owned source note\n")
    );
}

#[cfg(target_os = "macos")]
#[test]
#[ignore = "opt-in native APFS images; no GUI, Java or normal instances"]
fn journey_signed_channels_survive_native_remount() {
    use empack_tests::harness::native_volume::ApfsFixture;
    use std::{collections::BTreeMap, os::unix::fs::MetadataExt};
    fn files(root: &Path) -> BTreeMap<std::path::PathBuf, [u8; 32]> {
        fn visit(
            root: &Path,
            selected: &Path,
            entries: &mut BTreeMap<std::path::PathBuf, [u8; 32]>,
        ) {
            for entry in fs::read_dir(selected).unwrap() {
                let entry = entry.unwrap();
                if entry.file_type().unwrap().is_dir() {
                    visit(root, &entry.path(), entries);
                } else {
                    entries.insert(
                        entry.path().strip_prefix(root).unwrap().into(),
                        Sha256::digest(fs::read(entry.path()).unwrap()).into(),
                    );
                }
            }
        }
        let mut entries = BTreeMap::new();
        visit(root, root, &mut entries);
        entries
    }
    let fixture = ApfsFixture::new().unwrap();
    let project = fixture.root().join("fixture");
    fs::create_dir(&project).unwrap();
    publisher_player_roundtrip(&project, |instance| {
        let before = files(&project);
        let root_before = fs::metadata(instance).unwrap();
        fixture.remount().unwrap();
        let root_after = fs::metadata(instance).unwrap();
        assert_ne!(root_before.dev(), root_after.dev());
        assert_eq!(root_before.ino(), root_after.ino());
        assert_eq!(
            root_before.created().unwrap(),
            root_after.created().unwrap()
        );
        eprintln!(
            "Subscribed instance retained inode {} and birth {:?}",
            root_before.ino(),
            root_before.created().unwrap()
        );
        assert_eq!(files(&project), before, "remount changed fixture bytes");
        offline_command(instance)
            .args(["instance", "inspect"])
            .assert()
            .success();
        assert_eq!(
            files(&project),
            before,
            "read-only inspection changed retained state"
        );
    });
    fixture.finish().unwrap();
}
