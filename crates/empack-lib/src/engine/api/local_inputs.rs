//! Explicit host files satisfy captured build obligations; no locator or copy target is persisted.
use super::*;
use crate::engine::{
    acquisition::{LocalFileRequest, acquire_local_file},
    content::{ContentPool, InitialObservation},
    runtime::WorkScope,
};
use std::{collections::BTreeSet, time::Instant};

pub(super) async fn supply(
    prepared: RetainedOutput<PreparedBuild>,
    files: BTreeMap<AcquisitionKey, PathBuf>,
    limits: TransferLimits,
    file_limit: usize,
    scope: &mut WorkScope,
) -> Result<RetainedOutput<PreparedBuild>> {
    let mut total = prepared.acquisition.acquired.retained_bytes()?;
    ensure!(
        total <= limits.transfer_bytes,
        "Retained build content exceeds byte limit"
    );
    ensure!(
        prepared
            .acquisition
            .acquired
            .locked
            .values()
            .all(|file| file.content.lease().len() <= limits.file_bytes),
        "Retained build file exceeds byte limit"
    );
    if files.is_empty() {
        return Ok(prepared);
    }
    ensure!(
        files.len() <= file_limit,
        "Supplied build file count limit exceeded"
    );
    ensure!(
        files.values().all(|path| path.is_absolute()),
        "Supplied build files must use absolute host paths"
    );
    ensure!(
        prepared
            .acquisition
            .pending
            .iter()
            .filter(|need| files.contains_key(&need.key))
            .count()
            == files.len(),
        "Supplied local files must match pending build obligations; repeated or unrequested slots are not accepted"
    );
    let deadline = Instant::now()
        .checked_add(limits.deadline)
        .context("Local build acquisition deadline overflow")?;
    let metadata = scope.reserve_storage(ResourceRequest {
        memory_bytes: (files.len() as u64)
            .checked_mul(4096)
            .context("Supplied build metadata size overflow")?,
        ..Default::default()
    })?;
    let mut pool = ContentPool::owned(scope, limits.transfer_bytes - total).await?;
    let mut selected = BTreeMap::new();
    for need in &prepared.acquisition.pending {
        let Some(source) = files.get(&need.key) else {
            continue;
        };
        scope.cancellation().check()?;
        ensure!(
            Instant::now() < deadline,
            "Supplied build file acquisition deadline exceeded"
        );
        let mut file = acquire_local_file(
            scope,
            LocalFileRequest {
                source: source.clone(),
                expected: need.expected.clone(),
                maximum: limits
                    .file_bytes
                    .min(limits.transfer_bytes.saturating_sub(total)),
                evidence: prepared.request.evidence,
                initial: InitialObservation::RequireEvidence,
            },
        )
        .await?;
        total = total
            .checked_add(file.content.lease().len())
            .context("Supplied build size overflow")?;
        ensure!(
            total <= limits.transfer_bytes,
            "Supplied build batch exceeds byte limit"
        );
        file.content = pool.consolidate_owned(scope, file.content).await?;
        selected.insert(need.key.clone(), file);
    }
    scope.cancellation().check()?;
    ensure!(
        Instant::now() < deadline,
        "Supplied build file acquisition deadline exceeded"
    );
    let keys: BTreeSet<_> = selected.keys().cloned().collect();
    let updated = prepared
        .map(|mut value| {
            for (key, file) in selected {
                let previous = match key {
                    AcquisitionKey::Locked(key) => {
                        value.acquisition.acquired.locked.insert(key, file)
                    }
                };
                ensure!(
                    previous.is_none(),
                    "Supplied build file repeats an acquired slot"
                );
            }
            value
                .acquisition
                .pending
                .retain(|need| !keys.contains(&need.key));
            super::build_cache::refresh(&mut value);
            Ok::<_, anyhow::Error>(value)
        })
        .transpose()?;
    drop(metadata);
    Ok(updated)
}
