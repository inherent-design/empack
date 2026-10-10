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
}
impl SubscribedRelease {
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
    let channel = record.authenticated_channel(now, version)?;
    let selected = channel.release(&record.trust()?, envelope, version)?;
    root.revalidate(&snapshot, cancel)?;
    Ok(SubscribedRelease {
        release: selected,
        root: binding,
        subscription: release::hash(&bytes),
        expires: channel.document().expires,
    })
}
