//! Publisher authentication over exact payload bytes. No network or write authority.
use super::*;
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use std::collections::BTreeMap;

const ENVELOPE_SCHEMA: u32 = 1;
const MAX_ENVELOPE_BYTES: usize = MAX_RELEASE_BYTES * 2 + 16 * 1024;
const MAX_CHANNEL_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum EnvelopeKind {
    Release,
    Channel,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    schema: u32,
    kind: EnvelopeKind,
    payload: String,
    signatures: Vec<EnvelopeSignature>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EnvelopeSignature {
    algorithm: String,
    key: String,
    signature: String,
}
fn message(kind: EnvelopeKind, payload: &[u8]) -> Vec<u8> {
    let prefix: &[u8] = match kind {
        EnvelopeKind::Release => b"empack.release.v1\0",
        EnvelopeKind::Channel => b"empack.channel.v1\0",
    };
    [prefix, payload].concat()
}
/// Fingerprint displayed for explicit enrollment. A downloaded fingerprint is not trust.
pub fn key_id(key: &VerifyingKey) -> String {
    hash(key.as_bytes())
}

/// Sign already validated payload bytes. Keys are supplied separately from project documents.
pub fn sign(kind: EnvelopeKind, payload: &[u8], keys: &[&SigningKey]) -> Result<Vec<u8>> {
    validate_payload(kind, payload)?;
    ensure!(
        !keys.is_empty() && keys.len() <= 16,
        "Envelope requires one to sixteen signatures"
    );
    let message = message(kind, payload);
    let mut signatures = Vec::new();
    let mut unique = BTreeSet::new();
    for key in keys {
        let id = key_id(&key.verifying_key());
        ensure!(unique.insert(id.clone()), "Duplicate signing key");
        signatures.push(EnvelopeSignature {
            algorithm: "ed25519".into(),
            key: id,
            signature: hex(&key.sign(&message).to_bytes()),
        });
    }
    signatures.sort_by(|a, b| a.key.cmp(&b.key));
    Ok(serde_json::to_vec(&Envelope {
        schema: ENVELOPE_SCHEMA,
        kind,
        payload: hex(payload),
        signatures,
    })?)
}
fn validate_payload(kind: EnvelopeKind, payload: &[u8]) -> Result<()> {
    match kind {
        EnvelopeKind::Release => {
            DecodedRelease::decode(payload)?;
        }
        EnvelopeKind::Channel => {
            ChannelDocument::decode(payload)?;
        }
    }
    Ok(())
}

/// Explicit local enrollment. The caller obtains confirmation or out-of-band configuration.
/// This is deliberately not deserializable from downloaded envelopes.
pub struct PublisherTrust {
    pack: String,
    origin: String,
    keys: BTreeMap<String, VerifyingKey>,
}
impl PublisherTrust {
    pub fn enroll(pack: String, origin: &str, keys: Vec<VerifyingKey>) -> Result<Self> {
        identifier(&pack)?;
        let origin = https(origin)?.origin().ascii_serialization();
        ensure!(
            !keys.is_empty() && keys.len() <= 16,
            "Trust requires one to sixteen enrolled keys"
        );
        let mut result = BTreeMap::new();
        for key in keys {
            ensure!(!key.is_weak(), "Cannot enroll a weak publisher key");
            ensure!(
                result.insert(key_id(&key), key).is_none(),
                "Duplicate enrolled key"
            );
        }
        Ok(Self {
            pack,
            origin,
            keys: result,
        })
    }
    pub fn pack(&self) -> &str {
        &self.pack
    }
    pub fn origin(&self) -> &str {
        &self.origin
    }
    fn verify(&self, kind: EnvelopeKind, encoded: &[u8]) -> Result<Vec<u8>> {
        let limit = match kind {
            EnvelopeKind::Release => MAX_ENVELOPE_BYTES,
            EnvelopeKind::Channel => MAX_CHANNEL_BYTES * 2 + 16 * 1024,
        };
        ensure!(encoded.len() <= limit, "Signed envelope exceeds byte limit");
        let envelope: Envelope =
            serde_json::from_slice(encoded).context("Invalid signed envelope")?;
        ensure!(
            envelope.schema == ENVELOPE_SCHEMA && envelope.kind == kind,
            "Unexpected signed envelope context"
        );
        let payload_limit = match kind {
            EnvelopeKind::Release => MAX_RELEASE_BYTES,
            EnvelopeKind::Channel => MAX_CHANNEL_BYTES,
        };
        ensure!(
            envelope.payload.len() <= payload_limit * 2 && envelope.payload.len().is_multiple_of(2),
            "Invalid envelope payload length"
        );
        ensure!(
            envelope
                .payload
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "Noncanonical envelope encoding"
        );
        let payload = envelope
            .payload
            .as_bytes()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect::<Vec<_>>();
        ensure!(
            !envelope.signatures.is_empty() && envelope.signatures.len() <= 16,
            "Invalid signature count"
        );
        let message = message(kind, &payload);
        let mut seen = BTreeSet::new();
        let mut authenticated = false;
        for signature in envelope.signatures {
            ensure!(
                signature.algorithm == "ed25519",
                "Unsupported signature algorithm"
            );
            decode_hex::<32>(&signature.key)?;
            ensure!(
                seen.insert(signature.key.clone()),
                "Duplicate signature key"
            );
            let signature_bytes = decode_hex::<64>(&signature.signature)?;
            if let Some(key) = self.keys.get(&signature.key) {
                key.verify_strict(&message, &Signature::from_bytes(&signature_bytes))
                    .map_err(|_| anyhow::anyhow!("Publisher signature verification failed"))?;
                authenticated = true;
            }
        }
        ensure!(authenticated, "No enrolled publisher signed this envelope");
        Ok(payload)
    }
    /// Signature, expected identity, pack and engine compatibility are all checked before use.
    pub fn release(
        &self,
        envelope: &[u8],
        expected: &str,
        engine: &semver::Version,
    ) -> Result<AuthenticatedRelease> {
        decode_hex::<32>(expected)?;
        let payload = self.verify(EnvelopeKind::Release, envelope)?;
        ensure!(
            hash(&payload) == expected,
            "Release payload identity mismatch"
        );
        let release = DecodedRelease::decode(&payload)?;
        ensure!(
            release.document.pack == self.pack,
            "Publisher release belongs to another pack"
        );
        compatible(&release.document.minimum_engine, engine)?;
        Ok(AuthenticatedRelease {
            release,
            origin: self.origin.clone(),
        })
    }
    /// Authenticate historical publisher state for sequence ordering only. Expired metadata
    /// cannot select an installation through this method or construct an update proof.
    pub(in crate::engine) fn previous_channel(
        &self,
        envelope: &[u8],
        name: &str,
    ) -> Result<ChannelDocument> {
        let payload = self.verify(EnvelopeKind::Channel, envelope)?;
        let channel = ChannelDocument::decode(&payload)?;
        ensure!(
            channel.pack == self.pack && channel.channel == name,
            "Published channel identity mismatch"
        );
        ensure!(
            https(&channel.release.url)?.origin().ascii_serialization() == self.origin,
            "Published channel has another origin"
        );
        Ok(channel)
    }
    /// `floor` is durable subscription state, independently retained from installed releases.
    /// Returns a proposed floor; callers persist it before admitting update acquisition.
    pub fn channel(
        &self,
        envelope: &[u8],
        name: &str,
        now: i64,
        engine: &semver::Version,
        floor: Option<&SequenceFloor>,
    ) -> Result<AuthenticatedChannel> {
        identifier(name)?;
        ensure!(
            now >= 1_577_836_800,
            "Local clock cannot establish channel freshness"
        );
        let payload = self.verify(EnvelopeKind::Channel, envelope)?;
        let channel = ChannelDocument::decode(&payload)?;
        ensure!(
            channel.pack == self.pack && channel.channel == name,
            "Channel subscription identity mismatch"
        );
        ensure!(channel.expires > now, "Channel has expired");
        ensure!(
            channel.expires - now <= 31 * 24 * 60 * 60,
            "Channel expiry is beyond the supported freshness window"
        );
        ensure!(
            https(&channel.release.url)?.origin().ascii_serialization() == self.origin,
            "Channel release URL is outside the enrolled origin"
        );
        compatible(&channel.minimum_engine, engine)?;
        let candidate = SequenceFloor {
            pack: self.pack.clone(),
            origin: self.origin.clone(),
            channel: name.into(),
            sequence: channel.sequence,
            payload: hash(&payload),
        };
        if let Some(floor) = floor {
            ensure!(
                floor.pack == candidate.pack
                    && floor.origin == candidate.origin
                    && floor.channel == candidate.channel,
                "Saved sequence floor belongs to another subscription"
            );
            ensure!(
                candidate.sequence >= floor.sequence,
                "Channel sequence replay rejected"
            );
            ensure!(
                candidate.sequence != floor.sequence || candidate.payload == floor.payload,
                "Channel changed at an already observed sequence"
            );
        }
        Ok(AuthenticatedChannel {
            document: channel,
            floor: candidate,
        })
    }
}
fn compatible(requirement: &str, engine: &semver::Version) -> Result<()> {
    ensure!(
        semver::VersionReq::parse(requirement)?.matches(engine),
        "Release requires another empack version; tool installation needs separate authorization"
    );
    Ok(())
}
/// Proof is constructed only after verification against explicitly enrolled keys.
pub struct AuthenticatedRelease {
    release: DecodedRelease,
    origin: String,
}
impl AuthenticatedRelease {
    pub fn release(&self) -> &DecodedRelease {
        &self.release
    }
    pub fn origin(&self) -> &str {
        &self.origin
    }
}
/// Inspect a local publisher envelope using explicitly supplied verification keys.
/// Returns validated data, not subscriber trust or instance update authority.
pub(in crate::engine) fn publisher_release(
    envelope: &[u8],
    expected: &str,
    keys: &[VerifyingKey],
    engine: &semver::Version,
) -> Result<DecodedRelease> {
    let verifier = PublisherTrust::enroll(
        "publisher-inspection".into(),
        "https://publisher.invalid",
        keys.to_vec(),
    )?;
    let payload = verifier.verify(EnvelopeKind::Release, envelope)?;
    ensure!(
        hash(&payload) == expected,
        "Staged release identity mismatch"
    );
    let release = DecodedRelease::decode(&payload)?;
    compatible(&release.document.minimum_engine, engine)?;
    Ok(release)
}
/// Explicit local snapshot selection. No publisher or channel trust is inferred.
pub struct SelectedSnapshot {
    release: DecodedRelease,
}
impl SelectedSnapshot {
    pub fn select(bytes: &[u8], expected: &str, engine: &semver::Version) -> Result<Self> {
        decode_hex::<32>(expected)?;
        ensure!(
            hash(bytes) == expected,
            "Selected snapshot identity mismatch"
        );
        let release = DecodedRelease::decode(bytes)?;
        compatible(&release.document.minimum_engine, engine)?;
        Ok(Self { release })
    }
    pub fn release(&self) -> &DecodedRelease {
        &self.release
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ChannelDocument {
    pub schema: u32,
    pub pack: String,
    pub channel: String,
    pub sequence: u64,
    /// Unix seconds UTC, not a locale-dependent date string.
    pub expires: i64,
    pub release: ChannelRelease,
    pub minimum_engine: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ChannelRelease {
    /// SHA-256 of release payload bytes, not its signature envelope.
    pub id: String,
    pub url: String,
    /// Exact upper bound for the complete downloaded envelope.
    pub maximum_bytes: u64,
}
impl ChannelDocument {
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        ensure!(
            bytes.len() <= MAX_CHANNEL_BYTES,
            "Channel exceeds document byte limit"
        );
        let result: Self = serde_json::from_slice(bytes).context("Invalid channel document")?;
        ensure!(result.schema == 1, "Unsupported channel schema");
        identifier(&result.pack)?;
        identifier(&result.channel)?;
        ensure!(
            result.sequence > 0 && result.expires > 0,
            "Invalid channel sequence or expiry"
        );
        decode_hex::<32>(&result.release.id)?;
        https(&result.release.url)?;
        ensure!(
            result.release.maximum_bytes > 0
                && result.release.maximum_bytes <= MAX_ENVELOPE_BYTES as u64,
            "Invalid release envelope byte bound"
        );
        semver::VersionReq::parse(&result.minimum_engine)?;
        Ok(result)
    }
    pub fn encode(&self) -> Result<Vec<u8>> {
        let bytes = serde_json::to_vec(self)?;
        Self::decode(&bytes)?;
        Ok(bytes)
    }
}
/// Local data, not an authentication proof. Protect it with the instance's durable state.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SequenceFloor {
    pub pack: String,
    pub origin: String,
    pub channel: String,
    pub sequence: u64,
    pub payload: String,
}
pub struct AuthenticatedChannel {
    document: ChannelDocument,
    floor: SequenceFloor,
}
impl AuthenticatedChannel {
    pub fn document(&self) -> &ChannelDocument {
        &self.document
    }
    pub fn floor(&self) -> &SequenceFloor {
        &self.floor
    }
    /// Release must satisfy this channel's exact identity and envelope bound.
    pub fn release(
        &self,
        trust: &PublisherTrust,
        bytes: &[u8],
        engine: &semver::Version,
    ) -> Result<AuthenticatedRelease> {
        ensure!(
            trust.pack == self.floor.pack && trust.origin == self.floor.origin,
            "Channel and publisher trust differ"
        );
        ensure!(
            bytes.len() as u64 <= self.document.release.maximum_bytes,
            "Release exceeds channel envelope byte bound"
        );
        trust.release(bytes, &self.document.release.id, engine)
    }
}

/// Bounded, no-follow local input. Reading does not authenticate or enroll the publisher.
pub fn read_channel_envelope(
    selected: &std::path::Path,
    cancel: &crate::application::process_runtime::Cancellation,
) -> Result<Vec<u8>> {
    read_envelope(selected, (MAX_CHANNEL_BYTES * 2 + 16 * 1024) as u64, cancel)
}
/// Bounded local signed release input. The caller must authenticate against its enrolled channel.
pub fn read_release_envelope(
    selected: &std::path::Path,
    cancel: &crate::application::process_runtime::Cancellation,
) -> Result<Vec<u8>> {
    read_envelope(selected, MAX_ENVELOPE_BYTES as u64, cancel)
}
fn read_envelope(
    selected: &std::path::Path,
    maximum: u64,
    cancel: &crate::application::process_runtime::Cancellation,
) -> Result<Vec<u8>> {
    ensure!(
        selected.is_absolute(),
        "Envelope selection must be absolute"
    );
    let root = crate::engine::snapshot::ProjectReadRoot::open(
        selected
            .parent()
            .context("Envelope selection needs a parent")?,
    )?;
    let leaf = selected
        .file_name()
        .and_then(|name| name.to_str())
        .context("Invalid envelope filename")?;
    let relative = PortableRelPath::parse(leaf, PathSyntax::ProjectContent)?;
    let snapshot = root.capture(
        &[relative],
        crate::engine::snapshot::SnapshotLimits {
            file_bytes: maximum,
            total_bytes: maximum,
            ..Default::default()
        },
        cancel,
    )?;
    let bytes =
        crate::engine::project::read_document_limited(&root, &snapshot, leaf, maximum, cancel)?
            .context("Selected envelope does not exist")?;
    root.revalidate(&snapshot, cancel)?;
    Ok(bytes)
}
