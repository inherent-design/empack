//! Immutable upstream input; no latest-version lookup or automatic fixture refresh.
use empack_tests::harness::{
    archive::zip_members_with_limit,
    process::{TestProject, assert_dist_artifact_suffix, empack_cmd, read_project},
    workspace::snapshot_excluding,
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

// Compare the documented meaning of an absent optional env object. Keep the
// raw source-index comparison separate; no explicit value or other field is lost.
fn effective_index(mut index: Value) -> Value {
    for file in index["files"].as_array_mut().unwrap() {
        if file.get("env").is_none() {
            file["env"] = serde_json::json!({"client": "required", "server": "required"});
        }
    }
    index
}

#[test]
fn fixture_index_defaults_preserve_explicit_sides_and_other_metadata() {
    let index = serde_json::json!({
        "files": [{"path": "mods/a.jar", "fileSize": 7,
            "hashes": {"sha512": "original"}, "downloads": ["first", "second"]}],
        "extension": "keep"
    });
    let mut expected = index.clone();
    expected["files"][0]["env"] = serde_json::json!({"client": "required", "server": "required"});
    assert_eq!(effective_index(index.clone()), expected);
    for explicit in [
        serde_json::json!({"client": "optional", "server": "unsupported"}),
        serde_json::json!({"client": "required"}),
        Value::Null,
    ] {
        let mut declared = index.clone();
        declared["files"][0]["env"] = explicit;
        assert_eq!(effective_index(declared.clone()), declared);
    }
    let mut changed = expected.clone();
    changed["files"][0]["hashes"]["sha512"] = "changed".into();
    assert_ne!(effective_index(changed), effective_index(index));
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
    verify_pinned_pack(
        fixture,
        (50, 63, 256),
        (16 << 20, 64 << 20),
        [
            ("net.fabricmc.fabric-loader", "0.19.3"),
            ("net.minecraft", "1.21.1"),
        ],
        "config/modmenu.json",
        |bytes| {
            let mut settings: Value = serde_json::from_slice(bytes).unwrap();
            assert_eq!(settings["sorting"], "ascending");
            settings["sorting"] = "descending".into();
            serde_json::to_vec(&settings).unwrap()
        },
    );
}

#[test]
#[ignore = "pinned Adrenaline Quilt archive and 17 exact CDN files; live network required"]
fn live_pinned_quilt_import_retains_client_sides_and_complete_inventory() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/import-corpus/adrenaline-1.22.0-quilt.json"
    ))
    .unwrap();
    assert_eq!(
        fixture["index"]["files"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|file| file["env"]["server"] == "unsupported")
            .count(),
        9
    );
    verify_pinned_pack(
        fixture,
        (17, 10, 256),
        (16 << 20, 64 << 20),
        [
            ("org.quiltmc.quilt-loader", "0.26.0"),
            ("net.minecraft", "1.20.1"),
        ],
        "config/yosbr/config/ferritecore.mixin.properties",
        |bytes| {
            assert_eq!(bytes, b"useSmallThreadingDetector = true");
            b"useSmallThreadingDetector = false".to_vec()
        },
    );
}

#[test]
#[ignore = "pinned Universalized Optimized NeoForge archive and 13 exact CDN files; live network required"]
fn live_pinned_neoforge_import_preserves_local_jar_and_complete_inventory() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/import-corpus/universalized-optimized-1.21.1-neoforge.json"
    ))
    .unwrap();
    assert_eq!(
        fixture["overrides"]["overrides/mods/threadtweak-1.21.1-NeoForge-1.0.0.jar"]["size"],
        20148
    );
    verify_pinned_pack(
        fixture,
        (13, 14, 256),
        (16 << 20, 64 << 20),
        [("net.neoforged", "21.1.194"), ("net.minecraft", "1.21.1")],
        "config/ferritecore.mixin.properties",
        |bytes| {
            assert_eq!(bytes, b"useSmallThreadingDetector = true\n");
            b"useSmallThreadingDetector = false\n".to_vec()
        },
    );
}

