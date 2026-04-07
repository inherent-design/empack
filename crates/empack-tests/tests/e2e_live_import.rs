use std::time::Instant;

use empack_tests::e2e::{count_pw_toml_files, empack_cmd, TestProject};

/// Live CurseForge URL import: Cobblemon Updated (~30 mods).
///
/// Requires packwiz in PATH and network access. Self-skips otherwise.
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
        pw_count >= 1,
        "expected at least 1 mod .pw.toml file, found {pw_count}"
    );

    eprintln!(
        "cobblemon import: {:.1}s ({pw_count} mods)",
        elapsed.as_secs_f64()
    );
}

/// Live Modrinth URL import: Fabulously Optimized (~30 mods).
///
/// Requires packwiz in PATH and network access. Self-skips otherwise.
/// Runtime: 30-120s depending on network conditions.
#[test]
fn e2e_init_from_fabulously_optimized() {
    empack_tests::skip_if_no_packwiz!();

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

    project.assert_exists("fabopt/empack.yml");
    project.assert_exists("fabopt/pack/pack.toml");

    let pw_count = count_pw_toml_files(&project.dir().join("fabopt/pack"));
    assert!(
        pw_count >= 1,
        "expected at least 1 mod .pw.toml file, found {pw_count}"
    );

    eprintln!(
        "fabopt import: {:.1}s ({pw_count} mods)",
        elapsed.as_secs_f64()
    );
}

/// Live Modrinth URL import followed by mrpack build.
///
/// Validates the full init-from-URL then build-mrpack workflow end to end.
/// Requires packwiz in PATH and network access. Self-skips otherwise.
/// Runtime: 60-180s depending on network conditions.
#[test]
fn e2e_import_and_build_fabulously_optimized() {
    empack_tests::skip_if_no_packwiz!();

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
        .args(["build", "mrpack"])
        .output()
        .expect("spawn failed");
    assert!(
        build.status.success(),
        "build failed: {}",
        String::from_utf8_lossy(&build.stderr)
    );

    let dist = project.dir().join("fabopt/dist");
    let has_mrpack = std::fs::read_dir(&dist)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .any(|e| e.path().extension().is_some_and(|ext| ext == "mrpack"))
        })
        .unwrap_or(false);
    assert!(has_mrpack, "no .mrpack file in dist/");
}
