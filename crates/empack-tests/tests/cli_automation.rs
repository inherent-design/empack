//! Execute the emitted helper with the selected product; GitHub is a recorded protocol fixture.
#[cfg(unix)]
#[path = "support/files.rs"]
mod files;
use empack_tests::harness::{
    archive::zip_members,
    process::{TestProject, empack_bin, empack_cmd, offline_command},
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs,
    path::PathBuf,
    process::{Command, Output},
    time::Duration,
};

const AUTHOR: &str = r#"schema: 5
pack:
  id: automation.fixture
  name: Automation Pack
  version: '1.2.3-beta'
runtime:
  minecraft: '1.21.1'
  loader: vanilla
  java: 21
dependencies: {}
build:
  default: [launcher_profile, exchange]
  profiles:
    launcher_profile:
      target: prism
      delivery: bundled
    exchange: modrinth
"#;
const CLIENT: &str = "Automation Pack-1.2.3-beta-prism-bundled.zip";
const EXCHANGE: &str = "Automation Pack-1.2.3-beta.mrpack";

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

struct Fixture {
    project: TestProject,
    root: PathBuf,
    bin: PathBuf,
    runner: PathBuf,
    python: PathBuf,
    calls: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let project = TestProject::new();
        offline_command(project.dir())
            .args([
                "init",
                "--yes",
                "--modloader",
                "none",
                "--mc-version",
                "1.21.1",
                "--author",
                "Fixture",
                "author project",
            ])
            .assert()
            .success();
        let root = project.dir().join("author project");
        fs::write(root.join("empack.yml"), AUTHOR).unwrap();
        offline_command(&root)
            .args(["sync", "--yes"])
            .assert()
            .success();
        fs::create_dir_all(root.join("pack/config")).unwrap();
        fs::write(root.join("pack/config/test.properties"), b"fixture=true\n").unwrap();
        fs::create_dir_all(root.join("dist")).unwrap();
        fs::write(root.join("dist/stale.zip"), b"prior unrelated output").unwrap();
        let bin = project.dir().join("selected tools");
        fs::create_dir(&bin).unwrap();
        fs::copy(
            empack_bin(),
            bin.join(if cfg!(windows) {
                "empack.exe"
            } else {
                "empack"
            }),
        )
        .unwrap();
        let requested = std::env::var_os("EMPACK_TEST_PYTHON").unwrap_or_else(|| {
            if cfg!(windows) {
                "python".into()
            } else {
                "python3".into()
            }
        });
        let output = Command::new(requested)
            .args(["-c", "import sys; print(sys.executable)"])
            .output()
            .expect("Python is required for generated automation tests");
        assert!(
            output.status.success(),
            "Python discovery failed: {output:?}"
        );
        let python = PathBuf::from(String::from_utf8(output.stdout).unwrap().trim());
        assert!(python.is_absolute() && python.is_file());
        let calls = project.dir().join("github-calls.jsonl");
        let recorder = project.dir().join("github_recorder.py");
        fs::write(
            &recorder,
            r#"import hashlib, json, pathlib, sys
base = pathlib.Path(__file__).parent
args = sys.argv[1:]
assert args[:2] in [['release', 'create'], ['release', 'edit']], args
row = {'args': args}
if args[:2] == ['release', 'create']:
    row['assets'] = {pathlib.Path(p).name: hashlib.sha256(pathlib.Path(p).read_bytes()).hexdigest()
                     for p in args[3:] if not p.startswith('--')}
with (base / 'github-calls.jsonl').open('a', encoding='utf-8') as output:
    output.write(json.dumps(row) + '\n')
if (base / 'github-failure').read_text(encoding='utf-8') == args[1]:
    print('controlled GitHub ' + args[1] + ' failure', file=sys.stderr)
    sys.exit(23)
"#,
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let quote =
                |p: &std::path::Path| format!("'{}'", p.to_str().unwrap().replace('\'', "'\\''"));
            let gh = bin.join("gh");
            fs::write(
                &gh,
                format!(
                    "#!/bin/sh\nexec {} {} \"$@\"\n",
                    quote(&python),
                    quote(&recorder)
                ),
            )
            .unwrap();
            fs::set_permissions(gh, fs::Permissions::from_mode(0o755)).unwrap();
        }
        let runner = project.dir().join("runner temporary");
        fs::create_dir(&runner).unwrap();
        Self {
            project,
            root,
            bin,
            runner,
            python,
            calls,
        }
    }

    fn run(&self, action: &str, profiles: &str, tag: &str, fail: &str) -> Output {
        fs::write(self.project.dir().join("github-failure"), fail).unwrap();
        let isolated = empack_cmd(self.project.dir());
        let mut command = Command::new(&self.python);
        command.env_clear();
        for (key, value) in isolated.get_envs() {
            if let Some(value) = value {
                command.env(key, value);
            }
        }
        command
            .current_dir(&self.root)
            .env("PATH", &self.bin)
            .env("PYTHONUTF8", "1")
            .env("RUNNER_TEMP", &self.runner)
            .env("EMPACK_PROFILES", profiles)
            .env("GITHUB_REF_NAME", tag)
            .env("EMPACK_NET_TIMEOUT", "1")
            .env("HTTP_PROXY", "http://127.0.0.1:9")
            .env("HTTPS_PROXY", "http://127.0.0.1:9")
            .env("ALL_PROXY", "http://127.0.0.1:9")
            .env("NO_PROXY", "")
            .arg(self.root.join(".github/scripts/empack-ci.py"))
            .arg(action);
        assert_cmd::Command::from_std(command)
            .timeout(Duration::from_secs(60))
            .output()
            .unwrap()
    }

    fn staged(&self, source_names: &[&str]) -> Vec<(String, String)> {
        let stage = self.runner.join("empack-artifacts");
        let expected: BTreeSet<_> = source_names
            .iter()
            .map(|name| name.replace(' ', "-"))
            .chain(std::iter::once("SHA256SUMS".into()))
            .collect();
        let actual: BTreeSet<_> = fs::read_dir(&stage)
            .unwrap()
            .map(|p| p.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(expected, actual);
        let mut entries = vec![];
        for name in source_names {
            let published = name.replace(' ', "-");
            let bytes = fs::read(stage.join(&published)).unwrap();
            assert_eq!(bytes, fs::read(self.root.join("dist").join(name)).unwrap());
            entries.push((published, digest(&bytes)));
        }
        let checksums = fs::read_to_string(stage.join("SHA256SUMS")).unwrap();
        let expected: BTreeSet<_> = entries
            .iter()
            .map(|(name, hash)| format!("{hash}  {name}"))
            .collect();
        assert_eq!(
            checksums
                .lines()
                .map(str::to_owned)
                .collect::<BTreeSet<_>>(),
            expected
        );
        assert_eq!(
            fs::read(self.root.join("dist/stale.zip")).unwrap(),
            b"prior unrelated output"
        );
        entries
    }

    fn calls(&self) -> Vec<Value> {
        if !self.calls.exists() {
            return vec![];
        }
        fs::read_to_string(&self.calls)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }
}

