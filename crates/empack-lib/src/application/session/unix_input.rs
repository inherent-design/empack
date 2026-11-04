//! One Unix terminal-mode owner and cancellation-aware byte reader for native prompts.
use crate::application::process_runtime::{Cancellation, Interrupted};
use anyhow::{Context, Result, ensure};
use console::Key;
use std::{
    io,
    os::fd::RawFd,
    time::{Duration, Instant},
};

pub(super) fn read_byte(input: RawFd, cancellation: &Cancellation) -> Result<u8> {
    Ok(read_before(input, cancellation, None)?.expect("a read without a deadline cannot time out"))
}

fn read_before(
    input: RawFd,
    cancellation: &Cancellation,
    deadline: Option<Instant>,
) -> Result<Option<u8>> {
    ensure!(
        input >= 0 && (input as usize) < libc::FD_SETSIZE,
        "Interactive descriptor cannot be represented by select"
    );
    loop {
        cancellation.check()?;
        let remaining = deadline.map(|until| until.saturating_duration_since(Instant::now()));
        if remaining.is_some_and(|left| left.is_zero()) {
            return Ok(None);
        }
        let wait = remaining
            .unwrap_or(Duration::from_millis(20))
            .min(Duration::from_millis(20));
        let mut descriptors = unsafe { std::mem::zeroed() };
        // macOS terminals require select rather than poll. Rebuild its mutable
        // arguments after every wakeup, timeout or interrupted system call.
        unsafe {
            libc::FD_ZERO(&mut descriptors);
            libc::FD_SET(input, &mut descriptors);
        }
        let mut timeout = libc::timeval {
            tv_sec: 0,
            tv_usec: wait.as_micros() as _,
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
            return Err(error).context("Cannot wait for interactive input");
        }
        if ready == 0 {
            continue;
        }
        let mut byte = 0_u8;
        let count = unsafe { libc::read(input, std::ptr::from_mut(&mut byte).cast(), 1) };
        if count == 1 {
            return Ok(Some(byte));
        }
        if count == 0 {
            return Err(io::Error::from(io::ErrorKind::UnexpectedEof))
                .context("Interactive input closed");
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error).context("Cannot read interactive input");
        }
    }
}

pub(super) struct InputMode {
    input: RawFd,
    original: libc::termios,
    restored: bool,
}
impl InputMode {
    pub(super) fn capture(input: RawFd) -> Result<Self> {
        let mut original = unsafe { std::mem::zeroed() };
        if unsafe { libc::tcgetattr(input, &mut original) } != 0 {
            return Err(io::Error::last_os_error()).context("Cannot capture interactive terminal");
        }
        let mut raw = original;
        unsafe { libc::cfmakeraw(&mut raw) };
        // Keep normal newline output while reading individual keys, as console does.
        raw.c_oflag = original.c_oflag;
        if unsafe { libc::tcsetattr(input, libc::TCSANOW, &raw) } != 0 {
            return Err(io::Error::last_os_error())
                .context("Cannot configure interactive terminal");
        }
        Ok(Self {
            input,
            original,
            restored: false,
        })
    }

    pub(super) fn restore(&mut self) -> Result<()> {
        if !self.restored {
            if unsafe { libc::tcsetattr(self.input, libc::TCSANOW, &self.original) } != 0 {
                return Err(io::Error::last_os_error())
                    .context("Cannot restore interactive terminal");
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

// Escape alone is optional dismissal; escape-prefixed keys need a short framing
// interval. All waits, including partial UTF-8 and escape sequences, observe the
// same cancellation token and remain owned by this calling thread.
pub(super) fn read_key(input: RawFd, cancellation: &Cancellation) -> Result<Key> {
    fn checked(byte: u8, cancellation: &Cancellation) -> Result<u8> {
        if byte == 3 {
            cancellation.cancel();
            return Err(Interrupted.into());
        }
        Ok(byte)
    }
    let byte = checked(read_byte(input, cancellation)?, cancellation)?;
    Ok(match byte {
        b'\r' | b'\n' => Key::Enter,
        b'\t' => Key::Tab,
        8 | 127 => Key::Backspace,
        1 => Key::Home,
        5 => Key::End,
        4 => {
            return Err(io::Error::from(io::ErrorKind::UnexpectedEof))
                .context("Interactive input closed");
        }
        27 => {
            let deadline = Some(Instant::now() + Duration::from_millis(30));
            let Some(next) = read_before(input, cancellation, deadline)? else {
                return Ok(Key::Escape);
            };
            let next = checked(next, cancellation)?;
            if next != b'[' && next != b'O' {
                return Ok(Key::UnknownEscSeq(vec![char::from(next)]));
            }
            let mut sequence = [0_u8; 16];
            let mut length = 0;
            loop {
                let Some(next) = read_before(input, cancellation, deadline)? else {
                    return Ok(Key::Unknown);
                };
                let next = checked(next, cancellation)?;
                ensure!(
                    length < sequence.len(),
                    "Unsupported interactive key sequence"
                );
                sequence[length] = next;
                length += 1;
                if (0x40..=0x7e).contains(&next) {
                    break;
                }
            }
            match &sequence[..length] {
                b"A" => Key::ArrowUp,
                b"B" => Key::ArrowDown,
                b"C" => Key::ArrowRight,
                b"D" => Key::ArrowLeft,
                b"H" | b"1~" | b"7~" => Key::Home,
                b"F" | b"4~" | b"8~" => Key::End,
                b"3~" => Key::Del,
                b"5~" => Key::PageUp,
                b"6~" => Key::PageDown,
                b"Z" => Key::BackTab,
                _ => Key::Unknown,
            }
        }
        0..=31 => Key::Unknown,
        32..=126 => Key::Char(char::from(byte)),
        _ => {
            let length = match byte {
                0xc2..=0xdf => 2,
                0xe0..=0xef => 3,
                0xf0..=0xf4 => 4,
                _ => anyhow::bail!("Invalid UTF-8 in interactive input"),
            };
            let mut bytes = [byte, 0, 0, 0];
            for slot in &mut bytes[1..length] {
                *slot = checked(read_byte(input, cancellation)?, cancellation)?;
            }
            let value = std::str::from_utf8(&bytes[..length])
                .context("Invalid UTF-8 in interactive input")?
                .chars()
                .next()
                .expect("decoded key is nonempty");
            if value.is_control() {
                Key::Unknown
            } else {
                Key::Char(value)
            }
        }
    })
}
