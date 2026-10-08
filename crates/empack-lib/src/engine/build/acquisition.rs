//! Read-only content obligations followed by optional acquisition; publication is a separate step.
use super::*;
use crate::engine::{
    acquisition::{DownloadRequest, HttpAcquisition, TransferLimits},
    backend::BackendDownload,
    content::{ContentPool, InitialObservation},
    runtime::WorkScope,
};
use empack_core::{
    digest::{DigestAlgorithm, DigestSet},
    files::FilePermissions,
    model::{ExpectedContent, NonEmpty, ResolvedFile, ResolvedPin},
    path::PortableRelPath,
};

mod cache;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum AcquisitionKey {
    Locked(LockedFileKey),
    Observed(PortableRelPath),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildMaterialization {
    ReferenceArchive,
    AllContent,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcquisitionReason {
    ReferenceEvidence,
    LayeredReplacement,
    MaterializedTarget,
    RetainedBackendContent,
}
/// Missing manual/provider/archive acquisition remains typed input, never a guessed filename.
/// This type is not Debug/serializable: transient observed download URLs may contain secrets.
#[derive(PartialEq, Eq)]
pub enum BuildContentSource {
    Download(NonEmpty<String>),
    Provider {
        pin: ResolvedPin,
        slot: empack_core::model::FileSlot,
        /// Original durable origins remain usable when no catalog capability is configured.
        alternatives: Vec<String>,
    },
    Embedded {
        archive: PortableRelPath,
        member: PortableRelPath,
    },
    Manual {
        pin: Option<ResolvedPin>,
    },
}
pub struct AcquisitionNeed {
    pub key: AcquisitionKey,
    pub reason: AcquisitionReason,
    pub expected: ExpectedContent,
    pub source: BuildContentSource,
}
pub struct BuildAcquisitionPlan {
    needs: Vec<AcquisitionNeed>,
}
/// Acquired leases survive a pending decision, but this result cannot publish project/artifact data.
pub struct BuildAcquisitionResult {
    pub acquired: BuildAcquisitions,
    pub pending: Vec<AcquisitionNeed>,
}
impl BuildAcquisitionPlan {
    /// One logical file needed by several targets is acquired once. Conflicting assertions or
    /// locators cannot silently inherit whichever target happened to be visited first.
    pub fn combine(plans: impl IntoIterator<Item = Self>) -> Result<Self> {
        let mut needs = BTreeMap::<AcquisitionKey, AcquisitionNeed>::new();
        for plan in plans {
            for need in plan.needs {
                if let Some(previous) = needs.get(&need.key) {
                    ensure!(
                        previous.expected == need.expected && previous.source == need.source,
                        "Build targets disagree about a shared acquisition"
                    );
                } else {
                    needs.insert(need.key.clone(), need);
                }
            }
        }
        Ok(Self {
            needs: needs.into_values().collect(),
        })
    }
    pub fn needs(&self) -> &[AcquisitionNeed] {
        &self.needs
    }
    /// Bind explicit supplied bytes to current obligations. Extra selections and changed
    /// source assertions fail before approval; accepted inputs retain their owned leases.
    pub fn supply(
        self,
        mut supplied: BuildAcquisitions,
        evidence: SourceEvidencePolicy,
        cancel: &Cancellation,
    ) -> Result<BuildAcquisitionResult> {
        let mut acquired = BuildAcquisitions::default();
        let mut pending = Vec::new();
        for need in self.needs {
            let input = match &need.key {
                AcquisitionKey::Locked(key) => supplied.locked.remove(key),
                AcquisitionKey::Observed(path) => supplied.observed.remove(path),
            };
            if let Some(file) = input {
                let observed = crate::engine::content::verify_observation(
                    &mut file.content.lease().open(),
                    &need.expected,
                    file.content.lease().len(),
                    evidence,
                    InitialObservation::RequireEvidence,
                    cancel,
                )?;
                ensure!(
                    observed.observed.values().contains(
                        &empack_core::digest::ExpectedDigest::Sha256(
                            *file.content.lease().id().bytes()
                        )
                    ),
                    "Supplied build content changed after acquisition"
                );
                insert_acquired(&mut acquired, need.key, file)?;
            } else {
                pending.push(need);
            }
        }
        ensure!(
            supplied.locked.is_empty() && supplied.observed.is_empty(),
            "Supplied content does not match a pending build obligation"
        );
        Ok(BuildAcquisitionResult { acquired, pending })
    }
    pub fn begin(self) -> BuildAcquisitionResult {
        BuildAcquisitionResult {
            acquired: BuildAcquisitions::default(),
            pending: self.needs,
        }
    }
    pub async fn acquire_http(
        self,
        transport: &HttpAcquisition,
        scope: &mut WorkScope,
        evidence: SourceEvidencePolicy,
        limits: TransferLimits,
    ) -> Result<BuildAcquisitionResult> {
        self.begin()
            .acquire_http(transport, scope, evidence, limits)
            .await
    }
}
impl BuildAcquisitionResult {
    /// Resolve exact locked selections to execution-only locators. This does not update pins,
    /// expected hashes, size, accepted observations, destinations or durable lock documents.
    pub async fn refresh_provider_locators(
        mut self,
        catalog: &crate::engine::providers::ProviderCatalog,
        scope: &mut WorkScope,
        limits: crate::engine::providers::CatalogLimits,
    ) -> Result<Self> {
        let mut groups = BTreeMap::<ResolvedPin, Vec<usize>>::new();
        for (index, need) in self.pending.iter().enumerate() {
            if let BuildContentSource::Provider { pin, .. } = &need.source
                && catalog.availability().supports(&pin.project)
            {
                groups.entry(pin.clone()).or_default().push(index);
            }
        }
        for (pin, indices) in groups {
            // All slots use one bounded response. Release its admission before the next pin.
            let resolution = catalog.resolve_exact(scope, pin.clone(), limits).await?;
            for index in indices {
                let need = &mut self.pending[index];
                let BuildContentSource::Provider {
                    slot,
                    alternatives: saved,
                    ..
                } = &need.source
                else {
                    unreachable!("grouped provider obligation")
                };
                let mut alternatives = resolution.download_alternatives(slot, &need.expected)?;
                // Prefer current locators without revoking declared fallback origins. Every
                // alternative still has to satisfy the exact original byte assertions.
                alternatives.extend(saved.iter().cloned());
                let mut seen = BTreeSet::new();
                alternatives.retain(|url| seen.insert(url.clone()));
                need.source = if alternatives.is_empty() {
                    BuildContentSource::Manual {
                        pin: Some(pin.clone()),
                    }
                } else {
                    BuildContentSource::Download(NonEmpty::new(alternatives)?)
                };
            }
        }
        Ok(self)
    }

    /// Use existing download evidence when exact lookup is unavailable to this host. This
    /// does not change the recorded identity or relax the original byte verification.
    pub(in crate::engine) fn use_saved_provider_alternatives(mut self) -> Self {
        for need in &mut self.pending {
            if let BuildContentSource::Provider { alternatives, .. } = &mut need.source
                && !alternatives.is_empty()
            {
                need.source = BuildContentSource::Download(
                    NonEmpty::new(std::mem::take(alternatives)).expect("nonempty alternatives"),
                );
            }
        }
        self
    }

    /// Process every captured archive once. Missing source archives remain pending for a new
    /// preparation; changed, malformed or mismatched members fail the whole unpublished batch.
    /// Run this synchronous phase in an admitted blocking worker.
    pub fn acquire_embedded(
        self,
        workspace: &WorkspaceSnapshot,
        limits: crate::engine::artifacts::ArchiveLimits,
        evidence: SourceEvidencePolicy,
        cancel: &Cancellation,
    ) -> Result<Self> {
        let Self {
            mut acquired,
            pending: needs,
        } = self;
        let mut pending = Vec::new();
        let archive_count = needs
            .iter()
            .filter_map(|need| match &need.source {
                BuildContentSource::Embedded { archive, .. } => Some(archive),
                _ => None,
            })
            .collect::<BTreeSet<_>>()
            .len() as u64;
        let mut pool = ContentPool::new(
            limits
                .total_bytes
                .checked_mul(archive_count)
                .context("Embedded content size overflow")?,
        )?;
        let mut archives = BTreeMap::new();
        for need in needs {
            cancel.check()?;
            let BuildContentSource::Embedded { archive, member } = &need.source else {
                pending.push(need);
                continue;
            };
            if !captured_file(workspace, archive)? {
                pending.push(need);
                continue;
            }
            if !archives.contains_key(archive) {
                let (source, _) = workspace.acquire_file(
                    archive,
                    None,
                    SourceEvidencePolicy::Compatibility,
                    cancel,
                )?;
                archives.insert(
                    archive.clone(),
                    crate::engine::archive_source::ZipContentSource::open(&source, limits, cancel)?,
                );
            }
            let (content, permissions) = archives.get_mut(archive).unwrap().acquire(
                member,
                &need.expected,
                evidence,
                InitialObservation::RequireEvidence,
                cancel,
            )?;
            let content = pool.insert(content, cancel)?;
            insert_acquired(
                &mut acquired,
                need.key,
                AcquiredBuildFile {
                    content,
                    permissions,
                },
            )?;
        }
        Ok(Self { acquired, pending })
    }
    /// Every download must verify. A failure drops this batch's private leases and returns no
    /// successful subset. Pending manual/embedded work remains explicit for the owning driver.
    pub async fn acquire_http(
        self,
        transport: &HttpAcquisition,
        scope: &mut WorkScope,
        evidence: SourceEvidencePolicy,
        limits: TransferLimits,
    ) -> Result<BuildAcquisitionResult> {
        let Self {
            mut acquired,
            pending: needs,
        } = self.use_saved_provider_alternatives();
        let limits = TransferLimits {
            transfer_bytes: limits
                .transfer_bytes
                .checked_sub(acquired.retained_bytes()?)
                .context("Retained build content exceeds byte limit")?,
            ..limits
        };
        let mut pending = Vec::new();
        let mut keys = Vec::new();
        let mut requests = Vec::new();
        for need in needs {
            match need.source {
                BuildContentSource::Download(alternatives) => {
                    keys.push(need.key);
                    requests.push(DownloadRequest {
                        alternatives,
                        expected: need.expected,
                        limits,
                        evidence,
                        initial: InitialObservation::RequireEvidence,
                    });
                }
                _ => pending.push(need),
            }
        }
        if !requests.is_empty() {
            let content = transport.acquire_batch(scope, requests, limits).await?;
            for (key, content) in keys.into_iter().zip(content) {
                insert_acquired(
                    &mut acquired,
                    key,
                    AcquiredBuildFile {
                        content,
                        permissions: FilePermissions {
                            readonly: false,
                            executable: false,
                        },
                    },
                )?;
            }
        }
        Ok(BuildAcquisitionResult { acquired, pending })
    }
}

fn insert_acquired(
    acquired: &mut BuildAcquisitions,
    key: AcquisitionKey,
    file: AcquiredBuildFile,
) -> Result<()> {
    let previous = match key {
        AcquisitionKey::Locked(key) => acquired.locked.insert(key, file),
        AcquisitionKey::Observed(path) => acquired.observed.insert(path, file),
    };
    ensure!(previous.is_none(), "Duplicate acquisition obligation");
    Ok(())
}

/// Enumerate obligations from the same captured workspace used for final verification. This
/// performs no network requests, creates no content objects and has no publication capability.
pub fn plan_build_acquisitions(
    workspace: &WorkspaceSnapshot,
    external: &BuildAcquisitions,
    mode: BuildMaterialization,
    cancel: &Cancellation,
) -> Result<BuildAcquisitionPlan> {
    plan_acquisitions(workspace, external, mode, None, cancel)
}
fn plan_acquisitions(
    workspace: &WorkspaceSnapshot,
    external: &BuildAcquisitions,
    mode: BuildMaterialization,
    selected: Option<&BTreeSet<AcquisitionKey>>,
    cancel: &Cancellation,
) -> Result<BuildAcquisitionPlan> {
    let project = workspace.require_resolved()?;
    let records = workspace.backend_files(cancel)?;
    let available = external
        .locked
        .iter()
        .map(|(key, value)| (key.clone(), value))
        .collect();
    let backend = check_backend(&project, &records, &available)?;
    let mut occupied = BTreeSet::new();
    let mut destinations = BTreeMap::<PortableRelPath, usize>::new();
    for dependency in project.lock().dependencies.values() {
        for file in dependency.files.as_slice() {
            match &file.acquisition {
                AcquisitionSpec::Local(path) | AcquisitionSpec::Embedded { archive: path, .. } => {
                    occupied.insert(path.clone());
                }
                _ => {}
            }
            for placement in file.placements.as_slice() {
                *destinations
                    .entry(placement.destination.relative().clone())
                    .or_default() += 1;
                occupied.insert(ProjectLayout::path(&ManagedPath::Content {
                    layer: placement.layer,
                    path: placement.destination.relative().clone(),
                })?);
            }
        }
    }
    for record in &records {
        if backend.unlisted.contains(&record.metadata_path) {
            *destinations
                .entry(record.destination.relative().clone())
                .or_default() += 1;
            occupied.insert(ProjectLayout::path(&ManagedPath::Content {
                layer: ContentLayer::Common,
                path: record.destination.relative().clone(),
            })?);
        }
    }
    for source in workspace.source_entries(cancel)? {
        if !occupied.contains(&source.path) {
            *destinations
                .entry(source.destination.relative().clone())
                .or_default() += 1;
        }
    }
    let mut needs = Vec::new();
    for (key, dependency) in &project.lock().dependencies {
        for file in dependency.files.as_slice() {
            cancel.check()?;
            let key = LockedFileKey {
                dependency: key.clone(),
                slot: file.slot.clone(),
            };
            if selected.is_some_and(|keys| !keys.contains(&AcquisitionKey::Locked(key.clone()))) {
                continue;
            }
            let mut present = external.locked.contains_key(&key);
            if let AcquisitionSpec::Local(path) = &file.acquisition {
                ensure!(
                    captured_file(workspace, path)?,
                    "Tracked local source is missing: {}",
                    path.as_str()
                );
                present = true;
            }
            for placement in file.placements.as_slice() {
                present |= captured_file(
                    workspace,
                    &ProjectLayout::path(&ManagedPath::Content {
                        layer: placement.layer,
                        path: placement.destination.relative().clone(),
                    })?,
                )?;
            }
            if present {
                continue;
            }
            let layered = file.placements.as_slice().iter().any(|placement| {
                destinations
                    .get(placement.destination.relative())
                    .is_some_and(|count| *count > 1)
            });
            let reason = if mode == BuildMaterialization::AllContent {
                AcquisitionReason::MaterializedTarget
            } else if layered {
                AcquisitionReason::LayeredReplacement
            } else if !reference_ready(file) {
                AcquisitionReason::ReferenceEvidence
            } else {
                continue;
            };
            let source = match &file.acquisition {
                AcquisitionSpec::Url(urls) => BuildContentSource::Download(urls.clone()),
                AcquisitionSpec::Provider {
                    alternatives,
                    pin,
                    slot,
                } => BuildContentSource::Provider {
                    pin: pin.clone(),
                    slot: slot.clone(),
                    alternatives: alternatives.clone(),
                },
                AcquisitionSpec::Embedded { archive, member } => BuildContentSource::Embedded {
                    archive: archive.clone(),
                    member: member.clone(),
                },
                AcquisitionSpec::Manual { pin, .. } => {
                    BuildContentSource::Manual { pin: pin.clone() }
                }
                AcquisitionSpec::Local(_) => unreachable!("local sources were checked above"),
            };
            needs.push(AcquisitionNeed {
                key: AcquisitionKey::Locked(key),
                reason,
                expected: file.expected.clone(),
                source,
            });
        }
    }
    for record in records {
        if selected.is_some_and(|keys| {
            !keys.contains(&AcquisitionKey::Observed(record.metadata_path.clone()))
        }) || !backend.unlisted.contains(&record.metadata_path)
            || external.observed.contains_key(&record.metadata_path)
        {
            continue;
        }
        if captured_file(
            workspace,
            &ProjectLayout::path(&ManagedPath::Content {
                layer: ContentLayer::Common,
                path: record.destination.relative().clone(),
            })?,
        )? {
            continue;
        }
        let source = match record.download {
            BackendDownload::Url(url) => BuildContentSource::Download(NonEmpty::new(vec![url])?),
            BackendDownload::CurseForgeMetadata => BuildContentSource::Manual {
                pin: record.provider.and_then(|provider| {
                    provider.selection.map(|selection| ResolvedPin {
                        project: provider.project,
                        selection,
                    })
                }),
            },
        };
        needs.push(AcquisitionNeed {
            key: AcquisitionKey::Observed(record.metadata_path),
            reason: AcquisitionReason::RetainedBackendContent,
            expected: ExpectedContent {
                digests: Some(DigestSet::new(vec![record.digest])?),
                size: None,
                accepted_observation: None,
            },
            source,
        });
    }
    Ok(BuildAcquisitionPlan { needs })
}
fn captured_file(workspace: &WorkspaceSnapshot, path: &PortableRelPath) -> Result<bool> {
    Ok(match workspace.observations().entries().get(path) {
        Some(Observation::File(_)) => true,
        Some(Observation::Directory { .. } | Observation::Ancestor(_)) => {
            anyhow::bail!("File obligation names a directory: {}", path.as_str())
        }
        _ => false,
    })
}
fn reference_ready(file: &ResolvedFile) -> bool {
    let urls = match &file.acquisition {
        AcquisitionSpec::Url(urls) => urls.as_slice(),
        AcquisitionSpec::Provider { alternatives, .. } => alternatives.as_slice(),
        _ => return false,
    };
    !urls.is_empty()
        && file.expected.size.is_some()
        && file.expected.digests.as_ref().is_some_and(|digests| {
            [DigestAlgorithm::Sha1, DigestAlgorithm::Sha512]
                .iter()
                .all(|algorithm| {
                    digests
                        .values()
                        .iter()
                        .any(|digest| digest.algorithm() == *algorithm)
                })
                && file
                    .expected
                    .accepted_observation
                    .as_ref()
                    .is_none_or(|observed| {
                        digests.values().iter().any(|digest| {
                            digest.algorithm() == DigestAlgorithm::Sha256
                                && digest.bytes() == observed.bytes()
                        })
                    })
        })
}

mod selection;
pub use selection::plan_target_build_acquisitions;
pub(super) use selection::select_game_inputs;

#[cfg(test)]
mod tests;
