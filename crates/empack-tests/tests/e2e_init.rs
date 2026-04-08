use empack_tests::e2e::TestProject;

#[test]
fn e2e_init_yes_fabric() {
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

    let pack_dir = project.dir().join("test-pack");
    let config =
        std::fs::read_to_string(pack_dir.join("empack.yml")).expect("failed to read empack.yml");
    assert!(
        config.contains("loader: fabric"),
        "empack.yml missing 'loader: fabric'\n{config}"
    );
    assert!(
        config.contains("minecraft_version"),
        "empack.yml missing 'minecraft_version'\n{config}"
    );
    assert!(
        pack_dir.join("pack").join("pack.toml").exists(),
        "pack/pack.toml not found"
    );
}

#[test]
fn e2e_init_yes_neoforge() {
    empack_tests::skip_if_no_packwiz!();

    let project = TestProject::new();
    let status = project
        .cmd()
        .args([
            "init",
            "--yes",
            "--modloader",
            "neoforge",
            "--mc-version",
            "1.21.1",
            "test-pack",
        ])
        .status()
        .expect("failed to spawn");
    assert!(status.success());

    let pack_dir = project.dir().join("test-pack");
    let config =
        std::fs::read_to_string(pack_dir.join("empack.yml")).expect("failed to read empack.yml");
    assert!(
        config.contains("loader: neoforge"),
        "empack.yml missing 'loader: neoforge'\n{config}"
    );
    assert!(
        config.contains("minecraft_version"),
        "empack.yml missing 'minecraft_version'\n{config}"
    );
    assert!(
        pack_dir.join("pack").join("pack.toml").exists(),
        "pack/pack.toml not found"
    );
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

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("already contains"),
        "stderr did not mention 'already contains'\n{stderr}"
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

    let status = project
        .cmd()
        .args([
            "init",
            "--yes",
            "--force",
            "--modloader",
            "fabric",
            "--mc-version",
            "1.21.1",
            "test-pack",
        ])
        .status()
        .expect("failed to spawn");
    assert!(status.success(), "init --force failed on existing project");
}

#[test]
fn e2e_init_scaffolds_templates() {
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
    let status = project
        .cmd()
        .args([
            "init",
            "--yes",
            "--modloader",
            "fabric",
            "--mc-version",
            "1.20.1",
            "--datapack-folder",
            "datapacks",
            "test-pack",
        ])
        .status()
        .expect("failed to spawn");
    assert!(status.success());

    let pack_dir = project.dir().join("test-pack");

    let pack_toml = std::fs::read_to_string(pack_dir.join("pack").join("pack.toml"))
        .expect("failed to read pack.toml");
    assert!(
        pack_toml.contains("datapack-folder"),
        "pack.toml missing 'datapack-folder'\n{pack_toml}"
    );
    assert!(
        pack_toml.contains("datapacks"),
        "pack.toml missing 'datapacks' value\n{pack_toml}"
    );

    let config =
        std::fs::read_to_string(pack_dir.join("empack.yml")).expect("failed to read empack.yml");
    assert!(
        config.contains("datapack_folder"),
        "empack.yml missing 'datapack_folder'\n{config}"
    );
    assert!(
        config.contains("datapacks"),
        "empack.yml missing 'datapacks' value\n{config}"
    );
}

#[test]
fn e2e_init_dry_run_exits_zero() {
    empack_tests::skip_if_no_packwiz!();

    let project = TestProject::new();
    let output = project
        .cmd()
        .args([
            "init",
            "--dry-run",
            "--yes",
            "--modloader",
            "fabric",
            "--mc-version",
            "1.20.1",
            "test-dry-run",
        ])
        .output()
        .expect("failed to spawn");
    assert!(
        output.status.success(),
        "init --dry-run should exit 0: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
