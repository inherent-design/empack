//! Cache candidates satisfy captured obligations only after original assertions verify.
use super::*;
use crate::engine::{content::cache::ContentCache, runtime::WorkScope};

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
    let (mut prepared, permit) = prepared.into_parts();
    prepared.acquisition = prepared
        .acquisition
        .acquire_cached(&cache, scope, prepared.request.evidence, limits)
        .await?;
    refresh(&mut prepared);
    Ok(RetainedOutput::from_parts(prepared, permit))
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
    value.view.needs_network = value
        .request
        .outputs
        .as_slice()
        .iter()
        .any(|output| output.target.consumer() == empack_core::distribution::Consumer::Server)
        || value.acquisition.pending.iter().any(|need| {
            matches!(
                need.source,
                BuildContentSource::Download(_)
                    | BuildContentSource::Provider { .. }
                    | BuildContentSource::ProviderArchiveMember { .. }
            )
        });
}

#[cfg(test)]
mod tests;
