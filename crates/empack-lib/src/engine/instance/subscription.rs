//! Strict local subscription state, distinct from signature-verification proofs.
use crate::engine::release::{
    self,
    trust::{PublisherTrust, SequenceFloor},
};
use anyhow::{Context, Result, ensure};
use ed25519_dalek::VerifyingKey;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Local trust configuration is not a signature-verification proof.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubscriptionRecord {
    pub schema: u32,
    pub root: String,
    pub pack: String,
    pub channel: String,
    pub url: String,
    /// Explicitly enrolled Ed25519 public keys. Empty means updates are disabled.
    pub keys: Vec<String>,
    /// Independent of the currently installed or rolled-back release.
    pub floor: Option<SequenceFloor>,
    /// Exact authenticated envelope; reverified against current keys and time before selection.
    pub observed: Option<String>,
}
impl SubscriptionRecord {
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        ensure!(
            bytes.len() <= 128 << 10,
            "Subscription exceeds document limit"
        );
        let record: Self = serde_json::from_slice(bytes).context("Invalid subscription record")?;
        ensure!(record.schema == 1, "Unsupported subscription schema");
        release::identifier(&record.pack)?;
        release::identifier(&record.channel)?;
        ensure!(
            !record.root.is_empty() && record.root.len() <= 256,
            "Invalid instance root binding"
        );
        let origin = release::https(&record.url)?.origin().ascii_serialization();
        ensure!(
            record.floor.is_some() == record.observed.is_some(),
            "Subscription observation and floor disagree"
        );
        let keys = record.verifying_keys()?;
        if !keys.is_empty() {
            PublisherTrust::enroll(record.pack.clone(), &origin, keys)?;
        }
        if let Some(floor) = &record.floor {
            ensure!(
                floor.pack == record.pack
                    && floor.channel == record.channel
                    && floor.origin == origin
                    && floor.sequence > 0,
                "Subscription sequence floor has another identity"
            );
            release::decode_hex::<32>(&floor.payload)?;
        }
        Ok(record)
    }
    fn verifying_keys(&self) -> Result<Vec<VerifyingKey>> {
        ensure!(self.keys.len() <= 16, "Too many publisher keys");
        let mut unique = BTreeSet::new();
        self.keys
            .iter()
            .map(|encoded| {
                ensure!(unique.insert(encoded), "Duplicate publisher key");
                let key = VerifyingKey::from_bytes(&release::decode_hex::<32>(encoded)?)?;
                ensure!(!key.is_weak(), "Cannot enroll a weak publisher key");
                Ok(key)
            })
            .collect()
    }
    pub fn authenticated_channel(
        &self,
        now: i64,
        engine: &semver::Version,
    ) -> Result<release::trust::AuthenticatedChannel> {
        self.trust()?.channel(
            self.observed
                .as_ref()
                .context("Subscription has no authenticated observation")?
                .as_bytes(),
            &self.channel,
            now,
            engine,
            self.floor.as_ref(),
        )
    }
    pub fn trust(&self) -> Result<PublisherTrust> {
        PublisherTrust::enroll(self.pack.clone(), &self.url, self.verifying_keys()?)
    }
}

/// Release authentication bound to the current durable enrollment and observed channel.
/// Fields are private; a decoded subscription cannot manufacture this proof.
pub struct SubscribedRelease {
    release: release::trust::AuthenticatedRelease,
    root: String,
    subscription: String,
    expires: i64,
    assets: reqwest::Url,
    envelope: Vec<u8>,
}
impl SubscribedRelease {
    pub(in crate::engine) fn envelope(&self) -> &[u8] {
        &self.envelope
    }
    pub fn release(&self) -> &release::DecodedRelease {
        self.release.release()
    }
    pub(super) fn validate_current(&self, root: &str, bytes: &[u8]) -> Result<()> {
        ensure!(
            root == self.root && release::hash(bytes) == self.subscription,
            "Publisher enrollment or observed channel changed; select the release again"
        );
        ensure_fresh(self.expires)
    }
    pub(super) fn assets(&self) -> reqwest::Url {
        self.assets.clone()
    }
    pub(super) fn expires(&self) -> i64 {
        self.expires
    }
}
pub(super) fn ensure_fresh(expires: i64) -> Result<()> {
    let now: i64 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs()
        .try_into()?;
    ensure!(
        now >= 1_577_836_800 && now < expires,
        "Channel observation is no longer fresh"
    );
    Ok(())
}
/// Read and authenticate only. The returned proof does not authorize writes or acquisition.
pub fn select_release(
    selected_root: &std::path::Path,
    envelope: &[u8],
    now: i64,
    version: &semver::Version,
    recovery: crate::engine::publication::RecoveryReader,
    cancel: &crate::application::process_runtime::Cancellation,
) -> Result<SubscribedRelease> {
    let (record, subscription) = read_current(selected_root, recovery, cancel)?;
    let channel = record.authenticated_channel(now, version)?;
    let selected = channel.release(&record.trust()?, envelope, version)?;
    Ok(SubscribedRelease {
        release: selected,
        envelope: envelope.to_vec(),
        root: record.root,
        subscription,
        expires: channel.document().expires,
        assets: asset_base(&channel.document().release.url)?,
    })
}

