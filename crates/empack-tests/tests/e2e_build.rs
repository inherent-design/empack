use empack_tests::e2e::{TestProject, assert_dist_artifact_suffix};

#[test]
fn e2e_build_relative_workdir_exports_valid_mrpack() {
    use empack_lib::application::session::{ArchiveProvider, LiveArchiveProvider};
    empack_tests::skip_if_no_packwiz!();
    let project = TestProject::workflow_fixture("relative-pack", "fabric", "1.21.1");
    let mut cmd = project.cmd();
    cmd.current_dir(project.dir().parent().unwrap())
        .arg("--workdir")
        .arg(project.dir().file_name().unwrap())
        .args(["build", "mrpack"]);
    assert_cmd::Command::from_std(cmd)
        .timeout(std::time::Duration::from_secs(60))
        .assert()
        .success();
    let archive = assert_dist_artifact_suffix(project.dir(), ".mrpack");
    let extracted = project.dir().join("extracted");
    LiveArchiveProvider
        .extract_zip(&archive, &extracted)
        .unwrap();
    let manifest: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(extracted.join("modrinth.index.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(manifest["name"], "relative-pack");
    assert_eq!(manifest["dependencies"]["minecraft"], "1.21.1");
    assert!(manifest["files"].as_array().unwrap().is_empty());
}

#[test]
fn e2e_build_mrpack() {
    empack_tests::skip_if_no_java!();

    let project = TestProject::workflow_fixture("test-pack", "fabric", "1.21.1");
    let status = project
        .cmd()
        .args(["build", "mrpack"])
        .status()
        .expect("failed to spawn");
    assert!(status.success(), "empack build mrpack failed");

    let artifact = assert_dist_artifact_suffix(project.dir(), ".mrpack");
    assert!(
        artifact
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("test-pack-v")),
        "unexpected mrpack artifact path: {}",
        artifact.display()
    );
}

#[test]
fn e2e_build_client_tar_gz() {
    empack_tests::skip_if_no_java!();

    let project = TestProject::workflow_fixture("test-pack", "fabric", "1.21.1");
    let status = project
        .cmd()
        .args(["build", "--format", "tar.gz", "client"])
        .status()
        .expect("failed to spawn");
    assert!(
        status.success(),
        "empack build client --format tar.gz failed"
    );

    assert_dist_artifact_suffix(project.dir(), "-client.tar.gz");
}

#[test]
fn e2e_build_server_sevenz() {
    empack_tests::skip_if_no_java!();

    let project = TestProject::workflow_fixture("test-pack", "fabric", "1.21.1");
    let status = project
        .cmd()
        .args(["build", "--format", "7z", "server"])
        .status()
        .expect("failed to spawn");
    assert!(status.success(), "empack build server --format 7z failed");

    assert_dist_artifact_suffix(project.dir(), "-server.7z");
}

#[test]
fn e2e_clean_removes_artifacts() {
    empack_tests::skip_if_no_java!();

    let project = TestProject::workflow_fixture("test-pack", "fabric", "1.21.1");
    let status = project
        .cmd()
        .args(["build", "mrpack"])
        .status()
        .expect("failed to spawn");
    assert!(status.success(), "empack build mrpack failed");

    let dist = project.dir().join("dist");
    assert!(dist.is_dir(), "dist/ should exist after build");

    let status = project
        .cmd()
        .args(["clean"])
        .status()
        .expect("failed to spawn");
    assert!(status.success(), "empack clean failed");

    let dist_empty = !dist.exists()
        || std::fs::read_dir(&dist)
            .expect("failed to read dist/")
            .filter_map(Result::ok)
            .next()
            .is_none();
    assert!(dist_empty, "dist/ should be empty or absent after clean");
}
