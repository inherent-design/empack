//! Bounded unauthenticated metadata bytes. Authentication belongs to the release codec.
use super::*;
use crate::engine::runtime::RetainedOutput;

impl HttpAcquisition {
    /// Fetch metadata only from its enrolled HTTPS origin, including every redirect.
    /// No provider credentials or cache content are attached. The result is not trusted.
    pub async fn publisher_metadata(
        &self,
        scope: &mut WorkScope,
        url: &str,
        maximum: u64,
        deadline: Duration,
    ) -> Result<RetainedOutput<Vec<u8>>> {
        let url = crate::engine::release::https(url)?;
        ensure!(
            maximum > 0 && maximum <= (32 << 20) + (16 << 10),
            TransferError::ByteLimit
        );
        ensure!(!deadline.is_zero(), TransferError::Deadline);
        let mut transport = self.clone();
        transport.curseforge_key = None;
        let retained = ResourceRequest {
            memory_bytes: maximum,
            ..Default::default()
        };
        let work = scope.spawn(
            ResourceRequest {
                jobs: 1,
                memory_bytes: maximum + (256 << 10),
                open_files: 1,
                ..Default::default()
            },
            retained,
            move |cancel| async move {
                transport
                    .read_metadata(url, maximum, deadline, &cancel)
                    .await
            },
        )?;
        scope.accept(work.wait().await?)?.transpose()
    }

    async fn read_metadata(
        &self,
        mut url: Url,
        maximum: u64,
        deadline: Duration,
        cancel: &Cancellation,
    ) -> Result<Vec<u8>> {
        cancel.check()?;
        let origin = url.origin();
        let transfer = async {
            let mut redirects = 0;
            let mut response = loop {
                let response = self
                    .request(&url)
                    .send()
                    .await
                    .map_err(|error| transport_failure(&url, &error, "metadata request", 1))?;
                if response.status().is_redirection() {
                    ensure!(redirects < 5, TransferError::RedirectLimit);
                    let location = response
                        .headers()
                        .get(reqwest::header::LOCATION)
                        .and_then(|value| value.to_str().ok())
                        .ok_or(TransferError::InvalidLocator)?;
                    let next = url
                        .join(location)
                        .map_err(|_| TransferError::InvalidLocator)?;
                    let next = crate::engine::release::https(next.as_str())
                        .map_err(|_| TransferError::InvalidLocator)?;
                    ensure!(next.origin() == origin, TransferError::InvalidLocator);
                    url = next;
                    redirects += 1;
                    continue;
                }
                let status = response.status();
                ensure!(
                    status == StatusCode::OK,
                    match status {
                        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN =>
                            TransferError::Unauthorized,
                        StatusCode::NOT_FOUND => TransferError::NotFound,
                        StatusCode::TOO_MANY_REQUESTS => TransferError::RateLimited,
                        code if code.is_server_error() => TransferError::Server(code.as_u16()),
                        code => TransferError::Status(code.as_u16()),
                    }
                );
                break response;
            };
            ensure!(
                response
                    .headers()
                    .get(reqwest::header::CONTENT_ENCODING)
                    .is_none_or(|value| value == "identity"),
                TransferError::InvalidLocator
            );
            ensure!(
                response.content_length().is_none_or(|size| size <= maximum),
                TransferError::ByteLimit
            );
            // Reserve the exact bounded capacity, avoiding geometric growth past the reservation.
            let mut bytes = Vec::with_capacity(usize::try_from(maximum)?);
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|error| transport_failure(&url, &error, "metadata body", 1))?
            {
                ensure!(
                    chunk.len() as u64 <= maximum - bytes.len() as u64,
                    TransferError::ByteLimit
                );
                bytes.extend_from_slice(&chunk);
            }
            Ok(bytes)
        };
        tokio::select! {
            result = transfer => result,
            _ = cancel.cancelled() => { cancel.check()?; unreachable!() },
            _ = tokio::time::sleep(deadline) => Err(TransferError::Deadline.into()),
        }
    }
}

#[cfg(test)]
mod tests;
