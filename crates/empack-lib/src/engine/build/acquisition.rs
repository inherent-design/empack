//! Read-only content obligations followed by optional acquisition; publication is a separate step.
use super::*;
use crate::engine::{
    acquisition::{DownloadRequest, HttpAcquisition, TransferLimits},
    backend::BackendDownload,
    content::InitialObservation,
    runtime::WorkScope,
};
use empack_core::{
    digest::{DigestAlgorithm, DigestSet},
    files::FilePermissions,
    model::{ExpectedContent, NonEmpty, ResolvedFile, ResolvedPin},
    path::PortableRelPath,
};

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
pub enum BuildContentSource {
    Download(NonEmpty<String>),
    Provider {
        pin: ResolvedPin,
        slot: empack_core::model::FileSlot,
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
    pub fn needs(&self) -> &[AcquisitionNeed] {
        &self.needs
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
        let mut acquired = BuildAcquisitions::default();
        let mut pending = Vec::new();
        for need in self.needs {
            match need.source {
                BuildContentSource::Download(alternatives) => {
                    let content = transport
                        .acquire(
                            scope,
                            DownloadRequest {
                                alternatives,
                                expected: need.expected,
                                limits,
                                evidence,
                                initial: InitialObservation::RequireEvidence,
                            },
                        )
                        .await?;
                    let file = AcquiredBuildFile {
                        content,
                        permissions: FilePermissions {
                            readonly: false,
                            executable: false,
                        },
                    };
                    match need.key {
                        AcquisitionKey::Locked(key) => {
                            acquired.locked.insert(key, file);
                        }
                        AcquisitionKey::Observed(path) => {
                            acquired.observed.insert(path, file);
                        }
                    }
                }
                _ => pending.push(need),
            }
        }
        Ok(BuildAcquisitionResult { acquired, pending })
    }
}

/// Enumerate obligations from the same captured workspace used for final verification. This
/// performs no network requests, creates no content objects and has no publication capability.
pub fn plan_build_acquisitions(
    workspace: &WorkspaceSnapshot,
    external: &BuildAcquisitions,
    mode: BuildMaterialization,
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
            if let AcquisitionSpec::Local(path) = &file.acquisition {
                occupied.insert(path.clone());
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
                } => match NonEmpty::new(alternatives.clone()) {
                    Ok(urls) => BuildContentSource::Download(urls),
                    Err(_) => BuildContentSource::Provider {
                        pin: pin.clone(),
                        slot: slot.clone(),
                    },
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
        if !backend.unlisted.contains(&record.metadata_path)
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

#[cfg(test)]
mod tests;
