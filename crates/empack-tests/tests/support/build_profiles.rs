//! Literal author profiles and independent archive assertions, through the real CLI.
//! No launcher UI or game execution is implied by checking generated integration.
use crate::files::snapshot;
use empack_tests::harness::{
    archive::{tar_gz_members, zip_members},
    process::{TestProject, offline_command},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
};

const AUTHOR: &str = r#"# Keep the author's chosen subscription and archive format.
schema: 5
pack: {id: profiles.fixture, name: Profile fixture, version: '1', authors: [Fixture]}
runtime: {minecraft: '1.21.1', loader: vanilla, java: 21}
dependencies: {}
build:
  default: [curated]
  profiles:
    curated: {target: prism, delivery: bundled, environment: client, updates: empack, format: tar.gz}
"#;
const CONTENT: [(&str, &[u8]); 2] = [
    ("config/settings.txt", b"keep this configuration\n"),
    ("readme.txt", b"selected author content\n"),
];
const SUBSCRIBED: &str = "dist/Profile fixture-1-prism-bundled-subscribed.tar.gz";
const REFERENCE: &str = "dist/Profile fixture-1-prism-references.zip";
const STANDALONE: &str = "dist/Profile fixture-1-prism-bundled.zip";

struct Fixture {
    project: TestProject,
    host: tempfile::TempDir,
}
impl Fixture {
    fn new(ambiguous: bool) -> Self {
        let project = TestProject::new();
        let host = tempfile::tempdir().unwrap();
        let author = if ambiguous {
            AUTHOR.replace("    curated:", "    portable: {target: prism, delivery: bundled, environment: client, updates: empack, format: zip}\n    curated:")
                .replace("default: [curated]", "default: [curated, portable]")
        } else {
            AUTHOR.to_owned()
        };
        fs::write(project.dir().join("empack.yml"), author).unwrap();
        for (path, bytes) in CONTENT {
            let path = project.dir().join("pack").join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, bytes).unwrap();
        }
        fs::create_dir(project.dir().join("dist")).unwrap();
        fs::write(project.dir().join("dist/prior.zip"), b"prior artifact").unwrap();
        let result = Self { project, host };
        result.command().args(["--yes", "sync"]).assert().success();
        result
    }
    fn command(&self) -> assert_cmd::Command {
        let mut command = offline_command(self.project.dir());
        command
            .arg("--state-dir")
            .arg(self.host.path().join("state"))
            .arg("--cache-dir")
            .arg(self.host.path().join("cache"));
        command
    }
    fn explain(&self, args: &[&str]) -> Value {
        let before = snapshot(self.project.dir());
        let host_before = snapshot(self.host.path());
        let output = self
            .command()
            .args(["build", "--explain", "--json"])
            .args(args)
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        assert_eq!(snapshot(self.project.dir()), before);
        assert_eq!(snapshot(self.host.path()), host_before);
        let report: Value = serde_json::from_slice(&output).unwrap();
        assert_eq!(report["schema"], 2);
        assert_eq!(report["pack_id"], "profiles.fixture");
        assert_eq!(report["pack"], "Profile fixture");
        assert_eq!(report["version"], "1");
        assert_eq!(report["optional_policy"], json!({"mode":"preserve"}));
        assert_eq!(
            report["input_verification"],
            "Not performed; unavailable provider downloads and cache contents remain unknown"
        );
        report
    }
    fn build(&self, args: &[&str], output: &str, managed: bool, subscribed: bool) {
        self.build_outputs(args, &[(output, managed, subscribed)]);
    }
    fn build_outputs(&self, args: &[&str], outputs: &[(&str, bool, bool)]) {
        let mut expected = snapshot(self.project.dir());
        self.command()
            .args(["--yes", "build"])
            .args(args)
            .assert()
            .success();
        for &(output, managed, subscribed) in outputs {
            let path = self.project.dir().join(output);
            let members = if output.ends_with(".tar.gz") {
                tar_gz_members(&path)
            } else {
                zip_members(&path)
            };
            assert_inventory(&members, managed, subscribed);
            expected.insert(output.into(), fs::read(path).unwrap());
        }
        // Publication owns only selected outputs, including repeated builds.
        assert_eq!(snapshot(self.project.dir()), expected);
    }
}

fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn assert_inventory(members: &BTreeMap<String, Vec<u8>>, managed: bool, subscribed: bool) {
    let mut names = BTreeSet::from(["instance.cfg".to_owned(), "mmc-pack.json".to_owned()]);
    assert_eq!(
        serde_json::from_slice::<Value>(&members["mmc-pack.json"]).unwrap(),
        json!({
            "formatVersion":1,"components":[{"uid":"net.minecraft","version":"1.21.1","important":true}]
        })
    );
    let (prepare, wrapper) = if managed {
        let path = ".minecraft/.empack-consumer/release.json";
        names.insert(path.into());
        let descriptor: Value = serde_json::from_slice(&members[path]).unwrap();
        assert_eq!(descriptor["pack"], "profiles.fixture");
        assert_eq!(descriptor["version"], "1");
        assert_eq!(descriptor["require_subscription"], subscribed);
        assert_eq!(
            descriptor["runtime"],
            json!({"minecraft":"1.21.1","loader":{"kind":"vanilla"},"java_major":21})
        );
        assert_eq!(descriptor["choices"], json!([]));
        let files = descriptor["files"].as_array().unwrap();
        assert_eq!(files.len(), CONTENT.len());
        assert_eq!(
            files
                .iter()
                .map(|f| f["destination"].as_str().unwrap())
                .collect::<BTreeSet<_>>(),
            CONTENT.iter().map(|(path, _)| *path).collect()
        );
        for (destination, bytes) in CONTENT {
            let file = files
                .iter()
                .find(|f| f["destination"] == destination)
                .unwrap();
            let digest = hash(bytes);
            assert_eq!(file["sha256"], digest);
            assert_eq!(file["bytes"], bytes.len());
            assert_eq!(
                file["policy"],
                if destination.starts_with("config/") {
                    "seed"
                } else {
                    "managed"
                }
            );
            assert_eq!(
                file["source"],
                json!({"kind":"asset","path":format!("assets/{digest}")})
            );
            let member = format!(".minecraft/.empack-consumer/assets/{digest}");
            assert_eq!(members[&member], bytes);
            names.insert(member);
        }
        (
            format!(
                "empack --workdir \"$INST_DIR\" --yes instance prepare \"$INST_DIR/.minecraft/.empack-consumer/release.json\" --sha256 {} --layout prism --side client{}",
                hash(&members[path]),
                if subscribed {
                    " --require-subscription"
                } else {
                    ""
                }
            ),
            format!(
                "empack --workdir \"$INST_DIR\" --yes instance launch{} --",
                if subscribed { " --check-updates" } else { "" }
            ),
        )
    } else {
        for (path, bytes) in CONTENT {
            let member = format!(".minecraft/{path}");
            assert_eq!(members[&member], bytes);
            names.insert(member);
        }
        (String::new(), String::new())
    };
    assert_eq!(members.keys().cloned().collect::<BTreeSet<_>>(), names);
    // These known ASCII command fixtures need only JSON-compatible quote escaping.
    // This is an exact output oracle, not another general Prism INI parser.
    let prepare = if prepare.is_empty() {
        String::new()
    } else {
        json!(prepare).to_string()
    };
    let wrapper = if wrapper.is_empty() {
        String::new()
    } else {
        json!(wrapper).to_string()
    };
    assert_eq!(
        String::from_utf8(members["instance.cfg"].clone()).unwrap(),
        format!(
            "[General]\nConfigVersion=1.2\nInstanceType=OneSix\nExportAuthor=\"Fixture\"\nOverrideMemory=false\nname=\"Profile fixture\"\niconKey=empack_icon\nAutoUpdate=false\nManagedPack=true\nOverrideCommands=true\nPreLaunchCommand={prepare}\nWrapperCommand={wrapper}\n"
        )
    );
}

