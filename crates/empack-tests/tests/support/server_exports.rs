//! Public server builds with an independently pinned runtime inventory.
use empack_tests::harness::{
    archive::{Member, distribution_inventory},
    process::{TestProject, assert_dist_artifact_suffix},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    time::Duration,
};

#[path = "files.rs"]
mod files;
const AUTHOR: &str = "schema: 5\npack: {id: server.fixture, name: Server fixture, version: '1'}\nruntime: {minecraft: '1.21.1', loader: vanilla, java: 21}\ndependencies: {}\n";
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
    put("empack.yml", AUTHOR.as_bytes());
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
    put("templates/client/client-only.txt", b"excluded");
    put("dist/prior.zip", b"prior artifact");
    let command = || {
        let mut command = assert_cmd::Command::from_std(project.cmd());
        command.timeout(Duration::from_secs(240));
        command
    };
    command().args(["--yes", "sync"]).assert().success();
    let author = || {
        let mut values = BTreeMap::new();
        for name in ["empack.yml", "empack.lock"] {
            values.insert(name.into(), fs::read(project.dir().join(name)).unwrap());
        }
        for name in ["pack", "overrides", "templates"] {
            for (path, bytes) in files::snapshot(&project.dir().join(name)) {
                values.insert(std::path::PathBuf::from(name).join(path), bytes);
            }
        }
        values
    };
    let before = author();
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
    expected.extend(["banner.bin".into(), "notes.txt".into()]);
    assert_eq!(
        members["banner.bin"].small.as_deref(),
        Some(&[0, 255, 1][..])
    );
    assert_eq!(text(&members["notes.txt"]), "Server fixture / 1\n");
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
