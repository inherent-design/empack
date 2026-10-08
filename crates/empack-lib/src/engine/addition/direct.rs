//! Explicit direct-file selections acquire privately before one dependency publication.
use super::{AcquiredFileInput, AcquiredFileSource, FileAddition, FileEvidence};
use crate::engine::{
    acquisition::{
        DownloadRequest, HttpAcquisition, LocalFileRequest, TransferLimits, acquire_local_file,
    },
    archive_source::ZipContentSource,
    artifacts::ArchiveLimits,
    content::{InitialObservation, SourceEvidencePolicy, validate_expectation},
    documents::validate_download_url,
    mrpack::AcquiredBuildFile,
    resources::ResourceRequest,
    runtime::WorkScope,
};
use anyhow::{Context, Result, ensure};
use empack_core::{files::FilePermissions, model::*, requirements::Requirements};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

/// Download locations can expire; only the separately declared durable origins enter intent.
#[derive(Clone)]
pub enum DirectFileSource {
    Local(PathBuf),
    /// Observe installed bytes while retaining explicit URL ownership and origin assertions.
    ObservedUrl {
        path: PathBuf,
        origins: NonEmpty<String>,
    },
    Download {
        origins: NonEmpty<String>,
        alternatives: NonEmpty<String>,
    },
    /// Explicitly retain verified bytes as a local file, without persisting any locator.
    DownloadAsLocal {
        alternatives: NonEmpty<String>,
    },
}
#[derive(Clone, Copy, Default)]
pub enum FileKindPolicy {
    /// Archive layout must support the selected kind. This is not game compatibility proof.
    #[default]
    RequireRecognized,
    /// Explicitly keep an unidentified ZIP/JAR as the selected kind after structural checks.
    AcceptUnrecognized,
}
pub struct DirectFileInput {
    /// Stable member slot when this file belongs to a tracked local group.
    pub member: Option<empack_core::model::FileSlot>,
    pub key: DependencyKey,
    pub title: String,
    pub source: DirectFileSource,
    pub evidence: FileEvidence,
    pub kind: ContentKind,
    pub kind_policy: FileKindPolicy,
    pub requirements: Requirements,
    pub placements: NonEmpty<Placement>,
}
#[derive(Clone, Copy)]
pub struct DirectFileLimits {
    pub files: usize,
    pub transfer: TransferLimits,
    pub archive: ArchiveLimits,
}
impl Default for DirectFileLimits {
    fn default() -> Self {
        Self {
            files: 128,
            transfer: TransferLimits::default(),
            archive: ArchiveLimits::default(),
        }
    }
}
impl FileAddition {
    /// Resolve explicitly chosen local/URL representations. Provider identification is a separate
    /// choice: a failed catalog lookup must never silently become authorization for this path.
    /// All requested files must acquire and validate before a group can reach publication.
    pub async fn acquire(
        scope: &mut WorkScope,
        current: &ResolvedProject,
        inputs: NonEmpty<DirectFileInput>,
        transport: &HttpAcquisition,
        policy: SourceEvidencePolicy,
        limits: DirectFileLimits,
    ) -> Result<Self> {
        ensure!(
            inputs
                .as_slice()
                .iter()
                .map(|input| &input.key)
                .collect::<BTreeSet<_>>()
                .len()
                <= limits.files
                && inputs.as_slice().len() <= limits.archive.entries,
            "Direct-file count limit exceeded"
        );
        let mut keys = BTreeSet::new();
        let mut downloads = Vec::new();
        let mut download_keys = Vec::new();
        for input in inputs.as_slice() {
            scope.cancellation().check()?;
            ensure!(
                keys.insert((input.key.clone(), input.member.clone())),
                "Repeated direct-file logical key"
            );
            ensure!(
                input.kind != ContentKind::World
                    || input.member.is_some()
                    || matches!(
                        input.source,
                        DirectFileSource::Local(_) | DirectFileSource::DownloadAsLocal { .. }
                    ),
                "A remote world needs explicit download-as-local interpretation"
            );
            ensure!(
                input.member.is_none() || matches!(input.source, DirectFileSource::Local(_)),
                "Tracked members require local sources"
            );
            let (expected, initial) = expectation(&input.evidence);
            validate_expectation(&expected, limits.transfer.file_bytes, policy, initial)?;
            if let DirectFileSource::Download { origins, .. }
            | DirectFileSource::ObservedUrl { origins, .. } = &input.source
            {
                for origin in origins.as_slice() {
                    validate_download_url(origin)?;
                }
            }
            match &input.source {
                DirectFileSource::Local(path) | DirectFileSource::ObservedUrl { path, .. } => {
                    ensure!(path.is_absolute(), "Local source must be absolute")
                }
                DirectFileSource::Download { alternatives, .. }
                | DirectFileSource::DownloadAsLocal { alternatives } => {
                    download_keys.push(input.key.clone());
                    downloads.push(DownloadRequest {
                        alternatives: alternatives.clone(),
                        expected,
                        initial,
                        evidence: policy,
                        limits: limits.transfer,
                    });
                }
            }
        }
        let mut downloaded = BTreeMap::new();
        if !downloads.is_empty() {
            let files = transport
                .acquire_batch(scope, downloads, limits.transfer)
                .await?;
            downloaded.extend(download_keys.into_iter().zip(files));
        }
        let mut acquired = Vec::new();
        let mut worlds = Vec::new();
        let mut total = downloaded.values().try_fold(0u64, |total, file| {
            total
                .checked_add(file.lease().len())
                .context("Direct-file size overflow")
        })?;
        ensure!(
            total <= limits.transfer.transfer_bytes,
            "Direct-file batch exceeds byte limit"
        );
        for input in inputs.into_vec() {
            let (expected, initial) = expectation(&input.evidence);
            let local = matches!(
                input.source,
                DirectFileSource::Local(_) | DirectFileSource::ObservedUrl { .. }
            );
            let (source, file) = match input.source.clone() {
                DirectFileSource::Local(source) => (
                    AcquiredFileSource::Local,
                    acquire_local_file(
                        scope,
                        LocalFileRequest {
                            source,
                            expected,
                            maximum: limits
                                .transfer
                                .file_bytes
                                .min(limits.transfer.transfer_bytes.saturating_sub(total)),
                            evidence: policy,
                            initial,
                        },
                    )
                    .await?,
                ),
                DirectFileSource::ObservedUrl { path, origins } => (
                    AcquiredFileSource::ObservedUrl(origins),
                    acquire_local_file(
                        scope,
                        LocalFileRequest {
                            source: path,
                            expected,
                            maximum: limits
                                .transfer
                                .file_bytes
                                .min(limits.transfer.transfer_bytes.saturating_sub(total)),
                            evidence: policy,
                            initial,
                        },
                    )
                    .await?,
                ),
                source @ (DirectFileSource::Download { .. }
                | DirectFileSource::DownloadAsLocal { .. }) => (
                    match source {
                        DirectFileSource::Download { origins, .. } => {
                            AcquiredFileSource::Url(origins)
                        }
                        _ => AcquiredFileSource::DownloadedLocal,
                    },
                    AcquiredBuildFile {
                        content: downloaded
                            .remove(&input.key)
                            .context("Missing acquired direct file")?,
                        permissions: FilePermissions {
                            readonly: false,
                            executable: false,
                        },
                    },
                ),
            };
            if local {
                total = total
                    .checked_add(file.content.lease().len())
                    .context("Direct-file size overflow")?;
            }
            ensure!(
                total <= limits.transfer.transfer_bytes,
                "Direct-file batch exceeds byte limit"
            );
            if input.kind == ContentKind::World && input.member.is_none() {
                worlds
                    .push(super::world::expand(scope, input, file, limits.archive, policy).await?);
                continue;
            }
            if input.member.is_none() {
                validate_file_kind(scope, &file, input.kind, input.kind_policy, limits.archive)
                    .await?;
            }
            acquired.push(AcquiredFileInput {
                member: input.member,
                key: input.key,
                title: input.title,
                kind: input.kind,
                source,
                evidence: input.evidence,
                requirements: input.requirements,
                placements: input.placements,
                file,
            });
        }
        for world in &worlds {
            acquired.extend(world.iter().cloned());
        }
        Self::from_acquired(scope, current, NonEmpty::new(acquired)?, policy)
    }
}
fn expectation(evidence: &FileEvidence) -> (ExpectedContent, InitialObservation) {
    match evidence {
        FileEvidence::Declared(expected) => (expected.clone(), InitialObservation::RequireEvidence),
        FileEvidence::AcceptObserved => (
            ExpectedContent {
                digests: None,
                size: None,
                accepted_observation: None,
            },
            InitialObservation::Accepted,
        ),
    }
}