#[test]
fn cli_generated_validation_stages_only_selected_exact_artifacts() {
    for (profiles, names) in [
        ("", vec![CLIENT, EXCHANGE]),
        ("launcher_profile", vec![CLIENT]),
    ] {
        let fixture = Fixture::new();
        let author = fs::read(fixture.root.join("empack.yml")).unwrap();
        let lock = fs::read(fixture.root.join("empack.lock")).unwrap();
        let output = fixture.run("validate", profiles, "", "");
        assert!(output.status.success(), "{output:?}");
        fixture.staged(&names);
        assert!(fixture.calls().is_empty());
        let archive = zip_members(&fixture.root.join("dist").join(CLIENT));
        assert_eq!(
            archive[".minecraft/config/test.properties"],
            b"fixture=true\n"
        );
        assert_eq!(fs::read(fixture.root.join("empack.yml")).unwrap(), author);
        assert_eq!(fs::read(fixture.root.join("empack.lock")).unwrap(), lock);
    }
}

#[cfg(unix)]
#[test]
fn cli_generated_release_refuses_stale_tags_and_failed_builds_before_publication() {
    for failure in ["tag", "stale", "template"] {
        let fixture = Fixture::new();
        if failure == "stale" {
            fs::write(
                fixture.root.join("empack.yml"),
                AUTHOR.replace("1.2.3-beta", "1.2.4-beta"),
            )
            .unwrap();
        } else if failure == "template" {
            fs::write(
                fixture.root.join("templates/client/instance.cfg.template"),
                "{{#if BROKEN",
            )
            .unwrap();
        }
        let before = files::snapshot(&fixture.root);
        let output = fixture.run(
            "release",
            "",
            if failure == "tag" {
                "v9.9.9"
            } else {
                "v1.2.3-beta"
            },
            "",
        );
        assert!(!output.status.success(), "{failure}: {output:?}");
        let diagnostic = String::from_utf8_lossy(&output.stderr);
        let expected = match failure {
            "tag" => "must match authored version",
            "stale" => "current lock",
            "template" => "template",
            _ => unreachable!(),
        };
        assert!(
            diagnostic.to_ascii_lowercase().contains(expected),
            "{failure}: {diagnostic}"
        );
        assert!(fixture.calls().is_empty(), "{failure}");
        assert!(!fixture.runner.join("empack-artifacts").exists());
        assert_eq!(files::snapshot(&fixture.root), before, "{failure}");
    }
}

#[cfg(unix)]
#[test]
fn cli_generated_release_reveals_only_after_recorded_upload_success() {
    for failure in ["", "create", "edit"] {
        let fixture = Fixture::new();
        let output = fixture.run("release", "launcher_profile", "v1.2.3-beta", failure);
        assert_eq!(output.status.success(), failure.is_empty(), "{output:?}");
        let assets = fixture.staged(&[CLIENT]);
        let calls = fixture.calls();
        assert_eq!(calls.len(), if failure == "create" { 1 } else { 2 });
        let create = calls[0]["args"].as_array().unwrap();
        assert_eq!(
            &create[..3],
            &[
                Value::from("release"),
                Value::from("create"),
                Value::from("v1.2.3-beta")
            ]
        );
        for flag in [
            "--draft",
            "--verify-tag",
            "--generate-notes",
            "--prerelease",
        ] {
            assert!(create.contains(&Value::from(flag)));
        }
        let uploaded = calls[0]["assets"].as_object().unwrap();
        assert_eq!(uploaded.len(), 2);
        for (name, hash) in assets {
            assert_eq!(uploaded[&name], hash);
        }
        assert_eq!(
            uploaded["SHA256SUMS"],
            digest(&fs::read(fixture.runner.join("empack-artifacts/SHA256SUMS")).unwrap())
        );
        if calls.len() == 2 {
            assert_eq!(
                calls[1]["args"],
                serde_json::json!([
                    "release",
                    "edit",
                    "v1.2.3-beta",
                    "--draft=false",
                    "--prerelease"
                ])
            );
        }
    }
}
