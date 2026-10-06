use super::{CatalogLimits, ProjectSelector};
use crate::{
    application::process_runtime::Cancellation,
    networking::rate_budget::{HostBudgetRegistry, wait_for_budget},
};
use anyhow::{Result, ensure};
use empack_core::{
    identity::{PinSelector, ProviderProjectId},
    model::{ProviderKind, ResolvedPin},
};
use reqwest::{Client, StatusCode, Url, header::HeaderValue};
use std::{sync::Arc, time::Duration};
use tokio::time::Instant;

#[derive(Debug, thiserror::Error)]
pub enum CatalogError {
    #[error("Invalid provider selector")]
    InvalidSelector,
    #[error("Provider response exceeds the resolution byte allowance")]
    Limit,
    #[error("Provider resolution deadline elapsed")]
    Deadline,
    #[error("Provider authentication is required or was rejected")]
    Unauthorized,
    #[error("Provider project or file was not found")]
    NotFound,
    #[error("Provider request was rate limited")]
    RateLimited,
    #[error("Provider failed with HTTP {0}")]
    Server(u16),
    #[error("Unexpected provider response HTTP {0}")]
    Status(u16),
    #[error("Provider transport failed")]
    Network,
    #[error("Provider returned a redirect outside its fixed API contract")]
    Redirect,
    #[error("Provider returned an invalid or unsupported record")]
    InvalidRecord,
    #[error("No provider file satisfies the requested compatibility and release policy")]
    NoCompatibleSelection,
    #[error("Provider fingerprint index is incomplete")]
    IncompleteLookup,
    #[error("Provider content kind differs from the requested kind")]
    ContentKindMismatch,
    #[error("Provider returned a different project or file identity")]
    Identity,
    #[error("Provider content kind is unsupported by this adapter")]
    UnsupportedKind,
    #[error("Provider search did not identify exactly one project")]
    Ambiguous,
}
#[derive(Clone)]
pub(super) struct CatalogTransport {
    client: Client,
    curseforge_key: Option<HeaderValue>,
    budgets: Arc<HostBudgetRegistry>,
    #[cfg(test)]
    pub(super) test_origin: Option<Url>,
}
pub(super) struct RequestBudget {
    limits: CatalogLimits,
    received: u64,
    deadline: Instant,
}
impl RequestBudget {
    pub(super) fn check_deadline(&self) -> Result<()> {
        ensure!(Instant::now() < self.deadline, CatalogError::Deadline);
        Ok(())
    }
    pub(super) fn new(limits: CatalogLimits) -> Result<Self> {
        ensure!(
            limits.response_bytes > 0 && limits.transfer_bytes > 0,
            CatalogError::Limit
        );
        ensure!(!limits.deadline.is_zero(), CatalogError::Deadline);
        Ok(Self {
            limits,
            received: 0,
            deadline: Instant::now()
                .checked_add(limits.deadline)
                .ok_or(CatalogError::Deadline)?,
        })
    }
}
impl CatalogTransport {
    pub(super) fn has_curseforge_key(&self) -> bool {
        self.curseforge_key.is_some()
    }

    pub(super) fn new(key: Option<String>, budgets: Arc<HostBudgetRegistry>) -> Result<Self> {
        let key = key
            .map(|value| {
                let mut header =
                    HeaderValue::from_str(&value).map_err(|_| CatalogError::Unauthorized)?;
                header.set_sensitive(true);
                Ok::<_, CatalogError>(header)
            })
            .transpose()?;
        Ok(Self {
            client: Self::builder().build()?,
            curseforge_key: key,
            budgets,
            #[cfg(test)]
            test_origin: None,
        })
    }
    fn builder() -> reqwest::ClientBuilder {
        Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .no_gzip()
            .no_brotli()
            .no_deflate()
            .no_zstd()
            .connect_timeout(Duration::from_secs(15))
            .user_agent(concat!(
                "empack/",
                env!("CARGO_PKG_VERSION"),
                " (inherent-design/empack)"
            ))
    }
    #[cfg(test)]
    pub(super) fn test(origin: &str, key: Option<String>) -> Self {
        let origin = Url::parse(origin).unwrap();
        assert!(
            origin
                .host_str()
                .unwrap()
                .parse::<std::net::IpAddr>()
                .unwrap()
                .is_loopback()
        );
        Self {
            client: Self::builder()
                .no_proxy()
                .tls_certs_only([])
                .build()
                .unwrap(),
            curseforge_key: key.map(|key| HeaderValue::from_str(&key).unwrap()),
            budgets: Arc::new(HostBudgetRegistry::empty()),
            test_origin: Some(origin),
        }
    }