fn profile(
    report: &Value,
    name: &str,
    origin: &str,
    delivery: &str,
    updates: &str,
    format: &str,
    output: &str,
) {
    assert_eq!(report["profiles"].as_array().unwrap().len(), 1);
    let profile = &report["profiles"][0];
    assert_eq!(profile["name"], name);
    assert_eq!(profile["selection"], origin);
    assert_eq!(
        profile["recipe"],
        json!({"consumer":"prism","delivery":delivery,"environment":"client","updates":updates,"format":format})
    );
    assert_eq!(profile["output"], output);
    assert_eq!(
        profile["recipient_empack_required"],
        delivery == "references" || updates == "empack"
    );
    assert_eq!(profile["java"]["minimum_major"], 21);
    assert_eq!(
        profile["runtime_preparation"],
        "Prism prepares client game/runtime files"
    );
    assert_eq!(
        profile["author_acquisition"],
        "May require verified downloads even for references; not assessed by explanation"
    );
}

#[test]
fn cli_profiles_preserve_authored_policy_and_explain_actual_output() {
    let fixture = Fixture::new(false);
    for (args, origin) in [
        (&[][..], "default-profile"),
        (&["--profile", "curated"][..], "profile"),
        (&["prism"][..], "consumer-profile"),
        (
            &["--profile", "curated", "--profile", "curated"][..],
            "profile",
        ),
    ] {
        let report = fixture.explain(args);
        assert_eq!(report["lock"], "current");
        assert_eq!(report["declared_manual_inputs"], json!([]));
        profile(
            &report, "curated", origin, "bundled", "empack", "tar.gz", SUBSCRIBED,
        );
        for field in ["consumer", "delivery", "environment", "updates", "format"] {
            assert_eq!(report["profiles"][0]["origins"][field], origin);
        }
        fixture.build(args, SUBSCRIBED, true, true);
    }
    let args = [
        "--profile",
        "curated",
        "--updates",
        "snapshot",
        "--format",
        "zip",
    ];
    let report = fixture.explain(&args);
    profile(
        &report, "curated", "profile", "bundled", "snapshot", "zip", STANDALONE,
    );
    assert_eq!(report["profiles"][0]["origins"]["updates"], "cli-override");
    assert_eq!(report["profiles"][0]["origins"]["format"], "cli-override");
    fixture.build(&args, STANDALONE, false, false);
    let report = fixture.explain(&["--ad-hoc", "prism"]);
    profile(
        &report,
        "prism",
        "ad-hoc",
        "references",
        "snapshot",
        "zip",
        REFERENCE,
    );
    fixture.build(&["--ad-hoc", "prism"], REFERENCE, true, false);
    assert_eq!(
        fs::read_to_string(fixture.project.dir().join("empack.yml")).unwrap(),
        AUTHOR
    );
}

