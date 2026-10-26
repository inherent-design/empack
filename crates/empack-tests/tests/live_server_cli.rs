//! Live server acquisition and preparation, not game-startup acceptance.
use empack_tests::harness::process::{TestProject, assert_dist_artifact_suffix};

#[path = "support/local_exports.rs"]
mod local_exports;

#[path = "support/server_exports.rs"]
mod server_exports;

#[test]
#[ignore = "requires Java and official runtime downloads"]
fn live_cli_server_archives_preserve_pinned_inventory() {
    empack_tests::skip_if_no_java!();
    server_exports::verify();
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
    for format in ["zip", "7z"] {
        let mut command = assert_cmd::Command::from_std(project.cmd());
        command
            .timeout(std::time::Duration::from_secs(240))
            .args([
                "--yes",
                "build",
                "server",
                "--delivery",
                "references",
                "--format",
                format,
            ])
            .assert()
            .success();
        let artifact =
            assert_dist_artifact_suffix(project.dir(), &format!("-server-references.{format}"));
        let members = empack_tests::harness::archive::distribution_inventory(&artifact, format);
        let release: serde_json::Value = serde_json::from_slice(
            members[".empack-consumer/release.json"]
                .small
                .as_ref()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(release["pack"], "matrix.pack");
        assert_eq!(release["runtime"]["minecraft"], "1.21.1");
        assert_eq!(release["runtime"]["loader"]["kind"], "fabric");
        assert!(!members.keys().any(|name| name.contains("packwiz")));
    }
}
