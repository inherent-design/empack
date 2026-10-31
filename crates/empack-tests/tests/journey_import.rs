//! Pinned source archives and independent expected inventories; no moving upstream inputs.
use empack_tests::harness::{
    archive::zip_members,
    process::{TestProject, assert_dist_artifact_suffix, offline_command},
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs, path::Path};

fn corpus(id: &str) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/import-corpus");
    let manifest: Value =
        serde_json::from_slice(&fs::read(root.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest["schema"], 1);
    assert_eq!(manifest["license"], "Apache-2.0");
    assert_eq!(
        manifest["cases"]
            .as_array()
            .unwrap()
            .iter()
            .map(|case| case["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        [
            "modrinth-layered",
            "curseforge-overrides",
            "traversal-refused"
        ]
    );
    let case = manifest["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["id"] == id)
        .unwrap();
    let source = root.join(case["archive"].as_str().unwrap());
    let bytes = fs::read(&source).unwrap();
    assert_eq!(
        Sha256::digest(&bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>(),
        case["sha256"].as_str().unwrap()
    );
    let project = TestProject::new();
    let archive = project.dir().join(source.file_name().unwrap());
    fs::write(&archive, &bytes).unwrap();
    let imported = project.dir().join("imported-pack");
    let arguments = [
        "--yes",
        "init",
        "--from",
        archive.to_str().unwrap(),
        "--import-optional-default",
        "true",
        "--exclude-auxiliary",
        "imported-pack",
    ];
    if case["expected"]["refused"] == true {
        offline_command(project.dir())
            .args(arguments)
            .assert()
            .failure();
        assert!(!imported.exists());
        assert!(!project.dir().join("escaped.txt").exists());
        assert_eq!(fs::read(&archive).unwrap(), bytes);
        return;
    }
    offline_command(project.dir())
        .arg("--dry-run")
        .args(arguments)
        .assert()
        .success();
    assert!(!imported.exists());
    offline_command(project.dir())
        .args(arguments)
        .assert()
        .success();
    let config = fs::read(imported.join("empack.yml")).unwrap();
    assert!(String::from_utf8_lossy(&config).contains(case["expected"]["name"].as_str().unwrap()));
    let decoded = empack_lib::testing::read_authoring(&imported.join("empack.yml")).unwrap();
    assert_eq!(
        decoded.intent().runtime.loader,
        empack_core::model::LoaderKind::Fabric
    );
    assert_eq!(
        decoded
            .intent()
            .runtime
            .loader_version
            .as_ref()
            .unwrap()
            .as_str(),
        "0.15.11"
    );
    let locked = fs::read(imported.join("empack.lock")).unwrap();
    for _ in 0..2 {
        offline_command(&imported)
            .args(["--yes", "sync"])
            .assert()
            .success();
        assert_eq!(fs::read(imported.join("empack.yml")).unwrap(), config);
        assert_eq!(fs::read(imported.join("empack.lock")).unwrap(), locked);
    }
    offline_command(&imported)
        .args([
            "--yes",
            "build",
            "modrinth",
            "--allow-optional-metadata-loss",
        ])
        .assert()
        .success();
    let mut members = zip_members(&assert_dist_artifact_suffix(
        &imported,
        &format!(
            "{}-{}.mrpack",
            case["expected"]["name"].as_str().unwrap(),
            case["expected"]["version"].as_str().unwrap()
        ),
    ));
    let exported: Value =
        serde_json::from_slice(&members.remove("modrinth.index.json").unwrap()).unwrap();
    let expected = &case["expected"];
    assert_eq!(exported["name"], expected["name"]);
    assert_eq!(exported["versionId"], expected["version"]);
    assert_eq!(exported["dependencies"], expected["dependencies"]);
    assert_eq!(exported["formatVersion"], 1);
    assert_eq!(exported["game"], "minecraft");
    assert_eq!(exported["files"], serde_json::json!([]));
    let expected_members: BTreeMap<_, _> = expected["files"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(name, text)| (name.clone(), text.as_str().unwrap().as_bytes().to_vec()))
        .collect();
    assert_eq!(members, expected_members);
    assert_eq!(fs::read(imported.join("empack.yml")).unwrap(), config);
    assert_eq!(fs::read(imported.join("empack.lock")).unwrap(), locked);
    assert_eq!(fs::read(&archive).unwrap(), bytes);
}

#[test]
fn journey_local_mrpack_import_reexport() {
    corpus("modrinth-layered");
}

#[test]
fn journey_pinned_curseforge_import_reexport_preserves_inventory() {
    corpus("curseforge-overrides");
}

#[test]
fn journey_pinned_traversal_import_refuses_publication() {
    corpus("traversal-refused");
}

#[test]
fn journey_import_exclusion_requires_consent_and_preserves_declared_runtime() {
    let project = TestProject::new();
    let archive = project.dir().join("auxiliary.mrpack");
    empack_tests::fixtures::write_zip(
        &archive,
        &[
            ("modrinth.index.json", br#"{"formatVersion":1,"game":"minecraft","name":"Auxiliary Pack","versionId":"1.0.0","files":[],"dependencies":{"minecraft":"1.21.1"}}"#),
            ("overrides/config/example.toml", b"# author source\nenabled=true\n"),
            ("readme.txt", b"Author notes that must not be silently discarded"),
        ],
    ).unwrap();
    let archive_bytes = fs::read(&archive).unwrap();
    let neighbors = [
        ("neighbor/empack.yml", b"author document".as_slice()),
        ("neighbor/empack.lock", b"prior resolution".as_slice()),
        ("neighbor/dist/prior.zip", b"prior artifact".as_slice()),
        (
            "neighbor/worlds/playtest/level.dat",
            b"player world".as_slice(),
        ),
    ];
    for (name, bytes) in neighbors {
        let path = project.dir().join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
    let unchanged = || {
        assert_eq!(fs::read(&archive).unwrap(), archive_bytes);
        for (name, bytes) in neighbors {
            assert_eq!(fs::read(project.dir().join(name)).unwrap(), bytes, "{name}");
        }
    };
    let imported = project.dir().join("imported");
    for (extra, diagnostic) in [
        (vec![], "--exclude-auxiliary"),
        (
            vec!["--exclude-auxiliary", "--mc-version", "1.20.1"],
            "--mc-version differs",
        ),
    ] {
        offline_command(project.dir())
            .args(["--yes", "init", "--from", archive.to_str().unwrap()])
            .args(extra)
            .arg("imported")
            .assert()
            .failure()
            .stderr(predicates::str::contains(diagnostic));
        assert!(!imported.exists());
        unchanged();
    }
    offline_command(project.dir())
        .args([
            "--yes",
            "init",
            "--from",
            archive.to_str().unwrap(),
            "--exclude-auxiliary",
            "imported",
        ])
        .assert()
        .success();
    let author = fs::read(imported.join("empack.yml")).unwrap();
    let lock = fs::read(imported.join("empack.lock")).unwrap();
    let decoded = empack_lib::testing::read_authoring(&imported.join("empack.yml")).unwrap();
    assert_eq!(decoded.intent().runtime.minecraft.as_str(), "1.21.1");
    assert_eq!(decoded.intent().roots.len(), 1);
    assert!(!imported.join("readme.txt").exists());
    assert_eq!(
        fs::read(imported.join("overrides/common/config/example.toml")).unwrap(),
        b"# author source\nenabled=true\n"
    );
    offline_command(&imported)
        .args(["--yes", "build", "modrinth"])
        .assert()
        .success();
    let mut members = zip_members(&assert_dist_artifact_suffix(&imported, ".mrpack"));
    let index: Value =
        serde_json::from_slice(&members.remove("modrinth.index.json").unwrap()).unwrap();
    assert_eq!(
        index["dependencies"],
        serde_json::json!({"minecraft":"1.21.1"})
    );
    assert_eq!(index["files"], serde_json::json!([]));
    assert_eq!(
        members,
        BTreeMap::from([(
            "overrides/config/example.toml".into(),
            b"# author source\nenabled=true\n".to_vec()
        )])
    );
    assert_eq!(fs::read(imported.join("empack.yml")).unwrap(), author);
    assert_eq!(fs::read(imported.join("empack.lock")).unwrap(), lock);
    unchanged();
}