/// Verify a privately acquired file before interpreting it as the selected content kind.
/// Provider identification determines identity; it does not replace bounded archive checks.
pub async fn validate_file_kind(
    scope: &mut WorkScope,
    file: &AcquiredBuildFile,
    kind: ContentKind,
    acceptance: FileKindPolicy,
    archive: ArchiveLimits,
) -> Result<()> {
    if !matches!(kind, ContentKind::Config | ContentKind::OtherFile) {
        let source = file.content.clone();
        let worker = scope.spawn_blocking(ResourceRequest {
                    jobs: 1, open_files: 2,
                    memory_bytes: (archive.entries as u64).checked_mul(2048).and_then(|bytes| bytes.checked_add(128 << 10)).context("Archive estimate overflow")?,
                    ..Default::default()
                }, ResourceRequest::default(), move |cancel| {
                    let mut archive = ZipContentSource::open(&source, archive, &cancel)?;
                    archive.verify_members(&cancel)?;
                    let mut pack = false;
                    let mut data = false;
                    let mut assets = false;
                    let mut shaders = false;
                    let mut module = false;
                    for (path, _) in archive.files() {
                        cancel.check()?;
                        let name = path.as_str();
                        ensure!(!matches!(name, "modrinth.index.json" | "level.dat"),
                            "Selected archive requires project/world import interpretation");
                        pack |= name == "pack.mcmeta";
                        data |= name.starts_with("data/");
                        assets |= name.starts_with("assets/");
                        shaders |= name.starts_with("shaders/");
                        module |= matches!(name, "fabric.mod.json" | "quilt.mod.json" | "META-INF/mods.toml" | "META-INF/neoforge.mods.toml" | "mcmod.info");
                    }
                    let recognized = match kind {
                        ContentKind::Mod => module,
                        ContentKind::ResourcePack => pack && (assets || !data),
                        ContentKind::DataPack => pack && data,
                        ContentKind::ShaderPack => shaders,
                        _ => false,
                    };
                    ensure!(recognized || matches!(acceptance, FileKindPolicy::AcceptUnrecognized),
                        "Archive layout does not match the selected content kind; explicit acceptance is required");
                    Ok::<_, anyhow::Error>(())
                })?;
        scope.accept(worker.wait().await?)?.transpose()?;
    }
    Ok(())
}
