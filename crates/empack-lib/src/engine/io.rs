//! Bounded streaming shared by snapshots, staging and publication candidates.
use crate::application::process_runtime::Cancellation;
use anyhow::{Result, bail};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};

pub(super) fn copy_bounded(
    source: &mut dyn Read,
    output: &mut dyn Write,
    maximum: u64,
    cancel: &Cancellation,
) -> Result<([u8; 32], u64)> {
    let mut digest = Sha256::new();
    let mut total = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        cancel.check()?;
        let remaining = maximum - total;
        let allowed = remaining.min(buffer.len() as u64).max(1) as usize;
        let count = match source.read(&mut buffer[..allowed]) {
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            result => result?,
        };
        if count == 0 {
            break;
        }
        if count as u64 > remaining {
            bail!("Input exceeds byte limit");
        }
        output.write_all(&buffer[..count])?;
        total += count as u64;
        digest.update(&buffer[..count]);
    }
    Ok((digest.finalize().into(), total))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;
    #[test]
    fn limits_bound_consumption_and_never_write_the_probe_byte() {
        for (data, limit, succeeds, consumed) in [
            (&b""[..], 0, true, 0),
            (b"x", 0, false, 1),
            (b"exact", 5, true, 5),
            (b"excess", 2, false, 3),
            (b"small", u64::MAX, true, 5),
        ] {
            let mut reader = io::Cursor::new(data);
            let mut output = Vec::new();
            let result = copy_bounded(&mut reader, &mut output, limit, &Cancellation::default());
            assert_eq!(result.is_ok(), succeeds);
            assert_eq!(reader.position(), consumed);
            assert!(output.len() as u64 <= limit);
        }
    }
    struct InterruptedReader(bool);
    impl Read for InterruptedReader {
        fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
            if !self.0 {
                self.0 = true;
                return Err(io::ErrorKind::Interrupted.into());
            }
            (&b""[..]).read(output)
        }
    }
    struct ShortWriter(Vec<u8>);
    impl Write for ShortWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.push(bytes[0]);
            Ok(1)
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    #[test]
    fn interrupted_reads_and_short_writes_obey_standard_io_contracts() {
        copy_bounded(
            &mut InterruptedReader(false),
            &mut io::sink(),
            0,
            &Cancellation::default(),
        )
        .unwrap();
        let mut output = ShortWriter(Vec::new());
        let (hash, bytes) =
            copy_bounded(&mut &b"hello"[..], &mut output, 5, &Cancellation::default()).unwrap();
        assert_eq!(output.0, b"hello");
        assert_eq!(bytes, 5);
        assert_eq!(hash.as_slice(), Sha256::digest(b"hello").as_slice());
    }
    #[test]
    fn write_failure_and_cancellation_do_not_report_success() {
        let mut full = &mut [][..];
        assert!(copy_bounded(&mut &b"x"[..], &mut full, 1, &Cancellation::default()).is_err());
        let cancelled = Cancellation::default();
        cancelled.cancel();
        let mut input = io::Cursor::new(b"x");
        assert!(copy_bounded(&mut input, &mut io::sink(), 1, &cancelled).is_err());
        assert_eq!(input.position(), 0);
    }
}
