//! Public server builds with an independently pinned runtime inventory.
use empack_tests::harness::workspace::snapshot;
use empack_tests::harness::{
    archive::{Member, distribution_inventory},
    process::{TestProject, assert_dist_artifact_suffix},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::PathBuf,
    time::Duration,
};

const AUTHOR: &str = "schema: 5\npack: {id: server.fixture, name: Server fixture, version: '1'}\nruntime: {minecraft: '1.21.1', loader: vanilla, java: 21}\ndependencies: {}\n";
const DESCRIPTION: &str = " a\\u0041\r\né🦀\t#!=:value \u{c}";
// A literal oracle, independent of the product's properties encoder.
const PROPERTIES: &[u8] = b"motd=\\ a\\\\u0041\\u000d\\u000a\\u00e9\\ud83e\\udd80\\u0009#!=:value\\ \\u000c\r\nempty=\r\n";
const CONTENT: [(&str, &[u8], &str); 3] = [
    ("config/feature.txt", b"server configuration\n", "seed"),
    ("server.properties", b"motd=Fixture server\n", "managed"),
    ("readme.txt", b"pack notes\n", "managed"),
];
fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn text(member: &Member) -> &str {
    std::str::from_utf8(member.small.as_ref().expect("small fixture member")).unwrap()
}

