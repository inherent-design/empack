//! Cache capabilities belong to this transport instance, never process-global state.
use super::*;
use crate::engine::content::cache::ContentCache;

#[derive(Clone)]
pub(super) enum AcquisitionCache {
    Lookup(ContentCache),
    Execution(ContentCache),
}
impl HttpAcquisition {
    /// Preparation may reuse verified bytes; this capability cannot create persistent state.
    pub fn with_cache_lookup(mut self, cache: ContentCache) -> Self {
        self.cache = Some(AcquisitionCache::Lookup(cache));
        self
    }
    /// Attach only inside approved execution. Catalog/prepare transports never inherit it.
    pub(in crate::engine) fn with_execution_cache(mut self, cache: ContentCache) -> Self {
        self.cache = Some(AcquisitionCache::Execution(cache));
        self
    }
    pub(super) async fn cached(
        &self,
        scope: &mut WorkScope,
        expected: &ExpectedContent,
        evidence: SourceEvidencePolicy,
        initial: InitialObservation,
        limits: TransferLimits,
        budget: &mut TransferBudget,
    ) -> Result<Option<AcquiredContent>> {
        let Some(AcquisitionCache::Lookup(cache) | AcquisitionCache::Execution(cache)) =
            &self.cache
        else {
            return Ok(None);
        };
        // A URL is not byte identity. Unasserted catalogs always retain fresh observation.
        if expected.digests.is_none() && expected.accepted_observation.is_none() {
            return Ok(None);
        }
        let remaining = budget
            .maximum
            .checked_sub(budget.received)
            .context("Acquisition byte accounting overflow")?;
        ensure!(
            expected
                .size
                .is_none_or(|size| size <= remaining.min(limits.transfer_bytes)),
            TransferError::ByteLimit
        );
        let lookup = cache.lookup(scope).await?;
        let Some(lookup) = lookup.as_ref() else {
            return Ok(None);
        };
        let candidate = lookup
            .retain_expected(
                scope,
                expected.clone(),
                limits.file_bytes.min(remaining).min(limits.transfer_bytes),
                evidence,
                initial,
            )
            .await;
        scope.cancellation().check()?;
        ensure!(Instant::now() < budget.deadline, TransferError::Deadline);
        match candidate {
            Ok(Some(content)) => {
                budget.received = budget
                    .received
                    .checked_add(content.lease().len())
                    .context("Acquisition byte accounting overflow")?;
                Ok(Some(content))
            }
            Ok(None) => Ok(None),
            Err(_) => {
                tracing::debug!("Cached bytes did not satisfy this acquisition");
                Ok(None)
            }
        }
    }
    pub(super) async fn publish_cache(
        &self,
        scope: &mut WorkScope,
        files: &[AcquiredContent],
    ) -> Result<()> {
        let Some(AcquisitionCache::Execution(cache)) = &self.cache else {
            return Ok(());
        };
        // Unasserted catalog snapshots have no reusable source lookup key. Do not fill the
        // durable store with observations that a later request cannot independently select.
        let files = files
            .iter()
            .filter(|file| {
                matches!(
                    file.evidence(),
                    empack_core::digest::IntegrityEvidence::MatchedExpected { .. }
                )
            })
            .cloned()
            .collect();
        cache.publish(scope, files).await
    }
}
