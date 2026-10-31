//! Run the real inherited supervisor in a separate session with a controlling PTY.
use super::*;
use std::{
    fs::File,
    io::{Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::process::CommandExt,
    },
    process::{Child, Command, Stdio},
    time::Instant,
};

const HOST: &str = "application::process_runtime::terminal_tests::terminal_host_fixture";
const CHILD: &str = "application::process_runtime::terminal_tests::terminal_child_fixture";

fn fixture(name: &str, role: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command.args(["--exact", name, "--nocapture", "--test-threads=1"]);
    command.env("EMPACK_PTY_ROLE", role);
    command
}

struct Pty {
    master: Option<File>,
    host: Child,
    output: String,
    finished: bool,
}
impl Pty {
    fn start(mode: &str, root: &std::path::Path) -> Self {
        let (mut master, mut slave) = (-1, -1);
        assert_eq!(
            unsafe {
                libc::openpty(
                    &mut master,
                    &mut slave,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            },
            0
        );
        // openpty does not promise close-on-exec; never leak the master into
        // the session whose exit depends on the terminal being drained.
        for fd in [master, slave] {
            assert_eq!(
                unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) },
                0
            );
        }
        let master = unsafe { File::from_raw_fd(master) };
        let slave = unsafe { File::from_raw_fd(slave) };
        let mut command = fixture(HOST, "host");
        command.env("EMPACK_PTY_MODE", mode).current_dir(root);
        command
            .stdin(Stdio::from(slave.try_clone().unwrap()))
            .stdout(Stdio::from(slave.try_clone().unwrap()))
            .stderr(Stdio::from(slave));
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() < 0 || libc::ioctl(0, libc::TIOCSCTTY as _, 0) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        Self {
            master: Some(master),
            host: command.spawn().unwrap(),
            output: String::new(),
            finished: false,
        }
    }
    fn expect(&mut self, text: &str) {
        let deadline = Instant::now() + Duration::from_secs(15);
        while !self.output.contains(text) {
            assert!(
                Instant::now() < deadline,
                "missing {text:?}: {}",
                self.output
            );
            let mut poll = libc::pollfd {
                fd: self.master.as_ref().unwrap().as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            if unsafe { libc::poll(&mut poll, 1, 100) } > 0 {
                let mut bytes = [0; 4096];
                match self.master.as_mut().unwrap().read(&mut bytes) {
                    Ok(0) => panic!("PTY closed before {text:?}: {}", self.output),
                    Ok(n) => self.output.push_str(&String::from_utf8_lossy(&bytes[..n])),
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(e) => panic!("PTY read failed {e}, expected {text:?}: {}", self.output),
                }
            }
        }
    }
    fn send(&mut self, bytes: &[u8]) {
        self.master.as_mut().unwrap().write_all(bytes).unwrap();
    }
    fn finish(mut self) {
        self.expect("TERMINAL_RESTORED");
        // Keep draining until session exit: waiting without reading can block
        // terminal teardown on Unix even after the restoration marker arrived.
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = self.host.try_wait().unwrap() {
                self.finished = true;
                assert!(status.success(), "{}", self.output);
                break;
            }
            assert!(
                Instant::now() < deadline,
                "PTY host did not exit: {}",
                self.output
            );
            let mut poll = libc::pollfd {
                fd: self.master.as_ref().unwrap().as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            if unsafe { libc::poll(&mut poll, 1, 50) } > 0 {
                let mut bytes = [0; 4096];
                if let Ok(n) = self.master.as_mut().unwrap().read(&mut bytes) {
                    self.output.push_str(&String::from_utf8_lossy(&bytes[..n]));
                }
            }
        }
    }
}
impl Drop for Pty {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        // Close the master before waiting so session teardown cannot wait on us.
        drop(self.master.take());
        // A failing assertion must not leave the separate runtime group behind.
        if let Some(pid) = self
            .output
            .split("RUNTIME_PID=")
            .nth(1)
            .and_then(|s| s.split_whitespace().next())
            .and_then(|s| s.parse::<i32>().ok())
        {
            unsafe {
                libc::kill(-pid, libc::SIGKILL);
            }
        }
        let _ = self.host.kill();
        let _ = self.host.wait();
    }
}

