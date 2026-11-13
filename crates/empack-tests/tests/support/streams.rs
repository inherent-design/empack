//! Separate native streams while retaining the real CLI's controlling terminal.
use empack_tests::harness::{process::offline_empack_cmd, terminal, workspace::snapshot};
use expectrl::{Expect, Regex};
use std::{fs, path::Path, process::Command, time::Duration};

fn initialize(root: &Path, yes: bool, color: &str) -> Command {
    let mut command = offline_empack_cmd(root);
    command
        .env_remove("NO_COLOR")
        .env("TERM", "xterm-256color")
        .env("LC_ALL", "en_US.UTF-8")
        .args([
            "--color",
            color,
            "init",
            "--modloader",
            "none",
            "--mc-version",
            "1.99",
            "--java",
            "21",
            "--pack-name",
            "Stream Ω 日本",
            "--pack-version",
            "1",
            "--author",
            "Fixture",
            "pack",
        ]);
    if yes {
        command.arg("--yes");
    }
    command
}

fn redirect(command: Command, path: &Path, stdout: bool) -> Command {
    let mut shell = Command::new("/bin/sh");
    shell.env_clear();
    for (key, value) in command.get_envs() {
        if let Some(value) = value {
            shell.env(key, value);
        }
    }
    shell
        .current_dir(command.get_current_dir().unwrap())
        .env("STREAM_CAPTURE_PATH", path)
        // The shell only redirects a stream. Native arguments remain literal
        // positional values; paths and pack names never become shell source.
        .args([
            "-c",
            if stdout {
                "exec \"$@\" > \"$STREAM_CAPTURE_PATH\""
            } else {
                "exec \"$@\" 2> \"$STREAM_CAPTURE_PATH\""
            },
            "empack-stream-fixture",
        ])
        .arg(command.get_program())
        .args(command.get_args());
    shell
}

fn styled(bytes: &[u8]) -> bool {
    bytes.windows(2).enumerate().any(|(offset, prefix)| {
        prefix == b"\x1b["
            && bytes[offset + 2..]
                .iter()
                .copied()
                .find(|byte| !byte.is_ascii_digit() && *byte != b';')
                == Some(b'm')
    })
}

#[test]
fn cli_output_capabilities_follow_each_destination() {
    for stdout in [true, false] {
        for color in ["auto", "always", "never"] {
            let root = tempfile::tempdir().unwrap();
            let capture = tempfile::NamedTempFile::new().unwrap();
            let command = redirect(initialize(root.path(), true, color), capture.path(), stdout);
            let mut session = terminal::spawn(command);
            session.set_expect_timeout(Some(Duration::from_secs(20)));
            let shown = session.expect(expectrl::Eof).unwrap();
            terminal::wait_for_exit(session.get_process(), 0);
            let redirected = fs::read(capture.path()).unwrap();
            assert_eq!(
                styled(&redirected),
                color == "always",
                "redirected stdout={stdout}, color={color}: {}",
                String::from_utf8_lossy(&redirected)
            );
            assert_eq!(
                styled(shown.as_bytes()),
                color != "never",
                "terminal output stdout={}, color={color}: {}",
                !stdout,
                String::from_utf8_lossy(shown.as_bytes())
            );
            let all = [redirected.as_slice(), shown.as_bytes()].concat();
            let all = String::from_utf8_lossy(&all);
            assert!(all.contains("Initialized Stream Ω 日本"), "{all}");
            assert!(all.contains("Java requirements are not known"), "{all}");
            assert!(root.path().join("pack/empack.yml").is_file());
        }
    }
}

#[test]
fn cli_prompt_uses_input_and_stderr_instead_of_result_stdout() {
    for stdout in [true, false] {
        let root = tempfile::tempdir().unwrap();
        let capture = tempfile::NamedTempFile::new().unwrap();
        let command = redirect(
            initialize(root.path(), false, "never"),
            capture.path(),
            stdout,
        );
        let before = snapshot(root.path());
        let mut session = terminal::spawn(command);
        session.set_expect_timeout(Some(Duration::from_secs(20)));
        if stdout {
            session
                .expect(Regex("Apply this initialization plan"))
                .unwrap();
            session.send_line("n").unwrap();
            session.expect(expectrl::Eof).unwrap();
            terminal::wait_for_exit(session.get_process(), 0);
            assert!(
                String::from_utf8_lossy(&fs::read(capture.path()).unwrap()).contains("not applied")
            );
        } else {
            session.expect(expectrl::Eof).unwrap();
            terminal::wait_for_exit(session.get_process(), 2);
            let diagnostic = fs::read_to_string(capture.path()).unwrap();
            assert!(
                diagnostic.contains("Noninteractive execution requires --yes"),
                "{diagnostic}"
            );
            assert!(
                !diagnostic.contains("Apply this initialization plan"),
                "{diagnostic}"
            );
        }
        assert!(!root.path().join("pack").exists());
        assert_eq!(snapshot(root.path()), before);
    }
}