#[test]
#[ignore = "pinned Rapid Optimization Forge 1.7.10 archive and 5 exact CDN files; live network required"]
fn live_pinned_legacy_forge_import_preserves_default_sides_and_complete_inventory() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/import-corpus/rapid-optimization-7.10an-2.1.1.json"
    ))
    .unwrap();
    assert!(
        fixture["index"]["files"]
            .as_array()
            .unwrap()
            .iter()
            .all(|file| file.get("env").is_none())
    );
    verify_pinned_pack(
        fixture,
        (5, 31, 256),
        (16 << 20, 64 << 20),
        [
            ("net.minecraftforge", "10.13.4.1614"),
            ("net.minecraft", "1.7.10"),
        ],
        "config/gtnhlibEarly.properties",
        |bytes| {
            let text = std::str::from_utf8(bytes).unwrap();
            assert_eq!(text.matches("enableFontRendererMixin=true").count(), 1);
            text.replacen(
                "enableFontRendererMixin=true",
                "enableFontRendererMixin=false",
                1,
            )
            .into_bytes()
        },
    );
}

#[test]
#[ignore = "pinned Horror OneBlock world archive and 44 exact CDN files (253 MB); live network required"]
fn live_pinned_world_import_preserves_saved_world_and_complete_inventory() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/import-corpus/horror-oneblock-1.1.1-neoforge.json"
    ))
    .unwrap();
    let world = "overrides/saves/Horror OneBlock/";
    let overrides = fixture["overrides"].as_object().unwrap();
    assert_eq!(
        overrides
            .keys()
            .filter(|name| name.starts_with(world))
            .count(),
        11
    );
    assert_eq!(overrides[&format!("{world}level.dat")]["size"], 1642);
    verify_pinned_pack(
        fixture,
        (44, 100, 256),
        // This selected pack includes an 84,811,632-byte AmbientSounds JAR.
        // Keep its known workload separate from the four smaller fixtures.
        (96 << 20, 256 << 20),
        [("net.neoforged", "21.1.235"), ("net.minecraft", "1.21.1")],
        "saves/Horror OneBlock/serverconfig/readme.txt",
        |bytes| {
            assert!(bytes.starts_with(b"Any server configs put in this folder"));
            let mut edited = bytes.to_vec();
            edited.extend_from_slice(b"Author note: retain this world's server configuration.\n");
            edited
        },
    );
}

#[test]
#[ignore = "pinned Create Sky layered archive and 163 exact remote files; live network required"]
fn live_pinned_layered_import_preserves_common_client_server_inventory() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/import-corpus/create-sky-0.1.30-layered.json"
    ))
    .unwrap();
    for (prefix, count) in [
        ("overrides/", 245),
        ("client-overrides/", 24),
        ("server-overrides/", 5),
    ] {
        assert_eq!(
            fixture["overrides"]
                .as_object()
                .unwrap()
                .keys()
                .filter(|path| path.starts_with(prefix))
                .count(),
            count
        );
    }
    assert_eq!(
        fixture["index"]["files"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|file| file["env"]["client"] == "unsupported")
            .count(),
        1
    );
    verify_pinned_pack(
        fixture,
        (163, 274, 512),
        (32 << 20, 320 << 20),
        [("net.minecraft", "1.21.1"), ("net.neoforged", "21.1.264")],
        "config/create_sky_compat-common.toml",
        |bytes| {
            let mut edited = bytes.to_vec();
            edited.extend_from_slice(b"# Explicitly adopted verification edit\n");
            edited
        },
    );
}