    pub(super) async fn project(
        &self,
        selector: &ProjectSelector,
        budget: &mut RequestBudget,
        cancel: &Cancellation,
    ) -> Result<Vec<u8>> {
        let value = selector.value();
        match selector.provider() {
            ProviderKind::Modrinth => {
                self.get(
                    ProviderKind::Modrinth,
                    &["project", &value],
                    &[],
                    budget,
                    cancel,
                )
                .await
            }
            ProviderKind::CurseForge if selector.is_slug() => {
                self.get(
                    ProviderKind::CurseForge,
                    &["mods", "search"],
                    &[("gameId", "432"), ("slug", &value), ("pageSize", "2")],
                    budget,
                    cancel,
                )
                .await
            }
            ProviderKind::CurseForge => {
                self.get(
                    ProviderKind::CurseForge,
                    &["mods", &value],
                    &[],
                    budget,
                    cancel,
                )
                .await
            }
        }
    }
    pub(super) async fn selection(
        &self,
        pin: &ResolvedPin,
        budget: &mut RequestBudget,
        cancel: &Cancellation,
    ) -> Result<Vec<u8>> {
        match (&pin.project, &pin.selection) {
            (ProviderProjectId::Modrinth(_), PinSelector::ModrinthVersion(id)) => {
                self.get(
                    ProviderKind::Modrinth,
                    &["version", id.as_str()],
                    &[],
                    budget,
                    cancel,
                )
                .await
            }
            (ProviderProjectId::CurseForge(project), PinSelector::CurseForgeFile(file)) => {
                self.get(
                    ProviderKind::CurseForge,
                    &["mods", &project.to_string(), "files", &file.to_string()],
                    &[],
                    budget,
                    cancel,
                )
                .await
            }
            _ => Err(CatalogError::Identity.into()),
        }
    }
    pub(super) async fn get(
        &self,
        provider: ProviderKind,
        segments: &[&str],
        query: &[(&str, &str)],
        budget: &mut RequestBudget,
        cancel: &Cancellation,
    ) -> Result<Vec<u8>> {
        self.request(provider, segments, query, None, budget, cancel)
            .await
    }
    pub(super) async fn fingerprint(
        &self,
        fingerprint: u32,
        budget: &mut RequestBudget,
        cancel: &Cancellation,
    ) -> Result<Vec<u8>> {
        // This POST only queries records; retries cannot create or mutate provider content.
        self.request(
            ProviderKind::CurseForge,
            &["fingerprints", "432"],
            &[],
            Some(serde_json::json!({"fingerprints": [fingerprint]})),
            budget,
            cancel,
        )
        .await
    }
    async fn request(
        &self,
        provider: ProviderKind,
        segments: &[&str],
        query: &[(&str, &str)],
        body: Option<serde_json::Value>,
        budget: &mut RequestBudget,
        cancel: &Cancellation,
    ) -> Result<Vec<u8>> {
        ensure!(
            provider != ProviderKind::CurseForge || self.curseforge_key.is_some(),
            CatalogError::Unauthorized
        );
        let (origin, host) = match provider {
            ProviderKind::Modrinth => ("https://api.modrinth.com/v2/", "api.modrinth.com"),
            ProviderKind::CurseForge => ("https://api.curseforge.com/v1/", "api.curseforge.com"),
        };
        let mut url = Url::parse(origin).expect("static provider origin");
        #[cfg(test)]
        if let Some(origin) = &self.test_origin {
            url = origin.clone();
        }
        url.path_segments_mut()
            .expect("hierarchical origin")
            .pop_if_empty()
            .extend(segments);
        if !query.is_empty() {
            url.query_pairs_mut().extend_pairs(query.iter().copied());
        }
        let rate = self.budgets.for_host(host);
        let execute = async {
            for attempt in 0..3 {
                cancel.check()?;
                if let Some(rate) = &rate {
                    wait_for_budget(rate.as_ref()).await;
                }
                let mut request = match &body {
                    Some(body) => self.client.post(url.clone()).json(body),
                    None => self.client.get(url.clone()),
                };
                if provider == ProviderKind::CurseForge {
                    request = request.header(
                        "x-api-key",
                        self.curseforge_key
                            .as_ref()
                            .ok_or(CatalogError::Unauthorized)?
                            .clone(),
                    );
                }
                let mut response = request.send().await.map_err(|_| CatalogError::Network)?;
                let status = response.status();
                if let Some(rate) = &rate {
                    rate.record_response(response.headers(), status);
                }
                let retry = status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error();
                if status != StatusCode::OK {
                    let error = match status {
                        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
                            CatalogError::Unauthorized
                        }
                        StatusCode::NOT_FOUND => CatalogError::NotFound,
                        StatusCode::TOO_MANY_REQUESTS => CatalogError::RateLimited,
                        _ if status.is_redirection() => CatalogError::Redirect,
                        _ if status.is_server_error() => CatalogError::Server(status.as_u16()),
                        _ => CatalogError::Status(status.as_u16()),
                    };
                    if !retry || attempt == 2 {
                        return Err(error.into());
                    }
                    // Read-only endpoints; never read/log error bodies or reset the deadline.
                    let delay = response
                        .headers()
                        .get("retry-after")
                        .and_then(|v| v.to_str().ok())
                        .and_then(|v| v.parse::<u64>().ok())
                        .unwrap_or(1 << attempt);
                    drop(response);
                    tokio::time::sleep(Duration::from_secs(delay)).await;
                    continue;
                }
                let remaining = budget.limits.transfer_bytes.saturating_sub(budget.received);
                let maximum = budget.limits.response_bytes.min(remaining);
                ensure!(
                    response
                        .content_length()
                        .is_none_or(|bytes| bytes <= maximum),
                    CatalogError::Limit
                );
                // Reserve exactly the bounded response cap; chunked bodies cannot cause geometric
                // Vec growth beyond the admitted response allocation.
                let capacity = usize::try_from(maximum).map_err(|_| CatalogError::Limit)?;
                let mut bytes = Vec::new();
                bytes
                    .try_reserve_exact(capacity)
                    .map_err(|_| CatalogError::Limit)?;
                while let Some(chunk) = response.chunk().await.map_err(|_| CatalogError::Network)? {
                    budget.received = budget
                        .received
                        .checked_add(chunk.len() as u64)
                        .ok_or(CatalogError::Limit)?;
                    ensure!(
                        budget.received <= budget.limits.transfer_bytes
                            && chunk.len() as u64 <= maximum.saturating_sub(bytes.len() as u64),
                        CatalogError::Limit
                    );
                    bytes.extend_from_slice(&chunk);
                }
                return Ok(bytes);
            }
            unreachable!("last retry returns its classified error")
        };
        tokio::select! {
            biased;
            _ = cancel.cancelled() => { cancel.check()?; unreachable!() },
            result = tokio::time::timeout_at(budget.deadline, execute) => result.map_err(|_| CatalogError::Deadline)?,
        }
    }
}
