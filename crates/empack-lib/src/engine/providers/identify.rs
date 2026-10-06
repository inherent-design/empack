//! Content identity probes retain provider evidence; weak fingerprints only nominate candidates.
use super::*;
use crate::{application::process_runtime::Cancellation, engine::content::AcquiredContent};
use empack_core::{
    digest::{ContentId, DigestAlgorithm, DigestSet},
    identity::{CurseForgeProjectId, ModrinthProjectId},
    model::ProviderKind,
};
use serde::Deserialize;
use serde_json::Value;
use std::{
    collections::BTreeSet,
    io::{Read, Seek},
    time::Instant,
};

pub struct IdentifiedSelection {
    pub content: ContentId,
    pub resolution: ProviderResolution,
    /// All roles with matching bytes, not a guessed basename or automatically selected primary.
    pub matching_files: NonEmpty<String>,
}
pub enum Identification {
    Unknown,
    Exact(Box<RetainedOutput<IdentifiedSelection>>),
    Ambiguous(NonEmpty<RetainedOutput<IdentifiedSelection>>),
}
#[derive(Clone, Copy)]
pub struct IdentificationLimits {
    pub catalog: CatalogLimits,
    pub file_bytes: u64,
    pub matches: usize,
}
impl Default for IdentificationLimits {
    fn default() -> Self {
        Self {
            catalog: CatalogLimits::default(),
            file_bytes: 2 << 30,
            matches: 32,
        }
    }
}
#[derive(Clone)]
struct Probe {
    content: ContentId,
    size: u64,
    digests: DigestSet,
    fingerprint: Option<u32>,
}
struct Record {
    pin: ResolvedPin,
    bytes: Vec<u8>,
}
enum Request {
    Hash(String),
    Fingerprint(u32),
    Project(ProviderProjectId),
}
impl ProviderCatalog {
    /// Query exactly the requested providers. Not-found is an outcome; auth, throttling,
    /// incomplete indexes and transport failures remain errors, never "unidentified local file".
    pub async fn identify_file(
        &self,
        scope: &mut WorkScope,
        content: AcquiredContent,
        providers: NonEmpty<ProviderKind>,
        limits: IdentificationLimits,
    ) -> Result<Identification> {
        ensure!(
            limits.matches > 0 && content.lease().len() <= limits.file_bytes,
            CatalogError::Limit
        );
        let needs_fingerprint = providers.as_slice().contains(&ProviderKind::CurseForge);
        ensure!(
            !needs_fingerprint || self.transport.has_curseforge_key(),
            CatalogError::Unauthorized
        );
        let mut budget = transport::RequestBudget::new(limits.catalog)?;
        let deadline = Instant::now()
            .checked_add(limits.catalog.deadline)
            .ok_or(CatalogError::Deadline)?;
        let worker = scope.spawn_blocking(
            ResourceRequest {
                jobs: 1,
                memory_bytes: 65536,
                open_files: 1,
                ..Default::default()
            },
            ResourceRequest {
                memory_bytes: 4096,
                ..Default::default()
            },
            move |cancel| {
                let fingerprint = if needs_fingerprint {
                    Some(fingerprint(&mut content.lease().open(), deadline, &cancel)?)
                } else {
                    None
                };
                cancel.check()?;
                Ok::<_, anyhow::Error>(Probe {
                    content: content.lease().id(),
                    size: content.lease().len(),
                    digests: content.observed_digests().clone(),
                    fingerprint,
                })
            },
        )?;
        let probe = scope.accept(worker.wait().await?)?.transpose()?;
        let mut queried = BTreeSet::new();
        let mut seen = BTreeSet::new();
        let mut matches = Vec::new();
        let mut count = 0usize;
        for provider in providers.as_slice() {
            if !queried.insert(*provider) {
                continue;
            }
            let request = match provider {
                ProviderKind::Modrinth => Request::Hash(
                    probe
                        .digests
                        .values()
                        .iter()
                        .find(|d| d.algorithm() == DigestAlgorithm::Sha512)
                        .ok_or(CatalogError::InvalidRecord)?
                        .hex(),
                ),
                ProviderKind::CurseForge => {
                    Request::Fingerprint(probe.fingerprint.ok_or(CatalogError::InvalidRecord)?)
                }
            };
            let raw = fetch(&self.transport, scope, request, budget, limits.catalog).await?;
            let retained = parse_resources(raw.0.as_ref().map_or(0, |bytes| bytes.len() as u64))?;
            let provider = *provider;
            let fingerprint = probe.fingerprint;
            let remaining = limits.matches.saturating_sub(count);
            let worker = scope.spawn_blocking(
                ResourceRequest {
                    jobs: 1,
                    ..retained
                },
                retained,
                move |cancel| {
                    cancel.check()?;
                    let ((bytes, budget), _permit) = raw.into_parts();
                    let records = match bytes {
                        Some(bytes) => parse_lookup(provider, &bytes, fingerprint, remaining)?,
                        None => Vec::new(),
                    };
                    Ok::<_, anyhow::Error>((records, budget))
                },
            )?;
            let parsed = scope.accept(worker.wait().await?)?.transpose()?;
            let ((records, next_budget), _records_permit) = parsed.into_parts();
            budget = next_budget;
            count = count
                .checked_add(records.len())
                .ok_or(CatalogError::Limit)?;
            for record in records {
                ensure!(seen.insert(record.pin.clone()), CatalogError::InvalidRecord);
                let raw = fetch(
                    &self.transport,
                    scope,
                    Request::Project(record.pin.project.clone()),
                    budget,
                    limits.catalog,
                )
                .await?;
                let retained = parse_resources(
                    (record.bytes.len() as u64)
                        .checked_add(raw.0.as_ref().map_or(0, |v| v.len() as u64))
                        .ok_or(CatalogError::Limit)?,
                )?;
                let probe = (*probe).clone();
                let worker = scope.spawn_blocking(
                    ResourceRequest {
                        jobs: 1,
                        ..retained
                    },
                    retained,
                    move |cancel| {
                        cancel.check()?;
                        let ((bytes, budget), _permit) = raw.into_parts();
                        let bytes = bytes.ok_or(CatalogError::InvalidRecord)?;
                        let project = decode_project(
                            &ProjectSelector::canonical(record.pin.project.clone()),
                            &bytes,
                        )?;
                        let resolution = match record.pin.project {
                            ProviderProjectId::Modrinth(_) => {
                                modrinth::selection(project, &record.pin, &record.bytes)?
                            }
                            ProviderProjectId::CurseForge(_) => {
                                curseforge::selection(project, &record.pin, &record.bytes)?
                            }
                        };
                        let names: Vec<_> = resolution
                            .files
                            .as_slice()
                            .iter()
                            .filter(|file| {
                                file.expected.size == Some(probe.size)
                                    && file.expected.digests.as_ref().is_some_and(|expected| {
                                        expected.check(probe.digests.values()).is_ok()
                                    })
                            })
                            .map(|file| file.filename.clone())
                            .collect();
                        // A fingerprint collision is a negative match. A cryptographic hash endpoint
                        // returning unrelated bytes violates its provider contract and must fail.
                        ensure!(
                            !names.is_empty() || provider == ProviderKind::CurseForge,
                            CatalogError::Identity
                        );
                        let selection = if names.is_empty() {
                            None
                        } else {
                            Some(IdentifiedSelection {
                                content: probe.content,
                                resolution,
                                matching_files: NonEmpty::new(names)?,
                            })
                        };
                        cancel.check()?;
                        Ok::<_, anyhow::Error>((selection, budget))
                    },
                )?;
                let parsed = scope.accept(worker.wait().await?)?.transpose()?;
                let mut next_budget = None;
                let selected = parsed.map(|(selected, value)| {
                    next_budget = Some(value);
                    selected
                });
                budget = next_budget.expect("parsed identity returns request budget");
                if selected.is_some() {
                    matches.push(selected.map(|value| value.expect("checked selection")));
                }
            }
        }
        scope.cancellation().check()?;
        budget.check_deadline()?;
        Ok(match matches.len() {
            0 => Identification::Unknown,
            1 => Identification::Exact(Box::new(matches.remove(0))),
            _ => Identification::Ambiguous(NonEmpty::new(matches)?),
        })
    }
}
async fn fetch(
    transport: &transport::CatalogTransport,
    scope: &mut WorkScope,
    request: Request,
    mut budget: transport::RequestBudget,
    limits: CatalogLimits,
) -> Result<RetainedOutput<(Option<Vec<u8>>, transport::RequestBudget)>> {
    let transport = transport.clone();
    let retained = ResourceRequest {
        memory_bytes: limits.response_bytes,
        ..Default::default()
    };
    let worker = scope.spawn(
        ResourceRequest {
            jobs: 1,
            open_files: 1,
            ..retained
        },
        retained,
        move |cancel| async move {
            let result = match request {
                Request::Hash(hash) => {
                    match transport
                        .get(
                            ProviderKind::Modrinth,
                            &["version_file", &hash],
                            &[("algorithm", "sha512")],
                            &mut budget,
                            &cancel,
                        )
                        .await
                    {
                        Err(error)
                            if matches!(
                                error.downcast_ref::<CatalogError>(),
                                Some(CatalogError::NotFound)
                            ) =>
                        {
                            None
                        }
                        value => Some(value?),
                    }
                }
                Request::Fingerprint(value) => {
                    Some(transport.fingerprint(value, &mut budget, &cancel).await?)
                }
                Request::Project(project) => Some(
                    transport
                        .project(&ProjectSelector::canonical(project), &mut budget, &cancel)
                        .await?,
                ),
            };
            Ok::<_, anyhow::Error>((result, budget))
        },
    )?;
    scope.accept(worker.wait().await?)?.transpose()
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Fingerprints {
    is_cache_built: bool,
    exact_matches: Vec<Match>,
}
#[derive(Deserialize)]
struct Envelope {
    data: Fingerprints,
}
#[derive(Deserialize)]
struct Match {
    id: u64,
    file: Value,
}
fn parse_lookup(
    provider: ProviderKind,
    bytes: &[u8],
    fingerprint: Option<u32>,
    maximum: usize,
) -> Result<Vec<Record>> {
    match provider {
        ProviderKind::Modrinth => {
            ensure!(maximum >= 1, CatalogError::Limit);
            let value: Value = json(bytes)?;
            let project = ProviderProjectId::Modrinth(ModrinthProjectId::parse(
                value
                    .get("project_id")
                    .and_then(Value::as_str)
                    .ok_or(CatalogError::InvalidRecord)?,
            )?);
            let pin = ResolvedPin {
                selection: project.parse_pin(
                    value
                        .get("id")
                        .and_then(Value::as_str)
                        .ok_or(CatalogError::InvalidRecord)?,
                )?,
                project,
            };
            Ok(vec![Record {
                pin,
                bytes: bytes.to_vec(),
            }])
        }
        ProviderKind::CurseForge => {
            let value: Envelope = json(bytes)?;
            ensure!(value.data.is_cache_built, CatalogError::IncompleteLookup);
            ensure!(
                value.data.exact_matches.len() <= maximum,
                CatalogError::Limit
            );
            value
                .data
                .exact_matches
                .into_iter()
                .map(|matched| {
                    ensure!(
                        matched.file.get("modId").and_then(Value::as_u64) == Some(matched.id)
                            && matched.file.get("gameId").and_then(Value::as_u64) == Some(432)
                            && matched.file.get("fileFingerprint").and_then(Value::as_u64)
                                == fingerprint.map(u64::from),
                        CatalogError::Identity
                    );
                    let project = ProviderProjectId::CurseForge(CurseForgeProjectId::parse(
                        &matched.id.to_string(),
                    )?);
                    let id = matched
                        .file
                        .get("id")
                        .and_then(Value::as_u64)
                        .ok_or(CatalogError::InvalidRecord)?;
                    let pin = ResolvedPin {
                        selection: project.parse_pin(&id.to_string())?,
                        project,
                    };
                    Ok(Record {
                        pin,
                        bytes: serde_json::to_vec(&serde_json::json!({"data":matched.file}))?,
                    })
                })
                .collect()
        }
    }
}
/// CurseForge's whitespace-normalized Murmur2. Two bounded passes avoid buffering the file.
fn fingerprint(
    reader: &mut (impl Read + Seek),
    deadline: Instant,
    cancel: &Cancellation,
) -> Result<u32> {
    let check = || -> Result<()> {
        cancel.check()?;
        ensure!(Instant::now() < deadline, CatalogError::Deadline);
        Ok(())
    };
    let keep = |byte: u8| !matches!(byte, 9 | 10 | 13 | 32);
    let mut buffer = [0u8; 32768];
    let mut size = 0u32;
    reader.rewind()?;
    loop {
        check()?;
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        size = size
            .checked_add(buffer[..count].iter().filter(|b| keep(**b)).count() as u32)
            .ok_or(CatalogError::Limit)?;
    }
    reader.rewind()?;
    let mut hash = 1 ^ size;
    let mut word = [0u8; 4];
    let mut filled = 0;
    const MULTIPLIER: u32 = 0x5bd1e995;
    loop {
        check()?;
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        for byte in buffer[..count].iter().copied().filter(|byte| keep(*byte)) {
            word[filled] = byte;
            filled += 1;
            if filled == 4 {
                let mut value = u32::from_le_bytes(word).wrapping_mul(MULTIPLIER);
                value ^= value >> 24;
                value = value.wrapping_mul(MULTIPLIER);
                hash = hash.wrapping_mul(MULTIPLIER) ^ value;
                filled = 0;
                word = [0; 4];
            }
        }
    }
    if filled > 0 {
        hash ^= u32::from_le_bytes(word);
        hash = hash.wrapping_mul(MULTIPLIER);
    }
    hash ^= hash >> 13;
    hash = hash.wrapping_mul(MULTIPLIER);
    hash ^= hash >> 15;
    Ok(hash)
}
#[cfg(test)]
mod tests;
