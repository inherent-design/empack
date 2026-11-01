use super::*;
use crate::application::process_runtime::Interrupted;
use std::{
    fs::File,
    io::{self, Read, Write},
    os::fd::{AsRawFd, FromRawFd},
    sync::mpsc,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug)]
enum Prompt {
    Text,
    Select,
    Fuzzy,
}
#[derive(Debug, PartialEq, Eq)]
enum Answer {
    Text(String),
    Select(usize),
    Fuzzy(Option<usize>),
}

fn settings(fd: RawFd) -> libc::termios {
    let mut value = unsafe { std::mem::zeroed() };
    assert_eq!(unsafe { libc::tcgetattr(fd, &mut value) }, 0);
    value
}

fn drain(master: &mut File, output: &mut Vec<u8>) {
    let mut descriptors = unsafe { std::mem::zeroed() };
    unsafe {
        libc::FD_ZERO(&mut descriptors);
        libc::FD_SET(master.as_raw_fd(), &mut descriptors);
    }
    let mut timeout = libc::timeval {
        tv_sec: 0,
        tv_usec: 10_000,
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
        let mut bytes = [0; 4096];
        let length = master.read(&mut bytes).unwrap();
        assert_ne!(length, 0);
        output.extend_from_slice(&bytes[..length]);
    }
}

// Scope cleanup always cancels before joining, including on driver assertions.
struct CancelOnDrop<'a>(&'a Cancellation);
impl Drop for CancelOnDrop<'_> {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

