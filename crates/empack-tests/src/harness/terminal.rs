//! Native terminal fixture. ConPTY 0.7 fixes redirected-parent handles under Nextest.
#[cfg(not(windows))]
pub fn spawn(command: std::process::Command) -> expectrl::session::OsSession {
    expectrl::Session::spawn(command).unwrap()
}

#[cfg(windows)]
pub fn spawn(command: std::process::Command) -> expectrl::Session<conpty::Process, Stream> {
    // The verification runner's CREATE_NEW_PROCESS_GROUP suppresses Ctrl-C for
    // descendants. This owned test host must give its console children ordinary
    // signal processing. Handler registrations and the outer containment remain.
    assert_ne!(
        unsafe { windows_sys::Win32::System::Console::SetConsoleCtrlHandler(None, 0) },
        0,
        "restore Ctrl-C processing for the owned console fixture: {}",
        std::io::Error::last_os_error()
    );
    // ConPTY constructs its own command line and environment rather than using Command::spawn.
    let mut process = conpty::Process::spawn(conpty_command(&command)).unwrap();
    let stream = Stream {
        reader: process.output().unwrap(),
        writer: process.input().unwrap(),
    };
    expectrl::Session::new(process, stream).unwrap()
}

#[cfg(windows)]
fn conpty_command(command: &std::process::Command) -> std::process::Command {
    // The external harness has already selected a complete isolated environment.
    // Adding ambient variables here would undo its allowlist and env_clear().
    let mut native = std::process::Command::new(quote(command.get_program()));
    native.env_clear();
    assert!(
        command.get_envs().any(|(_, value)| value.is_some()),
        "ConPTY requires an explicit nonempty environment to prevent inheritance"
    );
    for (key, value) in command.get_envs() {
        match value {
            Some(value) => {
                native.env(key, value);
            }
            None => {
                native.env_remove(key);
            }
        }
    }
    if let Some(directory) = command.get_current_dir() {
        native.current_dir(directory);
    }
    native.args(command.get_args().map(quote));
    native
}

#[cfg(windows)]
#[test]
fn conpty_preserves_only_the_explicit_environment() {
    let mut command = std::process::Command::new("fixture.exe");
    command.env_clear().env("ONLY_FIXTURE", "value");
    let native = conpty_command(&command);
    let environment = native.get_envs().collect::<Vec<_>>();
    assert_eq!(environment, command.get_envs().collect::<Vec<_>>());
    assert_eq!(environment.len(), 1);
}

#[cfg(windows)]
fn quote(value: &std::ffi::OsStr) -> std::ffi::OsString {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    let mut result = vec![b'"' as u16];
    let mut slashes = 0;
    for value in value.encode_wide() {
        if value == b'\\' as u16 {
            slashes += 1;
        } else {
            let count = if value == b'"' as u16 {
                2 * slashes + 1
            } else {
                slashes
            };
            result.extend(std::iter::repeat_n(b'\\' as u16, count));
            result.push(value);
            slashes = 0;
        }
    }
    result.extend(std::iter::repeat_n(b'\\' as u16, 2 * slashes));
    result.push(b'"' as u16);
    std::ffi::OsString::from_wide(&result)
}

#[cfg(windows)]
pub struct Stream {
    reader: conpty::io::PipeReader,
    writer: conpty::io::PipeWriter,
}
#[cfg(windows)]
impl std::io::Read for Stream {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        std::io::Read::read(&mut self.reader, buffer)
    }
}
#[cfg(windows)]
impl std::io::Write for Stream {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        std::io::Write::write(&mut self.writer, buffer)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        std::io::Write::flush(&mut self.writer)
    }
}
#[cfg(windows)]
impl expectrl::process::NonBlocking for Stream {
    fn set_blocking(&mut self, blocking: bool) -> std::io::Result<()> {
        self.reader.blocking(blocking);
        Ok(())
    }
}

/// Decline an observed plan and wait for the executable, not just its output.
pub fn decline(command: std::process::Command, prompt: &str) {
    use expectrl::{Expect, Regex};
    let mut terminal = expectrl::session::log(spawn(command), std::io::stderr()).unwrap();
    terminal.set_expect_timeout(Some(std::time::Duration::from_secs(20)));
    terminal.expect(Regex(prompt)).unwrap();
    terminal.send_line("n").unwrap();
    terminal.expect(Regex("(?i)not applied")).unwrap();
    #[cfg(not(windows))]
    {
        terminal.expect(expectrl::Eof).unwrap();
        wait_for_exit(terminal.get_process(), 0);
    }
    // ConPTY's output pipe can outlive the child. Wait on its process handle.
    #[cfg(windows)]
    assert_eq!(terminal.get_process().wait(Some(20_000)).unwrap(), 0);
}

/// Interrupt an actual approval prompt and establish the executable's final status.
pub fn interrupt(command: std::process::Command, prompt: &str) {
    use expectrl::{Expect, Regex};
    let mut terminal = expectrl::session::log(spawn(command), std::io::stderr()).unwrap();
    terminal.set_expect_timeout(Some(std::time::Duration::from_secs(20)));
    terminal.expect(Regex(prompt)).unwrap();
    terminal.send(expectrl::ControlCode::EndOfText).unwrap();
    terminal.expect(Regex("(?i)operation interrupted")).unwrap();
    #[cfg(not(windows))]
    {
        terminal.expect(expectrl::Eof).unwrap();
        wait_for_exit(terminal.get_process(), 130);
    }
    #[cfg(windows)]
    assert_eq!(terminal.get_process().wait(Some(20_000)).unwrap(), 130);
}

/// Establish terminal-child exit after draining its output.
#[cfg(not(windows))]
pub fn wait_for_exit(
    process: &impl expectrl::process::Healthcheck<Status = expectrl::process::unix::WaitStatus>,
    expected: i32,
) {
    use expectrl::process::unix::WaitStatus;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        match process.get_status().unwrap() {
            WaitStatus::Exited(_, code) if code == expected => break,
            WaitStatus::StillAlive => {
                assert!(
                    std::time::Instant::now() < deadline,
                    "terminal child did not exit"
                );
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            status => panic!("terminal child did not exit with {expected}: {status:?}"),
        }
    }
}

#[cfg(unix)]
#[test]
fn terminal_decline_waits_for_exit_after_eof_and_rejects_nonzero_status() {
    for status in [0, 7] {
        let root = tempfile::tempdir().unwrap();
        let marker = root.path().join("finished");
        let outcome = std::panic::catch_unwind(|| {
            let mut command = std::process::Command::new("/bin/sh");
            let script = r#"printf 'Apply fixture plan? '
read answer
printf 'not applied\n'
exec 0<&- 1>&- 2>&-
/bin/sleep 0.1
printf finished > "$1"
exit "$2"
"#;
            command
                .env_clear()
                .args(["-c", script, "fixture"])
                .arg(&marker)
                .arg(status.to_string());
            decline(command, "Apply fixture plan");
        });
        assert_eq!(
            std::fs::read(&marker).unwrap(),
            b"finished",
            "EOF must not let assertions race the still-running child"
        );
        assert_eq!(
            outcome.is_ok(),
            status == 0,
            "the terminal fixture must establish successful process exit"
        );
    }
}