pub fn verify() {
    let pin: Value = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/runtime/vanilla-1.21.1.json"
    ))
    .unwrap();
    let project = TestProject::new();
    let put = |path: &str, bytes: &[u8]| {
        let path = project.dir().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    };
    // JSON's quoted string is also a YAML scalar; retain the exact metadata value.
    let author = AUTHOR.replace(
        "version: '1'",
        &format!(
            "version: '1', description: {}",
            serde_json::to_string(DESCRIPTION).unwrap()
        ),
    );
    put("empack.yml", author.as_bytes());
    put("pack/config/feature.txt", b"common configuration\n");
    put(
        "overrides/client/config/feature.txt",
        b"client configuration\n",
    );
    put("overrides/server/config/feature.txt", CONTENT[0].1);
    put("pack/server.properties", CONTENT[1].1);
    put("pack/readme.txt", CONTENT[2].1);
    put(
        "templates/server/notes.txt.template",
        b"{{NAME}} / {{VERSION}}\n",
    );
    put("templates/common/banner.bin", &[0, 255, 1]);
    put(
        "templates/server/generated.properties.template",
        b"motd={{properties_value DESCRIPTION}}\r\nempty={{properties_value \"\"}}\r\n",
    );
    put("templates/client/client-only.txt", b"excluded");
    put("dist/prior.zip", b"prior artifact");
    let command = || {
        let mut command = assert_cmd::Command::from_std(project.cmd());
        command.timeout(Duration::from_secs(240));
        command
    };
    command().args(["--yes", "sync"]).assert().success();
    let author = || {
        (
            ["empack.yml", "empack.lock"].map(|name| fs::read(project.dir().join(name)).unwrap()),
            ["pack", "overrides", "templates"].map(|name| snapshot(&project.dir().join(name))),
        )
    };
    let before = author();
    let probe = tempfile::Builder::new()
        .prefix("server properties Ω ")
        .tempdir()
        .unwrap();
    let mut properties = Vec::new();
    for delivery in ["references", "bundled"] {
        let mut previous = None;
        for format in ["zip", "tar.gz", "7z"] {
            command()
                .args([
                    "--yes",
                    "build",
                    "--ad-hoc",
                    "server",
                    "--delivery",
                    delivery,
                    "--format",
                    format,
                ])
                .assert()
                .success();
            let artifact =
                assert_dist_artifact_suffix(project.dir(), &format!("-server-{delivery}.{format}"));
            let members = distribution_inventory(&artifact, format);
            check(&members, delivery == "references", &pin);
            let captured = probe.path().join(format!("{delivery}-{format}.properties"));
            fs::write(
                &captured,
                members["generated.properties"].small.as_ref().unwrap(),
            )
            .unwrap();
            properties.push(captured);
            if let Some(previous) = previous.replace(members.clone()) {
                assert_eq!(
                    members, previous,
                    "formats must preserve bytes and output attributes"
                );
            }
            assert_eq!(author(), before);
            assert_eq!(
                fs::read(project.dir().join("dist/prior.zip")).unwrap(),
                b"prior artifact"
            );
        }
    }
    // A changed scalar must be observable independently of the archive oracle.
    let control = probe.path().join("changed.properties");
    fs::write(&control, b"motd=changed\r\nempty=\r\n").unwrap();
    properties.push(control);
    let source = probe.path().join("PropertiesProbe.java");
    fs::write(
        &source,
        include_str!("../../../../tests/fixtures/templates/PropertiesProbe.java"),
    )
    .unwrap();
    let inputs = snapshot(probe.path());
    // Reuse the product fixture's positive environment allowlist, excluding JVM
    // injection variables from the caller. The probe itself has no product state.
    let environment = project.cmd();
    // Match the verification runner's selected Java instead of allowing PATH to
    // choose a different installation when JAVA_HOME is present.
    let executable = std::env::var_os("JAVA_HOME")
        .map(|home| {
            PathBuf::from(home)
                .join("bin")
                .join(if cfg!(windows) { "java.exe" } else { "java" })
        })
        .unwrap_or_else(|| PathBuf::from("java"));
    let mut java = assert_cmd::Command::new(executable);
    java.env_clear()
        .envs(
            environment
                .get_envs()
                .filter_map(|(key, value)| value.map(|value| (key, value))),
        )
        .current_dir(probe.path())
        .timeout(Duration::from_secs(30))
        .arg("-showversion")
        // The Windows launcher lost the absolute path's Omega before entering
        // source-file mode. Keep the Unicode cwd and files; pass ASCII basenames
        // and verify their real Unicode parent inside the consumer below.
        .arg(source.file_name().unwrap())
        .args(properties.iter().map(|path| path.file_name().unwrap()));
    // Linux Java decodes native arguments and filenames using the locale even
    // with JDK 21's UTF-8 file-content default. Keep the Unicode-path fixture and
    // select an explicit UTF-8 locale for this consumer, not product commands.
    #[cfg(target_os = "linux")]
    java.env("LANG", "C.UTF-8").env("LC_ALL", "C.UTF-8");
    let output = java.assert().success().get_output().clone();
    eprintln!("{}", String::from_utf8_lossy(&output.stderr));
    let rows: Vec<Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(rows.len(), properties.len());
    let parent_points: Vec<u32> = probe
        .path()
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .chars()
        .map(u32::from)
        .collect();
    for (index, row) in rows.iter().enumerate() {
        let original = if index == properties.len() - 1 {
            "changed"
        } else {
            DESCRIPTION
        };
        let points: Vec<u32> = original.chars().map(u32::from).collect();
        assert_eq!(
            row,
            &json!({
                "stream": points, "reader": points,
                "stream_keys": 2, "reader_keys": 2,
                "stream_empty": true, "reader_empty": true,
                "parent": parent_points,
            }),
            "{}",
            properties[index].display()
        );
    }
    assert_ne!(rows[0], *rows.last().unwrap());
    assert_eq!(
        snapshot(probe.path()),
        inputs,
        "Java changed captured inputs"
    );
    assert_eq!(author(), before);
    for (name, invalid, diagnostic) in [
        (
            "bad.template",
            b"{{MISSING}}".as_slice(),
            "Template rendering failed",
        ),
        ("bad.template", &[0xff, 0], "Explicit template is not UTF-8"),
        (
            "generated.properties",
            b"duplicate destination",
            "Two templates produce the same destination",
        ),
    ] {
        let path = project.dir().join("templates/server").join(name);
        fs::write(&path, invalid).unwrap();
        let author_before = author();
        let artifacts_before = snapshot(&project.dir().join("dist"));
        for delivery in ["references", "bundled"] {
            for format in ["zip", "tar.gz", "7z"] {
                command()
                    .args([
                        "--yes",
                        "build",
                        "--ad-hoc",
                        "server",
                        "--delivery",
                        delivery,
                        "--format",
                        format,
                    ])
                    .assert()
                    .failure()
                    .stderr(predicates::str::contains(diagnostic));
                assert_eq!(author(), author_before);
                assert_eq!(snapshot(&project.dir().join("dist")), artifacts_before);
            }
        }
        fs::remove_file(path).unwrap();
    }
    assert_eq!(author(), before);
}

