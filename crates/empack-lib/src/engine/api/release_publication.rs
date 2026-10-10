//! Immutable publisher output. Signing never grants instance or executable authority.
use super::*;
use crate::engine::{
    content::{InitialObservation, SourceEvidencePolicy, verify_stream},
    layout::{CollisionIndex, ProjectLayout},
    native,
    publication::Publisher,
    release::{
        self, DecodedRelease, ReleaseFile, ReleaseSource,
        trust::{self, EnvelopeKind},
    },
    runtime::WorkScope,
    snapshot::{NativeSnapshot, Observation, ProjectReadRoot},
    staging::MutableStage,
    verification::{self, VerifiedFileChange},
};
use ed25519_dalek::SigningKey;
use empack_core::{
    digest::ContentId,
    files::{FileContent, FilePermissions, FilePlan, ManagedPath, ObservedPath},
    path::{PathSyntax, PortableRelPath},
};
use std::collections::BTreeSet;

/// Selected local export and independently supplied keys. Keys never enter the receipt or archive.
pub struct StageReleaseRequest {
    pub source: PathBuf,
    pub keys: Vec<SigningKey>,
}
#[derive(Clone)]
pub struct ReleasePublicationPreview {
    pub plan: PlanId,
    pub release: String,
    pub pack: String,
    pub envelope: String,
    pub keys: Vec<String>,
    pub files: FilePlan,
    pub replacement: ReplacementSummary,
}
pub struct ReleasePublicationReceipt {
    pub plan: PlanId,
    pub release: String,
    pub envelope: String,
    pub publication: PublicationReceipt,
}
pub(super) struct PreparedReleasePublication {
    pub(super) view: ReleasePublicationPreview,
    root: ProjectReadRoot,
    source: ProjectReadRoot,
    source_snapshot: NativeSnapshot,
    change: VerifiedFileChange,
}
struct Captured {
    root: ProjectReadRoot,
    source: ProjectReadRoot,
    source_snapshot: NativeSnapshot,
    release: DecodedRelease,
    envelope: Vec<u8>,
    keys: Vec<String>,
    assets: BTreeMap<PortableRelPath, Vec<ReleaseFile>>,
    bytes: u64,
    largest: u64,
}
fn path(value: &str) -> Result<PortableRelPath> {
    Ok(PortableRelPath::parse(value, PathSyntax::ProjectContent)?)
}
fn artifact(id: &str, suffix: &str) -> Result<ManagedPath> {
    Ok(ManagedPath::Artifact(path(&format!(
        "releases/{id}/{suffix}"
    ))?))
}
fn desired(digest: &str, bytes: u64) -> Result<FileContent> {
    Ok(FileContent {
        content: ContentId::from_sha256(release::decode_hex(digest)?),
        bytes,
        permissions: FilePermissions {
            readonly: false,
            executable: false,
        },
    })
}
pub(super) async fn prepare(
    target: ProjectTarget,
    request: StageReleaseRequest,
    config: &EngineConfig,
    scope: &mut WorkScope,
) -> Result<RetainedOutput<PreparedReleasePublication>> {
    let ProjectTarget::Existing(target) = target else {
        anyhow::bail!("Release publication requires an existing output root")
    };
    ensure!(
        target.is_absolute() && request.source.is_absolute(),
        "Release source and output roots must be absolute"
    );
    let limits = config.snapshot;
    let state = config.state_root.clone();
    let work = scope.spawn_blocking(
        config.resources.capture,
        config.resources.prepared,
        move |cancel| {
            let root = ProjectReadRoot::open(&target)?;
            let _guard = RecoveryReader::new(state).enter(&root)?;
            let source = ProjectReadRoot::open(&request.source)?;
            let initial = source.capture(&[path("release.json")?], limits, &cancel)?;
            let bytes = crate::engine::project::read_document_limited(
                &source,
                &initial,
                "release.json",
                release::MAX_RELEASE_BYTES as u64,
                &cancel,
            )?
            .context("Export has no release.json")?;
            let release = DecodedRelease::decode(&bytes)?;
            let mut assets = BTreeMap::<PortableRelPath, Vec<ReleaseFile>>::new();
            for file in &release.document().files {
                let selected = file.asset.as_deref().or(match &file.source {
                    ReleaseSource::Asset { path } => Some(path.as_str()),
                    _ => None,
                });
                if let Some(selected) = selected {
                    assets
                        .entry(path(selected)?)
                        .or_default()
                        .push(file.clone());
                }
            }
            let mut collisions = CollisionIndex::default();
            collisions.insert_file(&path("release.json")?)?;
            for asset in assets.keys() {
                collisions.insert_file(asset)?;
            }
            let paths = std::iter::once(path("release.json")?)
                .chain(assets.keys().cloned())
                .collect::<Vec<_>>();
            let source_snapshot = source.capture(&paths, limits, &cancel)?;
            source.revalidate(&initial, &cancel)?;
            let mut total = 0u64;
            let mut largest = 0u64;
            for (path, files) in &assets {
                let Some(Observation::File(observed)) = source_snapshot.entries().get(path) else {
                    anyhow::bail!(
                        "Release asset is missing or not a regular file: {}",
                        path.as_str()
                    )
                };
                for file in files {
                    ensure!(
                        observed.bytes == file.bytes
                            && observed.content == release::decode_hex::<32>(&file.sha256)?,
                        "Release asset differs from selected content: {}",
                        file.key
                    );
                }
                total = total
                    .checked_add(observed.bytes)
                    .context("Release asset size overflow")?;
                largest = largest.max(observed.bytes);
            }
            let refs = request.keys.iter().collect::<Vec<_>>();
            let envelope = trust::sign(EnvelopeKind::Release, release.bytes(), &refs)?;
            let keys = request
                .keys
                .iter()
                .map(|key| trust::key_id(&key.verifying_key()))
                .collect();
            total = total
                .checked_add(envelope.len() as u64)
                .context("Release size overflow")?;
            source.revalidate(&source_snapshot, &cancel)?;
            Ok::<_, anyhow::Error>(Captured {
                root,
                source,
                source_snapshot,
                release,
                envelope,
                keys,
                assets,
                bytes: total,
                largest,
            })
        },
    )?;
    let captured = scope.accept(work.wait().await?)?.transpose()?;
    let (mut resources, retained) = project_change::resources(captured.bytes, config)?;
    resources.scratch_bytes = resources
        .scratch_bytes
        .checked_add(captured.largest)
        .context("Release staging size overflow")?;
    let work = scope.spawn_blocking(resources, retained, move |cancel| {
        let (captured, _reservation) = captured.into_parts();
        let Captured {
            root,
            source,
            source_snapshot,
            release,
            envelope,
            keys,
            assets,
            ..
        } = captured;
        let envelope_target = artifact(release.id(), "release.json")?;
        let mut desired_files = BTreeMap::from([(
            envelope_target.clone(),
            desired(&release::hash(&envelope), envelope.len() as u64)?,
        )]);
        for (asset, files) in &assets {
            desired_files.insert(
                artifact(release.id(), asset.as_str())?,
                desired(&files[0].sha256, files[0].bytes)?,
            );
        }
        let paths = desired_files
            .keys()
            .map(ProjectLayout::path)
            .collect::<Result<Vec<_>>>()?;
        let snapshot = root.capture(&paths, limits, &cancel)?;
        let observed =
            verification::observed_artifacts_for(&snapshot, desired_files.keys().cloned())?;
        for (target, expected) in &desired_files {
            ensure!(
                match observed.get(target) {
                    Some(ObservedPath::Absent) => true,
                    Some(ObservedPath::File(actual)) =>
                        actual.equivalent(expected, verification::native_capabilities()),
                    _ => false,
                },
                "Immutable release output already differs: {}",
                ProjectLayout::path(target)?.as_str()
            );
        }
        let files = verification::plan_mutation_files(&observed, &desired_files, &BTreeSet::new())?;
        let mut stage = MutableStage::empty()?;
        if files.expected().contains_key(&envelope_target) {
            stage.write(
                &ProjectLayout::path(&envelope_target)?,
                &mut envelope.as_slice(),
                envelope.len() as u64,
                &cancel,
            )?;
        }
        for (asset, obligations) in &assets {
            let (parent, leaf) = native::parent(&source.directory, asset)?;
            let mut acquired = None;
            // Each provenance assertion remains an independent obligation, even for shared bytes.
            for file in obligations {
                drop(acquired.take());
                let mut input = native::open_file(&parent, &leaf)?;
                acquired = Some(verify_stream(
                    &mut input,
                    &file.expected()?,
                    file.bytes,
                    SourceEvidencePolicy::Compatibility,
                    InitialObservation::RequireEvidence,
                    &cancel,
                )?);
            }
            let target = artifact(release.id(), asset.as_str())?;
            if files.expected().contains_key(&target) {
                let content = acquired.context("Release asset has no content obligations")?;
                stage.write(
                    &ProjectLayout::path(&target)?,
                    &mut content.lease().open(),
                    content.lease().len(),
                    &cancel,
                )?;
            }
        }
        source.revalidate(&source_snapshot, &cancel)?;
        let stage = stage.freeze(limits, &cancel)?;
        let change = VerifiedFileChange::verify_artifacts(snapshot, files.clone(), stage)?;
        let view = ReleasePublicationPreview {
            plan: PlanId(
                NEXT_PLAN
                    .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
                    .map_err(|_| anyhow::anyhow!("Plan identifier exhausted"))?,
            ),
            release: release.id().into(),
            pack: release.document().pack.clone(),
            envelope: ProjectLayout::path(&envelope_target)?.as_str().into(),
            keys,
            replacement: project_change::summary(&files)?,
            files,
        };
        Ok::<_, anyhow::Error>(PreparedReleasePublication {
            view,
            root,
            source,
            source_snapshot,
            change,
        })
    })?;
    scope.accept(work.wait().await?)?.transpose()
}
pub(super) async fn run(
    prepared: RetainedOutput<PreparedReleasePublication>,
    config: EngineConfig,
    scope: WorkScope,
) -> Result<ExecutionOutcome, RuntimeError> {
    let cancellation = scope.cancellation();
    let result: Result<_> = async {
        let work = scope.spawn_blocking(
            config.resources.assembly,
            config.resources.receipt,
            move |cancel| {
                let (prepared, _reservation) = prepared.into_parts();
                prepared
                    .source
                    .revalidate(&prepared.source_snapshot, &cancel)?;
                let publication = Publisher::open(&config.state_root)?.publish(
                    &prepared.root,
                    prepared.change,
                    &cancel,
                )?;
                Ok::<_, anyhow::Error>(ReleasePublicationReceipt {
                    plan: prepared.view.plan,
                    release: prepared.view.release,
                    envelope: prepared.view.envelope,
                    publication,
                })
            },
        )?;
        scope
            .accept_publication(work.wait().await.map_err(PublicationWorkerFailed)?)?
            .transpose()
    }
    .await;
    Ok(match result {
        Ok(receipt) => {
            ExecutionOutcome::Completed(ExecutionReceipt::ReleasePublication(Box::new(receipt)))
        }
        Err(error) => ExecutionOutcome::failed(error, cancellation.is_cancelled()),
    })
}

#[cfg(test)]
mod tests;
