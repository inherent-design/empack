//! Official catalog integration; selected only by live verification.
use empack_tests::harness::process::{
    TestProject, assert_locked_loader_version_prefix, assert_locked_minecraft_version,
    assert_project_initialized, assert_project_loader, assert_project_minecraft_version,
};

#[test]
#[ignore = "requires official runtime catalogs; run the live suite explicitly"]
fn live_cli_init_fabric_binds_runtime() {
    let project = TestProject::new();
    let output = project.run_output(&[
        "init",
        "--yes",
        "--modloader",
        "fabric",
        "--mc-version",
        "1.21.1",
        "test-pack",
    ]);
    assert!(output.status.success(), "{:?}", output);

    let pack_dir = project.dir().join("test-pack");
    assert_project_initialized(&pack_dir);
    assert_project_loader(&pack_dir, "fabric");
    assert_project_minecraft_version(&pack_dir, "1.21.1");
    assert_locked_minecraft_version(&pack_dir, "1.21.1");
}

#[test]
#[ignore = "requires official runtime catalogs; run the live suite explicitly"]
fn live_cli_init_neoforge_binds_runtime() {
    let project = TestProject::new();
    let output = project.run_output(&[
        "init",
        "--yes",
        "--modloader",
        "neoforge",
        "--mc-version",
        "1.21.1",
        "test-pack",
    ]);
    assert!(output.status.success(), "{:?}", output);

    let pack_dir = project.dir().join("test-pack");
    assert_project_loader(&pack_dir, "neoforge");
    assert_project_minecraft_version(&pack_dir, "1.21.1");
    assert_locked_loader_version_prefix(&pack_dir, "neoforge", "21.1.");
}

#[test]
#[ignore = "requires official runtime catalogs; run the live suite explicitly"]
fn live_cli_init_early_neoforge_binds_runtime() {
    let project = TestProject::new();
    let output = project.run_output(&[
        "init",
        "--yes",
        "--modloader",
        "neoforge",
        "--mc-version",
        "1.20.1",
        "test-pack",
    ]);
    assert!(output.status.success(), "{:?}", output);

    let pack_dir = project.dir().join("test-pack");
    assert_project_loader(&pack_dir, "neoforge");
    assert_project_minecraft_version(&pack_dir, "1.20.1");
    assert_locked_loader_version_prefix(&pack_dir, "neoforge", "47.1.");
}
