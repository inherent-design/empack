use empack_tests::harness::process::{TestProject, offline_command};
use predicates::prelude::*;

macro_rules! cli_bad_flag_value {
    ($name:ident, args: [$($arg:expr),+], stderr_contains: $expected:expr) => {
        #[test]
        fn $name() {
            let project = TestProject::new();
            let mut cmd = offline_command(project.dir());
            $(cmd.arg($arg);)+
            cmd.assert()
                .failure()
                .stderr(predicate::str::contains($expected));
        }
    };
}

cli_bad_flag_value!(
    cli_matrix_bad_archive_format,
    args: ["build", "--format", "csv", "modrinth"],
    stderr_contains: "invalid value 'csv'"
);

cli_bad_flag_value!(
    cli_matrix_bad_platform,
    args: ["add", "--platform", "github", "sodium"],
    stderr_contains: "invalid value 'github'"
);

cli_bad_flag_value!(
    cli_matrix_bad_project_type,
    args: ["add", "--type", "invalid-content", "sodium"],
    stderr_contains: "invalid value 'invalid-content'"
);

macro_rules! cli_requires_modpack {
    ($name:ident, args: [$($arg:expr),+]) => {
        #[test]
        fn $name() {
            let project = TestProject::new();
            let output = offline_command(project.dir())
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

cli_requires_modpack!(cli_matrix_add_requires_modpack, args: ["add", "sodium"]);
cli_requires_modpack!(cli_matrix_remove_requires_modpack, args: ["remove", "sodium"]);
cli_requires_modpack!(cli_matrix_sync_requires_modpack, args: ["sync"]);
cli_requires_modpack!(cli_matrix_build_requires_modpack, args: ["build", "modrinth"]);
// clean in an empty directory exits 0 ("nothing to clean" is valid)

macro_rules! cli_no_args_succeeds {
    ($name:ident, args: [$($arg:expr),+]) => {
        #[test]
        fn $name() {
            let project = TestProject::new();
            offline_command(project.dir())
                .args([$($arg),+])
                .assert()
                .success();
        }
    };
}

cli_no_args_succeeds!(cli_matrix_version, args: ["version"]);
cli_no_args_succeeds!(cli_matrix_help, args: ["--help"]);
cli_no_args_succeeds!(cli_matrix_help_init, args: ["init", "--help"]);
cli_no_args_succeeds!(cli_matrix_help_add, args: ["add", "--help"]);
cli_no_args_succeeds!(cli_matrix_help_remove, args: ["remove", "--help"]);
cli_no_args_succeeds!(cli_matrix_help_build, args: ["build", "--help"]);
cli_no_args_succeeds!(cli_matrix_help_sync, args: ["sync", "--help"]);
cli_no_args_succeeds!(cli_matrix_help_clean, args: ["clean", "--help"]);
cli_no_args_succeeds!(cli_matrix_help_requirements, args: ["requirements", "--help"]);
