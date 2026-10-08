//! Bounded HTTP acquisition into verified private leases; no project or cache writer.
use super::{
    content::{
        AcquiredContent, InitialObservation, SourceEvidencePolicy, validate_expectation,
        verify_stream,
    },
    resources::ResourceRequest,
    runtime::WorkScope,
};
use crate::application::process_runtime::Cancellation;
use anyhow::{Context, Result, ensure};
use empack_core::model::{ExpectedContent, NonEmpty};
use reqwest::{Client, StatusCode, Url, header::HeaderValue};
use std::{
    io::{self, Read},
    time::Duration,
};
use tokio::{sync::mpsc, time::Instant};

mod local;
pub use local::{LocalFileRequest, acquire_local_file};

const CHUNK_BYTES: usize = 64 * 1024;

#[derive(Clone, Copy)]
pub struct TransferLimits {
    pub file_bytes: u64,
    /// Cumulative received bytes across alternatives; failure cannot reset this allowance.
    pub transfer_bytes: u64,
    pub deadline: Duration,
    pub redirects: usize,
}
impl Default for TransferLimits {
    fn default() -> Self {
        Self {
            file_bytes: 8 << 30,
            transfer_bytes: 8 << 30,
            deadline: Duration::from_secs(300),
            redirects: 5,
        }
    }
}
/// Transient URLs are deliberately not Debug/serializable; provider locators may contain secrets.
pub struct DownloadRequest {
    pub alternatives: NonEmpty<String>,
    pub expected: ExpectedContent,
    pub limits: TransferLimits,
    pub evidence: SourceEvidencePolicy,
    pub initial: InitialObservation,
}
#[derive(Debug, thiserror::Error)]
pub enum TransferError {
    #[error("Download locator or redirect violates transport policy")]
    InvalidLocator,
    #[error("Download deadline elapsed")]
    Deadline,
    #[error("Download exceeds its byte allowance")]
    ByteLimit,
    #[error("Download authentication was rejected")]
    Unauthorized,
    #[error("Download was not found")]
    NotFound,
    #[error("Download was rate limited")]
    RateLimited,
    #[error("Download provider failed with HTTP {0}")]
    Server(u16),
    #[error("Unexpected download response HTTP {0}")]
    Status(u16),
    #[error("Download transport failed")]
    Network,
    #[error("Download redirect limit exceeded")]
    RedirectLimit,
}
struct TransferBudget {
    maximum: u64,
    received: u64,
    deadline: Instant,
}
impl TransferBudget {
    fn new(limits: TransferLimits) -> Result<Self> {
        ensure!(!limits.deadline.is_zero(), TransferError::Deadline);
        Ok(Self {
            maximum: limits.transfer_bytes,
            received: 0,
            deadline: Instant::now()
                .checked_add(limits.deadline)
                .context("Download deadline overflow")?,
        })
    }
}
/// No automatic redirects, cookies or response decompression. Optional provider credentials
/// have fixed HTTPS origin rules, re-evaluated at every hop; no locator grants credential access.
#[derive(Clone)]
pub struct HttpAcquisition {
    client: Client,
    curseforge_key: Option<HeaderValue>,
    #[cfg(test)]
    allow_loopback_http: bool,
}
impl HttpAcquisition {
    pub fn new() -> Result<Self> {
        Ok(Self {
            client: Self::client_builder().build()?,
            curseforge_key: None,
            #[cfg(test)]
            allow_loopback_http: false,
        })
    }
    #[cfg(test)]
    pub(crate) fn for_loopback_tests() -> Self {
        Self {
            client: Self::client_builder()
                .no_proxy()
                .tls_certs_only([])
                .build()
                .unwrap(),
            curseforge_key: None,
            allow_loopback_http: true,
        }
    }
    pub(in crate::engine) fn with_curseforge_key(mut self, key: Option<HeaderValue>) -> Self {
        self.curseforge_key = key.map(|mut key| {
            key.set_sensitive(true);
            key
        });
        self
    }
    fn request(&self, url: &Url) -> reqwest::RequestBuilder {
        let mut request = self.client.get(url.clone());
        if url.scheme() == "https"
            && url.host_str() == Some("edge.forgecdn.net")
            && url.port_or_known_default() == Some(443)
            && let Some(key) = &self.curseforge_key
        {
            request = request.header("x-api-key", key.clone());
        }
        request
    }
    fn client_builder() -> reqwest::ClientBuilder {
        Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .no_gzip()
            .no_brotli()
            .no_deflate()
            .no_zstd()
            .user_agent(concat!("empack/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(15))
    }
    pub(in crate::engine) fn validate_locator(&self, input: &str) -> Result<()> {
        self.locator(input).map(|_| ())
    }
    fn locator(&self, input: &str) -> Result<Url> {
        let url = Url::parse(input).map_err(|_| TransferError::InvalidLocator)?;
        let allowed = url.scheme() == "https";
        #[cfg(test)]
        let allowed = allowed
            || (self.allow_loopback_http
                && url.scheme() == "http"
                && url.host_str().is_some_and(|host| {
                    host.parse::<std::net::IpAddr>()
                        .is_ok_and(|ip| ip.is_loopback())
                }));
        ensure!(
            allowed
                && url.host_str().is_some()
                && url.username().is_empty()
                && url.password().is_none()
                && url.fragment().is_none(),
            TransferError::InvalidLocator
        );
        Ok(url)
    }
    /// The scope owns the verifier until retirement. Dropping a caller future closes its byte
    /// channel; it cannot leave an unowned verifier or a usable unverified content object.
    pub async fn acquire(
        &self,
        scope: &mut WorkScope,
        request: DownloadRequest,
    ) -> Result<AcquiredContent> {
        let mut budget = TransferBudget::new(request.limits)?;
        self.acquire_budget(scope, request, &mut budget).await
    }
    /// All downloads share bytes and time; no successful subset escapes a later failure.
    pub(in crate::engine) async fn acquire_batch(
        &self,
        scope: &mut WorkScope,
        requests: Vec<DownloadRequest>,
        limits: TransferLimits,
    ) -> Result<Vec<AcquiredContent>> {
        let mut budget = TransferBudget::new(limits)?;
        // Validate every declaration before any payload transfer.
        for request in &requests {
            validate_expectation(
                &request.expected,
                request.limits.file_bytes,
                request.evidence,
                request.initial,
            )?;
            for locator in request.alternatives.as_slice() {
                self.locator(locator)?;
            }
        }
        let mut pool = super::content::ContentPool::owned(scope, limits.transfer_bytes).await?;
        let mut content = Vec::new();
        for request in requests {
            let acquired = self.acquire_budget(scope, request, &mut budget).await?;
            content.push(pool.consolidate_owned(scope, acquired).await?);
        }
        scope.cancellation().check()?;
        ensure!(Instant::now() < budget.deadline, TransferError::Deadline);
        Ok(content)
    }
    async fn acquire_budget(
        &self,
        scope: &mut WorkScope,
        request: DownloadRequest,
        budget: &mut TransferBudget,
    ) -> Result<AcquiredContent> {
        let DownloadRequest {
            alternatives,
            expected,
            limits,
            evidence,
            initial,
        } = request;
        validate_expectation(&expected, limits.file_bytes, evidence, initial)?;
        let urls = alternatives
            .as_slice()
            .iter()
            .map(|url| self.locator(url))
            .collect::<Result<Vec<_>>>()?;
        ensure!(!limits.deadline.is_zero(), TransferError::Deadline);
        let deadline = Instant::now()
            .checked_add(limits.deadline)
            .context("Download deadline overflow")?;
        let deadline = deadline.min(budget.deadline);
        ensure!(Instant::now() < deadline, TransferError::Deadline);
        let limits = TransferLimits {
            transfer_bytes: budget
                .maximum
                .min(budget.received.saturating_add(limits.transfer_bytes)),
            ..limits
        };
        let operation_cancel = scope.cancellation();
        let mut last = None;
        for url in urls {
            operation_cancel.check()?;
            let cancel = operation_cancel.child();
            let worker_cancel = cancel.clone();
            let expected = expected.clone();
            // Declared sizes are exact upper bounds. Unknown transfers use only currently
            // available scratch, with the same reduced cap enforced while receiving/writing.
            let maximum = expected
                .size
                .unwrap_or_else(|| limits.file_bytes.min(scope.available_scratch_bytes()));
            let attempt_limits = TransferLimits {
                file_bytes: maximum,
                ..limits
            };
            let (sender, receiver) = mpsc::channel(2);
            let retained = ResourceRequest {
                scratch_bytes: maximum,
                open_files: 3,
                ..ResourceRequest::default()
            };
            let worker = scope.spawn_blocking(
                ResourceRequest {
                    jobs: 1,
                    memory_bytes: (CHUNK_BYTES * 4) as u64,
                    ..retained
                },
                retained,
                move |_| {
                    verify_stream(
                        &mut ChunkReader {
                            receiver,
                            current: io::Cursor::new(Vec::new()),
                        },
                        &expected,
                        maximum,
                        evidence,
                        initial,
                        &worker_cancel,
                    )
                },
            )?;
            let transfer = self
                .pump(
                    url,
                    sender,
                    attempt_limits,
                    deadline,
                    &mut budget.received,
                    &cancel,
                )
                .await;
            // Pump owns the only sender. It is now closed on success or failure, unblocking reads.
            if transfer.is_err() {
                cancel.cancel();
            }
            let wait = worker.wait();
            tokio::pin!(wait);
            let result = tokio::select! {
                result = &mut wait => result?,
                _ = operation_cancel.cancelled() => { cancel.cancel(); let _ = wait.await; operation_cancel.check()?; unreachable!() },
                _ = tokio::time::sleep_until(deadline) => { cancel.cancel(); let _ = wait.await; return Err(TransferError::Deadline.into()); },
            };
            let output = scope.accept(result)?;
            ensure!(Instant::now() < deadline, TransferError::Deadline);
            match transfer {
                Ok(()) => return AcquiredContent::retain_resources(output.transpose()?),
                Err(error) => {
                    drop(output);
                    // Try mirrors only for absence or provider/transport failure. Integrity,
                    // authorization, quota and policy errors cannot silently become success.
                    if error.downcast_ref::<TransferError>().is_some_and(|error| {
                        matches!(
                            error,
                            TransferError::NotFound
                                | TransferError::Server(_)
                                | TransferError::Network
                        )
                    }) {
                        last = Some(error);
                    } else {
                        return Err(error);
                    }
                }
            }
        }
        Err(last.context("No download alternatives")?)
    }
    async fn pump(
        &self,
        mut url: Url,
        sender: mpsc::Sender<Vec<u8>>,
        limits: TransferLimits,
        deadline: Instant,
        received: &mut u64,
        cancel: &Cancellation,
    ) -> Result<()> {
        let transfer = async {
            let mut redirects = 0;
            let mut response = loop {
                let response = self
                    .request(&url)
                    .send()
                    .await
                    .map_err(|_| TransferError::Network)?;
                if response.status().is_redirection() {
                    ensure!(redirects < limits.redirects, TransferError::RedirectLimit);
                    let location = response
                        .headers()
                        .get(reqwest::header::LOCATION)
                        .and_then(|header| header.to_str().ok())
                        .ok_or(TransferError::InvalidLocator)?;
                    let next = url
                        .join(location)
                        .map_err(|_| TransferError::InvalidLocator)?;
                    url = self.locator(next.as_str())?;
                    redirects += 1;
                    continue;
                }
                let status = response.status();
                if status != StatusCode::OK {
                    return Err(match status {
                        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
                            TransferError::Unauthorized
                        }
                        StatusCode::NOT_FOUND => TransferError::NotFound,
                        StatusCode::TOO_MANY_REQUESTS => TransferError::RateLimited,
                        code if code.is_server_error() => TransferError::Server(code.as_u16()),
                        code => TransferError::Status(code.as_u16()),
                    }
                    .into());
                }
                break response;
            };
            ensure!(
                response
                    .content_length()
                    .is_none_or(|len| len <= limits.file_bytes
                        && len <= limits.transfer_bytes.saturating_sub(*received)),
                TransferError::ByteLimit
            );
            let mut file_bytes = 0u64;
            while let Some(chunk) = response.chunk().await.map_err(|_| TransferError::Network)? {
                *received = received
                    .checked_add(chunk.len() as u64)
                    .ok_or(TransferError::ByteLimit)?;
                file_bytes = file_bytes
                    .checked_add(chunk.len() as u64)
                    .ok_or(TransferError::ByteLimit)?;
                ensure!(
                    *received <= limits.transfer_bytes && file_bytes <= limits.file_bytes,
                    TransferError::ByteLimit
                );
                for part in chunk.chunks(CHUNK_BYTES) {
                    sender
                        .send(part.to_vec())
                        .await
                        .context("Content verifier stopped receiving bytes")?;
                }
            }
            Ok(())
        };
        tokio::select! {
            result = transfer => result,
            _ = cancel.cancelled() => { cancel.check()?; unreachable!() },
            _ = tokio::time::sleep_until(deadline) => Err(TransferError::Deadline.into()),
        }
    }
}
struct ChunkReader {
    receiver: mpsc::Receiver<Vec<u8>>,
    current: io::Cursor<Vec<u8>>,
}
impl Read for ChunkReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        loop {
            let count = self.current.read(buffer)?;
            if count != 0 {
                return Ok(count);
            }
            match self.receiver.blocking_recv() {
                Some(chunk) => self.current = io::Cursor::new(chunk),
                None => return Ok(0),
            }
        }
    }
}
#[cfg(test)]
mod tests;
