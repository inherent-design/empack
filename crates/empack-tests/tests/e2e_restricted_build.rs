use empack_tests::e2e::{
    TestProject, assert_pending_restricted_build, load_pending_restricted_build,
    seed_packwiz_installer_jars,
};
use std::path::{Path, PathBuf};

fn write_executable(path: &Path, script: &str) {
    std::fs::write(path, script)
        .unwrap_or_else(|e| panic!("failed to write {}: {}", path.display(), e));

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(path)
            .expect("script metadata")
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(path, perms).expect("set executable bit");
    }
}

#[cfg(windows)]
fn write_fake_restricted_mrpack_packwiz_binary(workdir: &Path, import_dir: &Path) -> PathBuf {
    let path = workdir.join("fake-restricted-packwiz.cmd");
    let script = format!(
        "@echo off\r\nsetlocal EnableExtensions\r\nif /I \"%~3\"==\"mr\" if /I \"%~4\"==\"export\" goto restricted\r\nif /I \"%~3\"==\"refresh\" exit /b 0\r\nexit /b 0\r\n:restricted\r\necho Found 1 manual downloads; these mods are unable to be downloaded by packwiz (due to API limitations) and must be manually downloaded:\r\necho Bee Fix ^(BeeFix-1.20-1.0.7.jar^) from https://www.curseforge.com/minecraft/mc-mods/bee-fix/files/4618962\r\n1>&2 echo Once you have done so, place these files in {} and re-run this command.\r\nexit /b 1\r\n",
        import_dir.display()
    );
    write_executable(&path, &script);
    path
}

#[cfg(not(windows))]
fn write_fake_restricted_mrpack_packwiz_binary(workdir: &Path, import_dir: &Path) -> PathBuf {
    let path = workdir.join("fake-restricted-packwiz");
    let script = format!(
        "#!/bin/sh\nset -eu\nif [ \"${{3-}}\" = \"mr\" ] && [ \"${{4-}}\" = \"export\" ]; then\n  printf 'Found 1 manual downloads; these mods are unable to be downloaded by packwiz (due to API limitations) and must be manually downloaded:\\n'\n  printf 'Bee Fix (BeeFix-1.20-1.0.7.jar) from https://www.curseforge.com/minecraft/mc-mods/bee-fix/files/4618962\\n'\n  printf 'Once you have done so, place these files in {} and re-run this command.\\n' >&2\n  exit 1\nfi\nif [ \"${{3-}}\" = \"refresh\" ]; then\n  exit 0\nfi\nexit 0\n",
        import_dir.display()
    );
    write_executable(&path, &script);
    path
}

fn project_assert_cmd(project: &TestProject) -> assert_cmd::Command {
    assert_cmd::Command::from_std(project.cmd())
}

fn combined_output(output: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn e2e_build_mrpack_restricted_records_pending_state() {
    let project = TestProject::workflow_fixture("restricted-mrpack", "fabric", "1.21.1");
    let import_dir = project.dir().join("fake-packwiz-cache").join("import");
    let fake_packwiz = write_fake_restricted_mrpack_packwiz_binary(project.dir(), &import_dir);

    let mut cmd = project_assert_cmd(&project);
    cmd.args(["build", "mrpack", "--yes"]);
    cmd.env("EMPACK_PACKWIZ_BIN", fake_packwiz);

    let assert = cmd.assert().failure();
    let output = assert.get_output();
    let combined = combined_output(output);
    assert!(
        combined.contains("empack build --continue"),
        "restricted mrpack build should point to continuation flow:\n{combined}"
    );

    let pending =
        assert_pending_restricted_build(project.dir(), &["mrpack"], &["BeeFix-1.20-1.0.7.jar"]);
    assert_eq!(
        pending.entries[0].url,
        "https://www.curseforge.com/minecraft/mc-mods/bee-fix/download/4618962"
    );
    assert_eq!(
        pending.entries[0].dest_path,
        import_dir.join("BeeFix-1.20-1.0.7.jar").to_string_lossy()
    );
}

#[test]
fn e2e_build_all_restricted_at_mrpack_stops_before_later_targets() {
    let project = TestProject::workflow_fixture("restricted-all", "fabric", "1.21.1");
    seed_packwiz_installer_jars(project.dir());
    let import_dir = project.dir().join("fake-packwiz-cache").join("import");
    let fake_packwiz = write_fake_restricted_mrpack_packwiz_binary(project.dir(), &import_dir);

    let mut cmd = project_assert_cmd(&project);
    cmd.args(["build", "all", "--yes"]);
    cmd.env("EMPACK_PACKWIZ_BIN", fake_packwiz);

    let assert = cmd.assert().failure();
    let output = assert.get_output();
    let combined = combined_output(output);
    assert!(
        combined.contains("empack build --continue"),
        "restricted all build should point to continuation flow:\n{combined}"
    );

    let pending = assert_pending_restricted_build(
        project.dir(),
        &["mrpack", "client", "server", "client-full", "server-full"],
        &["BeeFix-1.20-1.0.7.jar"],
    );
    assert_eq!(
        pending.entries[0].url,
        "https://www.curseforge.com/minecraft/mc-mods/bee-fix/download/4618962"
    );
    assert_eq!(
        pending.entries[0].dest_path,
        import_dir.join("BeeFix-1.20-1.0.7.jar").to_string_lossy()
    );
    assert!(
        !project.dir().join("dist").join("client").exists(),
        "client output should not be created after mrpack is blocked"
    );
    assert!(
        !project.dir().join("dist").join("server").exists(),
        "server output should not be created after mrpack is blocked"
    );
    assert!(
        !project.dir().join("dist").join("client-full").exists(),
        "client-full output should not be created after mrpack is blocked"
    );
    assert!(
        !project.dir().join("dist").join("server-full").exists(),
        "server-full output should not be created after mrpack is blocked"
    );
    assert!(
        load_pending_restricted_build(project.dir())
            .expect("load pending restricted build")
            .is_some(),
        "pending restricted build should persist after the failed all build"
    );
}
