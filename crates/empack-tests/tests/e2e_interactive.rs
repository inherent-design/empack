use empack_tests::e2e::{
    TestProject, assert_pack_loader_version, assert_pending_restricted_build,
    assert_project_initialized, assert_project_loader, assert_project_minecraft_version,
    configure_fake_packwiz, empack_cmd, load_pending_restricted_build, seed_packwiz_installer_jars,
};
use expectrl::{Expect, Regex, Session};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

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

fn prepend_path(cmd: &mut Command, dir: &Path) {
    let current = std::env::var_os("PATH").unwrap_or_default();
    let joined = std::env::join_paths(
        std::iter::once(dir.to_path_buf()).chain(std::env::split_paths(&current)),
    )
    .expect("join PATH");
    cmd.env("PATH", joined);
}

#[cfg(windows)]
fn write_fake_build_packwiz_binary(workdir: &Path) -> PathBuf {
    let path = workdir.join("fake-build-packwiz.cmd");
    write_executable(&path, "@echo off\r\nexit /b 0\r\n");
    path
}

#[cfg(not(windows))]
fn write_fake_build_packwiz_binary(workdir: &Path) -> PathBuf {
    let path = workdir.join("fake-build-packwiz");
    write_executable(&path, "#!/bin/sh\nset -eu\nexit 0\n");
    path
}

#[cfg(windows)]
fn write_fake_java_binary(bin_dir: &Path) {
    let path = bin_dir.join("java.cmd");
    let script = "@echo off\r\nsetlocal EnableExtensions\r\nset \"DEST=%CD%\\mods\\OptiFine.jar\"\r\necho Failed to download modpack, the following errors were encountered:\r\necho OptiFine.jar:\r\necho java.lang.Exception: This mod is excluded from the CurseForge API and must be downloaded manually. 1>&2\r\necho Please go to https://www.curseforge.com/minecraft/mc-mods/optifine/files/4912891 and save this file to %DEST% 1>&2\r\necho \tat link.infra.packwiz.installer.DownloadTask.download(DownloadTask.java:42) 1>&2\r\nexit /b 1\r\n";
    write_executable(&path, script);
}

#[cfg(not(windows))]
fn write_fake_java_binary(bin_dir: &Path) {
    let path = bin_dir.join("java");
    let script = "#!/bin/sh\nset -eu\nDEST=\"$PWD/mods/OptiFine.jar\"\nprintf 'Failed to download modpack, the following errors were encountered:\\n'\nprintf 'OptiFine.jar:\\n'\nprintf 'java.lang.Exception: This mod is excluded from the CurseForge API and must be downloaded manually.\\nPlease go to https://www.curseforge.com/minecraft/mc-mods/optifine/files/4912891 and save this file to %s\\n\\tat link.infra.packwiz.installer.DownloadTask.download(DownloadTask.java:42)\\n' \"$DEST\" >&2\nexit 1\n";
    write_executable(&path, script);
}

#[cfg(not(windows))]
fn write_fake_browser_binary(bin_dir: &Path, log_path: &Path) {
    let (browser_cmd, _) = empack_lib::platform::browser_open_command();
    let path = bin_dir.join(browser_cmd);
    let script = format!(
        "#!/bin/sh\nset -eu\nprintf '%s\\n' \"$@\" >> '{}'\nexit 0\n",
        log_path.display()
    );
    write_executable(&path, &script);
}