/// Inspect durable enrollment without granting trust or mutation authority.
pub fn inspect(
    selected_root: &std::path::Path,
    recovery: crate::engine::publication::RecoveryReader,
    cancel: &crate::application::process_runtime::Cancellation,
) -> Result<SubscriptionRecord> {
    read_current(selected_root, recovery, cancel).map(|(record, _)| record)
}

fn read_current(
    selected_root: &std::path::Path,
    recovery: crate::engine::publication::RecoveryReader,
    cancel: &crate::application::process_runtime::Cancellation,
) -> Result<(SubscriptionRecord, String)> {
    use crate::engine::{
        layout::ProjectLayout,
        publication::root_key,
        snapshot::{ProjectReadRoot, SnapshotLimits},
    };
    use empack_core::files::ManagedPath;
    ensure!(
        selected_root.is_absolute(),
        "Instance selection must be absolute"
    );
    let root = ProjectReadRoot::open(selected_root)?;
    let _guard = recovery.enter(&root)?;
    let target = ManagedPath::InstanceSubscription;
    let snapshot = root.capture(
        &[ProjectLayout::path(&target)?],
        SnapshotLimits {
            file_bytes: 128 << 10,
            total_bytes: 128 << 10,
            ..Default::default()
        },
        cancel,
    )?;
    let bytes = super::read_optional(&root, &snapshot, &target, cancel)?
        .context("Instance has no enrolled subscription")?;
    let record = SubscriptionRecord::decode(&bytes)?;
    let binding = root_key(&root)?;
    ensure!(
        record.root == binding,
        "Subscription belongs to another instance root"
    );
    root.revalidate(&snapshot, cancel)?;
    Ok((record, release::hash(&bytes)))
}

/// A transport-only failure before channel authentication or publication. Only this
/// result is eligible for an explicitly selected offline-launch policy.
pub enum ChannelFetch {
    Available(crate::engine::runtime::RetainedOutput<Vec<u8>>),
    Unavailable(anyhow::Error),
}
fn classify_channel_fetch(
    result: Result<crate::engine::runtime::RetainedOutput<Vec<u8>>>,
) -> Result<ChannelFetch> {
    match result {
        Ok(bytes) => Ok(ChannelFetch::Available(bytes)),
        Err(error)
            if matches!(
                error.downcast_ref::<crate::engine::acquisition::TransferError>(),
                Some(
                    crate::engine::acquisition::TransferError::Network
                        | crate::engine::acquisition::TransferError::Deadline
                )
            ) =>
        {
            Ok(ChannelFetch::Unavailable(error))
        }
        Err(error) => Err(error),
    }
}
/// Fetch the enrolled channel without authenticating its contents or updating the floor.
/// Missing/revoked enrollment, invalid transport policy and resource failures stay errors.
pub async fn fetch_channel(
    scope: &mut crate::engine::runtime::WorkScope,
    root: std::path::PathBuf,
    state: std::path::PathBuf,
    transport: &crate::engine::acquisition::HttpAcquisition,
) -> Result<ChannelFetch> {
    use crate::engine::{publication::RecoveryReader, resources::ResourceRequest};
    let work = scope.spawn_blocking(
        ResourceRequest {
            jobs: 1,
            memory_bytes: 1 << 20,
            open_files: 8,
            ..Default::default()
        },
        ResourceRequest {
            memory_bytes: 128 << 10,
            ..Default::default()
        },
        move |cancel| inspect(&root, RecoveryReader::new(state), &cancel),
    )?;
    let record = scope.accept(work.wait().await?)?.transpose()?;
    record.trust()?;
    classify_channel_fetch(
        transport
            .publisher_metadata(
                scope,
                &record.url,
                48 << 10,
                std::time::Duration::from_secs(30),
            )
            .await,
    )
}

