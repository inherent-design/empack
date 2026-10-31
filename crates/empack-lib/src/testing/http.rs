//! Bounded native HTTP fixture for transfer retirement, not a provider emulator.
use std::{
    io::{self, Read, Write},
    net::TcpListener,
    thread::JoinHandle,
    time::{Duration, Instant},
};
use tokio::sync::oneshot;

pub(crate) struct StalledHttp {
    pub url: String,
    ready: oneshot::Receiver<()>,
    worker: JoinHandle<io::Result<()>>,
}

impl StalledHttp {
    /// Send only a response prefix, then wait for the client to retire its socket.
    pub fn new(prefix: &'static [u8]) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        let (sender, ready) = oneshot::channel();
        let worker = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        if Instant::now() >= deadline {
                            return Err(io::Error::new(
                                io::ErrorKind::TimedOut,
                                "no fixture request",
                            ));
                        }
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => return Err(error),
                }
            };
            stream.set_nonblocking(false)?;
            stream.set_read_timeout(Some(Duration::from_secs(5)))?;
            stream.set_write_timeout(Some(Duration::from_secs(5)))?;
            let request_deadline = Instant::now() + Duration::from_secs(5);
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                if request.len() == 4096 {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "oversized fixture request",
                    ));
                }
                let remaining = request_deadline
                    .checked_duration_since(Instant::now())
                    .filter(|duration| !duration.is_zero())
                    .ok_or_else(|| {
                        io::Error::new(io::ErrorKind::TimedOut, "incomplete fixture request")
                    })?;
                stream.set_read_timeout(Some(remaining))?;
                let mut byte = [0];
                stream.read_exact(&mut byte)?;
                request.push(byte[0]);
            }
            stream.write_all(prefix)?;
            let _ = sender.send(());
            stream.set_read_timeout(Some(Duration::from_secs(5)))?;
            match stream.read(&mut [0]) {
                Ok(0) => Ok(()),
                Err(error) if error.kind() == io::ErrorKind::ConnectionReset => Ok(()),
                Err(error) => Err(error),
                Ok(_) => Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "client sent more bytes instead of closing the response",
                )),
            }
        });
        Self {
            url: format!("http://{address}/file"),
            ready,
            worker,
        }
    }

    pub async fn started(&mut self) {
        tokio::time::timeout(Duration::from_secs(5), &mut self.ready)
            .await
            .expect("fixture request did not start")
            .expect("fixture failed before sending its response prefix");
    }

    pub fn assert_closed(self) {
        self.worker
            .join()
            .expect("fixture thread panicked")
            .expect("client did not retire the response socket");
    }
}