fn wait_for_pending(
    project_dir: &Path,
) -> empack_lib::empack::restricted_build::PendingRestrictedBuild {
    for _ in 0..50 {
        if load_pending_restricted_build(project_dir)
            .expect("load pending restricted build")
            .is_some()
        {
            return assert_pending_restricted_build(
                project_dir,
                &["client-full"],
                &["OptiFine.jar"],
            );
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    panic!(
        "pending restricted build was not recorded under {}",
        project_dir.display()
    );
}

fn wait_for_path(path: &Path) {
    for _ in 0..50 {
        if path.exists() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    panic!("expected path at {}", path.display());
}

// Reference only: kept as a manual verification aid, not run in CI.
//
// The interactive init flow relies on dialoguer terminal widgets (FuzzySelect,
// Input) which render escape sequences that are difficult to match reliably
// across terminal emulators and CI environments. This test exercises the prompt
// sequence through a PTY but dialoguer rendering varies across platforms and
// CI runners, so it remains permanently ignored.
#[test]
#[ignore]
fn e2e_init_interactive_prompts() {
    empack_tests::skip_if_no_packwiz!();

    let project = TestProject::new();
    let mut cmd = empack_cmd(project.dir());
    cmd.args(["init", "test-pack"]);

    let mut session = Session::spawn(cmd).expect("failed to spawn empack init");
    session.set_expect_timeout(Some(Duration::from_secs(30)));

    // Prompt 1: Modpack name (dialoguer Input, default: "test-pack")
    let _ = session
        .expect(Regex("(?i)modpack.*name|name"))
        .expect("expected modpack name prompt");
    session
        .send_line("")
        .expect("failed to accept default name");

    // Prompt 2: Author (dialoguer Input, default: git user.name)
    let _ = session
        .expect(Regex("(?i)author"))
        .expect("expected author prompt");
    session
        .send_line("")
        .expect("failed to accept default author");

    // Prompt 3: Version (dialoguer Input, default: "1.0.0")
    let _ = session
        .expect(Regex("(?i)version"))
        .expect("expected version prompt");
    session
        .send_line("")
        .expect("failed to accept default version");

    // Network fetch for Minecraft versions happens here.
    // Prompt 4: Minecraft version (dialoguer FuzzySelect)
    let _ = session
        .expect(Regex("(?i)minecraft.*version|version"))
        .expect("expected minecraft version prompt");
    session
        .send_line("1.21.1")
        .expect("failed to send minecraft version");

    // Prompt 5: Mod loader (dialoguer Select or FuzzySelect)
    match session.expect(Regex("(?i)loader|compatible")) {
        Ok(_) => {
            session
                .send_line("fabric")
                .expect("failed to send loader selection");
        }
        Err(_) => {
            // Process may have chosen defaults or exited; continue.
        }
    }

    // Prompt 6: Loader version (dialoguer FuzzySelect, if shown)
    match session.expect(Regex("(?i)loader.*version|fabric")) {
        Ok(_) => {
            let _ = session.send_line("");
        }
        Err(_) => {
            // May not appear if the process already completed.
        }
    }

    // Wait for the process to produce final output or exit.
    let _ = session.expect(Regex("(?i)initialized|created|successfully"));

    let pack_dir = project.dir().join("test-pack");
    wait_for_path(&pack_dir.join("empack.yml"));
    assert!(
        pack_dir.join("empack.yml").exists(),
        "empack.yml not found after interactive init"
    );
}

/// Interactive init with CLI flags that pre-fill the hard prompts.
///
/// Uses `--modloader`, `--mc-version`, and `--loader-version` flags to bypass
/// FuzzySelect/Select widgets, which are unreliable under PTY. Only
/// line-oriented text prompts and the confirm prompt remain. On Windows,
/// dialoguer prompt widgets under ConPTY do not reliably bind pre-sent input
/// lines to the exact text fields, so this test only asserts the stable PTY
/// contract: the command completes successfully and creates the project.
///
#[test]
fn e2e_init_interactive_responds_to_prompts() {
    let project = TestProject::new();
    let mut cmd = empack_cmd(project.dir());
    configure_fake_packwiz(&mut cmd, project.dir());
    cmd.args([
        "init",
        "--modloader",
        "fabric",
        "--mc-version",
        "1.21.1",
        "--loader-version",
        "0.18.6",
        "interactive-test",
    ]);

    let mut session = Session::spawn(cmd).expect("failed to spawn empack init");
    session.set_expect_timeout(Some(Duration::from_secs(30)));

    // Keep the active PTY contract focused on resulting data, not exact prompt
    // rendering. We still use broad prompt-shaped regexes as pacing points so
    // the instrumented binary cannot outrun the input stream.
    let _ = session.expect(Regex("(?i)name"));
    session
        .send_line("my-test-pack")
        .expect("failed to send pack name");
    let _ = session.expect(Regex("(?i)author"));
    session
        .send_line("Test Author")
        .expect("failed to send author");
    let _ = session.expect(Regex("(?i)version"));
    session
        .send_line("")
        .expect("failed to accept default version");
    let _ = session.expect(Regex("(?i)datapack|folder|skip"));
    session
        .send_line("")
        .expect("failed to skip datapack folder");
    let _ = session.expect(Regex("(?i)create|settings|confirm"));
    session.send_line("y").expect("failed to confirm");

    // Wait for completion
    let _ = session.expect(Regex("(?i)initialized|created|successfully"));

    let pack_dir = project.dir().join("interactive-test");
    wait_for_path(&pack_dir.join("empack.yml"));
    wait_for_path(&pack_dir.join("pack").join("pack.toml"));
    assert_project_initialized(&pack_dir);
    assert_project_loader(&pack_dir, "fabric");
    assert_project_minecraft_version(&pack_dir, "1.21.1");
    assert_pack_loader_version(&pack_dir, "fabric", "0.18.6");
}

#[test]
fn e2e_build_restricted_browser_confirm_decline_preserves_pending_state() {
    let project = TestProject::workflow_fixture("browser-confirm-pty", "fabric", "1.21.1");
    seed_packwiz_installer_jars(project.dir());

    let fake_packwiz = write_fake_build_packwiz_binary(project.dir());
    let fake_bin_dir = project.dir().join("fake-bin");
    std::fs::create_dir_all(&fake_bin_dir).expect("create fake bin dir");
    write_fake_java_binary(&fake_bin_dir);

    let mut cmd = empack_cmd(project.dir());
    cmd.args(["build", "client-full"]);
    cmd.env("EMPACK_PACKWIZ_BIN", fake_packwiz);
    prepend_path(&mut cmd, &fake_bin_dir);

    let mut session = Session::spawn(cmd).expect("failed to spawn empack build");
    session.set_expect_timeout(Some(Duration::from_secs(30)));
    session
        .send_line("n")
        .expect("failed to decline browser open");
    let _ = session.expect(Regex("(?i)build --continue"));

    let pending = wait_for_pending(project.dir());
    assert_eq!(pending.entries.len(), 1);
    let dist_dir = project.dir().join("dist");
    let has_archive = std::fs::read_dir(&dist_dir)
        .ok()
        .into_iter()
        .flat_map(|entries| entries.filter_map(|entry| entry.ok()))
        .map(|entry| entry.path())
        .any(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(".zip"))
        });
    assert!(
        !has_archive,
        "failed build should not produce a client-full archive"
    );
}

#[cfg(not(windows))]
#[test]
fn e2e_build_restricted_browser_confirm_accept_launches_browser_opener() {
    let project = TestProject::workflow_fixture("browser-confirm-open", "fabric", "1.21.1");
    seed_packwiz_installer_jars(project.dir());

    let fake_packwiz = write_fake_build_packwiz_binary(project.dir());
    let fake_bin_dir = project.dir().join("fake-bin");
    std::fs::create_dir_all(&fake_bin_dir).expect("create fake bin dir");
    write_fake_java_binary(&fake_bin_dir);
    let browser_log = project.dir().join("browser-open.log");
    write_fake_browser_binary(&fake_bin_dir, &browser_log);

    let mut cmd = empack_cmd(project.dir());
    cmd.args(["build", "client-full"]);
    cmd.env("EMPACK_PACKWIZ_BIN", fake_packwiz);
    prepend_path(&mut cmd, &fake_bin_dir);

    let mut session = Session::spawn(cmd).expect("failed to spawn empack build");
    session.set_expect_timeout(Some(Duration::from_secs(30)));
    session
        .send_line("y")
        .expect("failed to accept browser open");
    let _ = session.expect(Regex("(?i)build --continue"));

    let pending = wait_for_pending(project.dir());
    assert_eq!(pending.entries.len(), 1);

    for _ in 0..20 {
        if browser_log.exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    let opened = std::fs::read_to_string(&browser_log)
        .unwrap_or_else(|_| panic!("failed to read {}", browser_log.display()));
    assert!(
        opened.contains("https://www.curseforge.com/minecraft/mc-mods/optifine/files/4912891"),
        "browser opener should receive the restricted download URL, got:\n{opened}"
    );
}
