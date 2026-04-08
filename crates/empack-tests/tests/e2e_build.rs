use empack_tests::e2e::TestProject;

fn assert_dist_has_artifact(dist: &std::path::Path, expected_suffix: &str) {
    let names: Vec<String> = std::fs::read_dir(dist)
        .expect("failed to read dist/")
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        names
            .iter()
            .any(|name| name.starts_with("test-pack-v") && name.ends_with(expected_suffix)),
        "expected dist/ to contain artifact ending with {expected_suffix}, found: {}",
        names.join(", ")
    );
}

#[test]
fn e2e_build_mrpack() {
    empack_tests::skip_if_no_java!();

    let project = TestProject::initialized("test-pack", "fabric", "1.21.1");
    let status = project
        .cmd()
        .args(["build", "mrpack"])
        .status()
        .expect("failed to spawn");
    assert!(status.success(), "empack build mrpack failed");

    let dist = project.dir().join("dist");
    assert!(dist.is_dir(), "dist/ directory not found");

    let has_mrpack = std::fs::read_dir(&dist)
        .expect("failed to read dist/")
        .filter_map(Result::ok)
        .any(|entry| entry.path().extension().is_some_and(|ext| ext == "mrpack"));
    assert!(has_mrpack, "no .mrpack file found in dist/");
}

#[test]
fn e2e_build_client_tar_gz() {
    empack_tests::skip_if_no_java!();

    let project = TestProject::initialized("test-pack", "fabric", "1.21.1");
    let status = project
        .cmd()
        .args(["build", "--format", "tar.gz", "client"])
        .status()
        .expect("failed to spawn");
    assert!(
        status.success(),
        "empack build client --format tar.gz failed"
    );

    let dist = project.dir().join("dist");
    assert!(dist.is_dir(), "dist/ directory not found");

    assert_dist_has_artifact(&dist, "-client.tar.gz");
}

#[test]
fn e2e_build_server_sevenz() {
    empack_tests::skip_if_no_java!();

    let project = TestProject::initialized("test-pack", "fabric", "1.21.1");
    let status = project
        .cmd()
        .args(["build", "--format", "7z", "server"])
        .status()
        .expect("failed to spawn");
    assert!(status.success(), "empack build server --format 7z failed");

    let dist = project.dir().join("dist");
    assert!(dist.is_dir(), "dist/ directory not found");

    assert_dist_has_artifact(&dist, "-server.7z");
}

#[test]
fn e2e_clean_removes_artifacts() {
    empack_tests::skip_if_no_java!();

    let project = TestProject::initialized("test-pack", "fabric", "1.21.1");
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
