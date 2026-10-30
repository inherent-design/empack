//! Offline publisher/player commands with real signatures and disposable keys.
//! Local envelopes exercise the documented CLI path, not HTTPS deployment.
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use empack_tests::harness::process::{assert_dist_artifact_suffix, offline_command};
use empack_tests::harness::workspace::snapshot;
use predicates::{prelude::PredicateBooleanExt, str::contains};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{fs, path::Path, time::SystemTime};

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn unhex(value: &str) -> Vec<u8> {
    assert_eq!(value.len() % 2, 0);
    (0..value.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&value[index..index + 2], 16).unwrap())
        .collect()
}

fn verify_staged(envelope: &Path, public: &VerifyingKey, id: &str, original: &Path) {
    let value: serde_json::Value = serde_json::from_slice(&fs::read(envelope).unwrap()).unwrap();
    assert_eq!(value["schema"], 1);
    assert_eq!(value["kind"], "release");
    let payload = unhex(value["payload"].as_str().unwrap());
    assert_eq!(payload, fs::read(original).unwrap());
    assert_eq!(hex(&Sha256::digest(&payload)), id);
    let signatures = value["signatures"].as_array().unwrap();
    assert_eq!(signatures.len(), 1);
    assert_eq!(signatures[0]["algorithm"], "ed25519");
    assert_eq!(
        signatures[0]["key"],
        hex(&Sha256::digest(public.as_bytes()))
    );
    let signature =
        Signature::from_slice(&unhex(signatures[0]["signature"].as_str().unwrap())).unwrap();
    public
        .verify_strict(
            &[b"empack.release.v1\0".as_slice(), &payload].concat(),
            &signature,
        )
        .unwrap();
}

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
    write(&instance, "game/managed.txt", b"local conflict");
    offline_command(&instance)
        .args(["--yes", "instance", "update"])
        .arg(&envelope_b)
        .assert()
        .failure();
    installed(&instance, &a, b"local conflict");
    offline_command(&instance)
        .args(["--yes", "instance", "update"])
        .arg(&envelope_b)
        .args(["--replace", "managed.txt"])
        .assert()
        .success();
    installed(&instance, &b, b"2.0.0");
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
