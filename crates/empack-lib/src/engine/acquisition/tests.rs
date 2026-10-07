use super::*;
use crate::engine::{
    resources::ResourceGovernor,
    runtime::{OperationOutcome, OperationRuntime},
};
use empack_core::digest::{DigestSet, ExpectedDigest};
use std::{io::Write, sync::Arc};

fn request(urls: Vec<String>, maximum: u64) -> DownloadRequest {
    DownloadRequest {
        alternatives: NonEmpty::new(urls).unwrap(),
        expected: ExpectedContent {
            digests: Some(
                DigestSet::new(vec![
                    ExpectedDigest::parse("md5", "321c3cf486ed509164edec1e1981fec8").unwrap(),
                ])
                .unwrap(),
            ),
            size: Some(7),
            accepted_observation: None,
        },
        limits: TransferLimits {
            file_bytes: maximum,
            transfer_bytes: maximum,
            deadline: Duration::from_secs(3),
            redirects: 2,
        },
        evidence: SourceEvidencePolicy::Compatibility,
        initial: InitialObservation::RequireEvidence,
    }
}
fn transport() -> HttpAcquisition {
    // Local HTTP fixtures need neither ambient proxy configuration nor OS TLS root discovery.
    HttpAcquisition::for_loopback_tests()
}
fn runtime() -> (OperationRuntime<Result<AcquiredContent>>, ResourceGovernor) {
    let governor = ResourceGovernor::new(ResourceRequest {
        jobs: 1,
        scratch_bytes: 1024,
        memory_bytes: 1 << 20,
        open_files: 10,
    });
    (OperationRuntime::new(governor.clone(), 8), governor)
}
async fn run(
    request: DownloadRequest,
) -> (
    Arc<OperationOutcome<Result<AcquiredContent>>>,
    ResourceGovernor,
) {
    let (runtime, governor) = runtime();
    let mut handle = runtime
        .start(move |mut scope| async move { Ok(transport().acquire(&mut scope, request).await) })
        .unwrap();
    let outcome = handle.wait().await;
    runtime.release_completed(handle.id());
    runtime.shutdown().await;
    (outcome, governor)
}
fn failure(outcome: &OperationOutcome<Result<AcquiredContent>>) -> &anyhow::Error {
    match outcome {
        OperationOutcome::Completed(Err(error)) => error,
        _ => panic!("expected transfer failure"),
    }
}

#[tokio::test]
async fn verified_bytes_keep_reservations_until_the_last_reader_retires() {
    let mut server = mockito::Server::new_async().await;
    let response = server
        .mock("GET", "/file")
        .with_body("payload")
        .create_async()
        .await;
    let (outcome, governor) = run(request(vec![format!("{}/file", server.url())], 16)).await;
    let content = match &*outcome {
        OperationOutcome::Completed(Ok(content)) => content.clone(),
        _ => panic!("transfer failed"),
    };
    assert_eq!(content.lease().len(), 7);
    assert!(
        matches!(content.evidence(), empack_core::digest::IntegrityEvidence::MatchedExpected { expected, .. }
        if expected.strongest() == empack_core::digest::DigestAlgorithm::Md5)
    );
    let mut reader = content.lease().open();
    drop(content);
    drop(outcome);
    assert_eq!(governor.status().reserved.jobs, 0);
    assert_eq!(governor.status().reserved.scratch_bytes, 7);
    assert_eq!(governor.status().reserved.open_files, 1);
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, b"payload");
    drop(reader);
    assert_eq!(governor.status().reserved, ResourceRequest::default());
    response.assert_async().await;
}

