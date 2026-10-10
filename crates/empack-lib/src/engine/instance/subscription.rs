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
