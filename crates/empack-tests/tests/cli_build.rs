use empack_tests::harness::archive::{zip_member, zip_members};
use empack_tests::harness::process::{TestProject, assert_dist_artifact_suffix, offline_command};

#[test]
fn cli_build_relative_workdir_writes_expected_mrpack_metadata() {
    let project = TestProject::workflow_fixture("relative-pack", "fabric", "1.21.1");
    let mut cmd = offline_command(project.dir());
    cmd.current_dir(project.dir().parent().unwrap())
        .arg("--workdir")
        .arg(project.dir().file_name().unwrap())
        .args(["--yes", "build", "modrinth"]);
    cmd.timeout(std::time::Duration::from_secs(60))
        .assert()
        .success();
    let archive = assert_dist_artifact_suffix(project.dir(), ".mrpack");
    let manifest: serde_json::Value =
        serde_json::from_slice(&zip_member(&archive, "modrinth.index.json")).unwrap();
    assert_eq!(manifest["name"], "relative-pack");
    assert_eq!(manifest["dependencies"]["minecraft"], "1.21.1");
    assert!(manifest["files"].as_array().unwrap().is_empty());
}

#[test]
fn cli_build_modrinth_creates_named_artifact() {
    let project = TestProject::workflow_fixture("test-pack", "fabric", "1.21.1");
    project.configure_native_distribution("test.pack", 21);
    let output = offline_command(project.dir())
        .args(["--yes", "build", "modrinth"])
        .output()
        .expect("failed to spawn");
    assert!(output.status.success(), "empack build modrinth failed");

    let artifact = assert_dist_artifact_suffix(project.dir(), ".mrpack");
    assert!(
        artifact
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("test-pack-")),
        "unexpected mrpack artifact path: {}",
        artifact.display()
    );
    let manifest: serde_json::Value =
        serde_json::from_slice(&zip_member(&artifact, "modrinth.index.json")).unwrap();
    assert_eq!(manifest["dependencies"]["minecraft"], "1.21.1");
    assert_eq!(manifest["name"], "test-pack");
}

#[test]
fn cli_build_prism_references_tar_gz_creates_named_artifact() {
    let project = TestProject::workflow_fixture("test-pack", "fabric", "1.21.1");
    project.configure_native_distribution("test.pack", 21);
    let output = offline_command(project.dir())
        .args([
            "--yes",
            "build",
            "--ad-hoc",
            "--format",
            "tar.gz",
            "prism",
            "--delivery",
            "references",
        ])
        .output()
        .expect("failed to spawn");
    assert!(
        output.status.success(),
        "empack build client --format tar.gz failed"
    );

    assert_dist_artifact_suffix(project.dir(), "-prism-references.tar.gz");
}

#[test]
fn cli_clean_builds_removes_generated_artifacts() {
    let project = TestProject::workflow_fixture("test-pack", "fabric", "1.21.1");
    project.configure_native_distribution("test.pack", 21);
    let output = offline_command(project.dir())
        .args(["--yes", "build", "modrinth"])
        .output()
        .expect("failed to spawn");
    assert!(output.status.success(), "empack build modrinth failed");

    let manifest = std::fs::read(project.dir().join("empack.yml")).unwrap();
    let lock = std::fs::read(project.dir().join("empack.lock")).unwrap();
    let dist = project.dir().join("dist");
    assert!(dist.is_dir(), "dist/ should exist after build");

    let output = offline_command(project.dir())
        .args(["--yes", "clean"])
        .output()
        .expect("failed to spawn");
    assert!(output.status.success(), "empack clean failed");

    let dist_empty = !dist.exists()
        || std::fs::read_dir(&dist)
            .expect("failed to read dist/")
            .filter_map(Result::ok)
            .next()
            .is_none();
    assert!(dist_empty, "dist/ should be empty or absent after clean");
    assert_eq!(
        std::fs::read(project.dir().join("empack.yml")).unwrap(),
        manifest
    );
    assert_eq!(
        std::fs::read(project.dir().join("empack.lock")).unwrap(),
        lock
    );
}

#[test]
fn cli_build_prism_references_records_runtime() {
    let project = TestProject::new();
    offline_command(project.dir())
        .args([
            "init",
            "--yes",
            "--modloader",
            "none",
            "--mc-version",
            "1.21.1",
            "--pack-name",
            "test-pack",
        ])
        .assert()
        .success();
    project.configure_native_distribution("matrix.pack", 21);
    offline_command(project.dir())
        .args(["--yes", "build", "prism", "--delivery", "references"])
        .assert()
        .success();
    let artifact = assert_dist_artifact_suffix(project.dir(), "-prism-references.zip");
    let members = zip_members(&artifact);
    let release: serde_json::Value =
        serde_json::from_slice(&members[".minecraft/.empack-consumer/release.json"]).unwrap();
    assert_eq!(release["pack"], "matrix.pack");
    assert_eq!(release["runtime"]["minecraft"], "1.21.1");
    assert!(!members.keys().any(|name| name.contains("packwiz")));
}