#[tokio::test]
async fn headers_digests_status_and_policy_cannot_be_mistaken_for_verified_content() {
    let mut server = mockito::Server::new_async().await;
    for (path, status, body) in [
        ("wrong", 200, "replace"),
        ("large", 200, "larger than sixteen bytes"),
        ("missing", 404, "missing"),
        ("auth", 403, "secret error"),
        ("quota", 429, "quota"),
        ("broken", 503, "unavailable"),
        ("partial", 206, "payload"),
    ] {
        let response = server
            .mock("GET", format!("/{path}").as_str())
            .with_status(status)
            .with_body(body)
            .create_async()
            .await;
        let (outcome, governor) = run(request(vec![format!("{}/{path}", server.url())], 16)).await;
        let error = failure(&outcome);
        assert!(!format!("{error:#}").contains(&server.url()));
        match path {
            "large" => assert!(matches!(
                error.downcast_ref(),
                Some(TransferError::ByteLimit)
            )),
            "auth" => assert!(matches!(
                error.downcast_ref(),
                Some(TransferError::Unauthorized)
            )),
            "quota" => assert!(matches!(
                error.downcast_ref(),
                Some(TransferError::RateLimited)
            )),
            "missing" => assert!(matches!(
                error.downcast_ref(),
                Some(TransferError::NotFound)
            )),
            _ => {}
        }
        assert_eq!(governor.status().reserved, ResourceRequest::default());
        response.assert_async().await;
    }
    let untouched = server.mock("GET", "/never").expect(0).create_async().await;
    let mut strong = request(vec![format!("{}/never", server.url())], 16);
    strong.evidence = SourceEvidencePolicy::StrongSourceRequired;
    let (outcome, _) = run(strong).await;
    assert!(failure(&outcome).to_string().contains("Strong source"));
    untouched.assert_async().await;
}

#[tokio::test]
async fn redirects_and_mirrors_keep_the_same_policy_and_hide_secret_locators() {
    let mut server = mockito::Server::new_async().await;
    let _missing = server
        .mock("GET", "/missing")
        .with_status(404)
        .create_async()
        .await;
    let _redirect = server
        .mock("GET", "/redirect")
        .with_status(302)
        .with_header("location", "/file")
        .create_async()
        .await;
    let _file = server
        .mock("GET", "/file")
        .with_body("payload")
        .create_async()
        .await;
    let (outcome, _) = run(request(
        vec![
            format!("{}/missing", server.url()),
            format!("{}/redirect", server.url()),
        ],
        16,
    ))
    .await;
    assert!(matches!(&*outcome, OperationOutcome::Completed(Ok(_))));
    let _bad = server
        .mock("GET", "/bad")
        .with_status(302)
        .with_header(
            "location",
            "https://secret:password@example.com/file?token=hidden",
        )
        .create_async()
        .await;
    let (outcome, _) = run(request(vec![format!("{}/bad", server.url())], 16)).await;
    let error = failure(&outcome);
    assert!(matches!(
        error.downcast_ref(),
        Some(TransferError::InvalidLocator)
    ));
    assert!(!format!("{error:?}").contains("hidden"));
    let mut denied = request(vec![format!("{}/redirect", server.url())], 16);
    denied.limits.redirects = 0;
    let (outcome, _) = run(denied).await;
    assert!(matches!(
        failure(&outcome).downcast_ref(),
        Some(TransferError::RedirectLimit)
    ));
    assert!(
        HttpAcquisition::new()
            .unwrap()
            .locator(&format!("{}/file", server.url()))
            .is_err()
    );
}

