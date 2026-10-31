//! Offline instance and publisher/player commands over exact local releases.
//! Signed cases use disposable keys and local envelopes, not HTTPS deployment.
use ed25519_dalek::{Signer, SigningKey};
use empack_tests::harness::process::{assert_dist_artifact_suffix, offline_command};
use empack_tests::harness::workspace::snapshot;
use predicates::{prelude::PredicateBooleanExt, str::contains};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

#[path = "support/approval.rs"]
mod approval;
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

fn staged_output(publisher: &Path) -> Option<empack_tests::harness::workspace::Snapshot> {
    // Capture the served tree; per-command harness state lives beside it.
    let dist = publisher.join("dist");
    dist.exists().then(|| snapshot(&dist))
}

#[test]
fn cli_pty_instance_conflict_decisions_require_approval_and_preserve_unrelated_files() {
    let root = tempfile::tempdir().unwrap();
    let instance = root.path().join("instance");
    let inputs = root.path().join("inputs");
    let state = root.path().join("host/state");
    fs::create_dir(&instance).unwrap();
    fs::create_dir(&inputs).unwrap();
    let files: Vec<_> = [
        ("mod", "mods/fixture.jar", b"publisher mod".as_slice()),
        ("settings", "config/locked.txt", b"publisher settings".as_slice()),
    ].into_iter().map(|(key, destination, bytes)| {
        write(&inputs, &format!("assets/{key}"), bytes);
        json!({"key":key,"destination":destination,"layer":"common","policy":"managed",
            "client":{"kind":"required"},"server":{"kind":"unsupported"},
            "sha256":hex(&Sha256::digest(bytes)),"bytes":bytes.len(),"readonly":false,"executable":false,
            "assertions":[],"source":{"kind":"asset","path":format!("assets/{key}")}})
    }).collect();
    // Literal bytes establish reconciliation and approval, not runnable game content.
    let payload = serde_json::to_vec(
        &json!({"schema":1,"pack":"verification.approval","version":"1",
        "minimum_engine":">=0.8.0-beta","require_subscription":false,
        "runtime":{"minecraft":"1.21.1","loader":{"kind":"vanilla"},"java_major":21},
        "choices":[],"files":files}),
    )
    .unwrap();
    write(&inputs, "release.json", &payload);
    write(&inputs, "merge.txt", b"explicit merged mod");
    write(&instance, "game/saves/player/level.dat", b"played world");
    write(&instance, "player notes.txt", b"retain player notes");
    let command = || {
        let mut command = offline_command(&instance);
        command
            .arg("--state-dir")
            .arg(&state)
            .arg("--cache-dir")
            .arg(state.with_extension("cache"));
        command
    };
    command()
        .args(["--yes", "instance", "install"])
        .arg(inputs.join("release.json"))
        .args([
            "--sha256",
            &hex(&Sha256::digest(&payload)),
            "--side",
            "client",
            "--layout",
            "game",
        ])
        .assert()
        .success();
    let source_before = snapshot(&inputs);
    let association = format!("mods/fixture.jar={}", inputs.join("merge.txt").display());
    write(&instance, "game/mods/fixture.jar", b"edited local mod");
    let before_diagnostic = snapshot(&instance);
    let host_before_diagnostic = snapshot(&root.path().join("host"));
    command()
        .args(["--yes", "instance", "repair"])
        .assert()
        .failure()
        .stderr(contains("game/mods/fixture.jar"))
        .stderr(contains("ModifiedManagedFile"));
    let help = command()
        .args(["instance", "repair", "--help"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let help = String::from_utf8(help).unwrap();
    let help = help.split_whitespace().collect::<Vec<_>>().join(" ");
    for guidance in [
        "release-relative path",
        "--preserve",
        "--replace",
        "--merge",
    ] {
        assert!(help.contains(guidance), "{help}");
    }
    assert_eq!(snapshot(&instance), before_diagnostic);
    assert_eq!(snapshot(&root.path().join("host")), host_before_diagnostic);
    for (option, value, expected) in [
        (
            "--preserve",
            "mods/fixture.jar",
            b"edited local mod".as_slice(),
        ),
        ("--replace", "mods/fixture.jar", b"publisher mod".as_slice()),
        (
            "--merge",
            association.as_str(),
            b"explicit merged mod".as_slice(),
        ),
    ] {
        write(&instance, "game/mods/fixture.jar", b"edited local mod");
        let before = snapshot(&instance);
        let host_before = snapshot(&root.path().join("host"));
        for decisions in [
            vec!["--replace", "unknown.txt"],
            vec![
                "--preserve",
                "mods/fixture.jar",
                "--replace",
                "mods/fixture.jar",
            ],
        ] {
            command()
                .args(["--yes", "instance", "repair"])
                .args(decisions)
                .assert()
                .failure();
            assert_eq!(snapshot(&instance), before);
            assert_eq!(snapshot(&root.path().join("host")), host_before);
        }
        approval::preserve_without_approval(
            &instance,
            &instance,
            &state,
            &["instance", "repair", option, value],
            "(?i)apply this instance maintenance plan",
        );
        assert_eq!(snapshot(&instance), before);
        assert_eq!(snapshot(&inputs), source_before);
        command()
            .args(["--yes", "instance", "repair", option, value])
            .assert()
            .success();
        let after = snapshot(&instance);
        assert_eq!(after.files[Path::new("game/mods/fixture.jar")], expected);
        assert_eq!(
            after.files.keys().collect::<Vec<_>>(),
            before.files.keys().collect::<Vec<_>>()
        );
        assert_eq!(
            after.entries.keys().collect::<Vec<_>>(),
            before.entries.keys().collect::<Vec<_>>()
        );
        for (path, bytes) in &before.files {
            if path != Path::new("game/mods/fixture.jar")
                && path != Path::new(".empack/instance.json")
            {
                assert_eq!(&after.files[path], bytes);
                assert_eq!(after.entries[path], before.entries[path]);
            }
        }
        let repaired = snapshot(&instance);
        command()
            .args(["--yes", "instance", "repair"])
            .assert()
            .success();
        assert_eq!(snapshot(&instance), repaired);
        command()
            .args(["instance", "inspect"])
            .assert()
            .success()
            .stdout(contains(format!(
                "release {};",
                hex(&Sha256::digest(&payload))
            )));
        assert_eq!(snapshot(&inputs), source_before);
    }
}

#[test]
fn cli_pty_publisher_key_setup_requires_approval_without_creating_private_state() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("publisher");
    fs::create_dir(&workspace).unwrap();
    write(&workspace, "author notes.txt", b"retain publisher work\n");
    write(&workspace, "dist/prior.zip", b"retain prior distribution");
    let private = root.path().join("private keys");
    private_directory(&private);
    let key = private.join("publisher.key");
    let before = snapshot(root.path());
    let publisher_before = snapshot(&workspace);
    approval::preserve_without_approval(
        &workspace,
        &workspace,
        &root.path().join("host state"),
        &["release", "init", "--key-file", key.to_str().unwrap()],
        "(?i)apply this publisher key setup plan",
    );
    assert_eq!(snapshot(root.path()), before);
    assert!(!key.exists());
    // The same selected path succeeds after explicit approval. This prevents an
    // unrelated setup error from making all the refusal checks look successful.
    offline_command(&workspace)
        .args(["--yes", "release", "init", "--key-file"])
        .arg(&key)
        .assert()
        .success()
        .stdout(contains("Created publisher key; public key:"));
    assert_eq!(fs::read_dir(&private).unwrap().count(), 1);
    assert_eq!(snapshot(&workspace), publisher_before);
}

#[test]
fn cli_publisher_key_setup_refuses_unsafe_or_occupied_destinations_without_changes() {
    for case in [
        "workspace",
        "missing-parent",
        "file",
        "directory",
        "reserved-name",
    ] {
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("publisher");
        fs::create_dir(&workspace).unwrap();
        write(&workspace, "author notes.txt", b"retain publisher work\n");
        write(&workspace, "dist/prior.zip", b"retain prior distribution");
        let private = root.path().join("private keys");
        private_directory(&private);
        let key = match case {
            "workspace" => workspace.join("publisher.key"),
            "missing-parent" => root.path().join("absent/publisher.key"),
            "file" => {
                let key = private.join("publisher.key");
                fs::write(&key, b"existing private material\n").unwrap();
                key
            }
            "directory" => {
                let key = private.join("publisher.key");
                fs::create_dir(&key).unwrap();
                key
            }
            "reserved-name" => private.join("CON"),
            _ => unreachable!(),
        };
        let before = snapshot(root.path());
        offline_command(&workspace)
            .args(["--yes", "release", "init", "--key-file"])
            .arg(&key)
            .assert()
            .failure()
            .stdout(contains("existing private material").not())
            .stderr(contains("existing private material").not());
        assert_eq!(snapshot(root.path()), before, "{case}");
    }
    #[cfg(unix)]
    {
        use std::os::unix::{fs::PermissionsExt, fs::symlink};
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("publisher");
        fs::create_dir(&workspace).unwrap();
        let private = root.path().join("private");
        private_directory(&private);
        let before = snapshot(root.path());
        let key = private.join("publisher.key");
        fs::set_permissions(&private, fs::Permissions::from_mode(0o755)).unwrap();
        offline_command(&workspace)
            .args(["--yes", "release", "init", "--key-file"])
            .arg(&key)
            .assert()
            .failure()
            .stderr(contains("existing private directory"));
        assert!(!key.exists());
        fs::set_permissions(&private, fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(snapshot(root.path()), before);
        let target = root.path().join("untouched.key");
        fs::write(&target, b"existing private material\n").unwrap();
        symlink(&target, &key).unwrap();
        offline_command(&workspace)
            .args(["--yes", "release", "init", "--key-file"])
            .arg(&key)
            .assert()
            .failure()
            .stderr(contains("refusing to overwrite"));
        assert_eq!(fs::read_link(&key).unwrap(), target);
        assert_eq!(fs::read(&target).unwrap(), b"existing private material\n");
        fs::remove_file(key).unwrap();
        fs::remove_file(target).unwrap();
        assert_eq!(snapshot(root.path()), before);
    }
}

#[cfg(windows)]
#[test]
fn cli_publisher_key_setup_refuses_shared_windows_directory_without_changing_acl() {
    let root = tempfile::tempdir().unwrap();
    let publisher = root.path().join("publisher");
    fs::create_dir(&publisher).unwrap();
    write(&publisher, "author notes.txt", b"retain publisher work");
    write(&publisher, "dist/prior.zip", b"retain prior output");
    let private = root.path().join("private keys");
    private_directory(&private);
    let retained = private.join("retained.key");
    fs::write(&retained, b"unrelated private fixture material").unwrap();
    let key = private.join("publisher.key");
    let tool = PathBuf::from(std::env::var_os("SystemRoot").unwrap()).join("System32/icacls.exe");
    let acl = |path: &Path, options: &[&str]| {
        assert_cmd::Command::new(&tool)
            .arg(path)
            .args(options)
            .timeout(std::time::Duration::from_secs(30))
            .assert()
            .success()
            .get_output()
            .stdout
            .clone()
    };
    let private_acl = acl(&private, &[]);
    let retained_acl = acl(&retained, &[]);
    // Numeric SIDs keep fixture setup independent of Windows display language.
    // Only this owned temporary directory and its contents receive the grant.
    acl(&private, &["/grant", "*S-1-1-0:(OI)(CI)F"]);
    let shared_acl = acl(&private, &[]);
    let shared_retained_acl = acl(&retained, &[]);
    assert_ne!(shared_acl, private_acl);
    let before = snapshot(root.path());
    offline_command(&publisher)
        .args(["--yes", "release", "init", "--key-file"])
        .arg(&key)
        .assert()
        .failure()
        .stderr(contains("existing private directory"))
        .stdout(contains("unrelated private fixture material").not())
        .stderr(contains("unrelated private fixture material").not());
    assert_eq!(snapshot(root.path()), before);
    assert!(!key.exists());
    assert_eq!(acl(&private, &[]), shared_acl);
    assert_eq!(acl(&retained, &[]), shared_retained_acl);

    // Fixture-owned restoration, not automatic permission repair by empack.
    acl(&private, &["/remove:g", "*S-1-1-0"]);
    assert_eq!(acl(&private, &[]), private_acl);
    assert_eq!(acl(&retained, &[]), retained_acl);
    let publisher_before = snapshot(&publisher);
    let created = offline_command(&publisher)
        .args(["--yes", "release", "init", "--key-file"])
        .arg(&key)
        .assert()
        .success();
    let encoded = fs::read_to_string(&key).unwrap();
    let seed: [u8; 32] = unhex(encoded.trim()).try_into().unwrap();
    let signer = SigningKey::from_bytes(&seed);
    let challenge = b"publisher key setup fixture";
    signer
        .verifying_key()
        .verify_strict(challenge, &signer.sign(challenge))
        .unwrap();
    created
        .stdout(contains(hex(signer.verifying_key().as_bytes())))
        .stdout(contains(encoded.trim()).not())
        .stderr(contains(encoded.trim()).not());
    assert_eq!(snapshot(&publisher), publisher_before);
    assert_eq!(
        fs::read(&retained).unwrap(),
        b"unrelated private fixture material"
    );
    assert_eq!(acl(&private, &[]), private_acl);
    assert_eq!(acl(&retained, &[]), retained_acl);
    assert_eq!(fs::read_dir(&private).unwrap().count(), 2);
}

#[test]
fn cli_pty_publisher_enrollment_requires_explicit_trust_and_preserves_installed_snapshot() {
    let signer = SigningKey::from_bytes(&[83; 32]);
    let public = hex(signer.verifying_key().as_bytes());
    let fingerprint = hex(&Sha256::digest(signer.verifying_key().as_bytes()));
    let url = "https://publisher.invalid/channels/stable.json";
    let args = [
        "instance",
        "subscribe",
        "--pack",
        "verification.publisher",
        "--channel",
        "stable",
        url,
        "--key",
        public.as_str(),
    ];
    for installed in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let instance = root.path().join("instance");
        let inputs = root.path().join("inputs");
        let host = root.path().join("host");
        for path in [&instance, &inputs, &host] {
            fs::create_dir(path).unwrap();
        }
        write(&instance, "player notes.txt", b"keep player work");
        let state = host.join("selected state");
        let command = || {
            let mut command = offline_command(&instance);
            command
                .arg("--state-dir")
                .arg(&state)
                .arg("--cache-dir")
                .arg(state.with_extension("cache"));
            command
        };
        // Literal supported input, not an edited private installation record.
        let payload = serde_json::to_vec(&json!({
            "schema":1, "pack":"verification.publisher", "version":"1.0.0",
            "minimum_engine":">=0.8.0-beta", "require_subscription":false,
            "runtime":{"minecraft":"1.21.1","loader":{"kind":"vanilla"},"java_major":21},
            "files":[], "choices":[]
        }))
        .unwrap();
        let release = hex(&Sha256::digest(&payload));
        write(&inputs, "release.json", &payload);
        if installed {
            command()
                .args(["--yes", "instance", "install"])
                .arg(inputs.join("release.json"))
                .args(["--sha256", &release, "--side", "client", "--layout", "game"])
                .assert()
                .success();
            write(
                &instance,
                "game/saves/player/level.dat",
                b"keep player world",
            );
        }
        let before = snapshot(&instance);
        let host_before = snapshot(&host);
        let inputs_before = snapshot(&inputs);
        let unchanged = || {
            assert_eq!(snapshot(&instance), before);
            assert_eq!(snapshot(&host), host_before);
            assert_eq!(snapshot(&inputs), inputs_before);
        };
        if !installed {
            for omitted in [2..4, 4..6, 6..7, 7..9] {
                let missing: Vec<_> = args
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| !omitted.contains(index))
                    .map(|(_, value)| *value)
                    .collect();
                command().arg("--yes").args(missing).assert().code(2);
                unchanged();
            }
            let weak = "0".repeat(64);
            let upper = "FF".repeat(32);
            for (position, value) in [
                (6, "http://publisher.invalid/channels/stable.json"),
                (8, weak.as_str()),
                (8, upper.as_str()),
            ] {
                let mut invalid = args;
                invalid[position] = value;
                command().arg("--yes").args(invalid).assert().failure();
                unchanged();
            }
        } else {
            let mut different_pack = args;
            different_pack[3] = "verification.other";
            command()
                .arg("--yes")
                .args(different_pack)
                .assert()
                .failure()
                .stderr(contains("Subscription belongs to another installed pack"));
            unchanged();
        }
        approval::preserve_without_approval(
            &instance,
            &instance,
            &state,
            &args,
            "(?i)apply this publisher trust and channel observation plan",
        );
        unchanged();
        command()
            .arg("--yes")
            .args(args)
            .assert()
            .success()
            .stdout(contains(&fingerprint))
            .stdout(contains("1 enrolled keys; sequence 0"))
            .stdout(contains(
                "Saved subscription for verification.publisher / stable",
            ));
        let after = snapshot(&instance);
        for (path, bytes) in &before.files {
            assert_eq!(&after.files[path], bytes);
        }
        for (path, entry) in &before.entries {
            assert_eq!(&after.entries[path], entry);
        }
        let added: BTreeSet<_> = after
            .files
            .keys()
            .filter(|path| !before.files.contains_key(*path))
            .cloned()
            .collect();
        assert_eq!(
            added,
            BTreeSet::from([PathBuf::from(".empack/subscription.json")])
        );
        assert_eq!(snapshot(&inputs), inputs_before);
        let stored = snapshot(&host);
        let output = command()
            .args(["status", "--context", "instance", "--json"])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let report: serde_json::Value = serde_json::from_slice(&output).unwrap();
        let context = report["contexts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|value| value["context"] == "instance")
            .unwrap();
        assert_eq!(report["content_verified"], false);
        assert_eq!(
            context["update_owner"],
            "publisher channel stable (authenticate before applying)"
        );
        if installed {
            assert_eq!(context["name"], "verification.publisher");
            assert_eq!(context["exact"], release);
            assert_eq!(context["state"], "valid");
        } else {
            assert!(context["exact"].is_null());
            assert!(!instance.join("game").exists());
        }
        command()
            .arg("--yes")
            .args(args)
            .assert()
            .failure()
            .stderr(contains("Instance already has a subscription"));
        assert_eq!(snapshot(&instance), after);
        assert_eq!(snapshot(&host), stored);
        assert_eq!(snapshot(&inputs), inputs_before);
    }
}

#[test]
fn cli_pty_publisher_trust_requires_approval_and_preserves_the_replay_floor() {
    let root = tempfile::tempdir().unwrap();
    let instance = root.path().join("instance");
    let inputs = root.path().join("inputs");
    let host = root.path().join("host");
    for path in [&instance, &inputs, &host] {
        fs::create_dir(path).unwrap();
    }
    write(&instance, "player notes.txt", b"keep player work");
    let state = host.join("selected state");
    let command = || {
        let mut command = offline_command(&instance);
        command
            .arg("--state-dir")
            .arg(&state)
            .arg("--cache-dir")
            .arg(state.with_extension("cache"));
        command
    };
    let old = SigningKey::from_bytes(&[86; 32]);
    let next = SigningKey::from_bytes(&[87; 32]);
    let old_public = hex(old.verifying_key().as_bytes());
    let next_public = hex(next.verifying_key().as_bytes());
    command()
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
            &old_public,
        ])
        .assert()
        .success();
    // Observation authenticates metadata only. No release is installed/fetched;
    // this literal supplies the advertised bound, not a private product record.
    let unfetched = inputs.join("unfetched-release-envelope.json");
    fs::write(&unfetched, b"content is never acquired by this fixture").unwrap();
    let offered = inputs.join("channel.json");
    let id = "a".repeat(64);
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let observe = || {
        command()
            .args(["--yes", "instance", "check-updates"])
            .arg(&offered)
            .assert()
    };
    channel(&offered, &old, &id, &unfetched, 2, now + 3600);
    observe().success().stdout(contains("sequence 2"));
    let both = [
        "instance",
        "trust",
        "--key",
        &old_public,
        "--key",
        &next_public,
    ];
    approval::preserve_without_approval(
        &instance,
        &instance,
        &state,
        &both,
        "(?i)apply this publisher trust and channel observation plan",
    );
    let before_invalid = snapshot(&instance);
    let host_before_invalid = snapshot(&host);
    command()
        .args([
            "--yes",
            "instance",
            "trust",
            "--key",
            &old_public,
            "--key",
            &old_public,
        ])
        .assert()
        .failure()
        .stderr(contains("Duplicate publisher key"));
    assert_eq!(snapshot(&instance), before_invalid);
    assert_eq!(snapshot(&host), host_before_invalid);
    command()
        .arg("--yes")
        .args(both)
        .assert()
        .success()
        .stdout(contains("2 enrolled keys; sequence 2"));
    // Each key independently authenticates the same payload across fresh
    // executable invocations; identical metadata is an admissible retry.
    for key in [&old, &next] {
        channel(&offered, key, &id, &unfetched, 2, now + 3600);
        observe().success().stdout(contains("sequence 2"));
    }
    channel(
        &offered,
        &SigningKey::from_bytes(&[88; 32]),
        &id,
        &unfetched,
        200,
        now + 3600,
    );
    let before_wrong_key = snapshot(&instance);
    observe()
        .failure()
        .stderr(contains("No enrolled publisher signed"));
    assert_eq!(snapshot(&instance), before_wrong_key);
    command()
        .args(["--yes", "instance", "trust", "--key", &next_public])
        .assert()
        .success()
        .stdout(contains("1 enrolled keys; sequence 2"));
    channel(&offered, &old, &id, &unfetched, 200, now + 3600);
    let after_rotation = snapshot(&instance);
    observe()
        .failure()
        .stderr(contains("No enrolled publisher signed"));
    assert_eq!(snapshot(&instance), after_rotation);
    channel(&offered, &next, &id, &unfetched, 3, now + 3600);
    observe().success().stdout(contains("sequence 3"));
    let revoke = ["instance", "trust", "--revoke-all"];
    approval::preserve_without_approval(
        &instance,
        &instance,
        &state,
        &revoke,
        "(?i)apply this publisher trust and channel observation plan",
    );
    command()
        .arg("--yes")
        .args(revoke)
        .assert()
        .success()
        .stdout(contains("0 enrolled keys; sequence 3"));
    let revoked = snapshot(&instance);
    observe()
        .failure()
        .stderr(contains("Trust requires one to sixteen enrolled keys"));
    assert_eq!(snapshot(&instance), revoked);
    command()
        .arg("--yes")
        .args(both)
        .assert()
        .success()
        .stdout(contains("2 enrolled keys; sequence 3"));
    channel(&offered, &next, &id, &unfetched, 2, now + 3600);
    let reenrolled = snapshot(&instance);
    observe()
        .failure()
        .stderr(contains("Channel sequence replay rejected"));
    assert_eq!(snapshot(&instance), reenrolled);
    channel(&offered, &next, &id, &unfetched, 3, now + 3600);
    observe().success();
    assert_eq!(
        fs::read(instance.join("player notes.txt")).unwrap(),
        b"keep player work"
    );
    assert!(!instance.join("game").exists());
    assert!(!instance.join(".empack/instance.json").exists());
    assert_eq!(
        fs::read(&unfetched).unwrap(),
        b"content is never acquired by this fixture"
    );
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
    // Literal authored bytes define the oracle, independently of the native
    // producer's inventory, its ZIP writer and the publication planner.
    let mut expected_game = BTreeMap::from([
        ("managed.txt", version.as_bytes().to_vec()),
        (
            "config/settings.txt",
            format!("publisher {version}").into_bytes(),
        ),
        (
            "saves/fixture/level.dat",
            format!("world {version}").into_bytes(),
        ),
    ]);
    if version == "2.0.0" {
        expected_game.insert(
            "saves/fixture/new-publisher-member.dat",
            b"new seed member".to_vec(),
        );
    }
    let document: serde_json::Value = serde_json::from_slice(&payload).unwrap();
    assert_eq!(document["pack"], "verification.publisher");
    assert_eq!(document["version"], version);
    assert_eq!(document["schema"], 1);
    assert_eq!(document["require_subscription"], true);
    assert_eq!(
        document["runtime"],
        json!({
            "minecraft":"1.21.1", "loader":{"kind":"vanilla"}, "java_major":21
        })
    );
    let files = document["files"].as_array().unwrap();
    assert_eq!(files.len(), expected_game.len());
    let mut expected_assets = BTreeMap::<PathBuf, Vec<u8>>::new();
    for file in files {
        let destination = file["destination"].as_str().unwrap();
        let bytes = expected_game
            .remove(destination)
            .expect("undeclared game destination");
        let digest = hex(&Sha256::digest(&bytes));
        let asset = format!("assets/{digest}");
        assert_eq!(file["sha256"], digest);
        assert_eq!(file["bytes"], bytes.len() as u64);
        assert_eq!(
            file["policy"],
            if destination == "managed.txt" {
                "managed"
            } else {
                "seed"
            }
        );
        assert_eq!(file["client"], json!({"kind":"required"}));
        assert_eq!(file["server"], json!({"kind":"unsupported"}));
        assert_eq!(file["source"], json!({"kind":"asset", "path":asset}));
        expected_assets.insert(PathBuf::from(asset), bytes);
    }
    assert!(expected_game.is_empty());
    write(
        export,
        "assets/not-selected.bin",
        b"unselected source content",
    );
    write(export, "author notes.txt", b"unselected author work");
    let author_before = snapshot(author);
    let publisher_before = staged_output(publisher);
    let asset_name = format!("assets/{}", hex(&Sha256::digest(version.as_bytes())));
    let asset = export.join(&asset_name);
    assert_eq!(fs::read(&asset).unwrap(), version.as_bytes());
    fs::write(&asset, b"wrong").unwrap();
    let bad_source = snapshot(export);
    offline_command(publisher)
        .args(["--yes", "release", "stage"])
        .arg(export)
        .arg("--key-file")
        .arg(key)
        .assert()
        .failure();
    assert!(!publisher.join(format!("dist/releases/{id}")).exists());
    assert_eq!(snapshot(export), bad_source);
    assert_eq!(staged_output(publisher), publisher_before);
    assert_eq!(snapshot(author), author_before);
    assert_eq!(fs::read(export.join("release.json")).unwrap(), payload);
    fs::write(&asset, version.as_bytes()).unwrap();
    let selected_source = snapshot(export);
    offline_command(publisher)
        .args(["--yes", "release", "stage"])
        .arg(export)
        .arg("--key-file")
        .arg(key)
        .assert()
        .success()
        .stdout(contains(&id));
    assert_eq!(fs::read(export.join("release.json")).unwrap(), payload);
    assert_eq!(snapshot(export), selected_source);
    assert_eq!(snapshot(author), author_before);
    let staged = snapshot(&publisher.join(format!("dist/releases/{id}")));
    assert_eq!(staged.files.len(), expected_assets.len() + 1);
    assert!(staged.files.contains_key(Path::new("release.json")));
    let mut expected_entries = BTreeSet::from([
        PathBuf::new(),
        PathBuf::from("assets"),
        PathBuf::from("release.json"),
    ]);
    expected_entries.extend(expected_assets.keys().cloned());
    assert_eq!(
        staged.entries.keys().cloned().collect::<BTreeSet<_>>(),
        expected_entries
    );
    for (path, bytes) in &expected_assets {
        assert_eq!(&staged.files[path], bytes);
    }
    // A second release may add its own files, never rewrite earlier immutable
    // output or neighboring publisher work.
    let publisher_after = staged_output(publisher).unwrap();
    if let Some(before) = publisher_before {
        for (path, bytes) in &before.files {
            assert_eq!(&publisher_after.files[path], bytes);
        }
        for (path, entry) in &before.entries {
            assert_eq!(&publisher_after.entries[path], entry);
        }
    }
    assert_eq!(
        fs::read(publisher.join(format!("dist/releases/{id}/{asset_name}"))).unwrap(),
        version.as_bytes()
    );
    assert!(!publisher.join("dist/channels").exists());
    id
}