fn verify_pinned_pack(
    fixture: Value,
    (files, overrides, archive_entries): (usize, usize, usize),
    (member_limit, total_limit): (u64, u64),
    expected_components: [(&str, &str); 2],
    edited_override: &str,
    edit: impl FnOnce(&[u8]) -> Vec<u8>,
) {
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
    let mut input = zip_members_with_limit(&archive, archive_entries);
    assert_eq!(
        canonical_index(&input.remove("modrinth.index.json").unwrap()),
        fixture["index"]
    );
    assert_eq!(override_inventory(&input), fixture["overrides"]);
    assert_eq!(input.len(), overrides);
    assert_eq!(fixture["index"]["files"].as_array().unwrap().len(), files);

    // A new isolated project/cache must acquire the pinned content. There are no
    // optional or auxiliary members in this input, so no lossy consent flags.
    run(
        project.dir(),
        &["init", "--from", archive.to_str().unwrap(), "imported"],
    );
    let imported = project.dir().join("imported");
    fs::create_dir_all(imported.join("author notes/empty")).unwrap();
    fs::write(imported.join("author notes/keep.txt"), b"author decision").unwrap();
    let observe = || {
        snapshot_excluding(
            &imported,
            &["dist", ".host-state", ".empack-cache", ".harness-home"],
        )
    };
    let author = fs::read(imported.join("empack.yml")).unwrap();
    let lock = fs::read(imported.join("empack.lock")).unwrap();
    for iteration in 0..2 {
        // The first sync may materialize references. The next must converge on
        // the whole author tree, not merely unchanged manifest and lock bytes.
        let before = (iteration == 1).then(observe);
        run(&imported, &["sync"]);
        assert_eq!(fs::read(imported.join("empack.yml")).unwrap(), author);
        assert_eq!(fs::read(imported.join("empack.lock")).unwrap(), lock);
        if let Some(before) = before {
            assert_eq!(observe(), before, "repeated sync changed author content");
        }
    }
    // Check both sides against the independently pinned raw index, including
    // server-only references. A client bundle alone cannot establish server policy.
    let resolved = read_project(&imported);
    for declared in fixture["index"]["files"].as_array().unwrap() {
        let path = declared["path"].as_str().unwrap();
        let placements: Vec<_> = resolved
            .lock()
            .dependencies
            .values()
            .flat_map(|dependency| dependency.files.as_slice())
            .flat_map(|file| file.placements.as_slice())
            .filter(|placement| placement.destination.relative().as_str() == path)
            .collect();
        assert_eq!(placements.len(), 1, "{path}");
        for (side, actual) in [
            ("client", &placements[0].requirements.client),
            ("server", &placements[0].requirements.server),
        ] {
            let expected = match declared
                .get("env")
                .map(|env| env[side].as_str().unwrap())
                .unwrap_or("required")
            {
                "required" => empack_core::requirements::Requirement::Required,
                "unsupported" => empack_core::requirements::Requirement::Unsupported,
                _ => panic!("this pinned scenario does not resolve optional files"),
            };
            assert_eq!(actual, &expected, "{path} {side}");
        }
    }
    run(&imported, &["build", "modrinth"]);
    let exported = assert_dist_artifact_suffix(&imported, ".mrpack");
    let mut output = zip_members_with_limit(&exported, archive_entries);
    assert_eq!(
        effective_index(canonical_index(
            &output.remove("modrinth.index.json").unwrap()
        )),
        effective_index(fixture["index"].clone())
    );
    assert_eq!(override_inventory(&output), fixture["overrides"]);
    assert_eq!(output, input);

    // Independently hash acquired bytes, rather than trusting copied assertions
    // in the re-exported index. Bound each member and the complete decoded ZIP.
    run(&imported, &["build", "prism", "--delivery", "bundled"]);
    let bundled = assert_dist_artifact_suffix(&imported, "prism-bundled.zip");
    let mut zip = zip::ZipArchive::new(fs::File::open(&bundled).unwrap()).unwrap();
    assert!(zip.len() < archive_entries);
    let mut actual = BTreeMap::new();
    let mut total = 0;
    for i in 0..zip.len() {
        let entry = zip.by_index(i).unwrap();
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().unwrap().into_owned();
        let size = entry.size();
        assert!(size <= member_limit);
        total += size;
        assert!(total <= total_limit);
        let mut bytes = Vec::new();
        entry.take(size + 1).read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes.len() as u64, size);
        assert!(actual.insert(name, bytes).is_none());
    }
    for file in fixture["index"]["files"].as_array().unwrap() {
        let path = format!(".minecraft/{}", file["path"].as_str().unwrap());
        if file
            .get("env")
            .is_some_and(|env| env["client"] == "unsupported")
        {
            assert!(!actual.contains_key(&path), "server-only reference: {path}");
            continue;
        }
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
    // Independent consumer projection: common files, then client files. Retain
    // all three source layers in the mrpack round-trip above. No server layer
    // may leak into the complete client inventory checked below.
    let mut client_overrides = BTreeMap::new();
    for prefix in ["overrides/", "client-overrides/"] {
        for (path, bytes) in &input {
            if let Some(relative) = path.strip_prefix(prefix) {
                client_overrides.insert(format!(".minecraft/{relative}"), bytes);
            }
        }
    }
    for (destination, bytes) in client_overrides {
        assert_eq!(
            actual.remove(&destination).unwrap(),
            *bytes,
            "{destination}"
        );
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
    assert_eq!(components, BTreeMap::from(expected_components));
    assert_eq!(fs::read(imported.join("empack.yml")).unwrap(), author);
    assert_eq!(fs::read(imported.join("empack.lock")).unwrap(), lock);
    // Keep the authored version: content freshness must not require a version bump.
    let config = imported.join("overrides/common").join(edited_override);
    let original = fs::read(&config).unwrap();
    let edited = edit(&original);
    assert_ne!(edited, original);
    fs::write(&config, &edited).unwrap();
    let prior_bundled = fs::read(&bundled).unwrap();
    let prior_export = fs::read(&exported).unwrap();
    let edited_author = observe();
    assert_cmd::Command::from_std(empack_cmd(&imported))
        .args(["--yes", "sync"])
        .timeout(Duration::from_secs(600))
        .assert()
        .failure()
        .stderr(predicates::str::contains("Source size differs from lock"));
    assert_eq!(
        observe(),
        edited_author,
        "failed sync changed author content"
    );
    assert_eq!(fs::read(imported.join("empack.lock")).unwrap(), lock);
    assert_eq!(fs::read(&exported).unwrap(), prior_export);
    assert_eq!(fs::read(&bundled).unwrap(), prior_bundled);
    // This imported source is also its managed destination. Adoption is
    // the explicit acceptance step; sync must not silently bless external edits.
    run(
        &imported,
        &["adopt", &format!("CommonOverride:{edited_override}")],
    );
    run(&imported, &["sync"]);
    assert_eq!(fs::read(&exported).unwrap(), prior_export);
    run(&imported, &["build", "modrinth"]);
    let mut refreshed = zip_members_with_limit(&exported, archive_entries);
    assert_eq!(
        effective_index(canonical_index(
            &refreshed.remove("modrinth.index.json").unwrap()
        )),
        effective_index(fixture["index"].clone())
    );
    output.insert(format!("overrides/{edited_override}"), edited.clone());
    assert_eq!(refreshed, output);
    assert_eq!(fs::read(&config).unwrap(), edited);
    assert_eq!(fs::read(&bundled).unwrap(), prior_bundled);
    assert_eq!(fs::read(imported.join("empack.yml")).unwrap(), author);
    assert_eq!(fs::read(archive).unwrap(), bytes);
    // Adoption changes the accepted lock; it does not authorize changing other
    // captured author files, directory structure or their portable attributes.
    let accepted_author = observe();
    assert!(accepted_author.files.keys().eq(edited_author.files.keys()));
    assert!(
        accepted_author
            .entries
            .keys()
            .eq(edited_author.entries.keys())
    );
    for (path, original) in edited_author.files {
        if path != Path::new("empack.lock") {
            assert_eq!(
                accepted_author.files.get(&path),
                Some(&original),
                "{path:?}"
            );
        }
    }
    for (path, original) in edited_author.entries {
        if path != Path::new("empack.lock") {
            assert_eq!(
                accepted_author.entries.get(&path),
                Some(&original),
                "{path:?}"
            );
        }
    }
}
