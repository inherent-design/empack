//! Pinned source archives and independent expected inventories; no moving upstream inputs.
use empack_tests::harness::process::{TestProject, assert_dist_artifact_suffix, offline_command};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs, io::Read, path::Path};

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
    let mut output = zip::ZipArchive::new(
        fs::File::open(assert_dist_artifact_suffix(
            &imported,
            &format!(
                "{}-{}.mrpack",
                case["expected"]["name"].as_str().unwrap(),
                case["expected"]["version"].as_str().unwrap()
            ),
        ))
        .unwrap(),
    )
    .unwrap();
    let mut members = BTreeMap::new();
    for index in 0..output.len() {
        let mut entry = output.by_index(index).unwrap();
        if entry.is_dir() {
            continue;
        }
        let mut value = Vec::new();
        entry.read_to_end(&mut value).unwrap();
        assert!(
            members
                .insert(entry.name().unwrap().into_owned(), value)
                .is_none(),
            "duplicate exported member"
        );
    }
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
