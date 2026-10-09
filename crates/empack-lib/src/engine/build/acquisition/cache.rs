//! Read-only cache reuse for exact build and synchronization obligations.
use super::*;
use crate::engine::{content::cache::ContentCache, resources::ResourceRequest};
use std::time::Instant;

impl BuildAcquisitionResult {
    /// Cache hints are disposable. Every hit must satisfy the unchanged source assertions.
    /// This creates only private leases, never cache entries or project files.
    pub async fn acquire_cached(
        mut self,
        cache: &ContentCache,
        scope: &mut WorkScope,
        evidence: SourceEvidencePolicy,
        limits: TransferLimits,
    ) -> Result<Self> {
        if self.pending.is_empty() {
            return Ok(self);
        }
        let lookup = cache.lookup(scope).await?;
        let Some(lookup) = lookup.as_ref() else {
            return Ok(self);
        };
        let mut total = self.acquired.retained_bytes()?;
        ensure!(
            total <= limits.transfer_bytes,
            "Retained build content exceeds byte limit"
        );
        let mut pool = ContentPool::owned(scope, limits.transfer_bytes - total).await?;
        let deadline = Instant::now()
            .checked_add(limits.deadline)
            .context("Cache acquisition deadline overflow")?;
        let _metadata = scope.reserve_storage(ResourceRequest {
            memory_bytes: (self.pending.len() as u64)
                .checked_mul(4096)
                .context("Cache selection metadata overflow")?,
            ..Default::default()
        })?;
        let mut selected = BTreeMap::new();
        for need in &self.pending {
            scope.cancellation().check()?;
            ensure!(
                Instant::now() < deadline,
                "Cache acquisition deadline exceeded"
            );
            // Archive members carry permissions and extraction evidence that a byte cache cannot establish.
            if matches!(
                need.source,
                BuildContentSource::Embedded { .. }
                    | BuildContentSource::ProviderArchiveMember { .. }
            ) {
                continue;
            }
            let candidate = lookup
                .retain_expected(
                    scope,
                    need.expected.clone(),
                    limits.file_bytes.min(limits.transfer_bytes - total),
                    evidence,
                    InitialObservation::RequireEvidence,
                )
                .await;
            scope.cancellation().check()?;
            let content = match candidate {
                Ok(Some(content)) => content,
                Ok(None) => continue,
                Err(_) => {
                    tracing::debug!(
                        "Cached candidate did not satisfy the original build obligation"
                    );
                    continue;
                }
            };
            total = total
                .checked_add(content.lease().len())
                .context("Cached content size overflow")?;
            ensure!(
                total <= limits.transfer_bytes,
                "Cached build batch exceeds byte limit"
            );
            let content = pool.consolidate_owned(scope, content).await?;
            selected.insert(
                need.key.clone(),
                AcquiredBuildFile {
                    content,
                    permissions: FilePermissions {
                        readonly: false,
                        executable: false,
                    },
                },
            );
        }
        ensure!(
            Instant::now() < deadline,
            "Cache acquisition deadline exceeded"
        );
        self.pending
            .retain(|need| !selected.contains_key(&need.key));
        for (key, file) in selected {
            insert_acquired(&mut self.acquired, key, file)?;
        }
        Ok(self)
    }
}
