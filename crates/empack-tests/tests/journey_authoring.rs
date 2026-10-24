//! Literal author documents and independent output expectations drive public commands.
use empack_tests::harness::{
    archive::zip_members,
    process::{TestProject, assert_dist_artifact_suffix, offline_command},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Cursor, Write},
    path::PathBuf,
};

#[path = "support/files.rs"]
mod files;

const SOURCE: &str = "vendor sources/fixture.jar";
const INSTALLED: &str = "pack/mods/private-rules.jar";

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

fn fixture_jar(version: &str) -> Vec<u8> {
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    archive
        .start_file("fabric.mod.json", zip::write::SimpleFileOptions::default())
        .unwrap();
    write!(
        archive,
        r#"{{"schemaVersion":1,"id":"fixture","version":"{version}"}}"#
    )
    .unwrap();
    archive.finish().unwrap().into_inner()
}

// Compare author inputs, materialized content and prior artifacts, excluding host caches/state.
fn author_snapshot(project: &TestProject) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut result = BTreeMap::new();
    for name in ["empack.yml", "empack.bindings.yml", "empack.lock"] {
        result.insert(
            PathBuf::from(name),
            fs::read(project.dir().join(name)).unwrap(),
        );
    }
    for name in ["vendor sources", "pack", "overrides", "templates", "dist"] {
        let root = project.dir().join(name);
        if root.exists() {
            for (path, bytes) in files::snapshot(&root) {
                result.insert(PathBuf::from(name).join(path), bytes);
            }
        }
    }
    result
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
    let jar = fixture_jar("1");
    write(&project, SOURCE, &jar);
    write(
        &project,
        "vendor sources/notes.txt",
        b"author-owned neighbor",
    );
    fs::create_dir(project.dir().join("vendor sources/empty directory")).unwrap();
    offline_command(project.dir())
        .args(["--yes", "add", SOURCE])
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
    // A changed author source cannot silently replace the accepted content.
    let updated = fixture_jar("2");
    write(&project, SOURCE, &updated);
    let before = author_snapshot(&project);
    offline_command(project.dir())
        .args(["--yes", "sync"])
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "Source differs from accepted lock observation",
        ));
    assert_eq!(author_snapshot(&project), before);
    offline_command(project.dir())
        .args(["--dry-run", "update", "fixture"])
        .assert()
        .success();
    assert_eq!(author_snapshot(&project), before);
    offline_command(project.dir())
        .args(["--yes", "update", "fixture"])
        .assert()
        .success();
    assert_eq!(fs::read(project.dir().join(SOURCE)).unwrap(), updated);
    assert_eq!(fs::read(project.dir().join(INSTALLED)).unwrap(), updated);
    assert_eq!(fs::read(project.dir().join("empack.yml")).unwrap(), author);
    assert_eq!(
        fs::read(project.dir().join("empack.bindings.yml")).unwrap(),
        bindings
    );
    assert_ne!(fs::read(project.dir().join("empack.lock")).unwrap(), lock);
    build_and_check(&project, Some(&updated), b"renderDistance:12\n");

    // Adoption records verified existing content; it must not install changed source bytes.
    let adopted = fixture_jar("3");
    write(&project, SOURCE, &adopted);
    let before = author_snapshot(&project);
    offline_command(project.dir())
        .args(["--yes", "adopt", "fixture"])
        .assert()
        .failure();
    assert_eq!(author_snapshot(&project), before);
    write(&project, INSTALLED, &adopted);
    let before = author_snapshot(&project);
    offline_command(project.dir())
        .args(["--yes", "adopt", "fixture"])
        .assert()
        .success();
    let mut after = author_snapshot(&project);
    let mut expected = before;
    assert_ne!(
        after.remove(&PathBuf::from("empack.lock")),
        expected.remove(&PathBuf::from("empack.lock"))
    );
    assert_eq!(
        after, expected,
        "adoption must change only the lock observation"
    );
    let stable = author_snapshot(&project);
    for _ in 0..2 {
        offline_command(project.dir())
            .args(["--yes", "sync"])
            .assert()
            .success();
        assert_eq!(author_snapshot(&project), stable);
    }
    build_and_check(&project, Some(&adopted), b"renderDistance:12\n");
    let before = author_snapshot(&project);
    offline_command(project.dir())
        .args(["--dry-run", "remove", "fixture"])
        .assert()
        .success();
    assert_eq!(author_snapshot(&project), before);
    // A destination replaced by a directory is not permission to remove its contents.
    fs::remove_file(project.dir().join(INSTALLED)).unwrap();
    fs::create_dir(project.dir().join(INSTALLED)).unwrap();
    write(
        &project,
        "pack/mods/private-rules.jar/notes.txt",
        b"keep occupied directory",
    );
    let occupied = author_snapshot(&project);
    offline_command(project.dir())
        .args(["--yes", "remove", "fixture"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("directory"));
    assert_eq!(author_snapshot(&project), occupied);
    fs::remove_file(project.dir().join("pack/mods/private-rules.jar/notes.txt")).unwrap();
    fs::remove_dir(project.dir().join(INSTALLED)).unwrap();
    write(&project, INSTALLED, &adopted);
    #[cfg(unix)]
    {
        // A linked placement ancestor must not authorize deleting files outside the project.
        let outside = tempfile::tempdir().unwrap();
        let before = author_snapshot(&project);
        fs::rename(project.dir().join("pack/mods"), outside.path().join("mods")).unwrap();
        fs::write(
            outside.path().join("notes.txt"),
            b"unrelated outside content",
        )
        .unwrap();
        std::os::unix::fs::symlink(outside.path().join("mods"), project.dir().join("pack/mods"))
            .unwrap();
        let retained = files::snapshot(outside.path());
        offline_command(project.dir())
            .args(["--yes", "remove", "fixture"])
            .assert()
            .failure();
        assert_eq!(files::snapshot(outside.path()), retained);
        assert!(
            fs::symlink_metadata(project.dir().join("pack/mods"))
                .unwrap()
                .file_type()
                .is_symlink()
        );
        fs::remove_file(project.dir().join("pack/mods")).unwrap();
        fs::rename(outside.path().join("mods"), project.dir().join("pack/mods")).unwrap();
        assert_eq!(author_snapshot(&project), before);
    }
    offline_command(project.dir())
        .args(["--yes", "remove", "fixture"])
        .assert()
        .success();
    offline_command(project.dir())
        .args(["--yes", "sync"])
        .assert()
        .success();
    build_and_check(&project, None, b"renderDistance:12\n");
    assert!(!project.dir().join(INSTALLED).exists());
    assert!(
        project
            .dir()
            .join("vendor sources/empty directory")
            .is_dir()
    );
    assert_eq!(
        fs::read(project.dir().join("vendor sources/notes.txt")).unwrap(),
        b"author-owned neighbor"
    );
    assert_eq!(fs::read(project.dir().join(SOURCE)).unwrap(), adopted);
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
