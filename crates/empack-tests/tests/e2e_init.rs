use empack_tests::e2e::{
    TestProject, assert_locked_loader_version_prefix, assert_locked_minecraft_version,
    assert_project_datapack_folder, assert_project_initialized, assert_project_loader,
    assert_project_minecraft_version,
};

#[test]
fn e2e_init_yes_fabric() {
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
    assert_locked_minecraft_version(&pack_dir, "1.21.1");
}

#[test]
fn e2e_init_yes_neoforge() {
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
    assert_locked_loader_version_prefix(&pack_dir, "neoforge", "21.1.");
}

#[test]
fn e2e_init_yes_neoforge_legacy_1_20_1() {
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
    assert_locked_loader_version_prefix(&pack_dir, "neoforge", "47.1.");
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

    let document = project.dir().join("test-pack/empack.yml");
    let before = std::fs::read(&document).unwrap();
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
        combined.contains("replacement requires an explicit decision"),
        "output did not explain the required replacement decision\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert_eq!(std::fs::read(document).unwrap(), before);
}

#[test]
fn e2e_init_force_overwrites() {
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
    for name in ["validate", "release"] {
        let bytes = std::fs::read_to_string(pack_dir.join(format!(".github/workflows/{name}.yml")))
            .unwrap();
        let workflow: serde_json::Value = serde_saphyr::from_str(&bytes).unwrap();
        assert_eq!(workflow["env"]["EMPACK_VERSION"], "v0.5.0-alpha.1");
        assert!(bytes.contains("empack build --yes mrpack"));
        assert!(!bytes.contains("packwiz"));
    }
    assert!(
        !pack_dir.join("pack").join(".packwizignore").exists(),
        "initialization must not create foreign control files"
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
    assert_locked_minecraft_version(&pack_dir, "1.20.1");
}

#[test]
fn e2e_init_dry_run_exits_zero() {
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
