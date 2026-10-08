use empack_tests::e2e::{TestProject, assert_dist_artifact_suffix};

#[test]
fn e2e_build_relative_workdir_exports_valid_mrpack() {
    let project = TestProject::workflow_fixture("relative-pack", "fabric", "1.21.1");
    let mut cmd = project.cmd();
    cmd.current_dir(project.dir().parent().unwrap())
        .arg("--workdir")
        .arg(project.dir().file_name().unwrap())
        .args(["--yes", "build", "mrpack"]);
    assert_cmd::Command::from_std(cmd)
        .timeout(std::time::Duration::from_secs(60))
        .assert()
        .success();
    let archive = assert_dist_artifact_suffix(project.dir(), ".mrpack");
    let manifest: serde_json::Value =
        serde_json::from_slice(&zip_bytes(&archive, "modrinth.index.json")).unwrap();
    assert_eq!(manifest["name"], "relative-pack");
    assert_eq!(manifest["dependencies"]["minecraft"], "1.21.1");
    assert!(manifest["files"].as_array().unwrap().is_empty());
}

#[test]
fn e2e_build_mrpack() {
    let project = TestProject::workflow_fixture("test-pack", "fabric", "1.21.1");
    let status = project
        .cmd()
        .args(["--yes", "build", "mrpack"])
        .status()
        .expect("failed to spawn");
    assert!(status.success(), "empack build mrpack failed");

    let artifact = assert_dist_artifact_suffix(project.dir(), ".mrpack");
    assert!(
        artifact
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("test-pack-")),
        "unexpected mrpack artifact path: {}",
        artifact.display()
    );
}

#[test]
fn e2e_build_client_tar_gz() {
    let project = TestProject::workflow_fixture("test-pack", "fabric", "1.21.1");
    let status = project
        .cmd()
        .args(["--yes", "build", "--format", "tar.gz", "client"])
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
        .args(["--yes", "build", "--format", "7z", "server"])
        .status()
        .expect("failed to spawn");
    assert!(status.success(), "empack build server --format 7z failed");

    assert_dist_artifact_suffix(project.dir(), "-server.7z");
}

#[test]
fn e2e_clean_removes_artifacts() {
    let project = TestProject::workflow_fixture("test-pack", "fabric", "1.21.1");
    let status = project
        .cmd()
        .args(["--yes", "build", "mrpack"])
        .status()
        .expect("failed to spawn");
    assert!(status.success(), "empack build mrpack failed");

    let dist = project.dir().join("dist");
    assert!(dist.is_dir(), "dist/ should exist after build");

    let status = project
        .cmd()
        .args(["--yes", "clean"])
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
    use empack_core::{model::PlacementIntent, requirements::Requirement};
    use empack_lib::engine::documents::DocumentCodec;
    use std::io::Write;
    empack_tests::skip_if_no_java!();
    let project = TestProject::workflow_fixture("local-content", "fabric", "1.21.1");
    let source = project.dir().join("local.zip");
    let mut archive = zip::ZipWriter::new(std::fs::File::create(&source).unwrap());
    archive
        .start_file("pack.mcmeta", zip::write::SimpleFileOptions::default())
        .unwrap();
    archive
        .write_all(br#"{"pack":{"pack_format":34,"description":"test assets"}}"#)
        .unwrap();
    archive
        .start_file(
            "assets/test/data.bin",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
    archive.write_all(b"tracked local resource bytes").unwrap();
    archive.finish().unwrap();
    let bytes = std::fs::read(&source).unwrap();
    assert_cmd::Command::from_std(project.cmd())
        .args(["--yes", "add", "--type", "resourcepack", "local.zip"])
        .assert()
        .success();
    // This fixture deliberately distributes the resource file to both environments.
    let manifest = project.dir().join("empack.yml");
    let mut intent = DocumentCodec
        .decode_intent(&std::fs::read(&manifest).unwrap(), "test")
        .unwrap()
        .intent()
        .clone();
    let root = intent.roots.values_mut().next().unwrap();
    root.requirements.server = Requirement::Required;
    if let PlacementIntent::Explicit(placements) = &mut root.placement {
        let mut values = placements.clone().into_vec();
        for placement in &mut values {
            placement.requirements.server = Requirement::Required;
        }
        *placements = empack_core::model::NonEmpty::new(values).unwrap();
    }
    std::fs::write(&manifest, DocumentCodec.encode_intent(&intent).unwrap()).unwrap();
    assert_cmd::Command::from_std(project.cmd())
        .args(["--yes", "sync"])
        .assert()
        .success();
    for target in ["mrpack", "client", "server"] {
        assert_cmd::Command::from_std(project.cmd())
            .args(["--yes", "build", target])
            .timeout(std::time::Duration::from_secs(90))
            .assert()
            .success();
        let suffix = if target == "mrpack" {
            ".mrpack".to_string()
        } else {
            format!("-{target}.zip")
        };
        let artifact = assert_dist_artifact_suffix(project.dir(), &suffix);
        let relative = if target == "mrpack" {
            "overrides/resourcepacks/local.zip"
        } else if target == "client" {
            ".minecraft/resourcepacks/local.zip"
        } else {
            "resourcepacks/local.zip"
        };
        assert_eq!(zip_bytes(&artifact, relative), bytes, "{target}");
    }
    std::fs::write(project.dir().join("local.zip"), &bytes).unwrap();
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
        .stderr(predicates::str::contains("intent"));
    // Project-relative authoring sources outside pack are valid after explicit reconciliation.
    assert_cmd::Command::from_std(project.cmd())
        .args(["--yes", "sync"])
        .assert()
        .success();
    assert_eq!(
        std::fs::read(project.dir().join("pack/resourcepacks/local.zip")).unwrap(),
        bytes
    );
    assert_cmd::Command::from_std(project.cmd())
        .args(["--yes", "build", "mrpack"])
        .assert()
        .success();
    assert_eq!(
        zip_bytes(
            &assert_dist_artifact_suffix(project.dir(), ".mrpack"),
            "overrides/resourcepacks/local.zip"
        ),
        bytes
    );
}

fn zip_bytes(path: &std::path::Path, member: &str) -> Vec<u8> {
    let mut archive = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut archive.by_name(member).unwrap(), &mut bytes).unwrap();
    bytes
}
