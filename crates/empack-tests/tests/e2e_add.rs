use empack_tests::e2e::TestProject;

#[test]
fn e2e_add_to_uninitialized() {
    let project = TestProject::new();
    let output = project
        .cmd()
        .args(["add", "sodium"])
        .output()
        .expect("failed to spawn empack");

    assert!(
        !output.status.success(),
        "empack add in uninitialized dir should exit non-zero"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{stdout}{stderr}");
    assert!(
        combined.contains("modpack")
            || combined.contains("init")
            || combined.contains("uninitialized"),
        "output should mention initialization requirement, got:\nstdout: {stdout}\nstderr: {stderr}"
    );
}

#[test]
fn e2e_add_sodium_live() {
    empack_tests::skip_if_no_packwiz!();
    if std::env::var("EMPACK_RUN_LIVE_TESTS").is_err() {
        eprintln!("SKIP: set EMPACK_RUN_LIVE_TESTS=1 to run live network tests");
        return;
    }

    let project = TestProject::initialized("test-pack", "fabric", "1.21.1");
    let output = project
        .cmd()
        .args(["add", "sodium"])
        .output()
        .expect("failed to spawn empack");

    assert!(
        output.status.success(),
        "empack add sodium failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    project.assert_contains("empack.yml", "sodium");

    let mods_dir = project.dir().join("pack/mods");
    let has_pw_toml = std::fs::read_dir(&mods_dir)
        .unwrap_or_else(|_| panic!("failed to read {}", mods_dir.display()))
        .filter_map(|e| e.ok())
        .any(|e| {
            e.path()
                .extension()
                .is_some_and(|ext| ext == "toml")
        });
    assert!(has_pw_toml, "expected at least one .pw.toml in pack/mods/");
}

#[test]
fn e2e_add_nonexistent_mod() {
    empack_tests::skip_if_no_packwiz!();

    let project = TestProject::initialized("test-pack", "fabric", "1.21.1");
    let output = project
        .cmd()
        .args(["add", "xyznonexistentmod12345"])
        .output()
        .expect("failed to spawn empack");

    assert!(!output.status.success());
}