#[test]
fn cli_release_stage_replays_exact_inventory_and_refuses_immutable_or_signer_conflicts() {
    let root = tempfile::tempdir().unwrap();
    let author = root.path().join("author");
    let publisher = root.path().join("publisher");
    let private = root.path().join("private keys");
    fs::create_dir(&author).unwrap();
    fs::create_dir(&publisher).unwrap();
    private_directory(&private);
    write(&publisher, "publisher notes.txt", b"retain publisher work");
    write(
        &publisher,
        "dist/releases/prior/retained.txt",
        b"retain older output",
    );
    // These public fixture seeds are disposable and never used outside tests.
    let signing = SigningKey::from_bytes(&[81; 32]);
    let key = private.join("publisher.key");
    fs::write(&key, format!("{}\n", hex(signing.as_bytes()))).unwrap();
    let alternate = private.join("alternate.key");
    fs::write(&alternate, format!("{}\n", hex(&[82; 32]))).unwrap();
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
    let export = root.path().join("export");
    let id = build_and_stage(&author, &publisher, &key, &export, "1.0.0");
    let output = publisher.join(format!("dist/releases/{id}"));
    let envelope = output.join("release.json");
    verify_staged(
        &envelope,
        &signing.verifying_key(),
        &id,
        &export.join("release.json"),
    );
    let original = staged_output(&publisher);
    let source = snapshot(&export);
    let authored = snapshot(&author);
    let secrets = snapshot(&private);
    offline_command(&publisher)
        .args(["--yes", "release", "stage"])
        .arg(&export)
        .arg("--key-file")
        .arg(&key)
        .assert()
        .success()
        .stdout(contains(&id));
    assert_eq!(staged_output(&publisher), original);
    let asset = output.join(format!("assets/{}", hex(&Sha256::digest(b"1.0.0"))));
    for path in [&asset, &envelope] {
        let retained = fs::read(path).unwrap();
        fs::write(path, b"publisher-owned conflicting bytes").unwrap();
        let changed = staged_output(&publisher);
        offline_command(&publisher)
            .args(["--yes", "release", "stage"])
            .arg(&export)
            .arg("--key-file")
            .arg(&key)
            .assert()
            .failure()
            .stderr(contains("Immutable release output already differs"));
        assert_eq!(staged_output(&publisher), changed);
        assert_eq!(
            fs::read(publisher.join("publisher notes.txt")).unwrap(),
            b"retain publisher work"
        );
        assert_eq!(snapshot(&export), source);
        assert_eq!(snapshot(&author), authored);
        assert_eq!(snapshot(&private), secrets);
        fs::write(path, retained).unwrap();
    }
    offline_command(&publisher)
        .args(["--yes", "release", "stage"])
        .arg(&export)
        .arg("--key-file")
        .arg(&alternate)
        .assert()
        .failure()
        .stderr(contains("Immutable release output already differs"));
    assert_eq!(staged_output(&publisher), original);
    assert_eq!(
        fs::read(publisher.join("publisher notes.txt")).unwrap(),
        b"retain publisher work"
    );
    assert_eq!(snapshot(&export), source);
    assert_eq!(snapshot(&author), authored);
    assert_eq!(snapshot(&private), secrets);
    assert!(!publisher.join("dist/channels").exists());
    verify_staged(
        &envelope,
        &signing.verifying_key(),
        &id,
        &export.join("release.json"),
    );
}

