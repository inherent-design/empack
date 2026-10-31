//! Immutable upstream input; no latest-version lookup or automatic fixture refresh.
use empack_tests::harness::{
    archive::zip_members,
    process::{TestProject, assert_dist_artifact_suffix, empack_cmd},
};
use serde_json::Value;
use sha2::{Digest, Sha256, Sha512};
use std::{collections::BTreeMap, fs, io::Read, path::Path, time::Duration};

fn hex(bytes: impl AsRef<[u8]>) -> String {
    bytes
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn run(root: &Path, args: &[&str]) {
    assert_cmd::Command::from_std(empack_cmd(root))
        .arg("--yes")
        .args(args)
        .timeout(Duration::from_secs(600))
        .assert()
        .success();
}

fn canonical_index(bytes: &[u8]) -> Value {
    let mut index: Value = serde_json::from_slice(bytes).unwrap();
    index["files"]
        .as_array_mut()
        .unwrap()
        .sort_by(|a, b| a["path"].as_str().cmp(&b["path"].as_str()));
    index
}

fn override_inventory(members: &BTreeMap<String, Vec<u8>>) -> Value {
    let entries: serde_json::Map<_, _> = members
        .iter()
        .map(|(name, bytes)| {
            (
                name.clone(),
                serde_json::json!({"size": bytes.len(), "sha256": hex(Sha256::digest(bytes))}),
            )
        })
        .collect();
    entries.into()
}

#[test]
#[ignore = "pinned Fabulously Optimized archive and 50 exact CDN files; live network required"]
fn live_pinned_modrinth_import_converges_and_exports_complete_inventory() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/import-corpus/fabulously-optimized-6.5.0.json"
    ))
    .unwrap();
    let project = TestProject::new();
    let source = &fixture["source"];
    let size = source["size"].as_u64().unwrap();
    assert!(size < 1 << 20);
    let response = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .unwrap()
        .get(source["url"].as_str().unwrap())
        .send()
        .unwrap()
        .error_for_status()
        .unwrap();
    let mut bytes = Vec::new();
    response.take(size + 1).read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes.len() as u64, size);
    assert_eq!(hex(Sha256::digest(&bytes)), source["sha256"]);
    assert_eq!(hex(Sha512::digest(&bytes)), source["sha512"]);
    let archive = project.dir().join("input.mrpack");
    fs::write(&archive, &bytes).unwrap();
    let mut input = zip_members(&archive);
    assert_eq!(
        canonical_index(&input.remove("modrinth.index.json").unwrap()),
        fixture["index"]
    );
    assert_eq!(override_inventory(&input), fixture["overrides"]);
    assert_eq!(input.len(), 63);
    assert_eq!(fixture["index"]["files"].as_array().unwrap().len(), 50);

    // A new isolated project/cache must acquire the pinned content. There are no
    // optional or auxiliary members in this input, so no lossy consent flags.
    run(
        project.dir(),
        &["init", "--from", archive.to_str().unwrap(), "imported"],
    );
    let imported = project.dir().join("imported");
    let author = fs::read(imported.join("empack.yml")).unwrap();
    let lock = fs::read(imported.join("empack.lock")).unwrap();
    for _ in 0..2 {
        run(&imported, &["sync"]);
        assert_eq!(fs::read(imported.join("empack.yml")).unwrap(), author);
        assert_eq!(fs::read(imported.join("empack.lock")).unwrap(), lock);
    }
    run(&imported, &["build", "modrinth"]);
    let exported = assert_dist_artifact_suffix(&imported, ".mrpack");
    let mut output = zip_members(&exported);
    assert_eq!(
        canonical_index(&output.remove("modrinth.index.json").unwrap()),
        fixture["index"]
    );
    assert_eq!(override_inventory(&output), fixture["overrides"]);
    assert_eq!(output, input);

    // Independently hash acquired bytes, rather than trusting copied assertions
    // in the re-exported index. Bound each member and the complete decoded ZIP.
    run(&imported, &["build", "prism", "--delivery", "bundled"]);
    let bundled = assert_dist_artifact_suffix(&imported, "prism-bundled.zip");
    let mut zip = zip::ZipArchive::new(fs::File::open(&bundled).unwrap()).unwrap();
    assert!(zip.len() < 256);
    let mut actual = BTreeMap::new();
    let mut total = 0;
    for i in 0..zip.len() {
        let entry = zip.by_index(i).unwrap();
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().unwrap().into_owned();
        let size = entry.size();
        assert!(size <= 16 << 20);
        total += size;
        assert!(total <= 64 << 20);
        let mut bytes = Vec::new();
        entry.take(size + 1).read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes.len() as u64, size);
        assert!(actual.insert(name, bytes).is_none());
    }
    for file in fixture["index"]["files"].as_array().unwrap() {
        let path = format!(".minecraft/{}", file["path"].as_str().unwrap());
        let bytes = actual
            .remove(&path)
            .unwrap_or_else(|| panic!("missing {path}"));
        assert_eq!(bytes.len() as u64, file["fileSize"]);
        assert_eq!(
            hex(Sha512::digest(&bytes)),
            file["hashes"]["sha512"],
            "{path}"
        );
    }
    for (path, bytes) in input {
        let destination = format!(".minecraft/{}", path.strip_prefix("overrides/").unwrap());
        assert_eq!(actual.remove(&destination).unwrap(), bytes, "{destination}");
    }
    // Only the two launcher-generated metadata members may remain.
    assert_eq!(
        actual.keys().map(String::as_str).collect::<Vec<_>>(),
        ["instance.cfg", "mmc-pack.json"]
    );
    let runtime: Value = serde_json::from_slice(&actual["mmc-pack.json"]).unwrap();
    assert_eq!(runtime["components"].as_array().unwrap().len(), 2);
    let components: BTreeMap<_, _> = runtime["components"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| (v["uid"].as_str().unwrap(), v["version"].as_str().unwrap()))
        .collect();
    assert_eq!(
        components,
        BTreeMap::from([
            ("net.fabricmc.fabric-loader", "0.19.3"),
            ("net.minecraft", "1.21.1")
        ])
    );
    assert_eq!(fs::read(imported.join("empack.yml")).unwrap(), author);
    assert_eq!(fs::read(imported.join("empack.lock")).unwrap(), lock);
    // The version stays 6.5.0: content freshness must not depend on a version bump.
    let config = imported.join("overrides/common/config/modmenu.json");
    let mut settings: Value = serde_json::from_slice(&fs::read(&config).unwrap()).unwrap();
    assert_eq!(settings["sorting"], "ascending");
    settings["sorting"] = "descending".into();
    let edited = serde_json::to_vec(&settings).unwrap();
    fs::write(&config, &edited).unwrap();
    let prior_bundled = fs::read(&bundled).unwrap();
    let prior_export = fs::read(&exported).unwrap();
    assert_cmd::Command::from_std(empack_cmd(&imported))
        .args(["--yes", "sync"])
        .timeout(Duration::from_secs(600))
        .assert()
        .failure()
        .stderr(predicates::str::contains("Source size differs from lock"));
    assert_eq!(fs::read(imported.join("empack.lock")).unwrap(), lock);
    assert_eq!(fs::read(&exported).unwrap(), prior_export);
    assert_eq!(fs::read(&bundled).unwrap(), prior_bundled);
    // This imported source is also its managed destination. Adoption is
    // the explicit acceptance step; sync must not silently bless external edits.
    run(&imported, &["adopt", "CommonOverride:config/modmenu.json"]);
    run(&imported, &["sync"]);
    assert_eq!(fs::read(&exported).unwrap(), prior_export);
    run(&imported, &["build", "modrinth"]);
    let mut refreshed = zip_members(&exported);
    assert_eq!(
        canonical_index(&refreshed.remove("modrinth.index.json").unwrap()),
        fixture["index"]
    );
    output.insert("overrides/config/modmenu.json".into(), edited.clone());
    assert_eq!(refreshed, output);
    assert_eq!(fs::read(&config).unwrap(), edited);
    assert_eq!(fs::read(&bundled).unwrap(), prior_bundled);
    assert_eq!(fs::read(imported.join("empack.yml")).unwrap(), author);
    assert_eq!(fs::read(archive).unwrap(), bytes);
}
