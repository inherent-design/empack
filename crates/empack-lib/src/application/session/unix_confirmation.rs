//! Confirmation owns terminal mode before rendering and never waits indefinitely for input.
use crate::application::process_runtime::{Cancellation, Interrupted};
use anyhow::{Context, Result, ensure};
use console::Term;
use dialoguer::theme::{SimpleTheme, Theme};
use std::{io, os::fd::RawFd};

pub(super) fn read(prompt: &str, default: bool, cancellation: &Cancellation) -> Result<bool> {
    confirm_on(
        libc::STDIN_FILENO,
        &Term::stderr(),
        prompt,
        default,
        cancellation,
    )
}

fn confirm_on(
    input: RawFd,
    terminal: &Term,
    prompt: &str,
    default: bool,
    cancellation: &Cancellation,
) -> Result<bool> {
    cancellation.check()?;
    ensure!(terminal.is_term(), "Confirmation output is not a terminal");
    let mut mode = InputMode::capture(input)?;
    let result = (|| {
        // Ctrl-C is now an input byte even if it arrives immediately after the
        // prompt is displayed. A host signal/token cancellation is checked by
        // the bounded read instead of depending on a signal interrupting libc.
        cancellation.check()?;
        let mut rendered = String::new();
        SimpleTheme.format_confirm_prompt(&mut rendered, prompt, Some(default))?;
        terminal.write_str(&rendered)?;
        terminal.flush()?;
        loop {
            let value = match read_byte(input, cancellation)? {
                b'y' | b'Y' => true,
                b'n' | b'N' => false,
                b'\r' | b'\n' => default,
                b'\x03' => {
                    cancellation.cancel();
                    return Err(Interrupted.into());
                }
                b'\x1b' => anyhow::bail!(
                    "Use y, n or Enter for confirmation; escape sequences cannot grant approval"
                ),
                _ => continue,
            };
            cancellation.check()?;
            rendered.clear();
            SimpleTheme.format_confirm_prompt_selection(&mut rendered, prompt, Some(value))?;
            terminal.clear_line()?;
            terminal.write_line(&rendered)?;
            return Ok(value);
        }
    })();
    if let Err(cleanup) = mode.restore() {
        return match result {
            Ok(_) => Err(cleanup),
            Err(error) => Err(cleanup.context(format!("Confirmation failed: {error:#}"))),
        };
    }
    result
}

fn read_byte(input: RawFd, cancellation: &Cancellation) -> Result<u8> {
    ensure!(
        input >= 0 && (input as usize) < libc::FD_SETSIZE,
        "Confirmation descriptor cannot be represented by select"
    );
    loop {
        cancellation.check()?;
        let mut descriptors = unsafe { std::mem::zeroed() };
        // macOS terminals require select rather than poll. Rebuild its mutable
        // arguments after every wakeup, timeout or interrupted system call.
        unsafe {
            libc::FD_ZERO(&mut descriptors);
            libc::FD_SET(input, &mut descriptors);
        }
        let mut timeout = libc::timeval {
            tv_sec: 0,
            tv_usec: 20_000,
        };
        let ready = unsafe {
            libc::select(
                input + 1,
                &mut descriptors,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut timeout,
            )
        };
        cancellation.check()?;
        if ready < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error).context("Cannot wait for confirmation input");
        }
        if ready == 0 {
            continue;
        }
        let mut byte = 0_u8;
        let count = unsafe { libc::read(input, std::ptr::from_mut(&mut byte).cast(), 1) };
        if count == 1 {
            return Ok(byte);
        }
        if count == 0 {
            return Err(io::Error::from(io::ErrorKind::UnexpectedEof))
                .context("Confirmation input closed");
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error).context("Cannot read confirmation input");
        }
    }
}

struct InputMode {
    input: RawFd,
    original: libc::termios,
    restored: bool,
}
impl InputMode {
    fn capture(input: RawFd) -> Result<Self> {
        let mut original = unsafe { std::mem::zeroed() };
        if unsafe { libc::tcgetattr(input, &mut original) } != 0 {
            return Err(io::Error::last_os_error()).context("Cannot capture confirmation terminal");
        }
        let mut raw = original;
        unsafe { libc::cfmakeraw(&mut raw) };
        // Keep normal newline output while reading individual keys, as console does.
        raw.c_oflag = original.c_oflag;
        if unsafe { libc::tcsetattr(input, libc::TCSANOW, &raw) } != 0 {
            return Err(io::Error::last_os_error())
                .context("Cannot configure confirmation terminal");
        }
        Ok(Self {
            input,
            original,
            restored: false,
        })
    }