/// Fetch only the release selected by an already durable authenticated observation.
/// The channel floor must have been published before this acquisition is called.
pub async fn fetch_release(
    scope: &mut crate::engine::runtime::WorkScope,
    root: std::path::PathBuf,
    state: std::path::PathBuf,
    transport: &crate::engine::acquisition::HttpAcquisition,
    version: semver::Version,
) -> Result<crate::engine::runtime::RetainedOutput<SubscribedRelease>> {
    use crate::engine::{publication::RecoveryReader, resources::ResourceRequest};
    let selected_root = root.clone();
    let selected_state = state.clone();
    let read = scope.spawn_blocking(
        ResourceRequest {
            jobs: 1,
            memory_bytes: 1 << 20,
            open_files: 8,
            ..Default::default()
        },
        ResourceRequest {
            memory_bytes: 128 << 10,
            ..Default::default()
        },
        move |cancel| inspect(&selected_root, RecoveryReader::new(selected_state), &cancel),
    )?;
    let record = scope.accept(read.wait().await?)?.transpose()?;
    let channel = record.authenticated_channel(current_time()?, &version)?;
    let envelope = transport
        .publisher_metadata(
            scope,
            &channel.document().release.url,
            channel.document().release.maximum_bytes,
            std::time::Duration::from_secs(60),
        )
        .await?;
    let work = scope.spawn_blocking(
        ResourceRequest {
            jobs: 1,
            memory_bytes: 128 << 20,
            open_files: 16,
            ..Default::default()
        },
        ResourceRequest {
            memory_bytes: 64 << 20,
            ..Default::default()
        },
        move |cancel| {
            select_release(
                &root,
                &envelope,
                current_time()?,
                &version,
                RecoveryReader::new(state),
                &cancel,
            )
        },
    )?;
    scope.accept(work.wait().await?)?.transpose()
}
fn current_time() -> Result<i64> {
    Ok(std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs()
        .try_into()?)
}

fn asset_base(release_url: &str) -> Result<reqwest::Url> {
    let mut base = release::https(release_url)?;
    base.set_query(None);
    base.path_segments_mut()
        .map_err(|_| anyhow::anyhow!("Release URL cannot contain assets"))?
        .pop_if_empty()
        .pop()
        .push("");
    Ok(base)
}

pub(super) fn asset_url(base: &reqwest::Url, path: &str) -> Result<String> {
    let path = empack_core::path::PortableRelPath::parse(
        path,
        empack_core::path::PathSyntax::ArchiveMember,
    )?;
    let mut url = base.clone();
    {
        let mut segments = url
            .path_segments_mut()
            .map_err(|_| anyhow::anyhow!("Invalid asset base"))?;
        segments.pop_if_empty();
        for component in path.as_str().split('/') {
            segments.push(component);
        }
    }
    Ok(url.to_string())
}

#[cfg(test)]
mod locator_tests {
    use super::*;
    #[test]
    fn offline_fallback_is_limited_to_channel_transport_failures() {
        use crate::engine::acquisition::TransferError;
        for error in [TransferError::Network, TransferError::Deadline] {
            assert!(matches!(
                classify_channel_fetch(Err(anyhow::Error::new(error).context("channel fetch")))
                    .unwrap(),
                ChannelFetch::Unavailable(_)
            ));
        }
        for error in [
            TransferError::InvalidLocator,
            TransferError::Unauthorized,
            TransferError::NotFound,
            TransferError::ByteLimit,
            TransferError::RateLimited,
            TransferError::Server(503),
            TransferError::Status(400),
            TransferError::RedirectLimit,
        ] {
            assert!(classify_channel_fetch(Err(error.into())).is_err());
        }
        assert!(classify_channel_fetch(Err(anyhow::anyhow!("invalid signature"))).is_err());
        assert!(
            classify_channel_fetch(Err(crate::engine::runtime::RuntimeError::Cancelled.into()))
                .is_err()
        );
    }
    #[test]
    fn asset_paths_are_literal_relative_components_without_release_credentials() {
        let base = asset_base("https://publisher.test/releases/v1.json?token=private").unwrap();
        assert_eq!(
            asset_url(&base, "assets/a%2fb").unwrap(),
            "https://publisher.test/releases/assets/a%252fb"
        );
        assert!(asset_url(&base, "../outside").is_err());
        assert!(asset_url(&base, "/absolute").is_err());
    }
}
