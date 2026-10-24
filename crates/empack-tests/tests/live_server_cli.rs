//! Live server acquisition and preparation, not game-startup acceptance.
use empack_tests::harness::process::{TestProject, assert_dist_artifact_suffix};

#[path = "support/local_exports.rs"]
mod local_exports;

#[test]
#[ignore = "requires Java and official runtime downloads"]
fn live_cli_build_server_references_7z_creates_named_artifact() {
    empack_tests::skip_if_no_java!();

    let project = TestProject::workflow_fixture("test-pack", "fabric", "1.21.1");
    project.configure_native_distribution("test.pack", 21);
    let status = project
        .cmd()
        .args([
            "--yes",
            "build",
            "--ad-hoc",
            "--format",
            "7z",
            "server",
            "--delivery",
            "references",
        ])
        .status()
        .expect("failed to spawn");
    assert!(status.success(), "empack build server --format 7z failed");

    assert_dist_artifact_suffix(project.dir(), "-server-references.7z");
}

#[test]
#[ignore = "requires Java and official runtime downloads"]
fn live_cli_local_server_export_prepare_preserves_bytes() {
    empack_tests::skip_if_no_java!();
    local_exports::export_prepare(&["server"], true);
}

#[test]
#[ignore = "requires Java and official runtime downloads"]
fn live_cli_build_server_references_records_runtime() {
    empack_tests::skip_if_no_java!();
    let project = TestProject::initialized("test-pack", "fabric", "1.21.1");
    project.configure_native_distribution("matrix.pack", 21);
    let output = project
        .cmd()
        .args(["--yes", "build", "server", "--delivery", "references"])
        .output()
        .expect("failed to spawn");
    assert!(
        output.status.success(),
        "build {} failed: {}",
        "server",
        String::from_utf8_lossy(&output.stderr)
    );

    let artifact = assert_dist_artifact_suffix(project.dir(), "-server-references.zip");
    let release: serde_json::Value = serde_json::from_slice(
        &empack_tests::harness::archive::zip_member(&artifact, ".empack-consumer/release.json"),
    )
    .unwrap();
    assert_eq!(release["pack"], "matrix.pack");
    assert_eq!(release["runtime"]["minecraft"], "1.21.1");
    let archive = zip::ZipArchive::new(std::fs::File::open(artifact).unwrap()).unwrap();
    assert!(
        !archive
            .file_names()
            .any(|name| name.unwrap().contains("packwiz"))
    );
}
