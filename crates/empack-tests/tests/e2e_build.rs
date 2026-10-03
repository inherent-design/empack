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

#[test]
fn e2e_tracked_local_content_survives_fresh_exports_and_light_builds() {
    use empack_lib::application::session::{
        ArchiveProvider, FileSystemProvider, LiveArchiveProvider, LiveFileSystemProvider,
    };
    use empack_lib::empack::config::{DependencyEntry, DependencyStatus, LocalDependencyRecord};
    use empack_lib::primitives::ProjectType;
    empack_tests::skip_if_no_packwiz!();
    empack_tests::skip_if_no_java!();
    let project = TestProject::workflow_fixture("local-content", "fabric", "1.21.1");
    let bytes = b"tracked local resource bytes";
    std::fs::create_dir_all(project.dir().join("pack/resourcepacks")).unwrap();
    std::fs::write(project.dir().join("pack/resourcepacks/local.zip"), bytes).unwrap();
    LiveFileSystemProvider
        .config_manager(project.dir().to_path_buf())
        .add_dependency_entry(
            "local",
            DependencyEntry::Local(LocalDependencyRecord {
                status: DependencyStatus::Local,
                title: "Local".into(),
                project_type: ProjectType::ResourcePack,
                path: "pack/resourcepacks/local.zip".into(),
                source_url: None,
                sha256: "1c7bdeb93532e2e899d11fa3ebd0cf1a78c1753645e8b92e504721b6649762f8".into(),
            }),
        )
        .unwrap();
    for target in ["mrpack", "client", "server"] {
        assert_cmd::Command::from_std(project.cmd())
            .args(["build", target])
            .timeout(std::time::Duration::from_secs(90))
            .assert()
            .success();
        let suffix = if target == "mrpack" {
            ".mrpack".to_string()
        } else {
            format!("-{target}.zip")
        };
        let artifact = assert_dist_artifact_suffix(project.dir(), &suffix);
        let extracted = project.dir().join(format!("inspect-{target}"));
        LiveArchiveProvider
            .extract_zip(&artifact, &extracted)
            .unwrap();
        let relative = if target == "mrpack" {
            "overrides/resourcepacks/local.zip"
        } else if target == "client" {
            ".minecraft/resourcepacks/local.zip"
        } else {
            "resourcepacks/local.zip"
        };
        assert_eq!(
            std::fs::read(extracted.join(relative)).unwrap_or_else(|error| panic!(
                "{target}: {}: {error}; entries={:?}",
                extracted.join(relative).display(),
                LiveFileSystemProvider.get_file_list(&extracted)
            )),
            bytes,
            "{target}"
        );
    }
    std::fs::write(project.dir().join("local.zip"), bytes).unwrap();
    let manifest = project.dir().join("empack.yml");
    let original = std::fs::read_to_string(&manifest).unwrap();
    std::fs::write(
        &manifest,
        original.replace("pack/resourcepacks/local.zip", "local.zip"),
    )
    .unwrap();
    assert_cmd::Command::from_std(project.cmd())
        .args(["--dry-run", "build", "client"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("must be stored under pack/"));
}