fn pty(prompt: Prompt, keys: &[u8], cancel: bool) -> (Result<Answer>, bool, Vec<u8>) {
    let (mut master, mut slave) = (-1, -1);
    let mut size = libc::winsize {
        ws_row: 8,
        ws_col: 24,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    assert_eq!(
        unsafe {
            libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                // Darwin declares a mutable pointer, Linux a const pointer.
                // Pass the owned raw pointer without a needless mutable borrow
                // at Linux's const FFI boundary.
                std::ptr::from_mut(&mut size),
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
    let terminal = Term::read_write_pair(slave.try_clone().unwrap(), slave.try_clone().unwrap());
    let before = settings(slave.as_raw_fd());
    let token = Cancellation::default();
    let mut output = Vec::new();
    let (result, cancelled) = std::thread::scope(|scope| {
        let _cancel = CancelOnDrop(&token);
        let (send, receive) = mpsc::channel();
        let worker_input = slave.as_raw_fd();
        let worker_terminal = &terminal;
        let worker_token = &token;
        let worker = scope.spawn(move || {
            let result = match prompt {
                Prompt::Text => text_on(
                    worker_input,
                    worker_terminal,
                    "Fixture",
                    "preset".into(),
                    worker_token,
                )
                .map(Answer::Text),
                Prompt::Select => select_on(
                    worker_input,
                    worker_terminal,
                    "Fixture",
                    &[
                        "alpha", "beta", "beta", "delta", "echo", "foxtrot", "golf", "hotel",
                    ],
                    worker_token,
                )
                .map(Answer::Select),
                Prompt::Fuzzy => fuzzy_on(
                    worker_input,
                    worker_terminal,
                    "Fixture",
                    &["alpha".into(), "beta".into(), "beta".into(), "界面".into()],
                    worker_token,
                )
                .map(Answer::Fuzzy),
            };
            send.send(result).unwrap();
        });
        let deadline = Instant::now() + Duration::from_secs(5);
        while !String::from_utf8_lossy(&output).contains("Fixture") {
            assert!(
                Instant::now() < deadline,
                "prompt not rendered: {prompt:?} {output:?}"
            );
            drain(&mut master, &mut output);
        }
        assert_eq!(
            settings(slave.as_raw_fd()).c_lflag & (libc::ICANON | libc::ISIG | libc::ECHO),
            0
        );
        master.write_all(keys).unwrap();
        if cancel {
            token.cancel();
        }
        let result = loop {
            if let Ok(result) = receive.try_recv() {
                break result;
            }
            assert!(
                Instant::now() < deadline,
                "prompt did not retire: {prompt:?}"
            );
            // Continuously consume output, including long edited inputs, so the
            // fixture cannot deadlock its writer on a full PTY output queue.
            drain(&mut master, &mut output);
        };
        worker.join().unwrap();
        drain(&mut master, &mut output);
        (result, token.is_cancelled())
    });
    // Darwin marks canonical input pending after raw-mode restoration. Querying
    // available input processes that kernel state without discarding queued bytes.
    let mut available = 0;
    assert_eq!(
        unsafe { libc::ioctl(slave.as_raw_fd(), libc::FIONREAD, &mut available) },
        0
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
    if keys.is_empty() {
        master.write_all(b"usable\n").unwrap();
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
            1
        );
        let mut line = [0; 7];
        (&slave).read_exact(&mut line).unwrap();
        assert_eq!(
            &line, b"usable\n",
            "cancellation left the terminal unusable"
        );
    }
    let transcript = String::from_utf8_lossy(&output);
    if let Some(hidden) = transcript.rfind("\x1b[?25l") {
        assert!(
            transcript
                .rfind("\x1b[?25h")
                .is_some_and(|shown| shown > hidden),
            "cursor remains hidden"
        );
    }
    (result, cancelled, output)
}

#[test]
fn text_pty_preserves_defaults_unicode_editing_and_word_navigation() {
    for (keys, expected) in [
        ("\n", "preset"),
        ("hélo\x1b[D\x1b[Dl\x1b[F!\x7f\n", "héllo"),
        ("alpha beta\x1bbX\x1bfY\n", "alpha XbetaY"),
        ("abc\x1b[H\x1b[3~界\x1b[F\n", "界bc"),
        (
            "a very long Unicode input 界面 with a tail\n",
            "a very long Unicode input 界面 with a tail",
        ),
    ] {
        let (result, cancelled, _) = pty(Prompt::Text, keys.as_bytes(), false);
        assert_eq!(result.unwrap(), Answer::Text(expected.into()), "{keys:?}");
        assert!(!cancelled);
    }
}

#[test]
fn selection_pty_requires_choice_and_preserves_original_indices_and_pages() {
    for (keys, expected) in [
        ("\n\x1b[B\n", 0),
        ("\x1b[B\x1b[B\n", 1),
        ("\x1b[A\n", 7),
        ("\x1b[6~\n", 6),
        ("\x1b[F\x1b[5~\n", 0),
    ] {
        let (result, cancelled, _) = pty(Prompt::Select, keys.as_bytes(), false);
        assert_eq!(result.unwrap(), Answer::Select(expected), "{keys:?}");
        assert!(!cancelled);
    }
}

#[test]
fn fuzzy_pty_preserves_query_editing_duplicate_indices_and_optional_escape() {
    for (keys, expected) in [
        ("beta\n", Some(1)),
        ("beta\x1b[B\n", Some(2)),
        ("zz\n\x7f\x7fbeta\n", Some(1)),
        ("界\n", Some(3)),
        ("\x1b", None),
    ] {
        let (result, cancelled, _) = pty(Prompt::Fuzzy, keys.as_bytes(), false);
        assert_eq!(result.unwrap(), Answer::Fuzzy(expected), "{keys:?}");
        assert!(!cancelled);
    }
}

#[test]
fn every_widget_pty_retires_token_and_keyboard_cancellation_including_partial_keys() {
    for prompt in [Prompt::Text, Prompt::Select, Prompt::Fuzzy] {
        for (keys, cancel) in [
            (&b""[..], true),
            (&b"partial"[..], true),
            (&b"\xc3"[..], true),
            (&b"\x1b["[..], true),
            (&b"\x03"[..], false),
            (&b"\xc3\x03"[..], false),
            (&b"\x1b[\x03"[..], false),
        ] {
            let (result, cancelled, _) = pty(prompt, keys, cancel);
            assert!(
                result.unwrap_err().is::<Interrupted>(),
                "{prompt:?} {keys:?}"
            );
            assert!(cancelled);
        }
    }
}

#[test]
fn every_widget_pty_rejects_closed_or_invalid_input_without_default_authority() {
    for prompt in [Prompt::Text, Prompt::Select, Prompt::Fuzzy] {
        for keys in [&b"\x04"[..], &b"\xff"[..], &b"\xc3\x04"[..]] {
            let (result, cancelled, _) = pty(prompt, keys, false);
            let error = result.unwrap_err();
            assert!(!error.is::<Interrupted>(), "{prompt:?} {keys:?}: {error:#}");
            assert!(!cancelled);
        }
    }
}
