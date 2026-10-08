//! Durable source/association retention. Resumption still resolves, verifies and plans anew.
use super::*;
use crate::engine::{
    content::{
        InitialObservation, SourceEvidencePolicy,
        store::{CachedFileRequest, ContentStoreLimits, FileContentLookup, FileContentStore},
        validate_expectation,
    },
    continuation_store::{self as native, Kind},
    runtime::RetainedOutput,
};
use empack_core::digest::{ContentId, DigestSet, IntegrityEvidence};
use std::path::PathBuf;
mod store;

/// Native inspection handle, not a record decoded from editable data.
pub struct SavedImportRecord(crate::engine::continuation_store::SavedRecord);
/// Exact native observation used only to discard unchanged host-private state.
pub struct PendingImportCleanup(crate::engine::continuation_store::Cleanup);
/// Inspect without decoding or trusting saved data. Stale records remain explicitly removable.
pub async fn observe_pending_import(
    scope: &mut WorkScope,
    state: PathBuf,
    target: PathBuf,
) -> Result<Option<PendingImportCleanup>> {
    let work = scope.spawn_blocking(resources(0)?, ResourceRequest::default(), move |cancel| {
        store::observe(&state, &target, &cancel)
    })?;
    Ok(scope
        .accept(work.wait().await?)?
        .transpose()?
        .into_parts()
        .0)
}
pub async fn discard_observed_import(
    scope: &mut WorkScope,
    saved: PendingImportCleanup,
) -> Result<bool> {
    let work = scope.spawn_blocking(resources(0)?, ResourceRequest::default(), move |cancel| {
        store::discard(&saved, &cancel)
    })?;
    Ok(scope
        .accept(work.wait().await?)?
        .transpose()?
        .into_parts()
        .0)
}
pub struct ResumedImport {
    pub archive: AcquiredContent,
    record: RetainedOutput<store::Record>,
    lookup: RetainedOutput<FileContentLookup>,
    pub saved: SavedImportRecord,
}
fn resources(bytes: u64) -> Result<ResourceRequest> {
    Ok(ResourceRequest {
        jobs: 1,
        memory_bytes: native::record_memory(bytes)?,
        open_files: 12,
        ..Default::default()
    })
}
fn archive_expectation(archive: &AcquiredContent) -> ExpectedContent {
    let digests = match archive.evidence() {
        IntegrityEvidence::MatchedExpected { expected, .. } => Some(expected.clone()),
        IntegrityEvidence::ObservedOnly { .. } => None,
    };
    ExpectedContent {
        accepted_observation: digests.is_none().then(|| archive.lease().id()),
        digests,
        size: Some(archive.lease().len()),
    }
}
impl ResumedImport {
    /// Current exact provider assertions must match the saved facts before old associations apply.
    pub async fn restore(
        self,
        scope: &mut WorkScope,
        plan: &ImportContentPlan,
        mut provided: BTreeMap<ImportContentKey, AcquiredContent>,
        policy: SourceEvidencePolicy,
    ) -> Result<(
        BTreeMap<ImportContentKey, AcquiredContent>,
        SavedImportRecord,
    )> {
        ensure!(
            *plan.resume_revision().bytes() == self.record.revision,
            "Saved import is stale: source or provider facts changed"
        );
        ensure!(
            !self.record.strong || policy == SourceEvidencePolicy::StrongSourceRequired,
            "Saved import requires strong source evidence"
        );
        validate_expectation(
            &archive_expectation(&self.archive),
            plan.limits.archive.compressed_bytes,
            policy,
            InitialObservation::Accepted,
        )?;
        let mut total = provided.values().try_fold(0u64, |total, file| {
            total
                .checked_add(file.lease().len())
                .context("Import content size overflow")
        })?;
        ensure!(
            total <= plan.limits.total_bytes,
            "Supplied import exceeds content allowance"
        );
        let mut pool = ContentPool::owned(scope, plan.limits.total_bytes - total).await?;
        for (selector, id) in &self.record.files {
            let need = plan
                .needs
                .iter()
                .find(|need| {
                    need.key.selector() == *selector
                        && matches!(need.source, ImportedAcquisition::Downloads(_))
                })
                .context("Saved association has no current import obligation")?;
            if provided.contains_key(&need.key) {
                continue;
            }
            if let Some(content) = self
                .lookup
                .retain(
                    scope,
                    CachedFileRequest {
                        id: ContentId::from_sha256(*id),
                        expected: need.expected.clone(),
                        maximum: plan
                            .limits
                            .transfer
                            .file_bytes
                            .min(plan.limits.total_bytes - total),
                        evidence: policy,
                        initial: InitialObservation::RequireEvidence,
                    },
                )
                .await?
            {
                total = total
                    .checked_add(content.lease().len())
                    .context("Import content size overflow")?;
                provided.insert(
                    need.key.clone(),
                    pool.consolidate_owned(scope, content).await?,
                );
            }
        }
        Ok((provided, self.saved))
    }
}
/// Read-only: absence/staleness/errors do not create, repair or delete host/project state.
pub async fn load_pending_import(
    scope: &mut WorkScope,
    state: PathBuf,
    target: PathBuf,
) -> Result<Option<ResumedImport>> {
    let selected_state = state.clone();
    let selected_target = target.clone();
    let stat = scope.spawn_blocking(resources(0)?, ResourceRequest::default(), move |cancel| {
        cancel.check()?;
        native::record_bytes(&selected_state, Kind::Import, &selected_target)
    })?;
    let Some(bytes) = scope
        .accept(stat.wait().await?)?
        .transpose()?
        .into_parts()
        .0
    else {
        return Ok(None);
    };
    let selected_state = state.clone();
    let read = scope.spawn_blocking(
        resources(bytes)?,
        ResourceRequest {
            memory_bytes: native::record_memory(bytes)?,
            ..Default::default()
        },
        move |cancel| store::read_bounded(&selected_state, &target, bytes, &cancel),
    )?;
    let loaded = scope.accept(read.wait().await?)?.transpose()?;
    let (loaded, permit) = loaded.into_parts();
    let Some((saved, record)) = loaded else {
        return Ok(None);
    };
    let record = RetainedOutput::from_parts(record, permit);
    let work = scope.spawn_blocking(
        ResourceRequest {
            jobs: 1,
            open_files: 4,
            ..Default::default()
        },
        ResourceRequest {
            open_files: 1,
            ..Default::default()
        },
        move |cancel| {
            cancel.check()?;
            FileContentLookup::open_existing(
                &state.join("pending-import-content"),
                ContentStoreLimits::default(),
            )?
            .context("Saved import content store is missing")
        },
    )?;
    let lookup = scope.accept(work.wait().await?)?.transpose()?;
    let digests = if record.archive_digests.is_empty() {
        None
    } else {
        Some(DigestSet::parse(record.archive_digests.iter().map(
            |(algorithm, value)| (algorithm.as_str(), value.as_str()),
        ))?)
    };
    let id = ContentId::from_sha256(record.archive);
    let expected = ExpectedContent {
        accepted_observation: digests.is_none().then_some(id.clone()),
        digests,
        size: Some(record.archive_bytes),
    };
    let archive = lookup
        .retain(
            scope,
            CachedFileRequest {
                id,
                expected,
                maximum: ImportLimits::default().archive.compressed_bytes,
                evidence: if record.strong {
                    SourceEvidencePolicy::StrongSourceRequired
                } else {
                    SourceEvidencePolicy::Compatibility
                },
                initial: InitialObservation::Accepted,
            },
        )
        .await?
        .context("Saved import archive is missing")?;
    Ok(Some(ResumedImport {
        archive,
        record,
        lookup,
        saved,
    }))
}
/// Call only after explicit host approval. No project write or replacement grant is retained.
pub async fn save_pending_import(
    scope: &mut WorkScope,
    state: PathBuf,
    target: PathBuf,
    plan: ImportContentPlan,
    provided: BTreeMap<ImportContentKey, AcquiredContent>,
    policy: SourceEvidencePolicy,
    prior: Option<SavedImportRecord>,
) -> Result<SavedImportRecord> {
    let selected = target.clone();
    let work = scope.spawn_blocking(resources(0)?, ResourceRequest::default(), move |cancel| {
        store::bind(&selected, &cancel)
    })?;
    let (_, binding) = scope
        .accept(work.wait().await?)?
        .transpose()?
        .into_parts()
        .0;
    if let Some(prior) = &prior {
        ensure!(
            prior.0.binding == binding,
            "Saved import target changed before extension"
        );
    }
    let mut files = BTreeMap::new();
    let mut total = 0u64;
    for (key, content) in &provided {
        let need = plan
            .needs
            .iter()
            .find(|need| {
                &need.key == key && matches!(need.source, ImportedAcquisition::Downloads(_))
            })
            .context("Saved content has no import obligation")?;
        validate_expectation(
            &need.expected,
            plan.limits.transfer.file_bytes,
            policy,
            InitialObservation::RequireEvidence,
        )?;
        if let Some(expected) = &need.expected.digests {
            expected.check(content.observed_digests().values())?;
        }
        ensure!(
            need.expected
                .size
                .is_none_or(|size| size == content.lease().len())
                && need
                    .expected
                    .accepted_observation
                    .as_ref()
                    .is_none_or(|id| *id == content.lease().id()),
            "Saved import content differs from its source assertions"
        );
        total = total
            .checked_add(content.lease().len())
            .context("Import content size overflow")?;
        ensure!(
            total <= plan.limits.total_bytes,
            "Saved import exceeds content allowance"
        );
        files.insert(key.selector(), *content.lease().id().bytes());
    }
    let archive = plan.imported.archive().clone();
    validate_expectation(
        &archive_expectation(&archive),
        plan.limits.archive.compressed_bytes,
        policy,
        InitialObservation::Accepted,
    )?;
    let archive_digests = match archive.evidence() {
        IntegrityEvidence::MatchedExpected { expected, .. } => expected
            .values()
            .iter()
            .map(|digest| (digest.algorithm().name().into(), digest.hex()))
            .collect(),
        IntegrityEvidence::ObservedOnly { .. } => Vec::new(),
    };
    let record = store::Record {
        schema: 1,
        binding,
        archive: *archive.lease().id().bytes(),
        archive_bytes: archive.lease().len(),
        archive_digests,
        revision: *plan.resume_revision().bytes(),
        strong: policy == SourceEvidencePolicy::StrongSourceRequired,
        files,
    };
    let save_guard = crate::engine::retained_cleanup::begin_save(scope, state.clone()).await?;
    let selected = state.clone();
    let work = scope.spawn_blocking(
        ResourceRequest {
            jobs: 1,
            open_files: 4,
            ..Default::default()
        },
        ResourceRequest {
            open_files: 1,
            ..Default::default()
        },
        move |cancel| {
            cancel.check()?;
            FileContentStore::open(
                &selected.join("pending-import-content"),
                ContentStoreLimits::default(),
            )
        },
    )?;
    let content_store = scope.accept(work.wait().await?)?.transpose()?;
    content_store.publish_verified(scope, archive).await?;
    for content in provided.into_values() {
        content_store.publish_verified(scope, content).await?;
    }
    let bytes = native::encoded_bytes(&record, Kind::Import.maximum())?;
    let work = scope.spawn_blocking(
        resources(bytes)?,
        ResourceRequest::default(),
        move |cancel| {
            let _save_guard = save_guard;
            store::save(&state, &target, &record, prior.as_ref(), &cancel)
        },
    )?;
    Ok(scope
        .accept(work.wait().await?)?
        .transpose()?
        .into_parts()
        .0)
}
/// Explicit cleanup or successful publication; only the unchanged inspected record is deleted.
pub async fn discard_pending_import(
    scope: &mut WorkScope,
    saved: SavedImportRecord,
) -> Result<bool> {
    discard_observed_import(scope, PendingImportCleanup(saved.0.into_cleanup())).await
}
