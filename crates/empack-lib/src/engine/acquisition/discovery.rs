//! Read-only download discovery. Candidates are suggestions; acquisition verifies again.
use crate::{
    application::process_runtime::Cancellation,
    engine::{
        build::acquisition::AcquisitionKey,
        content::{
            InitialObservation, SourceEvidencePolicy, validate_expectation, verify_observation,
        },
        native,
        resources::ResourceRequest,
        runtime::{RetainedOutput, WorkScope},
        snapshot::ProjectReadRoot,
    },
};
use anyhow::{Context, Result, ensure};
use empack_core::{digest::ContentId, model::ExpectedContent};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{self, Read},
    path::PathBuf,
    time::{Duration, Instant},
};

#[derive(Clone, Copy)]
pub struct DiscoveryLimits {
    pub roots: usize,
    pub entries: usize,
    pub requirements: usize,
    pub file_bytes: u64,
    /// Includes bytes read from nonmatching and failed candidates.
    pub total_bytes: u64,
    pub deadline: Duration,
}
impl Default for DiscoveryLimits {
    fn default() -> Self {
        Self {
            roots: 8,
            entries: 10_000,
            requirements: 10_000,
            file_bytes: 8 << 30,
            total_bytes: 32 << 30,
            deadline: Duration::from_secs(300),
        }
    }
}
/// Host paths are transient suggestions, not project destinations or durable evidence.
pub struct DownloadCandidate {
    pub source: PathBuf,
    pub content: ContentId,
}
pub struct DownloadDiscovery {
    /// Different bytes matching weaker assertions remain ambiguous. Duplicate bytes collapse.
    pub matches: BTreeMap<AcquisitionKey, Vec<DownloadCandidate>>,
    pub inspected_files: usize,
    pub skipped_files: usize,
    pub bytes_read: u64,
}
impl DownloadDiscovery {
    pub fn unique_files(&self) -> BTreeMap<AcquisitionKey, PathBuf> {
        self.matches
            .iter()
            .filter(|(_, candidates)| candidates.len() == 1)
            .map(|(key, candidates)| (key.clone(), candidates[0].source.clone()))
            .collect()
    }
}

/// Scans only direct regular children of explicitly selected roots. No directory, file,
/// cache object or access record is created. Incomplete scans return no successful subset.
pub async fn discover_downloads(
    scope: &mut WorkScope,
    roots: Vec<PathBuf>,
    requirements: BTreeMap<AcquisitionKey, ExpectedContent>,
    evidence: SourceEvidencePolicy,
    limits: DiscoveryLimits,
) -> Result<RetainedOutput<DownloadDiscovery>> {
    ensure!(
        roots.len() <= limits.roots,
        "Download root count limit exceeded"
    );
    ensure!(
        requirements.len() <= limits.requirements,
        "Download requirement count limit exceeded"
    );
    ensure!(
        roots.iter().all(|root| root.is_absolute()),
        "Download roots must be absolute host paths"
    );
    let memory = (limits.entries as u64)
        .checked_mul(8192)
        .and_then(|bytes| bytes.checked_add((requirements.len() as u64).checked_mul(4096)?))
        .and_then(|bytes| bytes.checked_add((roots.len() as u64).checked_mul(8192)?))
        .context("Download discovery memory size overflow")?;
    let work = scope.spawn_blocking(
        ResourceRequest {
            jobs: 1,
            memory_bytes: memory
                .checked_add(256 << 10)
                .context("Download discovery memory size overflow")?,
            open_files: 4,
            ..Default::default()
        },
        ResourceRequest {
            memory_bytes: memory,
            ..Default::default()
        },
        move |cancel| scan(roots, requirements, evidence, limits, &cancel),
    )?;
    scope.accept(work.wait().await?)?.transpose()
}

