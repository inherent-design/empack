use super::*;
use crate::engine::{
    resources::ResourceGovernor,
    runtime::{OperationOutcome, OperationRuntime},
};
use std::io::Write;

async fn fetch(origin: &str, path: &str, maximum: u64) -> Result<Vec<u8>> {
    let governor = ResourceGovernor::new(ResourceRequest {
        jobs: 1,
        memory_bytes: 1 << 20,
        open_files: 8,
        ..Default::default()
    });
    let runtime = OperationRuntime::new(governor.clone(), 1);
    let transport = HttpAcquisition::for_loopback_tests()
        .with_test_origin("https://edge.forgecdn.net", origin)
        .with_curseforge_key(Some(HeaderValue::from_static("must-not-be-sent")));
    let url = format!("https://edge.forgecdn.net{path}");
    let mut operation = runtime
        .start(move |mut scope| async move {
            Ok(transport
                .publisher_metadata(&mut scope, &url, maximum, Duration::from_millis(500))
                .await)
        })
        .unwrap();
    let result = operation.wait().await;
    let output = match &*result {
        OperationOutcome::Completed(Ok(bytes)) => {
            assert_eq!(governor.status().reserved.memory_bytes, maximum);
            Ok(bytes.to_vec())
        }
        OperationOutcome::Completed(Err(error)) => Err(anyhow::anyhow!("{error:#}")),
        OperationOutcome::Failed(error) => Err(anyhow::anyhow!("{error:#}")),
    };
    drop(result);
    runtime.release_completed(operation.id());
    drop(operation);
    runtime.shutdown().await;
    assert_eq!(governor.status().reserved, ResourceRequest::default());
    output
}

#[tokio::test]
async fn metadata_redirects_stay_on_origin_and_never_send_provider_credentials() {
    let mut server = mockito::Server::new_async().await;
    let first = server
        .mock("GET", "/channel")
        .match_header("x-api-key", mockito::Matcher::Missing)
        .with_status(302)
        .with_header("location", "/actual")
        .create_async()
        .await;
    let second = server
        .mock("GET", "/actual")
        .match_header("x-api-key", mockito::Matcher::Missing)
        .with_body("envelope")
        .create_async()
        .await;
    assert_eq!(
        fetch(&server.url(), "/channel", 32).await.unwrap(),
        b"envelope"
    );
    first.assert_async().await;
    second.assert_async().await;
    for location in [
        "https://other.test/file",
        "http://edge.forgecdn.net/file",
        "https://user:secret@edge.forgecdn.net/file",
    ] {
        let response = server
            .mock("GET", "/invalid")
            .with_status(302)
            .with_header("location", location)
            .create_async()
            .await;
        assert!(fetch(&server.url(), "/invalid", 32).await.is_err());
        response.assert_async().await;
        response.remove_async().await;
    }
}

#[tokio::test]
async fn metadata_rejects_large_encoded_and_unsuccessful_responses() {
    let mut server = mockito::Server::new_async().await;
    for (status, encoding, body) in [
        (200, "identity", "too many bytes"),
        (200, "gzip", "bytes"),
        (401, "identity", ""),
        (429, "identity", ""),
    ] {
        let response = server
            .mock("GET", "/file")
            .with_status(status)
            .with_header("content-encoding", encoding)
            .with_body(body)
            .create_async()
            .await;
        assert!(fetch(&server.url(), "/file", 8).await.is_err());
        response.assert_async().await;
        response.remove_async().await;
    }
}

#[tokio::test]
async fn chunked_metadata_enforces_running_limit_and_total_deadline() {
    for body in [
        &b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n9\r\n123456789\r\n"[..],
        &b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n"[..],
    ] {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let thread = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut request = [0; 4096];
            assert!(stream.read(&mut request).unwrap() > 0);
            stream.write_all(body).unwrap();
            assert_eq!(stream.read(&mut request).unwrap(), 0);
        });
        assert!(
            fetch(&format!("http://{address}"), "/file", 8)
                .await
                .is_err()
        );
        thread.join().unwrap();
    }
}
