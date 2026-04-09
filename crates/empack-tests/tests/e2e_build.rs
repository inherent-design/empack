use empack_tests::e2e::{TestProject, assert_dist_artifact_suffix};

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