#[test]
fn cli_profile_selection_refusals_preserve_existing_artifacts() {
    let fixture = Fixture::new(true);
    // Establish a previous usable output before checking rejected selections.
    fixture.build(&["--profile", "curated"], SUBSCRIBED, true, true);
    let before = snapshot(fixture.project.dir());
    let host_before = snapshot(fixture.host.path());
    for (args, diagnostic) in [
        (
            vec!["prism"],
            "Multiple profiles use 'prism'; select --profile NAME",
        ),
        (
            vec!["--profile", "missing"],
            "Unknown build profile 'missing'",
        ),
        (vec!["server"], "No configured profile for 'server'"),
        (
            vec![
                "--profile",
                "curated",
                "--profile",
                "portable",
                "--format",
                "zip",
            ],
            "File destination collides with another file or ancestor",
        ),
        (vec!["--profile", "curated", "prism"], "cannot be used with"),
        (vec!["--ad-hoc"], "required arguments"),
    ] {
        for explain in [false, true] {
            let mut command = fixture.command();
            command.args(["--yes", "build"]).args(&args);
            if explain {
                command.args(["--explain", "--json"]);
            }
            command
                .assert()
                .failure()
                .stderr(predicates::str::contains(diagnostic));
            assert_eq!(snapshot(fixture.project.dir()), before);
            assert_eq!(snapshot(fixture.host.path()), host_before);
        }
    }
    let zip = "dist/Profile fixture-1-prism-bundled-subscribed.zip";
    for args in [
        &[][..],
        &["--profile", "curated", "--profile", "portable"][..],
    ] {
        let report = fixture.explain(args);
        let selected = report["profiles"].as_array().unwrap();
        assert_eq!(selected.len(), 2);
        for (row, name, format, output) in [
            (&selected[0], "curated", "tar.gz", SUBSCRIBED),
            (&selected[1], "portable", "zip", zip),
        ] {
            assert_eq!(row["name"], name);
            assert_eq!(
                row["selection"],
                if args.is_empty() {
                    "default-profile"
                } else {
                    "profile"
                }
            );
            assert_eq!(
                row["recipe"],
                json!({"consumer":"prism","delivery":"bundled","environment":"client","updates":"empack","format":format})
            );
            assert_eq!(row["output"], output);
            assert_eq!(row["recipient_empack_required"], true);
        }
        fixture.build_outputs(args, &[(SUBSCRIBED, true, true), (zip, true, true)]);
    }
    let path = fixture.project.dir().join("empack.yml");
    let author = fs::read_to_string(&path).unwrap();
    fs::write(
        &path,
        author.replace("default: [curated, portable]", "default: []"),
    )
    .unwrap();
    let before = snapshot(fixture.project.dir());
    let host_before = snapshot(fixture.host.path());
    for explain in [false, true] {
        let mut command = fixture.command();
        command.args(["--yes", "build"]);
        if explain {
            command.arg("--explain");
        }
        command
            .assert()
            .failure()
            .stderr(predicates::str::contains("No default build profiles"));
        assert_eq!(snapshot(fixture.project.dir()), before);
        assert_eq!(snapshot(fixture.host.path()), host_before);
    }
}

#[test]
fn cli_profile_explanation_preserves_missing_and_stale_lock_states() {
    for missing in [false, true] {
        let fixture = Fixture::new(false);
        fixture.build(&["--profile", "curated"], SUBSCRIBED, true, true);
        if missing {
            fs::remove_file(fixture.project.dir().join("empack.lock")).unwrap();
        } else {
            fs::write(
                fixture.project.dir().join("empack.yml"),
                AUTHOR.replace("java: 21", "java: 22"),
            )
            .unwrap();
        }
        let before = snapshot(fixture.project.dir());
        let host_before = snapshot(fixture.host.path());
        let report = fixture.explain(&["--profile", "curated"]);
        assert_eq!(report["lock"], if missing { "missing" } else { "stale" });
        assert_eq!(report["declared_manual_inputs"], Value::Null);
        assert_eq!(report["profiles"][0]["output"], SUBSCRIBED);
        assert_eq!(
            report["profiles"][0]["java"]["minimum_major"],
            if missing { 21 } else { 22 }
        );
        fixture
            .command()
            .args(["build", "--explain", "--profile", "curated"])
            .assert()
            .success()
            .stdout(predicates::str::contains("updates: empack (profile)"))
            .stdout(predicates::str::contains("Recipient empack required: true"));
        fixture
            .command()
            .args(["--yes", "build", "--profile", "curated"])
            .assert()
            .failure()
            .stderr(predicates::str::contains(if missing {
                "Project has no exact resolution lock"
            } else {
                "Lock resolves a different intent revision"
            }));
        assert_eq!(snapshot(fixture.project.dir()), before);
        assert_eq!(snapshot(fixture.host.path()), host_before);
    }
}
