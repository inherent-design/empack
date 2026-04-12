use empack_tests::e2e::{
    TestProject, assert_pack_loader_version_prefix, assert_pack_minecraft_version,
    assert_pack_option_string, assert_project_datapack_folder, assert_project_initialized,
    assert_project_loader, assert_project_minecraft_version,
};

#[test]
fn e2e_init_yes_fabric() {
    empack_tests::skip_if_no_packwiz!();

    let project = TestProject::new();
    let output = project.run_output_with_retry(&[
        "init",
        "--yes",
        "--modloader",
        "fabric",
        "--mc-version",
        "1.21.1",
        "test-pack",
    ]);
    assert!(output.status.success(), "{:?}", output);

    let pack_dir = project.dir().join("test-pack");
    assert_project_initialized(&pack_dir);
    assert_project_loader(&pack_dir, "fabric");
    assert_project_minecraft_version(&pack_dir, "1.21.1");
    assert_pack_minecraft_version(&pack_dir, "1.21.1");
}

#[test]
fn e2e_init_yes_neoforge() {
    empack_tests::skip_if_no_packwiz!();

    let project = TestProject::new();
    let output = project.run_output_with_retry(&[
        "init",
        "--yes",
        "--modloader",
        "neoforge",
        "--mc-version",
        "1.21.1",
        "test-pack",
    ]);
    assert!(output.status.success(), "{:?}", output);

    let pack_dir = project.dir().join("test-pack");
    assert_project_loader(&pack_dir, "neoforge");
    assert_project_minecraft_version(&pack_dir, "1.21.1");
    assert_pack_loader_version_prefix(&pack_dir, "neoforge", "21.1.");
}

#[test]
fn e2e_init_yes_neoforge_legacy_1_20_1() {
    empack_tests::skip_if_no_packwiz!();

    let project = TestProject::new();
    let output = project.run_output_with_retry(&[
        "init",
        "--yes",
        "--modloader",
        "neoforge",
        "--mc-version",
        "1.20.1",
        "test-pack",
    ]);
    assert!(output.status.success(), "{:?}", output);

    let pack_dir = project.dir().join("test-pack");
    assert_project_loader(&pack_dir, "neoforge");
    assert_project_minecraft_version(&pack_dir, "1.20.1");
    assert_pack_loader_version_prefix(&pack_dir, "neoforge", "47.1.");
}

#[test]
fn e2e_init_yes_missing_modloader() {
    let project = TestProject::new();
    let output = project
        .cmd()
        .args(["init", "--yes", "test-pack"])
        .output()
        .expect("failed to spawn");
    assert!(!output.status.success());

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--modloader") || stderr.contains("requires"),
        "stderr did not mention --modloader or requires\n{stderr}"
    );
}

#[test]
fn e2e_init_existing_project() {
    empack_tests::skip_if_no_packwiz!();

    let project = TestProject::new();
    let output = project.run_output_with_retry(&[
        "init",
        "--yes",
        "--modloader",
        "fabric",
        "--mc-version",
        "1.21.1",
        "test-pack",
    ]);
    assert!(output.status.success(), "{:?}", output);

    let output = project
        .cmd()
        .args([
            "init",
            "--yes",
            "--modloader",
            "fabric",
            "--mc-version",
            "1.21.1",
            "test-pack",
        ])
        .output()
        .expect("failed to spawn");
    assert!(!output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{stdout}{stderr}");
    assert!(
        combined.contains("already contains"),
        "output did not mention 'already contains'\nstdout: {stdout}\nstderr: {stderr}"
    );
}

#[test]
fn e2e_init_force_overwrites() {
    empack_tests::skip_if_no_packwiz!();

    let project = TestProject::new();
    let status = project
        .cmd()
        .args([
            "init",
            "--yes",
            "--modloader",
            "fabric",
            "--mc-version",
            "1.21.1",
            "test-pack",
        ])
        .status()
        .expect("failed to spawn");
    assert!(status.success());

    let output = project.run_output_with_retry(&[
        "init",
        "--yes",
        "--force",
        "--modloader",
        "fabric",
        "--mc-version",
        "1.21.1",
        "test-pack",
    ]);
    assert!(
        output.status.success(),
        "init --force failed on existing project"
    );
}

#[test]
fn e2e_init_scaffolds_templates() {
    empack_tests::skip_if_no_packwiz!();

    let project = TestProject::new();
    let output = project.run_output_with_retry(&[
        "init",
        "--yes",
        "--modloader",
        "fabric",
        "--mc-version",
        "1.21.1",
        "test-pack",
    ]);
    assert!(output.status.success(), "{:?}", output);

    let pack_dir = project.dir().join("test-pack");
    assert!(pack_dir.join(".gitignore").exists(), ".gitignore not found");
    assert!(
        pack_dir.join("pack").join(".packwizignore").exists(),
        "pack/.packwizignore not found"
    );
    assert!(
        pack_dir.join("templates").join("server").is_dir(),
        "templates/server/ not found"
    );
    assert!(
        pack_dir.join("templates").join("client").is_dir(),
        "templates/client/ not found"
    );
}

#[test]
fn e2e_init_datapack_folder() {
    empack_tests::skip_if_no_packwiz!();

    let project = TestProject::new();
    let output = project.run_output_with_retry(&[
        "init",
        "--yes",
        "--modloader",
        "fabric",
        "--mc-version",
        "1.20.1",
        "--datapack-folder",
        "datapacks",
        "test-pack",
    ]);
    assert!(output.status.success(), "{:?}", output);

    let pack_dir = project.dir().join("test-pack");
    assert_project_initialized(&pack_dir);
    assert_project_loader(&pack_dir, "fabric");
    assert_project_minecraft_version(&pack_dir, "1.20.1");
    assert_project_datapack_folder(&pack_dir, "datapacks");
    assert_pack_minecraft_version(&pack_dir, "1.20.1");
    assert_pack_option_string(&pack_dir, "datapack-folder", "datapacks");
}

#[test]
fn e2e_init_dry_run_exits_zero() {
    empack_tests::skip_if_no_packwiz!();

    let project = TestProject::new();
    let output = project.run_output_with_retry(&[
        "init",
        "--dry-run",
        "--yes",
        "--modloader",
        "fabric",
        "--mc-version",
        "1.20.1",
        "test-dry-run",
    ]);
    assert!(
        output.status.success(),
        "init --dry-run should exit 0: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