fn stalled_server(prefix: &'static [u8]) -> (String, std::thread::JoinHandle<usize>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let worker = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut input = [0; 4096];
        assert!(stream.read(&mut input).unwrap() > 0);
        stream.write_all(prefix).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        stream.read(&mut input).unwrap_or(0)
    });
    (format!("http://{address}/file"), worker)
}
#[tokio::test]
async fn chunk_overflow_and_deadline_do_not_wait_for_a_stalled_eof() {
    for (body, deadline, expected_deadline) in [
        (
            &b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n8\r\n12345678\r\n"[..],
            Duration::from_secs(2),
            false,
        ),
        (
            &b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n"[..],
            Duration::from_millis(100),
            true,
        ),
    ] {
        let (url, worker) = stalled_server(body);
        let mut request = request(vec![url], 7);
        request.limits.deadline = deadline;
        let (outcome, governor) = tokio::time::timeout(Duration::from_secs(2), run(request))
            .await
            .unwrap();
        let error = failure(&outcome);
        assert_eq!(
            matches!(error.downcast_ref(), Some(TransferError::Deadline)),
            expected_deadline
        );
        assert_eq!(governor.status().reserved, ResourceRequest::default());
        assert_eq!(worker.join().unwrap(), 0);
    }
}
#[tokio::test]
async fn cancellation_retires_the_blocking_verifier_and_response() {
    let (url, worker) = stalled_server(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n");
    let (runtime, governor) = runtime();
    let mut handle = runtime
        .start(move |mut scope| async move {
            Ok(transport()
                .acquire(&mut scope, request(vec![url], 16))
                .await)
        })
        .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    handle.cancel();
    tokio::time::timeout(Duration::from_secs(2), handle.wait())
        .await
        .unwrap();
    runtime.shutdown().await;
    assert_eq!(governor.status().reserved, ResourceRequest::default());
    assert_eq!(worker.join().unwrap(), 0);
}

#[tokio::test]
async fn mirror_retry_counts_partial_bytes_from_the_failed_attempt() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let worker = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut input = [0; 4096];
        assert!(stream.read(&mut input).unwrap() > 0);
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 7\r\n\r\npayl")
            .unwrap();
        std::thread::sleep(Duration::from_millis(50));
        // Deliberately truncate the response after four charged bytes.
    });
    let mut server = mockito::Server::new_async().await;
    let response = server
        .mock("GET", "/mirror")
        .with_body("payload")
        .create_async()
        .await;
    let mut request = request(
        vec![
            format!("http://{address}/file"),
            format!("{}/mirror", server.url()),
        ],
        16,
    );
    request.limits.transfer_bytes = 10;
    let (outcome, governor) = run(request).await;
    assert!(matches!(
        failure(&outcome).downcast_ref(),
        Some(TransferError::ByteLimit)
    ));
    assert_eq!(governor.status().reserved, ResourceRequest::default());
    worker.join().unwrap();
    response.assert_async().await;
}

