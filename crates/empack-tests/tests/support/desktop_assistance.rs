use empack_tests::harness::process::TestProject;
use std::{fs, path::Path};

#[cfg(unix)]
pub fn browser_fixture() -> (TestProject, tempfile::TempDir) {
    use empack_core::{
        identity::{ModrinthProjectId, ProviderProjectId},
        model::ResolvedPin,
    };
    let identity = ProviderProjectId::Modrinth(ModrinthProjectId::parse("w0TnApzs").unwrap());
    let project = empack_tests::fixtures::restricted::with_provider(
        "browser-native",
        ResolvedPin {
            selection: identity.parse_pin("lDYpMiqk").unwrap(),
            project: identity,
        },
    );
    let host = tempfile::tempdir().unwrap();
    let (opener, _) = empack_lib::platform::browser_open_command();
    fs::create_dir(host.path().join("bin")).unwrap();
    fs::create_dir(host.path().join("downloads")).unwrap();
    let file = host.path().join("bin").join(opener);
    fs::write(&file, "#!/bin/sh\nset -eu\nprintf '%s\\n' \"$@\" >> \"$EMPACK_TEST_BROWSER_LOG\"\nprintf payload > \"$EMPACK_TEST_BROWSER_FILE\"\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&file, fs::Permissions::from_mode(0o755)).unwrap();
    (project, host)
}
#[cfg(unix)]
pub fn browser_command(project: &TestProject, host: &Path) -> std::process::Command {
    let mut cmd = project.cmd();
    let paths = std::env::var_os("PATH").unwrap_or_default();
    cmd.env(
        "PATH",
        std::env::join_paths(
            std::iter::once(host.join("bin")).chain(std::env::split_paths(&paths)),
        )
        .unwrap(),
    );
    cmd.env("EMPACK_TEST_BROWSER_LOG", host.join("opened"));
    cmd.env(
        "EMPACK_TEST_BROWSER_FILE",
        host.join("downloads/renamed.bin"),
    );
    cmd.arg("--state-dir").arg(host.join("state"));
    cmd
}
