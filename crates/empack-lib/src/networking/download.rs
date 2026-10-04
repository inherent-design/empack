//! Bounded, cancellable acquisition. Partial bytes stay in an owned temporary file.
use crate::application::session::ProcessProvider;
use anyhow::{Context, Result, ensure};
use std::io::{Seek, Write};

pub async fn acquire(
    client: &reqwest::Client,
    process: &dyn ProcessProvider,
    url: &str,
    limit: u64,
) -> Result<tempfile::NamedTempFile> {
    async fn wait_cancelled(process: &dyn ProcessProvider) -> Result<()> {
        loop {
            process.check_cancelled()?;
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    }
    let cancelled = wait_cancelled(process);
    tokio::pin!(cancelled);
    process.check_cancelled()?;
    let mut response = tokio::select! {
        result = &mut cancelled => { result?; unreachable!() },
        result = client.get(url).send() => result.context("Failed to acquire download headers")?,
    };
    ensure!(
        response.status().is_success(),
        "HTTP {} for {}",
        response.status(),
        url
    );
    ensure!(
        response
            .content_length()
            .is_none_or(|length| length <= limit),
        "Download exceeds {limit} byte limit"
    );
    let mut temporary = tempfile::NamedTempFile::new()?;
    let mut received = 0u64;
    loop {
        let chunk = tokio::select! {
            result = &mut cancelled => { result?; unreachable!() },
            result = response.chunk() => result.context("Failed to receive download body")?,
        };
        let Some(chunk) = chunk else { break };
        received = received
            .checked_add(chunk.len() as u64)
            .context("Download length overflow")?;
        ensure!(received <= limit, "Download exceeds {limit} byte limit");
        temporary.write_all(&chunk)?;
    }
    process.check_cancelled()?;
    temporary.as_file_mut().rewind()?;
    Ok(temporary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::session::{
        FileSystemProvider, LiveFileSystemProvider, LiveProcessProvider,
    };
    use std::io::Read;

    #[tokio::test]
    async fn streams_and_publishes_binary_bytes() {
        let mut server = mockito::Server::new_async().await;
        let _response = server
            .mock("GET", "/file")
            .with_body([0xff, 0, 1])
            .create_async()
            .await;
        let process = LiveProcessProvider::new();
        let mut staged = acquire(
            &reqwest::Client::new(),
            &process,
            &format!("{}/file", server.url()),
            3,
        )
        .await
        .unwrap();
        let project = tempfile::tempdir().unwrap();
        let dest = project.path().join("file");
        LiveFileSystemProvider
            .publish_reader(&dest, staged.as_file_mut())
            .unwrap();
        assert_eq!(std::fs::read(dest).unwrap(), [0xff, 0, 1]);
    }

    #[tokio::test]
    async fn rejects_oversized_headers_before_reading_body() {
        let mut server = mockito::Server::new_async().await;
        let _response = server
            .mock("GET", "/file")
            .with_body("too large")
            .create_async()
            .await;
        let error = acquire(
            &reqwest::Client::new(),
            &LiveProcessProvider::new(),
            &format!("{}/file", server.url()),
            2,
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("byte limit"));
    }

    #[tokio::test]
    async fn bounds_chunked_body_without_waiting_for_eof() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut request = [0; 4096];
            assert!(socket.read(&mut request).unwrap() > 0);
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\n1234\r\n")
                .unwrap();
            socket
                .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                .unwrap();
            // The client must close without waiting for the final chunk.
            socket.read(&mut request).unwrap_or(0)
        });
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            acquire(
                &reqwest::Client::new(),
                &LiveProcessProvider::new(),
                &format!("http://{address}/file"),
                3,
            ),
        )
        .await
        .unwrap();
        assert!(result.unwrap_err().to_string().contains("byte limit"));
        assert_eq!(server.join().unwrap(), 0);
    }

    #[tokio::test]
    async fn cancellation_interrupts_a_stalled_body() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut request = [0; 4096];
            assert!(socket.read(&mut request).unwrap() > 0);
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n")
                .unwrap();
            socket
                .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                .unwrap();
            socket.read(&mut request).unwrap_or(0)
        });
        let cancellation = crate::application::process_runtime::Cancellation::default();
        let process = LiveProcessProvider::new().with_cancellation(cancellation.clone());
        let client = reqwest::Client::new();
        let url = format!("http://{address}/file");
        let result = tokio::join!(acquire(&client, &process, &url, 3), async {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            cancellation.cancel();
        })
        .0;
        assert!(
            result
                .unwrap_err()
                .is::<crate::application::process_runtime::Interrupted>()
        );
        assert_eq!(server.join().unwrap(), 0);
    }
}
