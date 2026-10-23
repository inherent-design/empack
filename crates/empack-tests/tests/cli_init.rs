//! Initialization contracts that require no external catalog.
use empack_tests::harness::process::{
    TestProject, assert_project_datapack_folder, assert_project_initialized,
    assert_project_loader_absent, assert_project_minecraft_version,
};

#[test]
fn cli_init_missing_loader_reports_usage() {
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
fn cli_init_existing_project_requires_replacement_decision() {
    let project = TestProject::new();
    let output = project.run_output(&[
        "init",
        "--yes",
        "--modloader",
        "none",
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
            "none",
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
fn cli_init_forced_replacement_preserves_unowned_content() {
    let project = TestProject::new();
    let status = project
        .cmd()
        .args([
            "init",
            "--yes",
            "--modloader",
            "none",
            "--mc-version",
            "1.21.1",
            "test-pack",
        ])
        .status()
        .expect("failed to spawn");
    assert!(status.success());
    let notes = project.dir().join("test-pack/notes.txt");
    let custom = project.dir().join("test-pack/research/author.properties");
    std::fs::create_dir_all(custom.parent().unwrap()).unwrap();
    std::fs::write(&notes, b"author notes").unwrap();
    std::fs::write(&custom, b"keep=custom").unwrap();
    // Forced initialization replaces the captured source roots, unlike unrelated
    // root files and directories. Make that destructive scope visible in preview.
    let owned = project
        .dir()
        .join("test-pack/pack/config/previous.properties");
    std::fs::create_dir_all(owned.parent().unwrap()).unwrap();
    std::fs::write(&owned, b"old=source").unwrap();
    let preview = project.run_output(&[
        "init",
        "--yes",
        "--force",
        "--dry-run",
        "--modloader",
        "none",
        "--mc-version",
        "1.21.1",
        "test-pack",
    ]);
    assert!(preview.status.success(), "{preview:?}");
    assert!(String::from_utf8_lossy(&preview.stdout).contains("previous.properties"));
    assert_eq!(std::fs::read(&owned).unwrap(), b"old=source");

    let output = project.run_output(&[
        "init",
        "--yes",
        "--force",
        "--modloader",
        "none",
        "--mc-version",
        "1.21.1",
        "test-pack",
    ]);
    assert!(
        output.status.success(),
        "init --force failed on existing project: {output:?}"
    );
    assert_eq!(std::fs::read(notes).unwrap(), b"author notes");
    assert_eq!(std::fs::read(custom).unwrap(), b"keep=custom");
    assert!(!owned.exists());
}

#[test]
fn cli_init_emits_versioned_automation_and_templates() {
    let project = TestProject::new();
    let output = project.run_output(&[
        "init",
        "--yes",
        "--modloader",
        "none",
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
        let version = empack_tests::harness::process::expected_automation_version();
        assert_eq!(workflow["env"]["EMPACK_VERSION"], format!("v{version}"));
        assert!(!bytes.contains("__EMPACK_RELEASE_TAG__"));
        assert!(bytes.contains(&format!("python3 .github/scripts/empack-ci.py {name}")));
        assert!(pack_dir.join(".github/scripts/empack-ci.py").is_file());
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
fn cli_init_datapack_folder_is_authored() {
    let project = TestProject::new();
    let output = project.run_output(&[
        "init",
        "--yes",
        "--modloader",
        "none",
        "--mc-version",
        "1.20.1",
        "--datapack-folder",
        "datapacks",
        "test-pack",
    ]);
    assert!(output.status.success(), "{:?}", output);

    let pack_dir = project.dir().join("test-pack");
    assert_project_initialized(&pack_dir);
    assert_project_loader_absent(&pack_dir);
    assert_project_minecraft_version(&pack_dir, "1.20.1");
    assert_project_datapack_folder(&pack_dir, "datapacks");
}

#[test]
fn cli_init_preview_preserves_absent_destination() {
    let project = TestProject::new();
    let output = project.run_output(&[
        "init",
        "--dry-run",
        "--yes",
        "--modloader",
        "none",
        "--mc-version",
        "1.20.1",
        "test-dry-run",
    ]);
    assert!(
        output.status.success(),
        "init --dry-run should exit 0: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!project.dir().join("test-dry-run").exists());
}
