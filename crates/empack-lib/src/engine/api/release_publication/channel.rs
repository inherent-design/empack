//! A channel pointer becomes visible only after exact immutable hosted inputs verify.
use super::*;
use crate::engine::{
    acquisition::DownloadRequest,
    release::trust::{ChannelDocument, ChannelRelease, PublisherTrust},
};
use ed25519_dalek::VerifyingKey;
use empack_core::model::{ExpectedContent, NonEmpty};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub struct PublishChannelRequest {
    pub release: String,
    pub channel: String,
    /// HTTPS directory serving this output root's dist/ directory.
    pub base_url: String,
    pub sequence: u64,
    pub expires: i64,
    pub keys: Vec<SigningKey>,
    /// Explicit verification keys for the previous pointer, useful after key rotation.
    pub previous_keys: Vec<VerifyingKey>,
}
pub(super) struct HostedRelease {
    channel: ChannelDocument,
    envelope_hash: String,
    assets: Vec<(String, ExpectedContent, u64)>,
}
impl HostedRelease {
    pub(super) fn validate_fresh(&self) -> Result<()> {
        let now = now()?;
        ensure!(
            now >= 1_577_836_800
                && now < self.channel.expires
                && self.channel.expires - now <= 31 * 24 * 60 * 60,
            "Channel expiry is outside the current freshness window"
        );
        Ok(())
    }
}
fn now() -> Result<i64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)?
        .as_secs()
        .try_into()?)
}
fn version() -> Result<semver::Version> {
    Ok(semver::Version::parse(
        if env!("CARGO_PKG_VERSION") == "0.0.0-dev" {
            "0.6.0-beta"
        } else {
            env!("CARGO_PKG_VERSION")
        },
    )?)
}
fn url(base: &reqwest::Url, relative: &str) -> Result<String> {
    let relative = path(relative)?;
    let mut url = base.clone();
    let mut segments = url
        .path_segments_mut()
        .map_err(|_| anyhow::anyhow!("Publisher URL cannot contain paths"))?;
    segments.pop_if_empty();
    for part in relative.as_str().split('/') {
        segments.push(part);
    }
    drop(segments);
    Ok(url.into())
}
pub(in crate::engine::api) async fn prepare(
    target: ProjectTarget,
    request: PublishChannelRequest,
    config: &EngineConfig,
    scope: &mut WorkScope,
) -> Result<RetainedOutput<PreparedReleasePublication>> {
    let ProjectTarget::Existing(target) = target else {
        anyhow::bail!("Channel publication requires an existing output root")
    };
    ensure!(target.is_absolute(), "Publisher root must be absolute");
    let limits = config.snapshot;
    let state = config.state_root.clone();
    let work = scope.spawn_blocking(
        config.resources.capture,
        config.resources.prepared,
        move |cancel| prepare_local(target, request, state, limits, &cancel),
    )?;
    scope.accept(work.wait().await?)?.transpose()
}
fn prepare_local(
    target: PathBuf,
    request: PublishChannelRequest,
    state: PathBuf,
    limits: crate::engine::snapshot::SnapshotLimits,
    cancel: &crate::application::process_runtime::Cancellation,
) -> Result<PreparedReleasePublication> {
    release::decode_hex::<32>(&request.release)?;
    release::identifier(&request.channel)?;
    let base = release::https(&request.base_url)?;
    ensure!(
        base.path().ends_with('/') && base.query().is_none(),
        "Publisher URL must be an HTTPS directory without a query"
    );
    let root = ProjectReadRoot::open(&target)?;
    let source = ProjectReadRoot::open(&target)?;
    let _guard = RecoveryReader::new(state).enter(&root)?;
    let selected = artifact(&request.release, "release.json")?;
    let initial = source.capture(&[ProjectLayout::path(&selected)?], limits, cancel)?;
    let envelope = crate::engine::project::read_document_limited(
        &source,
        &initial,
        ProjectLayout::path(&selected)?.as_str(),
        2 * release::MAX_RELEASE_BYTES as u64 + 16384,
        cancel,
    )?
    .context("Stage the immutable release before publishing a channel")?;
    // Discover pack identity only after an enrolled local key authenticates the envelope.
    // The staged manifest accompanies an explicit local release selection; obtain the pack
    // from a verified signature through the publisher inspection helper.
    let public = request
        .keys
        .iter()
        .map(SigningKey::verifying_key)
        .collect::<Vec<_>>();
    let decoded = trust::publisher_release(&envelope, &request.release, &public, &version()?)?;
    let trust = PublisherTrust::enroll(
        decoded.document().pack.clone(),
        base.as_str(),
        public.clone(),
    )?;
    trust.release(&envelope, &request.release, &version()?)?;
    let release_url = url(&base, &format!("releases/{}/release.json", request.release))?;
    let channel = ChannelDocument {
        schema: 1,
        pack: decoded.document().pack.clone(),
        channel: request.channel.clone(),
        sequence: request.sequence,
        expires: request.expires,
        minimum_engine: decoded.document().minimum_engine.clone(),
        release: ChannelRelease {
            id: request.release.clone(),
            url: release_url,
            maximum_bytes: envelope.len() as u64,
        },
    };
    let bytes = trust::sign(
        EnvelopeKind::Channel,
        &channel.encode()?,
        &request.keys.iter().collect::<Vec<_>>(),
    )?;
    trust.channel(&bytes, &request.channel, now()?, &version()?, None)?;
    let destination = ManagedPath::Artifact(path(&format!("channels/{}.json", request.channel))?);
    let mut paths = BTreeSet::from([
        ProjectLayout::path(&selected)?,
        ProjectLayout::path(&destination)?,
    ]);
    let mut assets = BTreeMap::<String, (ExpectedContent, u64)>::new();
    for file in &decoded.document().files {
        if let Some(asset) = file.asset_path() {
            paths.insert(ProjectLayout::path(&artifact(&request.release, asset)?)?);
            let url = url(&base, &format!("releases/{}/{asset}", request.release))?;
            let expected = file.expected()?;
            if let Some((prior, bytes)) = assets.get_mut(&url) {
                ensure!(
                    *bytes == file.bytes
                        && prior.accepted_observation == expected.accepted_observation,
                    "Shared asset has contradictory byte identities"
                );
                let assertions = prior
                    .digests
                    .iter()
                    .chain(expected.digests.iter())
                    .flat_map(|set| set.values().iter().cloned())
                    .collect::<Vec<_>>();
                prior.digests = if assertions.is_empty() {
                    None
                } else {
                    Some(empack_core::digest::DigestSet::new(assertions)?)
                };
            } else {
                assets.insert(url, (expected, file.bytes));
            }
        }
    }
    let paths = paths.into_iter().collect::<Vec<_>>();
    let source_snapshot = source.capture(&paths, limits, cancel)?;
    source.revalidate(&initial, cancel)?;
    let snapshot = root.capture(&paths, limits, cancel)?;
    for file in &decoded.document().files {
        if let Some(asset) = file.asset_path() {
            let local = ProjectLayout::path(&artifact(&request.release, asset)?)?;
            let Some(Observation::File(observed)) = snapshot.entries().get(&local) else {
                anyhow::bail!("Published asset is absent or not a file")
            };
            ensure!(
                observed.bytes == file.bytes
                    && observed.content == release::decode_hex::<32>(&file.sha256)?,
                "Published asset differs from the selected release"
            );
        }
    }
    if let Some(previous) = crate::engine::project::read_document_limited(
        &root,
        &snapshot,
        ProjectLayout::path(&destination)?.as_str(),
        48 << 10,
        cancel,
    )? {
        let mut keys = public;
        for key in request.previous_keys {
            if !keys.contains(&key) {
                keys.push(key);
            }
        }
        let previous_trust = PublisherTrust::enroll(channel.pack.clone(), base.as_str(), keys)?;
        let prior = previous_trust.previous_channel(&previous, &channel.channel)?;
        ensure!(
            channel.sequence > prior.sequence
                || (channel.sequence == prior.sequence && channel == prior),
            "Channel sequence must advance; an existing sequence cannot select different metadata"
        );
    }
    let observed = verification::observed_artifacts_for(&snapshot, [destination.clone()])?;
    let desired = BTreeMap::from([(
        destination.clone(),
        desired(&release::hash(&bytes), bytes.len() as u64)?,
    )]);
    let files = verification::plan_mutation_files(&observed, &desired, &BTreeSet::new())?;
    let mut stage = MutableStage::empty()?;
    if files.expected().contains_key(&destination) {
        stage.write(
            &ProjectLayout::path(&destination)?,
            &mut bytes.as_slice(),
            bytes.len() as u64,
            cancel,
        )?;
    }
    let change = VerifiedFileChange::verify_artifacts(
        snapshot,
        files.clone(),
        stage.freeze(limits, cancel)?,
    )?;
    source.revalidate(&source_snapshot, cancel)?;
    let view = ReleasePublicationPreview {
        channel: Some(channel.channel.clone()),
        plan: PlanId(
            NEXT_PLAN
                .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
                .map_err(|_| anyhow::anyhow!("Plan identifier exhausted"))?,
        ),
        release: channel.release.id.clone(),
        pack: channel.pack.clone(),
        envelope: ProjectLayout::path(&destination)?.as_str().into(),
        keys: request
            .keys
            .iter()
            .map(|key| trust::key_id(&key.verifying_key()))
            .collect(),
        replacement: project_change::summary(&files)?,
        files,
    };
    Ok::<_, anyhow::Error>(PreparedReleasePublication {
        view,
        root,
        source,
        source_snapshot,
        change,
        remote: Some(HostedRelease {
            channel,
            envelope_hash: release::hash(&envelope),
            assets: assets
                .into_iter()
                .map(|(url, (expected, bytes))| (url, expected, bytes))
                .collect(),
        }),
    })
}

pub(super) async fn verify_hosted(
    remote: &HostedRelease,
    transport: &HttpAcquisition,
    config: &EngineConfig,
    scope: &mut WorkScope,
) -> Result<()> {
    remote.validate_fresh()?;
    let envelope = transport
        .publisher_metadata(
            scope,
            &remote.channel.release.url,
            remote.channel.release.maximum_bytes,
            Duration::from_secs(60),
        )
        .await?;
    ensure!(
        release::hash(&envelope) == remote.envelope_hash,
        "Hosted release envelope differs from the staged immutable release"
    );
    let requests = remote
        .assets
        .iter()
        .map(|(url, expected, bytes)| {
            Ok(DownloadRequest {
                alternatives: NonEmpty::new(vec![url.clone()])?,
                expected: expected.clone(),
                limits: TransferLimits {
                    file_bytes: config.transfer.file_bytes.min((*bytes).max(1)),
                    ..config.transfer
                },
                evidence: SourceEvidencePolicy::Compatibility,
                initial: InitialObservation::RequireEvidence,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let acquired = transport
        .acquire_batch(scope, requests, config.transfer)
        .await?;
    drop(acquired);
    remote.validate_fresh()
}