    fn restore(&mut self) -> Result<()> {
        if !self.restored {
            if unsafe { libc::tcsetattr(self.input, libc::TCSANOW, &self.original) } != 0 {
                return Err(io::Error::last_os_error())
                    .context("Cannot restore confirmation terminal");
            }
            self.restored = true;
        }
        Ok(())
    }
}
impl Drop for InputMode {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs::File,
        io::{Read, Write},
        os::fd::{AsRawFd, FromRawFd},
        time::{Duration, Instant},
    };

    fn settings(input: RawFd) -> libc::termios {
        let mut value = unsafe { std::mem::zeroed() };
        assert_eq!(unsafe { libc::tcgetattr(input, &mut value) }, 0);
        value
    }

    struct CancelOnDrop<'a>(&'a Cancellation);
    impl Drop for CancelOnDrop<'_> {
        fn drop(&mut self) {
            self.0.cancel();
        }
    }

    #[test]
    fn confirmation_pty_accepts_keys_and_retires_both_input_and_host_cancellation() {
        // Real PTYs exercise input readiness and terminal restoration. Token
        // cancellation sends no input or signal that could unblock an old read.
        for (input, default, expected) in [
            (Some(&b"Y"[..]), false, Some(true)),
            (Some(&b"Ykept"[..]), false, Some(true)),
            (Some(&b"n"[..]), true, Some(false)),
            (Some(&b"\n"[..]), false, Some(false)),
            (Some(&b"\r"[..]), true, Some(true)),
            (Some(&b"\x03"[..]), false, None),
            (Some(&b"\x1b[y"[..]), false, None),
            (None, false, None),
        ] {
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
            let mut master = unsafe { File::from_raw_fd(master) };
            let slave = unsafe { File::from_raw_fd(slave) };
            for fd in [master.as_raw_fd(), slave.as_raw_fd()] {
                let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
                assert!(flags >= 0);
                assert_eq!(
                    unsafe { libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) },
                    0
                );
            }
            let before = settings(slave.as_raw_fd());
            let terminal =
                Term::read_write_pair(slave.try_clone().unwrap(), slave.try_clone().unwrap());
            let cancellation = Cancellation::default();
            let (result, cancelled) = std::thread::scope(|scope| {
                // If an assertion in the driver fails, retire the read before
                // scope joins the worker rather than hanging the failure report.
                let _cancel = CancelOnDrop(&cancellation);
                let worker = scope.spawn(|| {
                    confirm_on(
                        slave.as_raw_fd(),
                        &terminal,
                        "Approve fixture?",
                        default,
                        &cancellation,
                    )
                });
                let deadline = Instant::now() + Duration::from_secs(5);
                let mut rendered = Vec::new();
                while !rendered.ends_with(b"] ") {
                    assert!(
                        Instant::now() < deadline,
                        "confirmation prompt not rendered: {rendered:?}"
                    );
                    let mut descriptors = unsafe { std::mem::zeroed() };
                    unsafe {
                        libc::FD_ZERO(&mut descriptors);
                        libc::FD_SET(master.as_raw_fd(), &mut descriptors);
                    }
                    let mut timeout = libc::timeval {
                        tv_sec: 0,
                        tv_usec: 100_000,
                    };
                    let ready = unsafe {
                        libc::select(
                            master.as_raw_fd() + 1,
                            &mut descriptors,
                            std::ptr::null_mut(),
                            std::ptr::null_mut(),
                            &mut timeout,
                        )
                    };
                    assert!(ready >= 0, "{}", io::Error::last_os_error());
                    if ready > 0 {
                        let mut bytes = [0; 128];
                        let count = master.read(&mut bytes).unwrap();
                        assert_ne!(count, 0);
                        rendered.extend_from_slice(&bytes[..count]);
                    }
                }
                assert!(String::from_utf8_lossy(&rendered).contains("Approve fixture?"));
                let active = settings(slave.as_raw_fd());
                assert_eq!(active.c_lflag & (libc::ICANON | libc::ISIG | libc::ECHO), 0);
                match input {
                    Some(bytes) => master.write_all(bytes).unwrap(),
                    None => cancellation.cancel(),
                }
                let result = worker.join().unwrap();
                assert!(
                    Instant::now() < deadline,
                    "confirmation did not retire promptly"
                );
                (result, cancellation.is_cancelled())
            });
            assert_eq!(cancelled, input.is_none() || input == Some(&b"\x03"[..]));
            match expected {
                Some(expected) => assert_eq!(result.unwrap(), expected),
                None if input == Some(&b"\x1b[y"[..]) => {
                    let error = result.unwrap_err();
                    assert!(!error.is::<Interrupted>());
                    assert!(
                        error
                            .to_string()
                            .contains("escape sequences cannot grant approval")
                    );
                }
                None => assert!(result.unwrap_err().is::<Interrupted>()),
            }
            // Darwin sets the transient PENDIN bit when ICANON is restored.
            // A readiness query processes that kernel state without consuming
            // pending user bytes. Keep exact flag comparisons after that query.
            let mut available = 0;
            assert_eq!(
                unsafe { libc::ioctl(slave.as_raw_fd(), libc::FIONREAD, &mut available) },
                0
            );
            // Readiness while switching modes differs between tty drivers. The
            // complete canonical bytes below must survive on either platform.
            let mut expected = match input {
                Some(b"Ykept") => b"kept".to_vec(),
                Some(b"\x1b[y") => b"[y".to_vec(),
                _ => Vec::new(),
            };
            expected.push(b'\n');
            master.write_all(b"\n").unwrap();
            let mut descriptors = unsafe { std::mem::zeroed() };
            unsafe {
                libc::FD_ZERO(&mut descriptors);
                libc::FD_SET(slave.as_raw_fd(), &mut descriptors);
            }
            let mut timeout = libc::timeval {
                tv_sec: 5,
                tv_usec: 0,
            };
            assert_eq!(
                unsafe {
                    libc::select(
                        slave.as_raw_fd() + 1,
                        &mut descriptors,
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                        &mut timeout,
                    )
                },
                1,
                "restored terminal did not admit a canonical line"
            );
            let mut observed = vec![0; expected.len()];
            (&slave).read_exact(&mut observed).unwrap();
            assert_eq!(
                observed, expected,
                "mode restoration discarded queued input"
            );
            let after = settings(slave.as_raw_fd());
            assert_eq!(after.c_iflag, before.c_iflag);
            assert_eq!(after.c_oflag, before.c_oflag);
            assert_eq!(after.c_cflag, before.c_cflag);
            assert_eq!(after.c_lflag, before.c_lflag);
            assert_eq!(after.c_cc, before.c_cc);
            assert_eq!(unsafe { libc::cfgetispeed(&after) }, unsafe {
                libc::cfgetispeed(&before)
            });
            assert_eq!(unsafe { libc::cfgetospeed(&after) }, unsafe {
                libc::cfgetospeed(&before)
            });
        }
    }
}