fn check(members: &BTreeMap<String, Member>, managed: bool, pin: &Value) {
    let scripts = [
        "start.sh",
        "start.bat",
        "start_gui.sh",
        "start_gui.bat",
        "install_pack.sh",
        "install_pack.bat",
    ];
    let mut expected: BTreeSet<String> = scripts.into_iter().map(str::to_owned).collect();
    expected.extend([
        "banner.bin".into(),
        "notes.txt".into(),
        "generated.properties".into(),
    ]);
    assert_eq!(
        members["banner.bin"].small.as_deref(),
        Some(&[0, 255, 1][..])
    );
    assert_eq!(text(&members["notes.txt"]), "Server fixture / 1\n");
    assert_eq!(
        members["generated.properties"].small.as_deref(),
        Some(PROPERTIES)
    );
    let release_hash = if managed {
        let path = ".empack-consumer/release.json";
        expected.insert(path.into());
        let release: Value = serde_json::from_slice(members[path].small.as_ref().unwrap()).unwrap();
        assert_eq!(release["pack"], "server.fixture");
        assert_eq!(release["version"], "1");
        assert_eq!(release["require_subscription"], false);
        assert_eq!(
            release["runtime"],
            json!({"minecraft":"1.21.1","loader":{"kind":"vanilla"},"java_major":21})
        );
        assert_eq!(
            release["server_launch"],
            json!({"kind":"jar","path":"server.jar"})
        );
        assert_eq!(release["choices"], json!([]));
        let files = release["files"].as_array().unwrap();
        assert_eq!(files.len(), CONTENT.len() + 1);
        let names: BTreeSet<_> = files
            .iter()
            .map(|f| f["destination"].as_str().unwrap())
            .collect();
        assert_eq!(
            names,
            BTreeSet::from([
                "config/feature.txt",
                "server.properties",
                "readme.txt",
                "server.jar"
            ])
        );
        for file in files {
            let destination = file["destination"].as_str().unwrap();
            let (digest, bytes, policy) = if destination == "server.jar" {
                assert_eq!(file["client"], json!({"kind":"unsupported"}));
                assert_eq!(file["server"], json!({"kind":"required"}));
                assert!(
                    file["assertions"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|d| d["algorithm"] == "sha1" && d["value"] == pin["server_sha1"])
                );
                (
                    pin["server_sha256"].as_str().unwrap().to_owned(),
                    pin["server_bytes"].as_u64().unwrap(),
                    "managed",
                )
            } else {
                let (_, bytes, policy) =
                    CONTENT.iter().find(|(p, _, _)| *p == destination).unwrap();
                (hash(bytes), bytes.len() as u64, *policy)
            };
            assert_eq!(file["sha256"], digest);
            assert_eq!(file["bytes"], bytes);
            assert_eq!(file["policy"], policy, "{destination}");
            assert_eq!(
                file["source"],
                json!({"kind":"asset","path":format!("assets/{digest}")})
            );
            let path = format!(".empack-consumer/assets/{digest}");
            assert_eq!(members[&path].sha256, digest);
            assert_eq!(members[&path].bytes, bytes);
            expected.insert(path);
        }
        Some(members[path].sha256.as_str())
    } else {
        for (destination, bytes, _) in CONTENT {
            let path = format!("game/{destination}");
            assert_eq!(members[&path].small.as_deref(), Some(bytes));
            expected.insert(path);
        }
        let jar = &members["game/server.jar"];
        assert_eq!(jar.sha256, pin["server_sha256"].as_str().unwrap());
        assert_eq!(jar.bytes, pin["server_bytes"].as_u64().unwrap());
        assert!(jar.small.is_none());
        expected.insert("game/server.jar".into());
        None
    };
    assert_eq!(members.keys().cloned().collect::<BTreeSet<_>>(), expected);
    for (name, member) in members {
        assert_eq!(
            member.mode,
            if scripts.contains(&name.as_str()) && name.ends_with(".sh") {
                0o755
            } else {
                0o644
            },
            "{name}"
        );
    }
    for name in scripts {
        let script = text(&members[name]);
        assert!(!script.contains("eula=true"));
        if name.ends_with(".bat") {
            assert!(!script.replace("\r\n", "").contains('\n'));
        }
        if name.starts_with("install_pack") {
            assert_eq!(script.contains("instance prepare"), managed);
            if let Some(hash) = release_hash {
                assert!(script.contains(&format!("--sha256 {hash} --layout game --side server")));
            }
            assert!(script.contains("EULA"));
        } else {
            assert!(script.contains("JAVA_HOME"));
            assert_eq!(script.contains("instance launch --server"), managed);
            assert_eq!(script.contains("nogui"), !name.starts_with("start_gui"));
            if managed {
                assert!(
                    script.find("install_pack").unwrap() < script.find("instance launch").unwrap()
                );
            } else {
                assert!(script.contains("-jar server.jar"));
            }
        }
    }
}