#[tokio::test]
async fn dropping_acquisition_future_closes_and_retires_its_verifier() {
    let (url, worker) = stalled_server(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n");
    let (runtime, governor) = runtime();
    let mut handle = runtime
        .start(move |mut scope| async move {
            let transport = transport();
            let acquired = tokio::time::timeout(
                Duration::from_millis(100),
                transport.acquire(&mut scope, request(vec![url], 16)),
            )
            .await;
            assert!(acquired.is_err());
            Ok(Err(anyhow::anyhow!("caller stopped preparation")))
        })
        .unwrap();
    tokio::time::timeout(Duration::from_secs(2), handle.wait())
        .await
        .unwrap();
    runtime.shutdown().await;
    assert_eq!(governor.status().reserved, ResourceRequest::default());
    assert_eq!(worker.join().unwrap(), 0);
}

#[tokio::test]
async fn small_files_share_a_tight_scratch_budget_with_or_without_declared_sizes() {
    for declared_size in [true, false] {
        let mut server = mockito::Server::new_async().await;
        let response = server
            .mock("GET", "/file")
            .with_body("payload")
            .expect(2)
            .create_async()
            .await;
        let governor = ResourceGovernor::new(ResourceRequest {
            jobs: 1,
            scratch_bytes: 16,
            memory_bytes: 1 << 20,
            open_files: 10,
        });
        let runtime = OperationRuntime::new(governor.clone(), 1);
        let url = format!("{}/file", server.url());
        let mut handle = runtime
            .start(move |mut scope| async move {
                let transport = transport();
                let result: Result<Vec<AcquiredContent>> = async {
                    let mut content = Vec::new();
                    for _ in 0..2 {
                        let mut request = request(vec![url.clone()], 16);
                        if !declared_size {
                            request.expected.size = None;
                        }
                        content.push(transport.acquire(&mut scope, request).await?);
                    }
                    Ok(content)
                }
                .await;
                Ok(result)
            })
            .unwrap();
        let outcome = handle.wait().await;
        assert!(matches!(&*outcome,OperationOutcome::Completed(Ok(content)) if content.len()==2));
        assert_eq!(governor.status().reserved.scratch_bytes, 14);
        assert_eq!(governor.status().reserved.jobs, 0);
        runtime.release_completed(handle.id());
        drop(handle);
        drop(outcome);
        runtime.shutdown().await;
        assert_eq!(governor.status().reserved, ResourceRequest::default());
        response.assert_async().await;
    }
}

#[test]
fn provider_download_credentials_are_scoped_to_the_exact_https_origin() {
    use crate::engine::providers::ProviderCatalog;
    // Catalog configuration transfers a rule, not an arbitrary default header.
    let catalog =
        ProviderCatalog::for_loopback_tests("http://127.0.0.1:1", Some("fixture-key".into()));
    let transport = catalog.configure_acquisition(transport());
    for (url, authenticated) in [
        ("https://edge.forgecdn.net/files/1/2/mod.jar", true),
        ("https://edge.forgecdn.net:443/files/1/2/mod.jar", true),
        ("https://EDGE.FORGECDN.NET/files/1/2/mod.jar", true),
        ("http://edge.forgecdn.net/files/1/2/mod.jar", false),
        ("https://edge.forgecdn.net:8443/files/1/2/mod.jar", false),
        (
            "https://edge.forgecdn.net.example.com/files/1/2/mod.jar",
            false,
        ),
        ("https://sub.edge.forgecdn.net/files/1/2/mod.jar", false),
        ("https://edge.forgecdn.net./files/1/2/mod.jar", false),
        ("https://api.curseforge.com/v1/mods/1", false),
        ("https://mediafilez.forgecdn.net/files/1/2/mod.jar", false),
        ("https://cdn.modrinth.com/data/file.jar", false),
        ("http://127.0.0.1:1/file", false),
    ] {
        let request = transport
            .request(&Url::parse(url).unwrap())
            .build()
            .unwrap();
        let key = request.headers().get("x-api-key");
        assert_eq!(key.is_some(), authenticated, "{url}");
        if let Some(key) = key {
            assert!(key.is_sensitive());
            assert_eq!(key, "fixture-key");
        }
        assert!(!request.url().as_str().contains("fixture-key"));
        assert!(!format!("{request:?}").contains("fixture-key"));
    }
    let anonymous = ProviderCatalog::for_loopback_tests("http://127.0.0.1:1", None);
    let transport = anonymous.configure_acquisition(transport);
    assert!(
        transport
            .request(&Url::parse("https://edge.forgecdn.net/file").unwrap())
            .build()
            .unwrap()
            .headers()
            .get("x-api-key")
            .is_none()
    );
}

#[tokio::test]
async fn configured_provider_downloads_do_not_leak_keys_to_redirects_or_alternatives() {
    use crate::engine::providers::ProviderCatalog;
    let mut source = mockito::Server::new_async().await;
    let mut mirror = mockito::Server::new_async().await;
    let missing = source
        .mock("GET", "/missing")
        .match_header("x-api-key", mockito::Matcher::Missing)
        .with_status(404)
        .create_async()
        .await;
    let redirect = source
        .mock("GET", "/redirect")
        .match_header("x-api-key", mockito::Matcher::Missing)
        .with_status(302)
        .with_header("location", &format!("{}/file", mirror.url()))
        .create_async()
        .await;
    let file = mirror
        .mock("GET", "/file")
        .match_header("x-api-key", mockito::Matcher::Missing)
        .with_body("payload")
        .create_async()
        .await;
    let catalog = ProviderCatalog::for_loopback_tests(&source.url(), Some("fixture-key".into()));
    let transport = catalog.configure_acquisition(transport());
    let request = request(
        vec![
            format!("{}/missing", source.url()),
            format!("{}/redirect", source.url()),
        ],
        32,
    );
    let (runtime, governor) = runtime();
    let mut handle = runtime
        .start(move |mut scope| async move { Ok(transport.acquire(&mut scope, request).await) })
        .unwrap();
    let outcome = handle.wait().await;
    assert!(matches!(&*outcome, OperationOutcome::Completed(Ok(_))));
    runtime.release_completed(handle.id());
    runtime.shutdown().await;
    drop(handle);
    drop(outcome);
    assert_eq!(governor.status().reserved, ResourceRequest::default());
    missing.assert_async().await;
    redirect.assert_async().await;
    file.assert_async().await;
}
