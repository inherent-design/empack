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

macro_rules! live_init_loader {
    ($name:ident, $loader:expr) => {
        #[test]
        #[ignore = "requires official runtime catalogs"]
        fn $name() {
            let project = TestProject::new();
            let output = project
                .cmd()
                .args([
                    "init",
                    "--yes",
                    "--modloader",
                    $loader,
                    "--mc-version",
                    "1.21.1",
                    concat!("test-", $loader),
                ])
                .output()
                .expect("failed to spawn");
            assert!(
                output.status.success(),
                "init --modloader {} failed: {}",
                $loader,
                String::from_utf8_lossy(&output.stderr)
            );

            let pack_dir = project.dir().join(concat!("test-", $loader));
            assert_project_initialized(&pack_dir);
            assert_project_loader(&pack_dir, $loader);
            assert_project_minecraft_version(&pack_dir, "1.21.1");
            assert_locked_minecraft_version(&pack_dir, "1.21.1");
        }
    };
}

live_init_loader!(live_cli_init_forge_binds_runtime, "forge");
live_init_loader!(live_cli_init_quilt_binds_runtime, "quilt");

#[test]
#[ignore = "requires official runtime catalogs"]
fn live_cli_init_neoforge_explicit_version_binds_runtime() {
    let project = TestProject::new();
    let mut cmd = project.cmd();
    let output = cmd
        .args([
            "init",
            "--yes",
            "--modloader",
            "neoforge",
            "--loader-version",
            "21.1.224",
            "--mc-version",
            "1.21.1",
            "test-neoforge",
        ])
        .output()
        .expect("failed to spawn");
    assert!(
        output.status.success(),
        "init --modloader neoforge failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let pack_dir = project.dir().join("test-neoforge");
    assert_project_initialized(&pack_dir);
    assert_project_loader(&pack_dir, "neoforge");
    assert_project_minecraft_version(&pack_dir, "1.21.1");
    assert_locked_minecraft_version(&pack_dir, "1.21.1");
}