#[test]
fn inherited_runtime_owns_terminal_and_restores_it() {
    for mode in [
        "stop",
        "interrupt",
        "cancel",
        "failure",
        "spawn-failure",
        "suspend-key",
        "external-resume",
        "cancel-stopped",
    ] {
        let root = tempfile::tempdir().unwrap();
        let mut pty = Pty::start(mode, root.path());
        if mode != "spawn-failure" {
            pty.expect("CONSOLE_READY");
            match mode {
                "stop" => {
                    pty.send(b"list\n");
                    pty.expect("CONSOLE_INPUT=list");
                    pty.send(b"stop\n");
                    pty.expect("CONSOLE_STOPPED");
                }
                "interrupt" => {
                    pty.send(&[3]);
                    pty.expect("INTERRUPTED_GRACEFULLY");
                }
                "suspend-key" => {
                    pty.send(&[26, b'\n']);
                    pty.expect("CONSOLE_INPUT=\u{1a}");
                    pty.send(b"list\n");
                    pty.expect("CONSOLE_INPUT=list");
                    pty.send(b"stop\n");
                }
                "external-resume" => {
                    pty.expect("RUNTIME_STOPPED");
                    std::fs::write(root.path().join("resume"), b"resume").unwrap();
                    pty.expect("RUNTIME_RESUMED");
                    pty.send(b"list\n");
                    pty.expect("CONSOLE_INPUT=list");
                    pty.send(b"stop\n");
                }
                "cancel-stopped" => {
                    pty.expect("RUNTIME_STOPPED");
                }
                "cancel" => std::fs::write(root.path().join("cancel"), b"cancel").unwrap(),
                "failure" => pty.send(b"fail\n"),
                _ => unreachable!(),
            }
        }
        pty.finish();
    }
}

