//! Turn exact provider evidence into explicit reference, verified byte or pending obligations.
use super::*;
use crate::engine::{
    acquisition::{DownloadRequest, HttpAcquisition, TransferLimits},
    content::{InitialObservation, SourceEvidencePolicy, validate_expectation},
    dependency_content::{DependencyContent, DependencyContents, validate_reference},
    mrpack::{AcquiredBuildFile, LockedFileKey},
};
use empack_core::files::FilePermissions;

#[derive(Clone)]
pub enum ProviderContentChoice {
    Reference,
    Acquire,
    Supplied(AcquiredBuildFile),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderInputReason {
    RestrictedDownload,
    UnsupportedTransport,
    SuppliedContent,
}
pub struct ProviderContentInput {
    pub expected: ExpectedContent,
    pub reason: ProviderInputReason,
}
/// No successful subset can become a complete addition. Native preparation independently
/// verifies complete slot coverage and all original assertions before publication.
pub struct ProviderContent {
    content: DependencyContents,
    pending: BTreeMap<LockedFileKey, ProviderContentInput>,
    deferred_downloads: BTreeSet<LockedFileKey>,
    _index: AdmissionPermit,
}
impl ProviderContent {
    /// Includes verified bytes and explicitly requested references, never guessed pending slots.
    pub fn content(&self) -> &DependencyContents {
        &self.content
    }
    pub fn pending(&self) -> &BTreeMap<LockedFileKey, ProviderContentInput> {
        &self.pending
    }
    /// Automatic transfers deliberately not started while another slot needs user input.
    pub fn deferred_downloads(&self) -> &BTreeSet<LockedFileKey> {
        &self.deferred_downloads
    }
    pub fn complete(&self) -> bool {
        self.pending.is_empty() && self.deferred_downloads.is_empty()
    }
}
impl ProviderAddition {
    /// Validate every slot decision before downloading. Exact catalog evidence is reused;
    /// locators stay transient and every HTTP file shares one byte/deadline allowance.
    /// Missing restricted/local/archive bytes remain explicit input with original assertions.
    pub async fn acquire_content(
        &self,
        scope: &mut WorkScope,
        transport: &HttpAcquisition,
        mut choices: BTreeMap<LockedFileKey, ProviderContentChoice>,
        policy: SourceEvidencePolicy,
        limits: TransferLimits,
    ) -> Result<ProviderContent> {
        let mut content = BTreeMap::new();
        let mut pending = BTreeMap::new();
        let mut keys = Vec::new();
        let mut requests = Vec::new();
        let mut bytes = 0u64;
        // Count borrowed evidence before allocating the execution-only locator copies.
        for selected in self.evidence.selections.values() {
            for file in selected.resolution.files.as_slice() {
                for url in &file.alternatives {
                    bytes = bytes
                        .checked_add(url.len() as u64)
                        .context("Locator size overflow")?;
                }
            }
        }
        let index = scope.reserve_storage(ResourceRequest {
            memory_bytes: bytes
                .checked_add(
                    (choices.len() as u64)
                        .checked_mul(4096)
                        .context("Content inventory size overflow")?,
                )
                .context("Content inventory size overflow")?,
            ..Default::default()
        })?;
        for (dependency, record) in &self.project.lock().dependencies {
            for file in record.files.as_slice() {
                scope.cancellation().check()?;
                let key = LockedFileKey {
                    dependency: dependency.clone(),
                    slot: file.slot.clone(),
                };
                let choice = choices
                    .remove(&key)
                    .context("Missing provider content decision")?;
                match choice {
                    ProviderContentChoice::Reference => {
                        validate_reference(file)?;
                        content.insert(key, DependencyContent::Reference);
                    }
                    ProviderContentChoice::Supplied(supplied) => {
                        validate_expectation(
                            &file.expected,
                            limits.file_bytes,
                            policy,
                            InitialObservation::RequireEvidence,
                        )?;
                        if let Some(digests) = &file.expected.digests {
                            digests.check(supplied.content.observed_digests().values())?;
                        }
                        ensure!(
                            file.expected
                                .size
                                .is_none_or(|size| size == supplied.content.lease().len())
                                && supplied.content.lease().len() <= limits.file_bytes
                                && file
                                    .expected
                                    .accepted_observation
                                    .as_ref()
                                    .is_none_or(|id| *id == supplied.content.lease().id()),
                            "Supplied content differs from the selected file"
                        );
                        content.insert(key, DependencyContent::Materialized(supplied));
                    }
                    ProviderContentChoice::Acquire => {
                        validate_expectation(
                            &file.expected,
                            limits.file_bytes,
                            policy,
                            InitialObservation::RequireEvidence,
                        )?;
                        let (alternatives, missing) = match &file.acquisition {
                            AcquisitionSpec::Provider { pin, slot, .. } => {
                                let selected = self
                                    .evidence
                                    .selections
                                    .get(&pin.project)
                                    .context("Provider selection evidence is missing")?;
                                ensure!(selected.resolution.pin == *pin, CatalogError::Identity);
                                (
                                    selected
                                        .resolution
                                        .download_alternatives(slot, &file.expected)?,
                                    ProviderInputReason::RestrictedDownload,
                                )
                            }
                            AcquisitionSpec::Url(urls) => (
                                urls.as_slice().to_vec(),
                                ProviderInputReason::SuppliedContent,
                            ),
                            _ => (vec![], ProviderInputReason::SuppliedContent),
                        };
                        let reason = if alternatives.is_empty() {
                            Some(missing)
                        } else if alternatives
                            .iter()
                            .any(|url| transport.validate_locator(url).is_err())
                        {
                            Some(ProviderInputReason::UnsupportedTransport)
                        } else {
                            None
                        };
                        if let Some(reason) = reason {
                            pending.insert(
                                key,
                                ProviderContentInput {
                                    expected: file.expected.clone(),
                                    reason,
                                },
                            );
                        } else {
                            keys.push(key);
                            requests.push(DownloadRequest {
                                alternatives: NonEmpty::new(alternatives)?,
                                expected: file.expected.clone(),
                                limits,
                                evidence: policy,
                                initial: InitialObservation::RequireEvidence,
                            });
                        }
                    }
                }
            }
        }
        ensure!(choices.is_empty(), "Unexpected provider content decision");
        if !pending.is_empty() {
            return Ok(ProviderContent {
                content,
                pending,
                deferred_downloads: keys.into_iter().collect(),
                _index: index,
            });
        }
        if !requests.is_empty() {
            let acquired = transport.acquire_batch(scope, requests, limits).await?;
            for (key, acquired) in keys.into_iter().zip(acquired) {
                content.insert(
                    key,
                    DependencyContent::Materialized(AcquiredBuildFile {
                        content: acquired,
                        permissions: FilePermissions {
                            readonly: false,
                            executable: false,
                        },
                    }),
                );
            }
        }
        scope.cancellation().check()?;
        Ok(ProviderContent {
            content,
            pending,
            deferred_downloads: BTreeSet::new(),
            _index: index,
        })
    }
}
