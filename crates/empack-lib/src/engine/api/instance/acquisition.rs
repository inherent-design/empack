//! Exact release acquisition: refresh locators without changing identities or byte assertions.
use super::*;
use crate::engine::{
    acquisition::DownloadRequest,
    archive_source::ZipContentSource,
    content::{ContentPool, InitialObservation},
    release::{ReleaseFile, ReleaseSelection, ReleaseSource},
};
use empack_core::model::{FileSlot, ResolvedPin};
use std::collections::BTreeSet;

struct Need {
    files: Vec<ReleaseFile>,
    expected: ExpectedContent,
    alternatives: Vec<String>,
    selection: Option<ReleaseSelection>,
    archive: bool,
}
fn selection(file: &ReleaseFile) -> Option<ReleaseSelection> {
    match &file.source {
        ReleaseSource::Provider {
            provider,
            project,
            selection,
            slot,
            ..
        } => Some(ReleaseSelection {
            provider: provider.clone(),
            project: project.clone(),
            selection: selection.clone(),
            slot: slot.clone(),
        }),
        ReleaseSource::ProviderArchiveMember { archive, .. } => Some(archive.selection.clone()),
        ReleaseSource::Manual { selection, .. } => selection.clone(),
        _ => None,
    }
}
pub(super) fn available(file: &ReleaseFile, access: ProviderAvailability) -> Result<bool> {
    Ok(!locators(file).is_empty()
        || selection(file)
            .map(|s| s.pin().map(|pin| access.supports(&pin.project)))
            .transpose()?
            .unwrap_or(false))
}
fn locators(file: &ReleaseFile) -> &[String] {
    match &file.source {
        ReleaseSource::Url { alternatives } | ReleaseSource::Provider { alternatives, .. } => {
            alternatives
        }
        ReleaseSource::ProviderArchiveMember { archive, .. } => &archive.alternatives,
        _ => &[],
    }
}
fn needs(files: &[ReleaseFile]) -> Result<Vec<Need>> {
    let mut needs = Vec::<Need>::new();
    let mut archives = BTreeMap::<(ResolvedPin, String), usize>::new();
    for file in files {
        let selection = selection(file);
        let (expected, is_archive) = match &file.source {
            ReleaseSource::ProviderArchiveMember { archive, .. } => {
                let key = (archive.selection.pin()?, archive.selection.slot.clone());
                if let Some(index) = archives.get(&key) {
                    let first = &needs[*index].files[0];
                    ensure!(
                        matches!(&first.source, ReleaseSource::ProviderArchiveMember { archive: previous, .. } if previous == archive),
                        "Release members disagree about their source archive"
                    );
                    needs[*index].files.push(file.clone());
                    continue;
                }
                archives.insert(key, needs.len());
                (archive.expected()?, true)
            }
            _ => (file.expected()?, false),
        };
        needs.push(Need {
            files: vec![file.clone()],
            expected,
            alternatives: locators(file).to_vec(),
            selection,
            archive: is_archive,
        });
    }
    Ok(needs)
}
pub(super) async fn acquire(
    files: &[ReleaseFile],
    transport: &HttpAcquisition,
    catalog: Option<&(ProviderCatalog, CatalogLimits)>,
    config: &EngineConfig,
    scope: &mut WorkScope,
) -> Result<BTreeMap<String, AcquiredContent>> {
    let mut needs = needs(files)?;
    if let Some((catalog, limits)) = catalog {
        let mut groups = BTreeMap::<ResolvedPin, Vec<usize>>::new();
        for (index, need) in needs.iter().enumerate() {
            if let Some(selection) = &need.selection {
                let pin = selection.pin()?;
                if catalog.availability().supports(&pin.project) {
                    groups.entry(pin).or_default().push(index);
                }
            }
        }
        for (pin, indices) in groups {
            let resolution = catalog.resolve_exact(scope, pin, *limits).await?;
            for index in indices {
                let need = &mut needs[index];
                let mut alternatives = resolution.download_alternatives(
                    &FileSlot::parse(&need.selection.as_ref().unwrap().slot)?,
                    &need.expected,
                )?;
                alternatives.append(&mut need.alternatives);
                let mut seen = BTreeSet::new();
                alternatives.retain(|value| seen.insert(value.clone()));
                need.alternatives = alternatives;
            }
        }
    }
    let requests = needs
        .iter()
        .map(|need| {
            ensure!(
                !need.alternatives.is_empty(),
                "Instance requires manual content for {}; supply --file {}=PATH",
                need.files[0].key,
                need.files[0].key
            );
            Ok(DownloadRequest {
                alternatives: NonEmpty::new(need.alternatives.clone())?,
                expected: need.expected.clone(),
                limits: TransferLimits {
                    file_bytes: if need.archive {
                        config
                            .transfer
                            .file_bytes
                            .min(config.archive.compressed_bytes)
                    } else {
                        config.transfer.file_bytes
                    },
                    ..config.transfer
                },
                evidence: SourceEvidencePolicy::Compatibility,
                initial: InitialObservation::RequireEvidence,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let downloaded = transport
        .acquire_batch(scope, requests, config.transfer)
        .await?;
    let mut content = BTreeMap::new();
    let mut expanded = 0u64;
    let mut pool = ContentPool::owned(scope, config.transfer.transfer_bytes).await?;
    for (need, acquired) in needs.into_iter().zip(downloaded) {
        if !need.archive {
            expanded = expanded
                .checked_add(acquired.lease().len())
                .context("Instance acquisition size overflow")?;
            ensure!(
                expanded <= config.transfer.transfer_bytes,
                "Instance content exceeds byte limit"
            );
            content.insert(need.files[0].key.clone(), acquired);
            continue;
        }
        let metadata = (config.archive.entries as u64)
            .checked_mul(4096)
            .context("Archive metadata size overflow")?;
        let retained = ResourceRequest {
            memory_bytes: metadata,
            open_files: 2,
            ..Default::default()
        };
        let limits = config.archive;
        let work = scope.spawn_blocking(
            ResourceRequest {
                jobs: 1,
                ..retained
            },
            retained,
            move |cancel| ZipContentSource::open(&acquired, limits, &cancel),
        )?;
        let (mut reader, _reader_permit) =
            scope.accept(work.wait().await?)?.transpose()?.into_parts();
        for file in need.files {
            let ReleaseSource::ProviderArchiveMember { member, .. } = &file.source else {
                unreachable!()
            };
            expanded = expanded
                .checked_add(file.bytes)
                .context("Instance acquisition size overflow")?;
            ensure!(
                expanded <= config.transfer.transfer_bytes
                    && file.bytes <= config.transfer.file_bytes,
                "Instance content exceeds byte limit"
            );
            let member = PortableRelPath::parse(member, PathSyntax::ArchiveMember)?;
            let expected = file.expected()?;
            let retained = ResourceRequest {
                open_files: 1,
                scratch_bytes: file.bytes,
                ..Default::default()
            };
            let work = scope.spawn_blocking(
                ResourceRequest {
                    jobs: 1,
                    memory_bytes: 256 << 10,
                    open_files: 5,
                    ..retained
                },
                retained,
                move |cancel| {
                    let (content, _) = reader.acquire(
                        &member,
                        &expected,
                        SourceEvidencePolicy::Compatibility,
                        InitialObservation::RequireEvidence,
                        &cancel,
                    )?;
                    Ok::<_, anyhow::Error>((reader, content))
                },
            )?;
            let retained = scope.accept(work.wait().await?)?.transpose()?;
            let mut returned = None;
            let retained = retained.map(|(reader, content)| {
                returned = Some(reader);
                content
            });
            reader = returned.context("Archive reader was lost")?;
            let acquired = pool
                .consolidate_owned(scope, AcquiredContent::retain_resources(retained)?)
                .await?;
            content.insert(file.key, acquired);
        }
    }
    Ok(content)
}
