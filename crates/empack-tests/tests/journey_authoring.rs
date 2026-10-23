//! Literal author documents and independent output expectations drive public commands.
use empack_tests::harness::{
    archive::zip_members,
    process::{TestProject, assert_dist_artifact_suffix, offline_command},
};
use std::{
    collections::BTreeSet,
    fs,
    io::{Cursor, Write},
};

const AUTHOR: &str = include_str!("../../../tests/fixtures/authoring/empack.yml");
const BINDINGS: &str = include_str!("../../../tests/fixtures/authoring/empack.bindings.yml");

fn write(project: &TestProject, name: &str, bytes: &[u8]) {
    let path = project.dir().join(name);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

fn build_and_check(project: &TestProject, jar: Option<&[u8]>, options: &[u8]) {
    offline_command(project.dir())
        .args(["--yes", "build"])
        .assert()
        .success();
    let client = zip_members(&assert_dist_artifact_suffix(
        project.dir(),
        "-prism-bundled.zip",
    ));
    let expected: BTreeSet<_> = [
        "instance.cfg",
        "mmc-pack.json",
        ".minecraft/mods/private-rules.jar",
        ".minecraft/options.txt",
        ".minecraft/config/menu/menu.json",
        ".minecraft/config/openloader/data/recipe.json",
        ".minecraft/config/review.properties",
    ]
    .into_iter()
    .filter(|name| jar.is_some() || !name.ends_with("mods/private-rules.jar"))
    .map(str::to_owned)
    .collect();
    assert_eq!(client.keys().cloned().collect::<BTreeSet<_>>(), expected);
    if let Some(bytes) = jar {
        assert_eq!(client[".minecraft/mods/private-rules.jar"], bytes);
    }
    assert_eq!(client[".minecraft/options.txt"], options);
    assert_eq!(
        client[".minecraft/config/menu/menu.json"],
        br#"{"theme":"client"}"#
    );
    assert_eq!(
        client[".minecraft/config/openloader/data/recipe.json"],
        br#"{"type":"fixture"}"#
    );
    assert_eq!(
        client[".minecraft/config/review.properties"],
        b"name=Verification Pack\nversion=1.0.0\n"
    );
    let runtime: serde_json::Value = serde_json::from_slice(&client["mmc-pack.json"]).unwrap();
    assert_eq!(runtime["formatVersion"], 1);
    assert_eq!(
        runtime["components"],
        serde_json::json!([
            {"uid":"net.minecraft", "version":"1.21.1", "important":true}
        ])
    );
    let exchange = zip_members(&assert_dist_artifact_suffix(project.dir(), ".mrpack"));
    let expected: BTreeSet<_> = [
        "modrinth.index.json",
        "overrides/mods/private-rules.jar",
        "overrides/options.txt",
        "overrides/config/menu/menu.json",
        "overrides/config/openloader/data/recipe.json",
        "client-overrides/config/menu/menu.json",
        "server-overrides/server.properties",
    ]
    .into_iter()
    .filter(|name| jar.is_some() || !name.ends_with("mods/private-rules.jar"))
    .map(str::to_owned)
    .collect();
    assert_eq!(exchange.keys().cloned().collect::<BTreeSet<_>>(), expected);
    if let Some(bytes) = jar {
        assert_eq!(exchange["overrides/mods/private-rules.jar"], bytes);
    }
    assert_eq!(exchange["overrides/options.txt"], options);
    assert_eq!(
        exchange["overrides/config/menu/menu.json"],
        br#"{"theme":"common"}"#
    );
    assert_eq!(
        exchange["client-overrides/config/menu/menu.json"],
        br#"{"theme":"client"}"#
    );
    assert_eq!(
        exchange["server-overrides/server.properties"],
        b"motd=Server fixture\n"
    );
    let index: serde_json::Value =
        serde_json::from_slice(&exchange["modrinth.index.json"]).unwrap();
    assert_eq!(index["name"], "Verification Pack");
    assert_eq!(index["versionId"], "1.0.0");
    assert_eq!(
        index["dependencies"],
        serde_json::json!({"minecraft":"1.21.1"})
    );
    assert_eq!(index["files"], serde_json::json!([]));
    assert_eq!(
        exchange["overrides/config/openloader/data/recipe.json"],
        br#"{"type":"fixture"}"#
    );
    let configuration = std::str::from_utf8(&client["instance.cfg"]).unwrap();
    for setting in [
        "ConfigVersion=1.2",
        "InstanceType=OneSix",
        "name=\"Verification Pack\"",
    ] {
        assert!(
            configuration.lines().any(|line| line == setting),
            "{configuration}"
        );
    }
}

#[test]
fn journey_literal_authoring_rebuilds_current_configuration_and_removes_only_the_dependency() {
    let project = TestProject::new();
    offline_command(project.dir())
        .args([
            "init",
            "--yes",
            "--modloader",
            "none",
            "--mc-version",
            "1.21.1",
        ])
        .assert()
        .success();
    write(&project, "empack.yml", AUTHOR.as_bytes());
    offline_command(project.dir())
        .args(["--yes", "sync"])
        .assert()
        .success();
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    archive
        .start_file("fabric.mod.json", zip::write::SimpleFileOptions::default())
        .unwrap();
    archive
        .write_all(br#"{"schemaVersion":1,"id":"fixture","version":"1"}"#)
        .unwrap();
    let jar = archive.finish().unwrap().into_inner();
    write(&project, "vendor/fixture.jar", &jar);
    offline_command(project.dir())
        .args(["--yes", "add", "vendor/fixture.jar"])
        .assert()
        .success();
    write(&project, "empack.bindings.yml", BINDINGS.as_bytes());
    for (name, bytes) in [
        ("pack/options.txt", &b"renderDistance:8\n"[..]),
        ("pack/config/menu/menu.json", &br#"{"theme":"common"}"#[..]),
        (
            "overrides/client/config/menu/menu.json",
            &br#"{"theme":"client"}"#[..],
        ),
        (
            "overrides/server/server.properties",
            &b"motd=Server fixture\n"[..],
        ),
        (
            "pack/config/openloader/data/recipe.json",
            &br#"{"type":"fixture"}"#[..],
        ),
        ("pack/private/notes.txt", &b"not distributed"[..]),
        (
            "templates/client/.minecraft/config/review.properties.template",
            &b"name={{NAME}}\nversion={{VERSION}}\n"[..],
        ),
    ] {
        write(&project, name, bytes);
    }
    offline_command(project.dir())
        .args(["manifest", "check"])
        .assert()
        .success();
    offline_command(project.dir())
        .args(["--yes", "sync", "--materialize"])
        .assert()
        .success();
    let author = fs::read(project.dir().join("empack.yml")).unwrap();
    let bindings = fs::read(project.dir().join("empack.bindings.yml")).unwrap();
    let lock = fs::read(project.dir().join("empack.lock")).unwrap();
    for _ in 0..2 {
        offline_command(project.dir())
            .args(["--yes", "sync"])
            .assert()
            .success();
        assert_eq!(fs::read(project.dir().join("empack.yml")).unwrap(), author);
        assert_eq!(
            fs::read(project.dir().join("empack.bindings.yml")).unwrap(),
            bindings
        );
        assert_eq!(fs::read(project.dir().join("empack.lock")).unwrap(), lock);
    }
    assert!(String::from_utf8_lossy(&author).starts_with("# An author-owned note"));
    build_and_check(&project, Some(&jar), b"renderDistance:8\n");
    write(&project, "pack/options.txt", b"renderDistance:12\n");
    build_and_check(&project, Some(&jar), b"renderDistance:12\n");
    assert_eq!(fs::read(project.dir().join("empack.yml")).unwrap(), author);
    assert_eq!(fs::read(project.dir().join("empack.lock")).unwrap(), lock);
    offline_command(project.dir())
        .args(["--yes", "remove", "fixture"])
        .assert()
        .success();
    offline_command(project.dir())
        .args(["--yes", "sync"])
        .assert()
        .success();
    build_and_check(&project, None, b"renderDistance:12\n");
    assert!(!project.dir().join("pack/mods/private-rules.jar").exists());
    assert_eq!(
        fs::read(project.dir().join("vendor/fixture.jar")).unwrap(),
        jar
    );
    assert_eq!(
        fs::read(project.dir().join("pack/options.txt")).unwrap(),
        b"renderDistance:12\n"
    );
    assert_eq!(
        fs::read(project.dir().join("pack/private/notes.txt")).unwrap(),
        b"not distributed"
    );
    assert_eq!(
        fs::read(project.dir().join("overrides/server/server.properties")).unwrap(),
        b"motd=Server fixture\n"
    );
    assert_eq!(
        fs::read(
            project
                .dir()
                .join("templates/client/.minecraft/config/review.properties.template")
        )
        .unwrap(),
        b"name={{NAME}}\nversion={{VERSION}}\n"
    );
    assert!(
        fs::read_to_string(project.dir().join("empack.yml"))
            .unwrap()
            .starts_with("# An author-owned note")
    );
}