#[test]
fn terminal_host_fixture() {
    if std::env::var("EMPACK_PTY_ROLE").as_deref() != Ok("host") {
        return;
    }
    let mode = std::env::var("EMPACK_PTY_MODE").unwrap();
    let group = unsafe { libc::tcgetpgrp(0) };
    assert_eq!(group, unsafe { libc::getpgrp() });
    let mut before = unsafe { std::mem::zeroed::<libc::termios>() };
    assert_eq!(unsafe { libc::tcgetattr(0, &mut before) }, 0);
    if mode == "stop" {
        let owner = terminal::ForegroundTerminal::capture().unwrap().unwrap();
        let error = terminal::ForegroundTerminal::capture()
            .err()
            .expect("duplicate tty owner");
        assert!(format!("{error:#}").contains("already owns"));
        drop(owner);
        let status = fixture(CHILD, "background")
            .process_group(0)
            .status()
            .unwrap();
        assert!(status.success());
        assert_eq!(unsafe { libc::tcgetpgrp(0) }, group);
    }
    let command = if mode == "spawn-failure" {
        Command::new("/empack-missing-runtime")
    } else {
        fixture(CHILD, "child")
    };
    let cancel = Cancellation::default();
    let token = cancel.clone();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let result = runtime.block_on(async {
        let cancel_task = tokio::spawn(async move {
            let deadline = Instant::now() + Duration::from_secs(10);
            while !std::path::Path::new("cancel").exists() && Instant::now() < deadline {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            token.cancel();
        });
        let stop_mode = mode.clone();
        let stopped_cancel = cancel.clone();
        let stop_task = tokio::spawn(async move {
            if !matches!(stop_mode.as_str(), "external-resume" | "cancel-stopped") {
                return;
            }
            while !std::path::Path::new("runtime-pid").exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            let pid: i32 = std::fs::read_to_string("runtime-pid")
                .unwrap()
                .parse()
                .unwrap();
            assert_eq!(unsafe { libc::kill(-pid, libc::SIGSTOP) }, 0);
            // Observe the direct child's stop without mistaking it for exit.
            // The child cannot exit here until the test supplies input/resumes it.
            let mut status = 0;
            loop {
                let observed =
                    unsafe { libc::waitpid(pid, &mut status, libc::WUNTRACED | libc::WNOHANG) };
                assert!(observed >= 0);
                if observed == pid {
                    assert!(libc::WIFSTOPPED(status));
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            assert_eq!(unsafe { libc::tcgetpgrp(0) }, pid);
            println!("RUNTIME_STOPPED");
            if stop_mode == "cancel-stopped" {
                stopped_cancel.cancel();
                return;
            }
            while !std::path::Path::new("resume").exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            assert_eq!(unsafe { libc::kill(-pid, libc::SIGCONT) }, 0);
            assert_eq!(unsafe { libc::tcgetpgrp(0) }, pid);
            println!("RUNTIME_RESUMED");
        });
        let result = execute_inherited(command, cancel).await;
        stop_task.await.unwrap();
        cancel_task.abort();
        result.unwrap().0
    });
    match mode.as_str() {
        "cancel" | "cancel-stopped" => assert!(result.unwrap_err().is::<Interrupted>()),
        "spawn-failure" => assert!(result.is_err()),
        "failure" => assert_eq!(result.unwrap().code(), Some(7)),
        _ => assert!(result.unwrap().success()),
    }
    assert_eq!(unsafe { libc::tcgetpgrp(0) }, group);
    let mut after = unsafe { std::mem::zeroed::<libc::termios>() };
    assert_eq!(unsafe { libc::tcgetattr(0, &mut after) }, 0);
    assert_eq!(before.c_lflag, after.c_lflag);
    assert_eq!(before.c_iflag, after.c_iflag);
    assert_eq!(before.c_oflag, after.c_oflag);
    assert_eq!(before.c_cc, after.c_cc);
    println!("TERMINAL_RESTORED");
}

static INTERRUPTED: AtomicBool = AtomicBool::new(false);
extern "C" fn interrupt(_: libc::c_int) {
    INTERRUPTED.store(true, Ordering::SeqCst);
}
#[test]
fn terminal_child_fixture() {
    if std::env::var("EMPACK_PTY_ROLE").as_deref() == Ok("background") {
        assert_ne!(unsafe { libc::tcgetpgrp(0) }, unsafe { libc::getpgrp() });
        let error = terminal::ForegroundTerminal::capture()
            .err()
            .expect("background stole terminal");
        assert!(format!("{error:#}").contains("background process group"));
        return;
    }
    if std::env::var("EMPACK_PTY_ROLE").as_deref() != Ok("child") {
        return;
    }
    println!("RUNTIME_PID={}", std::process::id());
    assert_eq!(
        unsafe { libc::tcgetpgrp(0) },
        unsafe { libc::getpgrp() },
        "runtime is not the foreground terminal group"
    );
    let mut action = unsafe { std::mem::zeroed::<libc::sigaction>() };
    action.sa_sigaction = interrupt as *const () as usize;
    unsafe {
        libc::sigemptyset(&mut action.sa_mask);
    }
    assert_eq!(
        unsafe { libc::sigaction(libc::SIGINT, &action, std::ptr::null_mut()) },
        0
    );
    let mut settings = unsafe { std::mem::zeroed::<libc::termios>() };
    assert_eq!(unsafe { libc::tcgetattr(0, &mut settings) }, 0);
    assert_eq!(settings.c_cc[libc::VSUSP], libc::_POSIX_VDISABLE);
    settings.c_lflag &= !libc::ECHO;
    assert_eq!(unsafe { libc::tcsetattr(0, libc::TCSANOW, &settings) }, 0);
    std::fs::write("runtime-host", unsafe { libc::getppid() }.to_string()).unwrap();
    std::fs::write("runtime-pid.tmp", std::process::id().to_string()).unwrap();
    std::fs::rename("runtime-pid.tmp", "runtime-pid").unwrap();
    println!("CONSOLE_READY");
    let deadline = Instant::now() + Duration::from_secs(12);
    loop {
        if INTERRUPTED.load(Ordering::SeqCst) {
            println!("INTERRUPTED_GRACEFULLY");
            return;
        }
        assert!(Instant::now() < deadline, "console input timed out");
        let mut poll = libc::pollfd {
            fd: 0,
            events: libc::POLLIN,
            revents: 0,
        };
        if unsafe { libc::poll(&mut poll, 1, 20) } > 0 {
            let mut bytes = [0; 128];
            let count = unsafe { libc::read(0, bytes.as_mut_ptr().cast(), bytes.len()) };
            if count < 0
                && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted
            {
                continue;
            }
            assert!(count > 0);
            let text = String::from_utf8_lossy(&bytes[..count as usize]);
            println!("CONSOLE_INPUT={}", text.trim());
            match text.trim() {
                "stop" => {
                    println!("CONSOLE_STOPPED");
                    return;
                }
                "fail" => std::process::exit(7),
                _ => (),
            }
        }
    }
}