fn channel(path: &Path, key: &SigningKey, id: &str, envelope: &Path, sequence: u64, expires: u64) {
    channel_with_identity(
        path,
        key,
        id,
        envelope,
        sequence,
        expires,
        ("verification.publisher", "stable"),
    );
}

fn channel_with_identity(
    path: &Path,
    key: &SigningKey,
    id: &str,
    envelope: &Path,
    sequence: u64,
    expires: u64,
    (pack, name): (&str, &str),
) {
    // Independent fixture encoding of the documented signed channel envelope.
    // No product planner, transport override or signature bypass supplies it.
    let payload = serde_json::to_vec(&json!({
        "schema": 1, "pack": pack, "channel": name,
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
    write(
        instance,
        "dist/held-artifact.zip",
        b"keep until runtime retirement",
    );
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
    for target in ["builds", "all"] {
        let before_cleanup = snapshot(instance);
        offline_command(instance)
            .args(["--yes", "clean", target])
            .assert()
            .failure()
            .stderr(contains("Project publication is busy"));
        assert_eq!(snapshot(instance), before_cleanup);
        assert_held();
    }
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
    let player_files = snapshot(&game);
    offline_command(instance)
        .args(["--yes", "clean", "builds"])
        .assert()
        .success();
    assert!(!instance.join("dist/held-artifact.zip").exists());
    assert_eq!(snapshot(&game), player_files);
    fs::remove_dir(instance.join("dist")).unwrap();
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
    let before_rejected_observation = snapshot(&instance);
    for identity in [
        ("another.pack", "stable"),
        ("verification.publisher", "preview"),
    ] {
        let mismatched = root.join("wrong-subscription.json");
        channel_with_identity(
            &mismatched,
            &key,
            &b,
            &envelope_b,
            200,
            now + 3600,
            identity,
        );
        // Verify the independent fixture really has an enrolled signature; the
        // rejection must establish subscription identity, not a bad signature.
        let envelope: serde_json::Value =
            serde_json::from_slice(&fs::read(&mismatched).unwrap()).unwrap();
        let payload = unhex(envelope["payload"].as_str().unwrap());
        let signature = ed25519_dalek::Signature::from_slice(&unhex(
            envelope["signatures"][0]["signature"].as_str().unwrap(),
        ))
        .unwrap();
        key.verifying_key()
            .verify_strict(
                &[b"empack.channel.v1\0".as_slice(), &payload].concat(),
                &signature,
            )
            .unwrap();
        observe(&instance, &mismatched)
            .failure()
            .stderr(contains("Channel subscription identity mismatch"));
        assert_eq!(snapshot(&instance), before_rejected_observation);
    }
    let wrong = root.join("wrong-key.json");
    channel(
        &wrong,
        &SigningKey::from_bytes(&[7; 32]),
        &b,
        &envelope_b,
        2,
        now + 3600,
    );
    observe(&instance, &wrong)
        .failure()
        .stderr(contains("No enrolled publisher signed"));
    assert_eq!(snapshot(&instance), before_rejected_observation);
    installed(&instance, &a, b"1.0.0");
    let expired = root.join("expired.json");
    channel(&expired, &key, &b, &envelope_b, 20, now - 1);
    observe(&instance, &expired)
        .failure()
        .stderr(contains("Channel has expired"));
    assert_eq!(snapshot(&instance), before_rejected_observation);
    let too_distant = root.join("distant-expiry.json");
    channel(
        &too_distant,
        &key,
        &b,
        &envelope_b,
        200,
        now + 32 * 24 * 60 * 60,
    );
    observe(&instance, &too_distant).failure().stderr(contains(
        "Channel expiry is beyond the supported freshness window",
    ));
    assert_eq!(snapshot(&instance), before_rejected_observation);
    installed(&instance, &a, b"1.0.0");
    // Change authenticated payload bytes without replacing the signature.
    let tampered = root.join("tampered.json");
    let mut altered: serde_json::Value =
        serde_json::from_slice(&fs::read(&channel_b).unwrap()).unwrap();
    let payload = String::from_utf8(unhex(altered["payload"].as_str().unwrap())).unwrap();
    assert!(payload.contains("stable"));
    altered["payload"] = json!(hex(payload.replace("stable", "unsafe").as_bytes()));
    fs::write(&tampered, serde_json::to_vec(&altered).unwrap()).unwrap();
    observe(&instance, &tampered)
        .failure()
        .stderr(contains("Publisher signature verification failed"));
    assert_eq!(snapshot(&instance), before_rejected_observation);
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
    let before_rollback = snapshot(&instance);
    let unknown_release = "0".repeat(64);
    assert_ne!(unknown_release, a);
    assert_ne!(unknown_release, b);
    offline_command(&instance)
        .args(["--yes", "instance", "rollback", &unknown_release])
        .assert()
        .failure()
        .stderr(contains("Release is not retained by this instance"));
    assert_eq!(snapshot(&instance), before_rollback);
    offline_command(&instance)
        .args(["--yes", "instance", "rollback", &a])
        .args(["--choice", "unknown=enabled"])
        .assert()
        .failure()
        .stderr(contains("Unknown requested choice"));
    assert_eq!(snapshot(&instance), before_rollback);
    write(&instance, "game/managed.txt", b"local edit before rollback");
    let conflicted = snapshot(&instance);
    offline_command(&instance)
        .args(["--yes", "instance", "rollback", &a, "--assets"])
        .arg(envelope_a.parent().unwrap())
        .assert()
        .failure()
        .stderr(contains("ModifiedManagedFile"));
    assert_eq!(snapshot(&instance), conflicted);
    offline_command(&instance)
        .args(["--yes", "instance", "rollback"])
        .arg(&a)
        .arg("--assets")
        .arg(envelope_a.parent().unwrap())
        .args(["--replace", "managed.txt"])
        .assert()
        .success();
    installed(&instance, &a, b"1.0.0");
    let rolled_back = snapshot(&instance);
    observe(&instance, &channel_a)
        .failure()
        .stderr(contains("Channel sequence replay rejected"));
    assert_eq!(snapshot(&instance), rolled_back); // Rollback retained the floor.
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
    let after_rotation = snapshot(&instance);
    offline_command(&instance)
        .args(["--yes", "instance", "update"])
        .arg(&envelope_b)
        .assert()
        .failure()
        .stderr(contains("No enrolled publisher signed"));
    assert_eq!(snapshot(&instance), after_rotation);
    // Supply the same selected release payload with the newly enrolled key.
    // The old immutable envelope remains untouched; ordinary relative assets
    // still reside beside this separate local input.
    let rotated_release = envelope_b
        .parent()
        .unwrap()
        .join("replacement-signature.json");
    let mut resigned: serde_json::Value = serde_json::from_slice(&signed_b).unwrap();
    let payload = unhex(resigned["payload"].as_str().unwrap());
    let signature = replacement.sign(&[b"empack.release.v1\0".as_slice(), &payload].concat());
    resigned["signatures"] = json!([{
        "algorithm":"ed25519", "key":hex(&Sha256::digest(replacement.verifying_key().as_bytes())),
        "signature":hex(&signature.to_bytes())
    }]);
    fs::write(&rotated_release, serde_json::to_vec(&resigned).unwrap()).unwrap();
    verify_staged(
        &rotated_release,
        &replacement.verifying_key(),
        &b,
        &export_b.join("release.json"),
    );
    offline_command(&instance)
        .args(["--yes", "instance", "update"])
        .arg(&rotated_release)
        .assert()
        .success();
    installed(&instance, &b, b"2.0.0");
    assert_eq!(fs::read(&envelope_b).unwrap(), signed_b);
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
    installed(&instance, &b, b"2.0.0");
    assert_eq!(fs::read_to_string(&key_file).unwrap(), secret);
    assert!(
        fs::read_to_string(author.join("empack.yml"))
            .unwrap()
            .starts_with("# Publisher-owned source note\n")
    );
}

fn stage_runtime_fixture(
    root: &Path,
    publisher: &Path,
    key: &Path,
    version: &str,
    fabric: bool,
) -> (std::path::PathBuf, String, std::path::PathBuf) {
    let export = root.join(format!("runtime export {version}"));
    fs::create_dir(&export).unwrap();
    let runtime = if fabric {
        json!({"minecraft":"1.21.2","loader":{"kind":"fabric","version":"0.16.0"},"java_major":21})
    } else {
        json!({"minecraft":"1.21.1","loader":{"kind":"vanilla"},"java_major":21})
    };
    let files: Vec<_> = [
        ("mod", if fabric { "mods/b.jar" } else { "mods/a.jar" }, "managed", version.as_bytes()),
        ("config", "config/player.txt", "seed", b"publisher defaults".as_slice()),
    ].into_iter().map(|(key, destination, policy, bytes)| {
        write(&export, &format!("assets/{key}"), bytes);
        json!({"key":key,"destination":destination,"layer":"common","policy":policy,
            "client":{"kind":"required"},"server":{"kind":"unsupported"},
            "sha256":hex(&Sha256::digest(bytes)),"bytes":bytes.len(),"readonly":false,"executable":false,
            "assertions":[],"source":{"kind":"asset","path":format!("assets/{key}")}})
    }).collect();
    // Literal native input; no game/mod bytes or provider/build planner creates the oracle.
    let payload = serde_json::to_vec(&json!({"schema":1,"pack":"verification.publisher","version":version,
        "minimum_engine":">=0.8.0-beta","require_subscription":true,"runtime":runtime,"files":files,"choices":[]})).unwrap();
    let (id, envelope) = stage_literal_release(&export, publisher, key, &payload);
    (export, id, envelope)
}

fn stage_literal_release(
    export: &Path,
    publisher: &Path,
    key: &Path,
    payload: &[u8],
) -> (String, PathBuf) {
    write(export, "release.json", payload);
    let id = hex(&Sha256::digest(payload));
    offline_command(publisher)
        .args(["--yes", "release", "stage"])
        .arg(export)
        .arg("--key-file")
        .arg(key)
        .assert()
        .success()
        .stdout(contains(&id));
    let envelope = publisher.join(format!("dist/releases/{id}/release.json"));
    (id, envelope)
}

#[test]
fn journey_signed_choices_and_restricted_input_compose_across_three_releases() {
    let root = tempfile::tempdir().unwrap();
    let publisher = root.path().join("publisher");
    let instance = root.path().join("instance");
    let inputs = root.path().join("inputs");
    let keys = root.path().join("private keys");
    for path in [&publisher, &instance, &inputs] {
        fs::create_dir(path).unwrap();
    }
    private_directory(&keys);
    let key = SigningKey::from_bytes(&[91; 32]);
    let key_file = keys.join("fixture.key");
    fs::write(&key_file, format!("{}\n", hex(&[91; 32]))).unwrap();
    let host = tempfile::tempdir().unwrap();
    let state = host.path().join("selected state");
    let command = || {
        let mut command = offline_command(&instance);
        command
            .arg("--state-dir")
            .arg(&state)
            .arg("--cache-dir")
            .arg(state.with_extension("cache"));
        command
    };
    let mut releases = Vec::new();
    for version in ["A", "B", "C"] {
        let export = root.path().join(format!("literal export {version}"));
        fs::create_dir(&export).unwrap();
        let rich = format!("rich {version}");
        let mut files = Vec::new();
        for (name, destination, policy, bytes, participation) in [
            (
                "core",
                "mods/core.jar",
                "managed",
                version.as_bytes(),
                json!({"kind":"required"}),
            ),
            (
                "rich",
                "mods/rich.jar",
                "managed",
                rich.as_bytes(),
                json!({"kind":"choice","key":"visuals","value":"rich"}),
            ),
            (
                "config",
                "config/player.txt",
                "seed",
                b"publisher defaults".as_slice(),
                json!({"kind":"required"}),
            ),
        ] {
            write(&export, &format!("assets/{name}"), bytes);
            files.push(json!({"key":name,"destination":destination,"layer":"common","policy":policy,
                "client":participation,"server":{"kind":"unsupported"},"sha256":hex(&Sha256::digest(bytes)),
                "bytes":bytes.len(),"readonly":false,"executable":false,"assertions":[],
                "source":{"kind":"asset","path":format!("assets/{name}")}}));
        }
        if version == "B" {
            files.push(json!({"key":"private-addon","destination":"mods/private.jar","layer":"common","policy":"managed",
                "client":{"kind":"required"},"server":{"kind":"unsupported"},"sha256":hex(&Sha256::digest(b"private B")),
                "bytes":9,"readonly":false,"executable":false,"assertions":[],
                "source":{"kind":"manual","instructions":"Supply the exact private add-on","selection":null}}));
        }
        let runtime = if version == "C" {
            json!({"minecraft":"1.21.2","loader":{"kind":"fabric","version":"0.16.0"},"java_major":21})
        } else {
            json!({"minecraft":"1.21.1","loader":{"kind":"vanilla"},"java_major":21})
        };
        let mut choices = vec![
            json!({"key":"visuals","alternatives":["lite","balanced","rich"],
            "default":if version == "A" { "lite" } else { "balanced" },"description":"Optional visual content"}),
        ];
        if version == "C" {
            choices.push(
                json!({"key":"textures","alternatives":["none","extra"],"default":"none",
                "description":"New publisher choice requires an explicit decision"}),
            );
        }
        let payload = serde_json::to_vec(&json!({"schema":1,"pack":"verification.publisher","version":version,
            "minimum_engine":">=0.8.0-beta","require_subscription":true,"runtime":runtime,"files":files,
            "choices":choices})).unwrap();
        let (id, envelope) = stage_literal_release(&export, &publisher, &key_file, &payload);
        verify_staged(
            &envelope,
            &key.verifying_key(),
            &id,
            &export.join("release.json"),
        );
        assert_eq!(
            snapshot(&envelope.parent().unwrap().join("assets")).files,
            BTreeMap::from([
                (PathBuf::from("core"), version.as_bytes().to_vec()),
                (PathBuf::from("rich"), rich.into_bytes()),
                (PathBuf::from("config"), b"publisher defaults".to_vec()),
            ]),
            "restricted bytes must not enter the publisher assets"
        );
        releases.push((id, envelope));
    }
    // The fixture's publisher, keys, exports and external manual inputs are
    // independently retained; only the instance and channel input may change.
    let publisher_before = snapshot(&publisher);
    let keys_before = snapshot(&keys);
    let exports_before: Vec<_> = ["A", "B", "C"]
        .into_iter()
        .map(|version| {
            let export = root.path().join(format!("literal export {version}"));
            let before = snapshot(&export);
            (export, before)
        })
        .collect();
    command()
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
    let expires = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + 3600;
    let offered = inputs.join("channel.json");
    let observe = |index: usize| {
        let (id, envelope) = &releases[index];
        channel(&offered, &key, id, envelope, index as u64 + 1, expires);
        command()
            .args(["--yes", "instance", "check-updates"])
            .arg(&offered)
            .assert()
            .success();
    };
    observe(0);
    command()
        .args(["--yes", "instance", "update"])
        .arg(&releases[0].1)
        .args([
            "--side",
            "client",
            "--layout",
            "game",
            "--choice",
            "visuals=rich",
        ])
        .assert()
        .success();
    write(&instance, "game/config/player.txt", b"player settings");
    write(&instance, "game/saves/played/level.dat", b"player world");
    write(&instance, "player notes.txt", b"player notes");
    let expected = |version: &str, index: usize, private: bool| {
        let mut files = BTreeMap::from([
            (
                PathBuf::from(".empack-layout"),
                b"empack-game-layout-v1\n".to_vec(),
            ),
            (PathBuf::from("mods/core.jar"), version.as_bytes().to_vec()),
            (
                PathBuf::from("mods/rich.jar"),
                format!("rich {version}").into_bytes(),
            ),
            (
                PathBuf::from("config/player.txt"),
                b"player settings".to_vec(),
            ),
            (
                PathBuf::from("saves/played/level.dat"),
                b"player world".to_vec(),
            ),
        ]);
        if private {
            files.insert(PathBuf::from("mods/private.jar"), b"private B".to_vec());
        }
        assert_eq!(snapshot(&instance.join("game")).files, files);
        assert_eq!(
            fs::read(instance.join("player notes.txt")).unwrap(),
            b"player notes"
        );
        let inspection = command()
            .args(["instance", "inspect"])
            .assert()
            .success()
            .stdout(contains(format!("release {};", releases[index].0)))
            .stdout(contains("visuals=rich"));
        if version == "C" {
            inspection.stdout(contains("textures=none"));
        } else {
            inspection.stdout(contains("textures=").not());
        }
    };
    expected("A", 0, false);
    observe(1);
    let before_b = snapshot(&instance);
    command()
        .args(["--yes", "instance", "update"])
        .arg(&releases[1].1)
        .args(["--choice", "visuals=unknown"])
        .assert()
        .failure();
    assert_eq!(snapshot(&instance), before_b);
    command()
        .args(["--yes", "instance", "update"])
        .arg(&releases[1].1)
        .assert()
        .failure()
        .stderr(contains("exact manual inputs remain pending"));
    assert_eq!(snapshot(&instance), before_b);
    let before_status = snapshot(host.path());
    let status = command()
        .args(["status", "--context", "instance", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let status: serde_json::Value = serde_json::from_slice(&status).unwrap();
    assert_eq!(status["content_verified"], false);
    let current = status["contexts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|context| context["context"] == "instance")
        .unwrap();
    assert_eq!(current["state"], "valid");
    assert_eq!(current["version"], "A");
    assert_eq!(current["exact"], releases[0].0);
    assert!(
        status["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|finding| finding["code"] == "pending-input"
                && finding["next"] == json!(["instance", "continue"]))
    );
    expected("A", 0, false);
    assert_eq!(snapshot(host.path()), before_status);
    assert_eq!(snapshot(&instance), before_b);
    let before_list = snapshot(host.path());
    let pending = command()
        .args(["pending", "list", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let pending: serde_json::Value = serde_json::from_slice(&pending).unwrap();
    assert_eq!(snapshot(host.path()), before_list);
    assert_eq!(snapshot(&instance), before_b);
    assert_eq!(pending["schema"], 1);
    let operations = pending["operations"].as_array().unwrap();
    assert_eq!(operations.len(), 1);
    assert_eq!(operations[0]["kind"], "instance");
    let pending_id = operations[0]["id"].as_str().unwrap();
    let requirements = operations[0]["inputs"].as_array().unwrap();
    let input = requirements
        .iter()
        .find(|row| row["key"] == "private-addon")
        .unwrap();
    assert_eq!(input["bytes"], 9);
    assert_eq!(input["needed"], true);
    assert_eq!(
        input["digests"],
        json!([["selected-sha256", hex(&Sha256::digest(b"private B"))]])
    );
    let supplied = inputs.join("renamed private add-on.bin");
    fs::write(&supplied, b"incorrect").unwrap();
    let association = format!("private-addon={}", supplied.display());
    let pending_before = snapshot(host.path());
    command()
        .args([
            "--yes",
            "pending",
            "resume",
            pending_id,
            "--file",
            &association,
        ])
        .assert()
        .failure()
        .stderr(contains("Content differs from the accepted observation"));
    assert_eq!(snapshot(&instance), before_b);
    assert_eq!(snapshot(host.path()), pending_before);
    fs::write(&supplied, b"private B").unwrap();
    command()
        .args([
            "--yes",
            "pending",
            "resume",
            pending_id,
            "--file",
            &association,
        ])
        .assert()
        .success();
    expected("B", 1, true);
    observe(2);
    let before_choice = snapshot(&instance);
    let host_before_choice = snapshot(host.path());
    command()
        .args(["--yes", "instance", "update"])
        .arg(&releases[2].1)
        .assert()
        .failure()
        .stderr(contains("New release choice requires a decision: textures"));
    assert_eq!(snapshot(&instance), before_choice);
    assert_eq!(snapshot(host.path()), host_before_choice);
    expected("B", 1, true);
    command()
        .args(["--yes", "instance", "update"])
        .arg(&releases[2].1)
        .args(["--choice", "textures=none"])
        .assert()
        .success();
    expected("C", 2, false);
    let before_retired = snapshot(&instance);
    let host_before_retired = snapshot(host.path());
    command()
        .args(["--yes", "instance", "rollback", &releases[1].0, "--assets"])
        .arg(releases[1].1.parent().unwrap())
        .args(["--file", &association, "--choice", "textures=none"])
        .assert()
        .failure()
        .stderr(contains("Unknown requested choice"));
    assert_eq!(snapshot(&instance), before_retired);
    assert_eq!(snapshot(host.path()), host_before_retired);
    command()
        .args(["--yes", "instance", "rollback", &releases[1].0, "--assets"])
        .arg(releases[1].1.parent().unwrap())
        .args(["--file", &association])
        .assert()
        .success();
    expected("B", 1, true);
    let rolled_back = snapshot(&instance);
    channel(&offered, &key, &releases[0].0, &releases[0].1, 1, expires);
    command()
        .args(["--yes", "instance", "check-updates"])
        .arg(&offered)
        .assert()
        .failure()
        .stderr(contains("Channel sequence replay rejected"));
    assert_eq!(snapshot(&instance), rolled_back);
    let pending = command()
        .args(["pending", "list", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let pending: serde_json::Value = serde_json::from_slice(&pending).unwrap();
    assert_eq!(pending["operations"], json!([]));
    assert_eq!(snapshot(&publisher), publisher_before);
    assert_eq!(snapshot(&keys), keys_before);
    for (export, before) in exports_before {
        assert_eq!(snapshot(&export), before);
    }
}

#[test]
fn journey_signed_updates_retain_side_and_layout_and_refuse_context_changes() {
    let root = tempfile::tempdir().unwrap();
    let publisher = root.path().join("publisher");
    fs::create_dir(&publisher).unwrap();
    let keys = root.path().join("private keys");
    private_directory(&keys);
    let key = SigningKey::from_bytes(&[92; 32]);
    let key_file = keys.join("fixture.key");
    fs::write(&key_file, format!("{}\n", hex(&[92; 32]))).unwrap();
    let mut releases = Vec::new();
    for version in ["A", "B", "incompatible-icon"] {
        let export = root.path().join(format!("context export {version}"));
        fs::create_dir(&export).unwrap();
        let files: Vec<_> = [
            ("core", "mods/core.jar", "managed", version.as_bytes()),
            ("config", "config/player.txt", "seed", b"publisher defaults".as_slice()),
            ("icon", "icon.png", if version == "incompatible-icon" { "managed" } else { "seed" }, b"publisher icon".as_slice()),
            ("other", "other.png", "managed", b"other publisher image".as_slice()),
            ("nested", "images/icon.png", "managed", b"nested publisher image".as_slice()),
        ].into_iter().map(|(name, destination, policy, bytes)| {
            write(&export, &format!("assets/{name}"), bytes);
            json!({"key":name,"destination":destination,"layer":"common","policy":policy,
                "client":{"kind":"required"},"server":{"kind":"required"},
                "sha256":hex(&Sha256::digest(bytes)),"bytes":bytes.len(),"readonly":false,"executable":false,
                "assertions":[],"source":{"kind":"asset","path":format!("assets/{name}")}})
        }).collect();
        let payload = serde_json::to_vec(
            &json!({"schema":1,"pack":"verification.publisher","version":version,
            "minimum_engine":">=0.8.0-beta","require_subscription":true,
            "runtime":{"minecraft":"1.21.1","loader":{"kind":"vanilla"},"java_major":21},
            "files":files,"choices":[]}),
        )
        .unwrap();
        let (id, envelope) = stage_literal_release(&export, &publisher, &key_file, &payload);
        verify_staged(
            &envelope,
            &key.verifying_key(),
            &id,
            &export.join("release.json"),
        );
        releases.push((id, envelope));
    }
    let publisher_before = snapshot(&publisher);
    let keys_before = snapshot(&keys);
    let expires = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + 3600;
    for (layout, side) in [("game", "client"), ("game", "server"), ("prism", "client")] {
        let instance = root.path().join(format!("{layout}-{side}"));
        fs::create_dir(&instance).unwrap();
        let host = tempfile::tempdir().unwrap();
        let state = host.path().join("selected state");
        let command = || {
            let mut command = offline_command(&instance);
            command
                .arg("--state-dir")
                .arg(&state)
                .arg("--cache-dir")
                .arg(state.with_extension("cache"));
            command
        };
        command()
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
        let offered = host.path().join("channel.json");
        channel(&offered, &key, &releases[0].0, &releases[0].1, 1, expires);
        command()
            .args(["--yes", "instance", "check-updates"])
            .arg(&offered)
            .assert()
            .success();
        command()
            .args(["--yes", "instance", "update"])
            .arg(&releases[0].1)
            .args(["--layout", layout, "--side", side])
            .assert()
            .success();
        let game = if layout == "prism" {
            ".minecraft"
        } else {
            "game"
        };
        write(
            &instance,
            &format!("{game}/config/player.txt"),
            b"player settings",
        );
        write(
            &instance,
            &format!("{game}/saves/played/level.dat"),
            b"player world",
        );
        write(&instance, &format!("{game}/icon.png"), b"launcher image");
        let icon = instance.join(game).join("icon.png");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&icon, fs::Permissions::from_mode(0o600)).unwrap();
        }
        let icon_before = snapshot(&instance.join(game)).entries[Path::new("icon.png")];
        if layout == "prism" {
            // Observe an authentic but consumer-incompatible release. Neither
            // approval nor an explicit replacement grants launcher ownership.
            channel(&offered, &key, &releases[2].0, &releases[2].1, 2, expires);
            command()
                .args(["--yes", "instance", "check-updates"])
                .arg(&offered)
                .assert()
                .success();
            let before = snapshot(&instance);
            let host_before = snapshot(host.path());
            for decisions in [vec![], vec!["--replace", "icon.png"]] {
                command()
                    .args(["--yes", "instance", "update"])
                    .arg(&releases[2].1)
                    .args(decisions)
                    .assert()
                    .failure()
                    .stderr(contains("Prism rewrites game-root icon.png"))
                    .stderr(contains("icon.png: seed"))
                    .stderr(contains("SeedOwnership").not());
                assert_eq!(snapshot(&instance), before);
                assert_eq!(snapshot(host.path()), host_before);
            }
        }
        channel(&offered, &key, &releases[1].0, &releases[1].1, 3, expires);
        let before_observation = snapshot(&instance.join(game));
        command()
            .args(["--yes", "instance", "check-updates"])
            .arg(&offered)
            .assert()
            .success();
        assert_eq!(snapshot(&instance.join(game)), before_observation);
        let before = snapshot(&instance);
        for (option, value) in [
            ("--side", if side == "client" { "server" } else { "client" }),
            ("--layout", if layout == "prism" { "game" } else { "prism" }),
        ] {
            let diagnostic = match option {
                "--side" => "Explicit side conflicts with the installed environment",
                "--layout" => "Explicit layout conflicts with the installed layout",
                _ => unreachable!(),
            };
            command()
                .args(["--yes", "instance", "update"])
                .arg(&releases[1].1)
                .args([option, value])
                .assert()
                .failure()
                .stderr(contains(diagnostic));
            assert_eq!(snapshot(&instance), before);
        }
        command()
            .args(["--yes", "instance", "update"])
            .arg(&releases[1].1)
            .assert()
            .success();
        assert_eq!(
            snapshot(&instance.join(game)).files,
            BTreeMap::from([
                (
                    PathBuf::from(".empack-layout"),
                    if layout == "prism" {
                        b"empack-prism-layout-v1\n".to_vec()
                    } else {
                        b"empack-game-layout-v1\n".to_vec()
                    }
                ),
                (PathBuf::from("mods/core.jar"), b"B".to_vec()),
                (PathBuf::from("icon.png"), b"launcher image".to_vec()),
                (
                    PathBuf::from("other.png"),
                    b"other publisher image".to_vec()
                ),
                (
                    PathBuf::from("images/icon.png"),
                    b"nested publisher image".to_vec()
                ),
                (
                    PathBuf::from("config/player.txt"),
                    b"player settings".to_vec()
                ),
                (
                    PathBuf::from("saves/played/level.dat"),
                    b"player world".to_vec()
                ),
            ])
        );
        assert!(
            !instance
                .join(if layout == "prism" {
                    "game"
                } else {
                    ".minecraft"
                })
                .exists()
        );
        command()
            .args(["instance", "inspect"])
            .assert()
            .success()
            .stdout(contains(format!("release {};", releases[1].0)))
            .stdout(contains(format!("Retained release: {}", releases[0].0)));
        let status = command()
            .args(["status", "--context", "instance", "--json"])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let status: serde_json::Value = serde_json::from_slice(&status).unwrap();
        let context = status["contexts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["context"] == "instance")
            .unwrap();
        assert_eq!(context["side"], side);
        assert_eq!(context["game_directory"], game);
        assert_eq!(context["exact"], releases[1].0);
        assert_eq!(context["state"], "valid");
        assert_eq!(status["content_verified"], false);
        for name in ["other.png", "images/icon.png"] {
            write(&instance, &format!("{game}/{name}"), b"local managed image");
            let before = snapshot(&instance);
            command()
                .args(["--yes", "instance", "repair"])
                .assert()
                .failure()
                .stderr(contains("ModifiedManagedFile"));
            assert_eq!(snapshot(&instance), before);
            command()
                .args(["--yes", "instance", "repair", "--replace", name])
                .assert()
                .success();
        }
        let after_repair = snapshot(&instance.join(game));
        assert_eq!(
            after_repair.files[Path::new("other.png")],
            b"other publisher image"
        );
        assert_eq!(
            after_repair.files[Path::new("images/icon.png")],
            b"nested publisher image"
        );
        assert_eq!(after_repair.entries[Path::new("icon.png")], icon_before);
        command()
            .args(["--yes", "instance", "rollback", &releases[0].0, "--assets"])
            .arg(releases[0].1.parent().unwrap())
            .assert()
            .success();
        let after_rollback = snapshot(&instance.join(game));
        assert_eq!(after_rollback.files[Path::new("mods/core.jar")], b"A");
        for (name, bytes) in &after_repair.files {
            if name != Path::new("mods/core.jar") {
                assert_eq!(&after_rollback.files[name], bytes);
                assert_eq!(after_rollback.entries[name], after_repair.entries[name]);
            }
        }
        assert_eq!(after_rollback.files.len(), after_repair.files.len());
        assert_eq!(after_rollback.entries[Path::new("icon.png")], icon_before);
    }
    assert_eq!(snapshot(&publisher), publisher_before);
    assert_eq!(snapshot(&keys), keys_before);
}

#[test]
fn journey_signed_runtime_changes_preserve_prism_components_and_player_data() {
    let root = tempfile::Builder::new()
        .prefix("runtime transition Ω ")
        .tempdir()
        .unwrap();
    let publisher = root.path().join("publisher");
    let instance = root.path().join("Prism instance");
    fs::create_dir(&publisher).unwrap();
    fs::create_dir(&instance).unwrap();
    let keys = root.path().join("keys");
    private_directory(&keys);
    let key = SigningKey::from_bytes(&[59; 32]);
    let key_file = keys.join("public-fixture.key");
    fs::write(&key_file, format!("{}\n", hex(&[59; 32]))).unwrap();
    let (export_a, a, envelope_a) =
        stage_runtime_fixture(root.path(), &publisher, &key_file, "A", false);
    let (export_b, b, envelope_b) =
        stage_runtime_fixture(root.path(), &publisher, &key_file, "B", true);
    for (export, id, envelope) in [(&export_a, &a, &envelope_a), (&export_b, &b, &envelope_b)] {
        verify_staged(
            envelope,
            &key.verifying_key(),
            id,
            &export.join("release.json"),
        );
    }
    let sources = [
        snapshot(&export_a),
        snapshot(&export_b),
        snapshot(&publisher),
        snapshot(&keys),
    ];
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
    prepare_bootstrap(&instance, &export_a, &a)
        .args(["--layout", "prism"])
        .assert()
        .success();
    write(
        &instance,
        ".minecraft/config/player.txt",
        b"player settings",
    );
    write(
        &instance,
        ".minecraft/saves/played/level.dat",
        b"played world",
    );
    write(&instance, "instance.cfg", b"[General]\nname=Player name\n");
    let profile_path = instance.join("mmc-pack.json");
    let profile_a = json!({"formatVersion":1,"authorMetadata":{"keep":["custom",false]},"components":[
        {"uid":"net.minecraft","version":"1.21.1","important":true,"cachedName":"Game label"},
        {"uid":"user.component","version":"7","cachedName":"keep"}]});
    fs::write(
        &profile_path,
        serde_json::to_vec_pretty(&profile_a).unwrap(),
    )
    .unwrap();
    let profile_b = json!({"formatVersion":1,"authorMetadata":{"keep":["custom",false]},"components":[
        {"uid":"net.minecraft","version":"1.21.2","important":true,"cachedName":"Game label"},
        {"uid":"user.component","version":"7","cachedName":"keep"},
        {"uid":"net.fabricmc.fabric-loader","version":"0.16.0"}]});
    let expected = |id: &str, profile: &serde_json::Value, mod_path: &str, bytes: &[u8]| {
        assert!(
            !instance.join("game").exists(),
            "persisted Prism layout must remain selected"
        );
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&fs::read(&profile_path).unwrap()).unwrap(),
            *profile
        );
        let game = snapshot(&instance.join(".minecraft"));
        assert_eq!(
            game.files,
            std::collections::BTreeMap::from([
                (
                    std::path::PathBuf::from(".empack-layout"),
                    b"empack-prism-layout-v1\n".to_vec()
                ),
                (std::path::PathBuf::from(mod_path), bytes.to_vec()),
                (
                    std::path::PathBuf::from("config/player.txt"),
                    b"player settings".to_vec()
                ),
                (
                    std::path::PathBuf::from("saves/played/level.dat"),
                    b"played world".to_vec()
                ),
            ])
        );
        assert_eq!(
            fs::read(instance.join("instance.cfg")).unwrap(),
            b"[General]\nname=Player name\n"
        );
        offline_command(&instance)
            .args(["instance", "inspect"])
            .assert()
            .success()
            .stdout(contains(format!("release {id};")));
    };
    expected(&a, &profile_a, "mods/a.jar", b"A");
    let expires = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + 3600;
    let channel_a = root.path().join("runtime-channel-a.json");
    let channel_b = root.path().join("runtime-channel-b.json");
    channel(&channel_a, &key, &a, &envelope_a, 1, expires);
    channel(&channel_b, &key, &b, &envelope_b, 2, expires);
    observe(&instance, &channel_a).success();
    observe(&instance, &channel_b).success();
    let before = snapshot(&instance);
    offline_command(&instance)
        .args(["--yes", "--dry-run", "instance", "update"])
        .arg(&envelope_b)
        .assert()
        .success()
        .stdout(contains("Dry run complete - no changes applied"));
    offline_command(&instance)
        .args(["instance", "update"])
        .arg(&envelope_b)
        .assert()
        .failure()
        .stderr(contains("Noninteractive execution requires --yes"));
    assert_eq!(snapshot(&instance), before);
    let mut invalid = profile_a.clone();
    invalid["components"][0]["important"] = json!(false);
    fs::write(&profile_path, serde_json::to_vec_pretty(&invalid).unwrap()).unwrap();
    let edited = snapshot(&instance);
    offline_command(&instance)
        .args(["--yes", "instance", "update"])
        .arg(&envelope_b)
        .assert()
        .failure()
        .stderr(contains("Client game differs from locked runtime"));
    assert_eq!(snapshot(&instance), edited);
    fs::write(
        &profile_path,
        serde_json::to_vec_pretty(&profile_a).unwrap(),
    )
    .unwrap();
    offline_command(&instance)
        .args(["--yes", "instance", "update"])
        .arg(&envelope_b)
        .assert()
        .success();
    expected(&b, &profile_b, "mods/b.jar", b"B");
    let current_game = snapshot(&instance.join(".minecraft"));
    let current_profile = fs::read(&profile_path).unwrap();
    let subscription = fs::read(instance.join(".empack/subscription.json")).unwrap();
    for _ in 0..2 {
        prepare_bootstrap(&instance, &export_a, &a)
            .assert()
            .success();
        assert_eq!(snapshot(&instance.join(".minecraft")), current_game);
        assert_eq!(fs::read(&profile_path).unwrap(), current_profile);
        expected(&b, &profile_b, "mods/b.jar", b"B");
    }
    offline_command(&instance)
        .args(["--yes", "instance", "rollback"])
        .arg(&a)
        .arg("--assets")
        .arg(envelope_a.parent().unwrap())
        .assert()
        .success();
    expected(&a, &profile_a, "mods/a.jar", b"A");
    assert_eq!(
        fs::read(instance.join(".empack/subscription.json")).unwrap(),
        subscription
    );
    let restored = snapshot(&instance);
    observe(&instance, &channel_a)
        .failure()
        .stderr(contains("Channel sequence replay rejected"));
    assert_eq!(snapshot(&instance), restored);
    assert_eq!(
        [
            snapshot(&export_a),
            snapshot(&export_b),
            snapshot(&publisher),
            snapshot(&keys)
        ],
        sources
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
