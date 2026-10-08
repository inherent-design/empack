//! Cache candidates satisfy captured obligations only after original assertions verify.
use super::*;
use crate::engine::{
    content::{ContentPool, InitialObservation, cache::ContentCache},
    mrpack::AcquiredBuildFile,
    runtime::WorkScope,
};
use empack_core::files::FilePermissions;
use std::time::Instant;

pub(super) async fn supply(
    prepared: RetainedOutput<PreparedBuild>,
    cache: Option<ContentCache>,
    limits: TransferLimits,
    scope: &mut WorkScope,
) -> Result<RetainedOutput<PreparedBuild>> {
    let Some(cache) = cache else {
        return Ok(prepared);
    };
    if prepared.acquisition.pending.is_empty() {
        return Ok(prepared);
    }
    let lookup = cache.lookup(scope).await?;
    let Some(lookup) = lookup.as_ref() else {
        return Ok(prepared);
    };
    let mut total = prepared.acquisition.acquired.retained_bytes()?;
    ensure!(
        total <= limits.transfer_bytes,
        "Retained build content exceeds byte limit"
    );
    let mut pool = ContentPool::owned(scope, limits.transfer_bytes - total).await?;
    let deadline = Instant::now()
        .checked_add(limits.deadline)
        .context("Cache acquisition deadline overflow")?;
    let _metadata = scope.reserve_storage(ResourceRequest {
        memory_bytes: (prepared.acquisition.pending.len() as u64)
            .checked_mul(4096)
            .context("Cache selection metadata overflow")?,
        ..Default::default()
    })?;
    let mut selected = BTreeMap::new();
    for need in &prepared.acquisition.pending {
        scope.cancellation().check()?;
        ensure!(
            Instant::now() < deadline,
            "Cache acquisition deadline exceeded"
        );
        // Embedded members carry archive permissions that a byte cache cannot establish.
        if matches!(need.source, BuildContentSource::Embedded { .. }) {
            continue;
        }
        let candidate = lookup
            .retain_expected(
                scope,
                need.expected.clone(),
                limits.file_bytes.min(limits.transfer_bytes - total),
                prepared.request.evidence,
                InitialObservation::RequireEvidence,
            )
            .await;
        scope.cancellation().check()?;
        let content = match candidate {
            Ok(Some(content)) => content,
            Ok(None) => continue,
            Err(_) => {
                tracing::debug!("Cached candidate did not satisfy the original build obligation");
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
    prepared
        .map(|mut value| {
            for (key, file) in &selected {
                let prior = match key {
                    AcquisitionKey::Locked(key) => value
                        .acquisition
                        .acquired
                        .locked
                        .insert(key.clone(), file.clone()),
                    AcquisitionKey::Observed(path) => value
                        .acquisition
                        .acquired
                        .observed
                        .insert(path.clone(), file.clone()),
                };
                ensure!(prior.is_none(), "Cached content repeated an acquired slot");
            }
            value
                .acquisition
                .pending
                .retain(|need| !selected.contains_key(&need.key));
            refresh(&mut value);
            Ok::<_, anyhow::Error>(value)
        })
        .transpose()
}

pub(super) fn refresh(value: &mut PreparedBuild) {
    let pending: std::collections::BTreeSet<_> = value
        .acquisition
        .pending
        .iter()
        .map(|need| &need.key)
        .collect();
    value.view.content = value.acquisition.pending.iter().map(describe).collect();
    value.view.file_names.retain(|key, _| pending.contains(key));
    value.view.unresolved.retain(|key| pending.contains(key));
    value.view.needs_network = value.request.outputs.as_slice().iter().any(|output| {
        matches!(
            output.target,
            BuildTarget::Client | BuildTarget::Server | BuildTarget::ServerFull
        )
    }) || value.acquisition.pending.iter().any(|need| {
        matches!(
            need.source,
            BuildContentSource::Download(_) | BuildContentSource::Provider { .. }
        )
    });
}

#[cfg(test)]
mod tests;