fn scan(
    roots: Vec<PathBuf>,
    requirements: BTreeMap<AcquisitionKey, ExpectedContent>,
    evidence: SourceEvidencePolicy,
    limits: DiscoveryLimits,
    cancel: &Cancellation,
) -> Result<DownloadDiscovery> {
    let deadline = Instant::now()
        .checked_add(limits.deadline)
        .context("Download discovery deadline overflow")?;
    let check = || -> Result<()> {
        cancel.check()?;
        ensure!(
            Instant::now() < deadline,
            "Download discovery deadline exceeded"
        );
        Ok(())
    };
    check()?;
    let requirements: BTreeMap<_, _> = requirements
        .into_iter()
        .filter(|(_, expected)| {
            validate_expectation(
                expected,
                limits.file_bytes,
                evidence,
                InitialObservation::RequireEvidence,
            )
            .is_ok()
        })
        .collect();
    let mut matches: BTreeMap<AcquisitionKey, BTreeMap<ContentId, PathBuf>> = BTreeMap::new();
    let mut inspected_files = 0;
    let mut skipped_files = 0;
    let mut bytes_read = 0;
    let mut entries = 0usize;
    let mut associations = 0usize;
    let mut visited_roots = BTreeSet::new();
    let mut visited_files = BTreeSet::new();
    for selected in roots {
        check()?;
        if requirements.is_empty() {
            break;
        }
        let root =
            ProjectReadRoot::open(&selected).context("Cannot inspect selected download root")?;
        let binding = root.binding;
        if !visited_roots.insert((binding.volume, binding.object, binding.created)) {
            continue;
        }
        for entry in root.directory.entries()? {
            check()?;
            entries = entries
                .checked_add(1)
                .context("Download entry count overflow")?;
            ensure!(
                entries <= limits.entries,
                "Download discovery entry limit exceeded"
            );
            let Ok(entry) = entry else {
                skipped_files += 1;
                continue;
            };
            let name = entry.file_name();
            let source = selected.join(&name);
            ensure!(
                source.as_os_str().len() <= 4096,
                "Download candidate path is too long"
            );
            let Ok(mut file) = native::open_native_file(&root.directory, &name) else {
                skipped_files += 1;
                continue;
            };
            let Ok(metadata) = file.metadata() else {
                skipped_files += 1;
                continue;
            };
            let Ok(object) = native::identity(&file) else {
                skipped_files += 1;
                continue;
            };
            if !visited_files.insert((object.volume, object.object, object.created)) {
                continue;
            }
            if metadata.len() > limits.file_bytes
                || !requirements
                    .values()
                    .any(|expected| expected.size.is_none_or(|size| size == metadata.len()))
            {
                skipped_files += 1;
                continue;
            }
            let remaining = limits
                .total_bytes
                .checked_sub(bytes_read)
                .context("Download discovery byte limit exceeded")?;
            ensure!(
                metadata.len() <= remaining,
                "Download discovery byte limit exceeded"
            );
            let mut reader = CountedReader {
                file: &mut file,
                count: 0,
                deadline,
            };
            let result = verify_observation(
                &mut reader,
                &ExpectedContent {
                    digests: None,
                    size: Some(metadata.len()),
                    accepted_observation: None,
                },
                limits.file_bytes.min(remaining),
                SourceEvidencePolicy::Compatibility,
                InitialObservation::Accepted,
                cancel,
            );
            bytes_read = bytes_read
                .checked_add(reader.count)
                .context("Download discovery size overflow")?;
            check()?;
            ensure!(
                bytes_read <= limits.total_bytes,
                "Download discovery byte limit exceeded"
            );
            let Ok(observed) = result else {
                skipped_files += 1;
                continue;
            };
            inspected_files += 1;
            root.check_binding()?;
            let Ok(current) = native::open_native_file(&root.directory, &name) else {
                skipped_files += 1;
                continue;
            };
            if native::identity(&current).ok() != Some(object) {
                skipped_files += 1;
                continue;
            }
            for (key, expected) in &requirements {
                if !observed.matches(expected) {
                    continue;
                }
                let selected = matches.entry(key.clone()).or_default();
                let id = observed.address();
                if let Some(previous) = selected.get_mut(&id) {
                    if source < *previous {
                        *previous = source.clone();
                    }
                } else {
                    associations += 1;
                    ensure!(
                        associations <= limits.entries,
                        "Download association limit exceeded"
                    );
                    selected.insert(id, source.clone());
                }
            }
        }
        root.check_binding()?;
    }
    check()?;
    Ok(DownloadDiscovery {
        matches: matches
            .into_iter()
            .map(|(key, candidates)| {
                (
                    key,
                    candidates
                        .into_iter()
                        .map(|(content, source)| DownloadCandidate { source, content })
                        .collect(),
                )
            })
            .collect(),
        inspected_files,
        skipped_files,
        bytes_read,
    })
}
struct CountedReader<'a> {
    file: &'a mut std::fs::File,
    count: u64,
    deadline: Instant,
}
impl Read for CountedReader<'_> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        if Instant::now() >= self.deadline {
            return Err(io::ErrorKind::TimedOut.into());
        }
        let count = self.file.read(bytes)?;
        self.count = self
            .count
            .checked_add(count as u64)
            .ok_or_else(|| io::Error::other("Download discovery size overflow"))?;
        Ok(count)
    }
}

#[cfg(test)]
mod tests;
