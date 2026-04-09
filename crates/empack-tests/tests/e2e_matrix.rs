use empack_tests::e2e::{
    TestProject, assert_pack_minecraft_version, assert_project_initialized, assert_project_loader,
    assert_project_loader_absent, assert_project_minecraft_version, configure_fake_packwiz,
    empack_assert_cmd, seed_loader_version_cache,
};
use predicates::prelude::*;

macro_rules! e2e_init_modloader {
    ($name:ident, $loader:expr) => {
        #[test]
        fn $name() {
            empack_tests::skip_if_no_packwiz!();

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
            if $loader == "none" {
                assert_project_loader_absent(&pack_dir);
            } else {
                assert_project_loader(&pack_dir, $loader);
            }
            assert_project_minecraft_version(&pack_dir, "1.21.1");
            assert_pack_minecraft_version(&pack_dir, "1.21.1");
        }
    };
}

e2e_init_modloader!(e2e_matrix_init_fabric, "fabric");
e2e_init_modloader!(e2e_matrix_init_forge, "forge");
// quilt loader not available for MC 1.21.1 in current packwiz
// e2e_init_modloader!(e2e_matrix_init_quilt, "quilt");
e2e_init_modloader!(e2e_matrix_init_vanilla, "none");

#[test]
fn e2e_matrix_init_neoforge() {
    let project = TestProject::new();
    seed_loader_version_cache(project.dir(), "neoforge", "1.21.1", &["21.1.224"]);
    let mut cmd = project.cmd();
    configure_fake_packwiz(&mut cmd, project.dir());
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
    assert_pack_minecraft_version(&pack_dir, "1.21.1");
}

macro_rules! e2e_bad_flag_value {
    ($name:ident, args: [$($arg:expr),+], stderr_contains: $expected:expr) => {
        #[test]
        fn $name() {
            let mut cmd = empack_assert_cmd();
            $(cmd.arg($arg);)+
            cmd.assert()
                .failure()
                .stderr(predicate::str::contains($expected));
        }
    };
}

e2e_bad_flag_value!(
    e2e_matrix_bad_archive_format,
    args: ["build", "--format", "csv", "mrpack"],
    stderr_contains: "invalid value 'csv'"
);

e2e_bad_flag_value!(
    e2e_matrix_bad_platform,
    args: ["add", "--platform", "github", "sodium"],
    stderr_contains: "invalid value 'github'"
);

e2e_bad_flag_value!(
    e2e_matrix_bad_project_type,
    args: ["add", "--type", "world", "sodium"],
    stderr_contains: "invalid value 'world'"
);

macro_rules! e2e_requires_modpack {
    ($name:ident, args: [$($arg:expr),+]) => {
        #[test]
        fn $name() {
            let project = TestProject::new();
            let output = project
                .cmd()
                .args([$($arg),+])
                .output()
                .expect("failed to spawn");
            assert!(
                !output.status.success(),
                "command should fail in empty directory"
            );
        }
    };
}

e2e_requires_modpack!(e2e_matrix_add_requires_modpack, args: ["add", "sodium"]);
e2e_requires_modpack!(e2e_matrix_remove_requires_modpack, args: ["remove", "sodium"]);
e2e_requires_modpack!(e2e_matrix_sync_requires_modpack, args: ["sync"]);
e2e_requires_modpack!(e2e_matrix_build_requires_modpack, args: ["build", "mrpack"]);
// clean in an empty directory exits 0 ("nothing to clean" is valid)

macro_rules! e2e_build_target {
    ($name:ident, $target:expr) => {
        #[test]
        fn $name() {
            empack_tests::skip_if_no_java!();

            let project = TestProject::initialized("test-pack", "fabric", "1.21.1");
            let output = project
                .cmd()
                .args(["build", $target])
                .output()
                .expect("failed to spawn");
            assert!(
                output.status.success(),
                "build {} failed: {}",
                $target,
                String::from_utf8_lossy(&output.stderr)
            );

            let dist = project.dir().join("dist");
            assert!(dist.exists(), "dist/ should exist after build {}", $target);
        }
    };
}

e2e_build_target!(e2e_matrix_build_mrpack, "mrpack");
e2e_build_target!(e2e_matrix_build_server, "server");
e2e_build_target!(e2e_matrix_build_client, "client");

macro_rules! e2e_no_args_succeeds {
    ($name:ident, args: [$($arg:expr),+]) => {
        #[test]
        fn $name() {
            empack_assert_cmd()
                .args([$($arg),+])
                .assert()
                .success();
        }
    };
}

e2e_no_args_succeeds!(e2e_matrix_version, args: ["version"]);
e2e_no_args_succeeds!(e2e_matrix_help, args: ["--help"]);
e2e_no_args_succeeds!(e2e_matrix_help_init, args: ["init", "--help"]);
e2e_no_args_succeeds!(e2e_matrix_help_add, args: ["add", "--help"]);
e2e_no_args_succeeds!(e2e_matrix_help_remove, args: ["remove", "--help"]);
e2e_no_args_succeeds!(e2e_matrix_help_build, args: ["build", "--help"]);
e2e_no_args_succeeds!(e2e_matrix_help_sync, args: ["sync", "--help"]);
e2e_no_args_succeeds!(e2e_matrix_help_clean, args: ["clean", "--help"]);
e2e_no_args_succeeds!(e2e_matrix_help_requirements, args: ["requirements", "--help"]);
