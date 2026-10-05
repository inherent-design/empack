//! Verified private content leases keep byte ownership separate from source assurance.
use super::{
    snapshot::SnapshotLimits,
    staging::{FrozenStage, MutableStage},
};
use crate::application::process_runtime::Cancellation;
use anyhow::{Context, Result, ensure};
use empack_core::{
    digest::{ContentId, DigestAlgorithm, DigestSet, ExpectedDigest, IntegrityEvidence},
    model::ExpectedContent,
    path::{PathSyntax, PortableRelPath},
};
use sha2::Digest;
use std::{
    io::{self, Read, Seek, SeekFrom, Write},
    sync::{Arc, Mutex},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceEvidencePolicy {
    /// Provider MD5/SHA-1 assertions remain visibly weaker source evidence.
    Compatibility,
    /// Require independently declared SHA-256 or SHA-512, not an internally computed address.
    StrongSourceRequired,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InitialObservation {
    /// No source evidence means preparation needs an explicit user choice.
    RequireEvidence,
    /// An explicitly selected initial local/manual file may establish an observation.
    Accepted,
}
struct ContentObject {
    stage: Mutex<FrozenStage>,
    path: PortableRelPath,
    id: ContentId,
    bytes: u64,
    _reservation: Option<super::resources::AdmissionPermit>,
}
/// Retains an owned private copy. Reader positions are independent even on Windows.
#[derive(Clone)]
pub struct ContentLease(Arc<ContentObject>);
impl ContentLease {
    pub fn id(&self) -> ContentId {
        self.0.id.clone()
    }
    pub fn len(&self) -> u64 {
        self.0.bytes
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn open(&self) -> ContentReader {
        ContentReader {
            lease: self.clone(),
            position: 0,
        }
    }
    /// Publication/staging recipients discard incomplete output when verification fails.
    pub fn copy_verified(&self, output: &mut dyn Write, cancel: &Cancellation) -> Result<()> {
        self.0
            .stage
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .copy_verified(&self.0.path, output, cancel)
    }
}
pub struct ContentReader {
    lease: ContentLease,
    position: u64,
}
impl Read for ContentReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let count = self
            .lease
            .0
            .stage
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .read_at(&self.lease.0.path, self.position, buffer)?;
        self.position = self
            .position
            .checked_add(count as u64)
            .ok_or_else(|| io::Error::other("Content offset overflow"))?;
        Ok(count)
    }
}
impl Seek for ContentReader {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        let next = match from {
            SeekFrom::Start(value) => i128::from(value),
            SeekFrom::Current(value) => i128::from(self.position) + i128::from(value),
            SeekFrom::End(value) => i128::from(self.lease.len()) + i128::from(value),
        };
        self.position = u64::try_from(next)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "Invalid content seek"))?;
        Ok(self.position)
    }
}
/// Constructible only after every expected digest, size and accepted observation matches.
#[derive(Clone)]
pub struct AcquiredContent {
    lease: ContentLease,
    evidence: IntegrityEvidence,
    observed: DigestSet,
}
impl AcquiredContent {
    /// Attach admission to the lease itself so clones/readers keep retained bytes charged.
    pub(super) fn retain_resources(value: super::runtime::RetainedOutput<Self>) -> Result<Self> {
        let (mut content, mut permit) = value.into_parts();
        let object = Arc::get_mut(&mut content.lease.0)
            .context("Acquisition must attach resources before sharing its lease")?;
        object._reservation = Some(permit.split(super::resources::ResourceRequest {
            scratch_bytes: object.bytes,
            open_files: 1,
            ..super::resources::ResourceRequest::default()
        })?);
        Ok(content)
    }

    pub fn lease(&self) -> &ContentLease {
        &self.lease
    }
    pub fn evidence(&self) -> &IntegrityEvidence {
        &self.evidence
    }
    /// Observed export hashes never replace the independent source assertions in `evidence`.
    pub fn observed_digests(&self) -> &DigestSet {
        &self.observed
    }
}

