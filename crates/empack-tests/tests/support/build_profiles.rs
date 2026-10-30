//! Literal author profiles and independent archive assertions, through the real CLI.
//! No launcher UI or game execution is implied by checking generated integration.
use crate::files::snapshot;
use empack_tests::harness::{
    archive::{distribution_inventory, tar_gz_members, zip_members},
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
            assert_inventory(&members, managed, subscribed, &CONTENT);
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

fn assert_inventory(
    members: &BTreeMap<String, Vec<u8>>,
    managed: bool,
    subscribed: bool,
    content: &[(&str, &[u8])],
) {
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
        assert_eq!(files.len(), content.len());
        assert_eq!(
            files
                .iter()
                .map(|f| f["destination"].as_str().unwrap())
                .collect::<BTreeSet<_>>(),
            content.iter().map(|(path, _)| *path).collect()
        );
        for &(destination, bytes) in content {
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
        for &(path, bytes) in content {
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
    // The bundled template follows checkout line endings. Both LF and CRLF are
    // supported Prism syntax; normalize only CRLF, never fields or asset bytes.
    let configuration = String::from_utf8(members["instance.cfg"].clone()).unwrap();
    let configuration = configuration.replace("\r\n", "\n");
    assert!(!configuration.contains('\r'), "bare CR in generated INI");
    assert_eq!(
        configuration,
        format!(
            "[General]\nConfigVersion=1.2\nInstanceType=OneSix\nExportAuthor=\"Fixture\"\nOverrideMemory=false\nname=\"Profile fixture\"\niconKey=empack_icon\nAutoUpdate=false\nManagedPack=true\nOverrideCommands=true\nPreLaunchCommand={prepare}\nWrapperCommand={wrapper}\n"
        )
    );
}

#[test]
fn cli_prism_template_archives_preserve_inventory_attributes_and_failed_rebuilds() {
    let fixture = Fixture::new(false);
    let root = fixture.project.dir();
    let description = "line\nkey='雪'\\suffix";
    fs::write(
        root.join("empack.yml"),
        AUTHOR.replace(
            "authors: [Fixture]",
            "authors: [Fixture], description: \"line\\nkey='雪'\\\\suffix\"",
        ),
    )
    .unwrap();
    fixture.command().args(["--yes", "sync"]).assert().success();
    let templates: &[(&str, &[u8])] = &[
        ("common/notice.txt.template", b"common {{NAME}}"),
        (
            "client/notice.txt.template",
            "client {{NAME}} / 雪\n".as_bytes(),
        ),
        ("server/server-only.txt.template", b"excluded"),
        ("common/nested/raw.bin", &[0xff, 0, 0x80, b'{', b'{']),
        (
            "client/settings.ini.template",
            b"value={{ini_quote DESCRIPTION}}\n",
        ),
        (
            "client/settings.properties.template",
            b"value={{properties_value DESCRIPTION}}\n",
        ),
        (
            "client/show.sh.template",
            b"#!/bin/sh\nprintf '%s' {{shell_quote DESCRIPTION}}\n",
        ),
    ];
    for (name, bytes) in templates {
        let path = root.join("templates").join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            root.join("templates/client/show.sh.template"),
            fs::Permissions::from_mode(0o555),
        )
        .unwrap();
    }
    // Expected output is literal and independent of the product's encoder.
    let rendered: &[(&str, &[u8])] = &[
        ("notice.txt", "client Profile fixture / 雪\n".as_bytes()),
        ("nested/raw.bin", &[0xff, 0, 0x80, b'{', b'{']),
        (
            "settings.ini",
            "value=\"line\\xakey='雪'\\\\suffix\"\n".as_bytes(),
        ),
        (
            "settings.properties",
            b"value=line\\u000akey='\\u96ea'\\\\suffix\n",
        ),
        (
            "show.sh",
            "#!/bin/sh\nprintf '%s' 'line\nkey='\"'\"'雪'\"'\"'\\suffix'\n".as_bytes(),
        ),
    ];
    let mut author = snapshot(root);
    #[cfg(unix)]
    let mut shell = None;
    for delivery in ["references", "bundled"] {
        let mut previous = None;
        for format in ["zip", "tar.gz", "7z"] {
            fixture
                .command()
                .args([
                    "--yes",
                    "build",
                    "--ad-hoc",
                    "prism",
                    "--delivery",
                    delivery,
                    "--format",
                    format,
                ])
                .assert()
                .success();
            let output = format!("dist/Profile fixture-1-prism-{delivery}.{format}");
            let archive = root.join(&output);
            let members = distribution_inventory(&archive, format);
            let mut bytes: BTreeMap<_, _> = members
                .iter()
                .map(|(name, member)| {
                    (
                        name.clone(),
                        member.small.clone().expect("small fixture member"),
                    )
                })
                .collect();
            for &(name, expected) in rendered {
                assert_eq!(
                    bytes.remove(name).unwrap(),
                    expected,
                    "{delivery}/{format}/{name}"
                );
                let member = &members[name];
                assert_eq!(member.bytes, expected.len() as u64);
                assert_eq!(member.sha256, hash(expected));
                assert_eq!(
                    member.mode,
                    if name == "show.sh" && cfg!(unix) {
                        0o555
                    } else {
                        0o644
                    }
                );
            }
            #[cfg(unix)]
            {
                shell = Some((
                    members["show.sh"].small.clone().unwrap(),
                    members["show.sh"].mode,
                ));
            }
            assert_inventory(&bytes, delivery == "references", false, &CONTENT);
            assert!(
                members
                    .iter()
                    .filter(|(name, _)| *name != "show.sh")
                    .all(|(_, member)| member.mode == 0o644)
            );
            if let Some(previous) = &previous {
                assert_eq!(
                    &members, previous,
                    "archive formats changed the effective inventory"
                );
            }
            previous = Some(members);
            author.insert(output.into(), fs::read(&archive).unwrap());
            assert_eq!(
                snapshot(root),
                author,
                "build changed author inputs or prior outputs"
            );
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let script = tempfile::tempdir().unwrap();
        let executable = script.path().join("show.sh");
        let (bytes, mode) = shell.unwrap();
        fs::write(&executable, bytes).unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(mode)).unwrap();
        assert_cmd::Command::new(&executable)
            .env_clear()
            .timeout(std::time::Duration::from_secs(5))
            .assert()
            .success()
            .stdout(description);
        assert_eq!(
            fs::metadata(root.join("templates/client/show.sh.template"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o555
        );
    }
    #[cfg(not(unix))]
    let _ = description;
    for (name, invalid, diagnostic) in [
        (
            "bad.template",
            b"{{MISSING}}".as_slice(),
            "Template rendering failed",
        ),
        ("bad.template", &[0xff, 0], "Explicit template is not UTF-8"),
        (
            "notice.txt",
            b"duplicate destination",
            "Two templates produce the same destination",
        ),
    ] {
        let path = root.join("templates/client").join(name);
        fs::write(&path, invalid).unwrap();
        let before = snapshot(root);
        for format in ["zip", "tar.gz", "7z"] {
            fixture
                .command()
                .args([
                    "--yes",
                    "build",
                    "--ad-hoc",
                    "prism",
                    "--delivery",
                    "references",
                    "--format",
                    format,
                ])
                .assert()
                .failure()
                .stderr(predicates::str::contains(diagnostic));
            assert_eq!(
                snapshot(root),
                before,
                "failed preparation changed author work or prior archives"
            );
        }
        fs::remove_file(path).unwrap();
    }
    assert_eq!(snapshot(root), author);
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
fn cli_multi_output_failure_preserves_prior_archives_and_rebuilds_same_version_inputs() {
    let fixture = Fixture::new(false);
    let root = fixture.project.dir();
    let author = AUTHOR
        .replace("default: [curated]", "default: [exchange, curated]")
        .replace(
            "    curated:",
            "    exchange: {target: modrinth, format: zip}\n    curated:",
        );
    fs::write(root.join("empack.yml"), &author).unwrap();
    fixture.command().args(["--yes", "sync"]).assert().success();
    fs::create_dir_all(root.join("templates/client")).unwrap();
    let template = root.join("templates/client/notice.txt.template");
    fs::write(&template, b"{{NAME}} / {{VERSION}} / first\n").unwrap();
    fs::create_dir_all(root.join("dist/obsolete")).unwrap();
    fs::write(root.join("dist/obsolete/old.zip"), b"obsolete artifact").unwrap();
    fs::write(root.join("author-notes.txt"), b"outside managed outputs").unwrap();
    let exchange = "dist/Profile fixture-1.mrpack";

    let build = |configuration: &[u8], notice: &[u8], clean: bool| {
        let mut before = snapshot(root);
        let mut command = fixture.command();
        command.args(["--yes", "build"]);
        if clean {
            command.arg("--clean");
            before.retain(|path, _| !path.starts_with("dist"));
        }
        command.assert().success();
        let mut client = tar_gz_members(&root.join(SUBSCRIBED));
        assert_eq!(client.remove("notice.txt").unwrap(), notice);
        assert_inventory(
            &client,
            true,
            true,
            &[(CONTENT[0].0, configuration), CONTENT[1]],
        );
        let platform = zip_members(&root.join(exchange));
        assert_eq!(
            platform.keys().map(String::as_str).collect::<BTreeSet<_>>(),
            BTreeSet::from([
                "modrinth.index.json",
                "overrides/config/settings.txt",
                "overrides/readme.txt",
            ])
        );
        assert_eq!(platform["overrides/config/settings.txt"], configuration);
        assert_eq!(platform["overrides/readme.txt"], CONTENT[1].1);
        let manifest: Value = serde_json::from_slice(&platform["modrinth.index.json"]).unwrap();
        assert_eq!(manifest["formatVersion"], 1);
        assert_eq!(manifest["game"], "minecraft");
        assert_eq!(manifest["name"], "Profile fixture");
        assert_eq!(manifest["versionId"], "1");
        assert_eq!(manifest["dependencies"], json!({"minecraft":"1.21.1"}));
        assert_eq!(manifest["files"], json!([]));
        for output in [SUBSCRIBED, exchange] {
            before.insert(output.into(), fs::read(root.join(output)).unwrap());
        }
        assert_eq!(
            snapshot(root),
            before,
            "build changed files outside its selected output/cleanup set"
        );
        (client, platform)
    };

    let (initial_client, _) = build(CONTENT[0].1, b"Profile fixture / 1 / first\n", false);
    let before = snapshot(root);
    fixture
        .command()
        .args(["--dry-run", "build", "--clean"])
        .assert()
        .success();
    assert_eq!(
        snapshot(root),
        before,
        "cleanup preview changed prior output"
    );

    let changed = b"changed configuration, same pack version\n";
    fs::write(root.join("pack/config/settings.txt"), changed).unwrap();
    fs::write(&template, b"{{missing_required_value}}").unwrap();
    let refused = snapshot(root);
    for clean in [false, true] {
        let mut command = fixture.command();
        command.args(["--yes", "build"]);
        if clean {
            command.arg("--clean");
        }
        command
            .assert()
            .failure()
            .stderr(predicates::str::contains("missing_required_value"));
        assert_eq!(
            snapshot(root),
            refused,
            "a later recipe failure replaced an earlier output or cleaned prior files"
        );
    }

    fs::write(&template, b"{{NAME}} / {{VERSION}} / revised\n").unwrap();
    let (revised_client, revised_platform) =
        build(changed, b"Profile fixture / 1 / revised\n", false);
    let descriptor = ".minecraft/.empack-consumer/release.json";
    assert_ne!(
        initial_client[descriptor], revised_client[descriptor],
        "same-version content edit reused a stale release"
    );
    assert!(root.join("dist/prior.zip").is_file());
    assert!(root.join("dist/obsolete/old.zip").is_file());

    // Template-only edits must also be fresh; they do not change game content.
    fs::write(&template, b"{{NAME}} / {{VERSION}} / final\n").unwrap();
    let (final_client, final_platform) = build(changed, b"Profile fixture / 1 / final\n", true);
    assert_eq!(final_client, revised_client);
    assert_eq!(final_platform, revised_platform);
    assert!(!root.join("dist/prior.zip").exists());
    assert!(!root.join("dist/obsolete/old.zip").exists());
    assert!(
        root.join("dist/obsolete").is_dir(),
        "cleanup owns files, not directories"
    );
    assert_eq!(
        fs::read(root.join("empack.yml")).unwrap(),
        author.as_bytes()
    );
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

#[test]
fn cli_reference_prism_prepares_exact_content_and_rebuilds_without_version_change() {
    let fixture = Fixture::new(false);
    let mut prior_release = None;
    for settings in [CONTENT[0].1, &b"changed author configuration\n"[..]] {
        fs::write(
            fixture.project.dir().join("pack/config/settings.txt"),
            settings,
        )
        .unwrap();
        let content = [("config/settings.txt", settings), CONTENT[1]];
        let mut author = snapshot(fixture.project.dir());
        fixture
            .command()
            .args(["--yes", "build", "--ad-hoc", "prism"])
            .assert()
            .success();
        let artifact = fixture.project.dir().join(REFERENCE);
        let members = zip_members(&artifact);
        assert_inventory(&members, true, false, &content);
        author.insert(REFERENCE.into(), fs::read(&artifact).unwrap());
        assert_eq!(snapshot(fixture.project.dir()), author);
        let descriptor_name = ".minecraft/.empack-consumer/release.json";
        let descriptor = &members[descriptor_name];
        let digest = hash(descriptor);
        if let Some(previous) = prior_release.replace(digest.clone()) {
            assert_ne!(
                digest, previous,
                "changed bytes require a different release identity at the same version"
            );
        }
        // This extracts a known emitted fixture string, not a general Qt parser.
        // Actual Prism interpretation/import remains a separate consumer check.
        let ini = std::str::from_utf8(&members["instance.cfg"]).unwrap();
        let prepare: String = serde_json::from_str(
            ini.lines()
                .find_map(|l| l.strip_prefix("PreLaunchCommand="))
                .unwrap(),
        )
        .unwrap();
        let embedded_digest = prepare.strip_prefix("empack --workdir \"$INST_DIR\" --yes instance prepare \"$INST_DIR/.minecraft/.empack-consumer/release.json\" --sha256 ").unwrap()
            .strip_suffix(" --layout prism --side client").unwrap();
        assert_eq!(embedded_digest, digest);
        let instance = tempfile::Builder::new()
            .prefix("Prism instance café ")
            .tempdir()
            .unwrap();
        let host = tempfile::tempdir().unwrap();
        zip::ZipArchive::new(fs::File::open(&artifact).unwrap())
            .unwrap()
            .extract(instance.path())
            .unwrap();
        let game = instance.path().join(".minecraft");
        fs::create_dir_all(game.join("saves/player world")).unwrap();
        fs::write(
            game.join("saves/player world/level.dat"),
            b"player world sentinel",
        )
        .unwrap();
        let prepare = || {
            let mut command = offline_command(instance.path());
            command
                .arg("--state-dir")
                .arg(host.path().join("state"))
                .arg("--cache-dir")
                .arg(host.path().join("cache"))
                .args([
                    "--yes",
                    "instance",
                    "prepare",
                    descriptor_name,
                    "--sha256",
                    embedded_digest,
                    "--layout",
                    "prism",
                    "--side",
                    "client",
                ]);
            command
        };
        let selected = instance.path().join(descriptor_name);
        let mut changed = descriptor.clone();
        changed.push(b'\n');
        fs::write(&selected, changed).unwrap();
        let before = snapshot(instance.path());
        prepare()
            .assert()
            .failure()
            .stderr(predicates::str::contains(
                "Selected release payload identity mismatch",
            ));
        assert_eq!(snapshot(instance.path()), before);
        fs::write(&selected, descriptor).unwrap();
        // A valid descriptor cannot authorize corrupted included content, even
        // when the replacement has the declared length. This uses a cold store.
        let asset = game.join(".empack-consumer/assets").join(hash(settings));
        fs::write(&asset, vec![b'!'; settings.len()]).unwrap();
        let before = snapshot(instance.path());
        prepare().assert().failure();
        assert_eq!(snapshot(instance.path()), before);
        fs::write(&asset, settings).unwrap();
        prepare().assert().success();
        for (path, bytes) in &members {
            assert_eq!(fs::read(instance.path().join(path)).unwrap(), *bytes);
        }
        let visible = || {
            snapshot(&game)
                .into_iter()
                .filter(|(p, _)| !p.starts_with(".empack-consumer"))
                .collect::<BTreeMap<_, _>>()
        };
        let mut expected: BTreeMap<_, _> = content
            .iter()
            .map(|(p, b)| (std::path::PathBuf::from(p), b.to_vec()))
            .collect();
        expected.insert(
            ".empack-layout".into(),
            b"empack-prism-layout-v1\n".to_vec(),
        );
        expected.insert(
            "saves/player world/level.dat".into(),
            b"player world sentinel".to_vec(),
        );
        assert_eq!(visible(), expected);
        // Preparing again respects the seed's player-owned content and does
        // not rebuild the release descriptor to disguise a local modification.
        fs::write(game.join("config/settings.txt"), b"player configuration").unwrap();
        let installed = snapshot(instance.path());
        for _ in 0..2 {
            prepare().assert().success();
            assert_eq!(snapshot(instance.path()), installed);
        }
        assert_eq!(fs::read(&selected).unwrap(), *descriptor);
        assert_eq!(snapshot(fixture.project.dir()), author);
    }
}
