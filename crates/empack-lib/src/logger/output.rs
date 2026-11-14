//! Log writes suspend the same terminal region as command progress and status.
use crate::primitives::LogOutput;
use indicatif::MultiProgress;
use std::{
    io::{self, Write},
    sync::Arc,
};
use tracing_subscriber::fmt::MakeWriter;

#[derive(Clone)]
pub(super) struct Output {
    pub progress: Arc<MultiProgress>,
    pub destination: LogOutput,
}

impl<'a> MakeWriter<'a> for Output {
    type Writer = Self;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

impl Write for Output {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.progress.suspend(|| {
            match self.destination {
                LogOutput::Stdout => io::stdout().lock().write_all(bytes)?,
                LogOutput::Stderr => io::stderr().lock().write_all(bytes)?,
            }
            Ok(bytes.len())
        })
    }

    fn flush(&mut self) -> io::Result<()> {
        self.progress.suspend(|| match self.destination {
            LogOutput::Stdout => io::stdout().lock().flush(),
            LogOutput::Stderr => io::stderr().lock().flush(),
        })
    }
}
