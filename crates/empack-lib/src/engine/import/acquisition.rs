//! Resolve and verify the complete imported byte inventory without a live project capability.
use super::*;
use crate::engine::{
    acquisition::{DownloadRequest, HttpAcquisition, TransferBudget, TransferLimits},
    content::{ContentPool, verify_stream},
    providers::{CatalogLimits, ExactBatch, ProviderCatalog},
    resources::AdmissionPermit,
};
use empack_core::model::NonEmpty;
mod identity;
mod local;
pub mod suspension;
pub use local::ImportLocalFile;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum ImportContentKey {
    Declared(usize),
    Override(usize),
    Provider { pin: ResolvedPin, filename: String },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportInputReason {
    RestrictedDownload,
    UnsupportedTransport,
}
#[derive(Debug, Clone)]
pub struct ImportContentInput {
    pub key: ImportContentKey,
    pub expected: ExpectedContent,
    pub reason: ImportInputReason,
}
#[derive(Clone, Copy)]
pub struct ImportContentLimits {
    pub catalog: CatalogLimits,
    pub archive: ArchiveLimits,
    pub transfer: TransferLimits,
    pub records: usize,
    /// All declared, embedded and provider bytes, even when their contents are equal.
    pub total_bytes: u64,
}
impl Default for ImportContentLimits {
    fn default() -> Self {
        Self {
            catalog: CatalogLimits {
                transfer_bytes: 256 << 20,
                deadline: std::time::Duration::from_secs(300),
                ..Default::default()
            },
            archive: ArchiveLimits::default(),
            transfer: TransferLimits {
                transfer_bytes: 64 << 30,
                deadline: std::time::Duration::from_secs(1800),
                ..Default::default()
            },
            records: 100_000,
            total_bytes: 64 << 30,
        }
    }
}
struct Need {
    key: ImportContentKey,
    expected: ExpectedContent,
    source: ImportedAcquisition,
    permissions: Option<FilePermissions>,
}
/// Declarations and provider facts are preserved. This does not choose destinations, optional
/// defaults, runtime alternatives, a dependency policy or a live-project replacement footprint.
pub struct ImportContentPlan {
    imported: RetainedOutput<ImportedProject>,
    providers: ExactBatch,
    needs: Vec<Need>,
    limits: ImportContentLimits,
    _index: Vec<AdmissionPermit>,
}
pub enum ImportContentOutcome {
    NeedsInput {
        plan: ImportContentPlan,
        pending: Vec<ImportContentInput>,
        provided: BTreeMap<ImportContentKey, AcquiredContent>,
    },
    Ready(VerifiedImportContent),
}
pub struct VerifiedImportContent {
    pub(super) evidence: SourceEvidencePolicy,
    plan: ImportContentPlan,
    content: BTreeMap<ImportContentKey, AcquiredContent>,
    permissions: BTreeMap<ImportContentKey, FilePermissions>,
}
impl VerifiedImportContent {
    pub fn plan(&self) -> &ImportContentPlan {
        &self.plan
    }
    pub fn content(&self) -> &BTreeMap<ImportContentKey, AcquiredContent> {
        &self.content
    }
    pub fn permissions(&self) -> &BTreeMap<ImportContentKey, FilePermissions> {
        &self.permissions
    }
}
impl ImportContentPlan {
    pub(super) fn limits(&self) -> ImportContentLimits {
        self.limits
    }
    pub fn imported(&self) -> &ImportedProject {
        &self.imported
    }
    pub fn providers(&self) -> &ExactBatch {
        &self.providers
    }
    /// Resolve every exact provider reference under one cumulative budget before acquiring files.
    pub async fn resolve(
        scope: &mut WorkScope,
        imported: RetainedOutput<ImportedProject>,
        catalog: &ProviderCatalog,
        limits: ImportContentLimits,
    ) -> Result<Self> {
        let initial = imported
            .files
            .len()
            .checked_add(imported.overrides.len())
            .and_then(|n| n.checked_add(imported.providers.len()))
            .context("Import record count overflow")?;
        ensure!(
            initial <= limits.records,
            "Import exceeds content record allowance"
        );
        let mut index = vec![
            scope.reserve_storage(ResourceRequest {
                memory_bytes: (initial as u64)
                    .checked_mul(4096)
                    .context("Import content estimate overflow")?,
                ..Default::default()
            })?,
        ];
        let pins: Vec<_> = imported
            .providers
            .iter()
            .map(|record| record.selection.clone())
            .collect();
        let providers = catalog
            .resolve_exact_batch(scope, &pins, limits.records, limits.catalog)
            .await?;
        let actual = providers.records().values().try_fold(
            imported.files.len() + imported.overrides.len(),
            |count, record| {
                count
                    .checked_add(record.files.as_slice().len())
                    .context("Import record overflow")
            },
        )?;
        ensure!(
            actual <= limits.records,
            "Import exceeds content record allowance"
        );
        index.push(
            scope.reserve_storage(ResourceRequest {
                memory_bytes: (actual.saturating_sub(initial) as u64)
                    .checked_mul(4096)
                    .context("Import content estimate overflow")?,
                ..Default::default()
            })?,
        );
        let mut needs = Vec::new();
        for (overrides, records) in [(false, &imported.files), (true, &imported.overrides)] {
            for (index, file) in records.iter().enumerate() {
                scope.cancellation().check()?;
                needs.push(Need {
                    key: if overrides {
                        ImportContentKey::Override(index)
                    } else {
                        ImportContentKey::Declared(index)
                    },
                    expected: file.expected.clone(),
                    source: file.acquisition.clone(),
                    permissions: file.permissions,
                });
            }
        }
        for (pin, record) in providers.records() {
            for file in record.files.as_slice() {
                ensure!(
                    needs.len() < limits.records,
                    "Import exceeds content record allowance"
                );
                needs.push(Need {
                    key: ImportContentKey::Provider {
                        pin: pin.clone(),
                        filename: file.filename.clone(),
                    },
                    expected: file.expected.clone(),
                    source: ImportedAcquisition::Downloads(file.alternatives.clone()),
                    permissions: None,
                });
            }
        }
        let mut total = 0u64;
        for need in &needs {
            // Both supported formats and exact catalog records declare sizes. Missing lengths
            // cannot silently reserve zero bytes in preparation.
            let size = need
                .expected
                .size
                .context("Imported content lacks declared size")?;
            ensure!(
                size <= limits.transfer.file_bytes,
                "Imported file exceeds byte allowance"
            );
            total = total.checked_add(size).context("Imported size overflow")?;
            ensure!(
                total <= limits.total_bytes,
                "Import exceeds total content allowance"
            );
        }
        Ok(Self {
            imported,
            providers,
            needs,
            limits,
            _index: index,
        })
    }
    /// Supplied files bind to an exact obligation and are independently reverified. Missing
    /// restricted files are reported before archive extraction or any unrelated download.
    pub async fn acquire(
        self,
        scope: &mut WorkScope,
        transport: &HttpAcquisition,
        mut provided: BTreeMap<ImportContentKey, AcquiredContent>,
        evidence: SourceEvidencePolicy,
    ) -> Result<ImportContentOutcome> {
        ensure!(
            provided.keys().all(|key| self
                .needs
                .iter()
                .any(|need| &need.key == key
                    && matches!(need.source, ImportedAcquisition::Downloads(_)))),
            "Supplied content has no import obligation"
        );
        let mut pending = Vec::new();
        let mut budget = TransferBudget::new(self.limits.transfer)?;
        let mut cached_pool = None;
        for need in &self.needs {
            scope.cancellation().check()?;
            if provided.contains_key(&need.key) {
                continue;
            }
            if let ImportedAcquisition::Downloads(urls) = &need.source {
                let reason = if urls.is_empty() {
                    Some(ImportInputReason::RestrictedDownload)
                } else if urls
                    .iter()
                    .all(|url| transport.validate_locator(url).is_err())
                {
                    Some(ImportInputReason::UnsupportedTransport)
                } else {
                    None
                };
                if let Some(reason) = reason {
                    if let Some(content) = transport
                        .cached(
                            scope,
                            &need.expected,
                            evidence,
                            InitialObservation::RequireEvidence,
                            self.limits.transfer,
                            &mut budget,
                        )
                        .await?
                    {
                        if cached_pool.is_none() {
                            cached_pool =
                                Some(ContentPool::owned(scope, self.limits.total_bytes).await?);
                        }
                        let pool = cached_pool.as_mut().expect("cache pool initialized");
                        provided.insert(
                            need.key.clone(),
                            pool.consolidate_owned(scope, content).await?,
                        );
                        continue;
                    }
                    pending.push(ImportContentInput {
                        key: need.key.clone(),
                        expected: need.expected.clone(),
                        reason,
                    });
                }
            }
        }
        if !pending.is_empty() {
            return Ok(ImportContentOutcome::NeedsInput {
                plan: self,
                pending,
                provided,
            });
        }
        let mut content = BTreeMap::new();
        let mut permissions = BTreeMap::new();
        let mut pool = match cached_pool {
            Some(pool) => pool,
            None => ContentPool::owned(scope, self.limits.total_bytes).await?,
        };
        // Open the retained archive once, with its metadata reservation traveling through workers.
        let mut archive = None;
        if self.needs.iter().any(|need| {
            matches!(need.source, ImportedAcquisition::Embedded(_))
                && !provided.contains_key(&need.key)
        }) {
            let source = self.imported.archive().clone();
            let limits = self.limits.archive;
            let retained = ResourceRequest {
                memory_bytes: (limits.entries as u64)
                    .checked_mul(2048)
                    .context("Archive estimate overflow")?,
                open_files: 2,
                ..Default::default()
            };
            let worker = scope.spawn_blocking(
                ResourceRequest {
                    jobs: 1,
                    ..retained
                },
                retained,
                move |cancel| ZipContentSource::open(&source, limits, &cancel),
            )?;
            archive = Some(
                scope
                    .accept(worker.wait().await?)?
                    .transpose()?
                    .into_parts(),
            );
        }
        let mut downloads = Vec::new();
        let mut download_keys = Vec::new();
        for need in &self.needs {
            scope.cancellation().check()?;
            let expected = need.expected.clone();
            let maximum = expected.size.context("Imported size disappeared")?;
            let supplied = provided.get(&need.key).cloned();
            let embedded = match &need.source {
                ImportedAcquisition::Embedded(member) => Some(member.clone()),
                _ => None,
            };
            if supplied.is_some() || embedded.is_some() {
                let reader = archive.take();
                let retained = ResourceRequest {
                    scratch_bytes: maximum,
                    open_files: 3,
                    ..Default::default()
                };
                let worker = scope.spawn_blocking(
                    ResourceRequest {
                        jobs: 1,
                        memory_bytes: 64 << 10,
                        ..retained
                    },
                    retained,
                    move |cancel| -> Result<_> {
                        let mut reader = reader;
                        let (acquired, mode) = if let Some(supplied) = supplied {
                            (
                                verify_stream(
                                    &mut supplied.lease().open(),
                                    &expected,
                                    maximum,
                                    evidence,
                                    InitialObservation::RequireEvidence,
                                    &cancel,
                                )?,
                                None,
                            )
                        } else {
                            let (archive, _) =
                                reader.as_mut().context("Embedded archive was not opened")?;
                            let (acquired, mode) = archive.acquire(
                                &embedded.unwrap(),
                                &expected,
                                evidence,
                                InitialObservation::Accepted,
                                &cancel,
                            )?;
                            (acquired, Some(mode))
                        };
                        Ok((reader, acquired, mode))
                    },
                )?;
                let output = scope.accept(worker.wait().await?)?.transpose()?;
                let mut mode = None;
                let acquired = AcquiredContent::retain_resources(output.map(
                    |(reader, acquired, permissions)| {
                        archive = reader;
                        mode = permissions;
                        acquired
                    },
                ))?;
                if let Some(mode) = mode.or(need.permissions) {
                    permissions.insert(need.key.clone(), mode);
                }
                content.insert(
                    need.key.clone(),
                    pool.consolidate_owned(scope, acquired).await?,
                );
            } else if let ImportedAcquisition::Downloads(urls) = &need.source {
                download_keys.push(need.key.clone());
                downloads.push(DownloadRequest {
                    alternatives: NonEmpty::new(
                        urls.iter()
                            .filter(|url| transport.validate_locator(url).is_ok())
                            .cloned()
                            .collect(),
                    )?,
                    expected,
                    limits: self.limits.transfer,
                    evidence,
                    initial: InitialObservation::RequireEvidence,
                });
            }
        }
        drop(archive);
        let downloaded = if downloads.is_empty() {
            Vec::new()
        } else {
            transport
                .acquire_batch(
                    scope,
                    downloads,
                    budget.remaining_limits(self.limits.transfer)?,
                )
                .await?
        };
        for (key, acquired) in download_keys.into_iter().zip(downloaded) {
            content.insert(key, acquired);
        }
        scope.cancellation().check()?;
        ensure!(
            content.len() == self.needs.len(),
            "Import acquisition omitted a requested record"
        );
        Ok(ImportContentOutcome::Ready(VerifiedImportContent {
            evidence,
            plan: self,
            content,
            permissions,
        }))
    }
}
#[cfg(test)]
pub(in crate::engine) mod tests;