/// Synchronous byte work belongs in an admitted blocking worker; this has no cache/project writer.
pub fn verify_stream(
    input: &mut dyn Read,
    expected: &ExpectedContent,
    maximum: u64,
    policy: SourceEvidencePolicy,
    initial: InitialObservation,
    cancel: &Cancellation,
) -> Result<AcquiredContent> {
    validate_expectation(expected, maximum, policy, initial)?;
    let path = PortableRelPath::parse("content", PathSyntax::ProjectContent)?;
    let mut stage = MutableStage::empty()?;
    let mut reader = HashingReader {
        input,
        count: 0,
        sha256: sha2::Sha256::new(),
        sha512: sha2::Sha512::new(),
        sha1: sha1::Sha1::new(),
        md5: md5::Md5::new(),
    };
    stage.write(&path, &mut reader, maximum, cancel)?;
    ensure!(
        expected.size.is_none_or(|size| size == reader.count),
        "Source size differs from expected content"
    );
    let bytes = reader.count;
    let address = ContentId::from_sha256(reader.sha256.finalize().into());
    let hashes = vec![
        ExpectedDigest::Md5(reader.md5.finalize().into()),
        ExpectedDigest::Sha1(reader.sha1.finalize().into()),
        ExpectedDigest::Sha256(*address.bytes()),
        ExpectedDigest::Sha512(reader.sha512.finalize().into()),
    ];
    if let Some(digests) = &expected.digests {
        digests.check(&hashes)?;
    }
    ensure!(
        expected
            .accepted_observation
            .as_ref()
            .is_none_or(|prior| prior == &address),
        "Content differs from the accepted observation"
    );
    let observed = DigestSet::new(hashes)?;
    let evidence = match &expected.digests {
        Some(digests) => IntegrityEvidence::MatchedExpected {
            expected: digests.clone(),
            actual: address.clone(),
        },
        None => IntegrityEvidence::ObservedOnly {
            actual: address.clone(),
        },
    };
    let frozen = stage.freeze(
        SnapshotLimits {
            entries: 1,
            depth: 1,
            file_bytes: maximum,
            total_bytes: maximum,
        },
        cancel,
    )?;
    ensure!(
        matches!(frozen.inventory().get(&path), Some(super::snapshot::Observation::File(file)) if file.content == *address.bytes() && file.bytes == bytes),
        "Quarantined bytes changed during verification"
    );
    Ok(AcquiredContent {
        lease: ContentLease(Arc::new(ContentObject {
            stage: Mutex::new(frozen),
            path,
            id: address,
            bytes,
            _reservation: None,
        })),
        evidence,
        observed,
    })
}
pub(super) fn validate_expectation(
    expected: &ExpectedContent,
    maximum: u64,
    policy: SourceEvidencePolicy,
    initial: InitialObservation,
) -> Result<()> {
    ensure!(
        expected.size.is_none_or(|size| size <= maximum),
        "Expected content exceeds acquisition limit"
    );
    if policy == SourceEvidencePolicy::StrongSourceRequired {
        ensure!(
            expected
                .digests
                .as_ref()
                .is_some_and(|set| set.values().iter().any(|digest| matches!(
                    digest.algorithm(),
                    DigestAlgorithm::Sha256 | DigestAlgorithm::Sha512
                ))),
            "Strong source evidence is required; an observed content address is insufficient"
        );
    }
    ensure!(
        expected.digests.is_some()
            || expected.accepted_observation.is_some()
            || initial == InitialObservation::Accepted,
        "Initial content requires an explicit observation decision"
    );
    Ok(())
}
struct HashingReader<'a> {
    input: &'a mut dyn Read,
    count: u64,
    sha256: sha2::Sha256,
    sha512: sha2::Sha512,
    sha1: sha1::Sha1,
    md5: md5::Md5,
}
impl Read for HashingReader<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let count = self.input.read(buffer)?;
        self.count = self
            .count
            .checked_add(count as u64)
            .ok_or_else(|| io::Error::other("Content size overflow"))?;
        let bytes = &buffer[..count];
        self.sha256.update(bytes);
        self.sha512.update(bytes);
        self.sha1.update(bytes);
        self.md5.update(bytes);
        Ok(count)
    }
}
#[cfg(test)]
mod tests;
