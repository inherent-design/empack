//! Native terminal fixture. ConPTY 0.7 fixes redirected-parent handles under Nextest.
#[cfg(not(windows))]
pub fn spawn(command: std::process::Command) -> expectrl::session::OsSession {
    expectrl::Session::spawn(command).unwrap()
}

#[cfg(windows)]
pub fn spawn(command: std::process::Command) -> expectrl::Session<conpty::Process, Stream> {
    // ConPTY constructs its own command line and environment rather than using Command::spawn.
    // Quote each argument and carry the inherited environment explicitly.
    let mut native = std::process::Command::new(quote(command.get_program()));
    native.envs(std::env::vars_os());
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
    let mut process = conpty::Process::spawn(native).unwrap();
    let stream = Stream {
        reader: process.output().unwrap(),
        writer: process.input().unwrap(),
    };
    expectrl::Session::new(process, stream).unwrap()
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
