use std::time::Instant;

use empack_tests::e2e::{
    TestProject, assert_dist_artifact_suffix, count_pw_toml_files, empack_cmd, read_project,
};

const LIVE_IMPORTED_MRPACK_BUILD_TIMEOUT_SECS: &str = "600";

/// Live CurseForge URL import: Cobblemon Updated (~30 mods).
///
/// Requires live provider access; strict mode enforces external prerequisites.
/// Runtime: 30-120s depending on network conditions.
#[test]
fn e2e_init_from_cobblemon_updated() {
    empack_tests::skip_if_no_cf_key!();

    let project = TestProject::new();
    let start = Instant::now();
    let output = empack_cmd(project.dir())
        .args([
            "init",
            "--from",
            "https://www.curseforge.com/minecraft/modpacks/cobblemon-updated",
            "--yes",
            "cobblemon",
        ])
        .output()
        .expect("spawn failed");
    let elapsed = start.elapsed();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "cobblemon import failed:\nstdout: {stdout}\nstderr: {stderr}",
    );

    project.assert_exists("cobblemon/empack.yml");
    project.assert_exists("cobblemon/pack/pack.toml");

    let pw_count = count_pw_toml_files(&project.dir().join("cobblemon/pack"));
    assert!(
        pw_count >= 5,
        "expected at least 5 mod .pw.toml files, found {pw_count}"
    );

    eprintln!(
        "cobblemon import: {:.1}s ({pw_count} mods)",
        elapsed.as_secs_f64()
    );
}

/// Live Modrinth URL import: Fabulously Optimized (~30 mods).
///
/// Requires live provider access; strict mode enforces external prerequisites.
/// Runtime: 30-120s depending on network conditions.
#[test]
fn e2e_init_from_fabulously_optimized() {
    let project = TestProject::new();
    let start = Instant::now();
    let output = empack_cmd(project.dir())
        .args([
            "init",
            "--from",
            "https://modrinth.com/modpack/fabulously-optimized",
            "--yes",
            "fabopt",
        ])
        .output()
        .expect("spawn failed");
    let elapsed = start.elapsed();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "fabopt import failed:\nstdout: {stdout}\nstderr: {stderr}",
    );

    let pack_dir = project.dir().join("fabopt");
    let resolved = read_project(&pack_dir);
    let count = resolved.lock().dependencies.len();
    assert!(
        count >= 5,
        "expected imported content declarations, found {count}"
    );
    let intent = std::fs::read(pack_dir.join("empack.yml")).unwrap();
    let lock = std::fs::read(pack_dir.join("empack.lock")).unwrap();
    for _ in 0..2 {
        let sync = empack_cmd(&pack_dir)
            .args(["--yes", "sync"])
            .output()
            .unwrap();
        assert!(
            sync.status.success(),
            "{}",
            String::from_utf8_lossy(&sync.stderr)
        );
        assert_eq!(std::fs::read(pack_dir.join("empack.yml")).unwrap(), intent);
        assert_eq!(std::fs::read(pack_dir.join("empack.lock")).unwrap(), lock);
    }
    eprintln!(
        "fabopt import: {:.1}s ({count} dependencies)",
        elapsed.as_secs_f64()
    );
}

/// Live Modrinth URL import followed by mrpack build.
///
/// Validates the full init-from-URL then build-mrpack workflow end to end.
/// Requires live provider access; strict mode enforces external prerequisites.
/// Runtime: 60-180s depending on network conditions.
#[test]
fn e2e_import_and_build_fabulously_optimized() {
    let project = TestProject::new();

    let import = empack_cmd(project.dir())
        .args([
            "init",
            "--from",
            "https://modrinth.com/modpack/fabulously-optimized",
            "--yes",
            "fabopt",
        ])
        .output()
        .expect("spawn failed");
    assert!(
        import.status.success(),
        "import failed: {}",
        String::from_utf8_lossy(&import.stderr)
    );

    let build = empack_cmd(&project.dir().join("fabopt"))
        .env(
            "EMPACK_PROCESS_TIMEOUT_SECS",
            LIVE_IMPORTED_MRPACK_BUILD_TIMEOUT_SECS,
        )
        .args(["--yes", "build", "mrpack", "--allow-optional-metadata-loss"])
        .output()
        .expect("spawn failed");
    assert!(
        build.status.success(),
        "build failed: {}",
        String::from_utf8_lossy(&build.stderr)
    );

    let artifact = assert_dist_artifact_suffix(&project.dir().join("fabopt"), ".mrpack");
    let mut archive = zip::ZipArchive::new(std::fs::File::open(artifact).unwrap()).unwrap();
    let manifest: serde_json::Value =
        serde_json::from_reader(archive.by_name("modrinth.index.json").unwrap()).unwrap();
    assert_eq!(manifest["name"], "Fabulously Optimized");
    assert!(manifest["files"].as_array().unwrap().len() >= 5);
    for file in manifest["files"].as_array().unwrap() {
        assert!(
            file["hashes"]["sha512"]
                .as_str()
                .is_some_and(|hash| hash.len() == 128)
        );
        assert!(file["fileSize"].as_u64().is_some_and(|bytes| bytes > 0));
    }
}
