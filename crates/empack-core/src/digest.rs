//! Declared source digests and observed content addresses have distinct meanings.
use alloc::{string::String, vec::Vec};
use core::fmt;

/// Supported source digest algorithms, ordered from weaker to stronger evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DigestAlgorithm {
    /// Compatibility evidence only.
    Md5,
    /// Older provider evidence; not collision resistant.
    Sha1,
    /// SHA-256 source evidence.
    Sha256,
    /// SHA-512 source evidence.
    Sha512,
}
impl DigestAlgorithm {
    /// Canonical wire spelling.
    pub fn name(self) -> &'static str {
        match self {
            Self::Md5 => "md5",
            Self::Sha1 => "sha1",
            Self::Sha256 => "sha256",
            Self::Sha512 => "sha512",
        }
    }
}

/// Exactly sized source declaration; parsing does not verify any bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExpectedDigest {
    /// Declared MD5.
    Md5([u8; 16]),
    /// Declared SHA-1.
    Sha1([u8; 20]),
    /// Declared SHA-256.
    Sha256([u8; 32]),
    /// Declared SHA-512.
    Sha512([u8; 64]),
}
impl ExpectedDigest {
    /// Parse a supported algorithm and exact-width hexadecimal value.
    pub fn parse(algorithm: &str, hex: &str) -> Result<Self, DigestError> {
        match algorithm {
            "md5" => decode_hex(hex).map(Self::Md5),
            "sha1" => decode_hex(hex).map(Self::Sha1),
            "sha256" => decode_hex(hex).map(Self::Sha256),
            "sha512" => decode_hex(hex).map(Self::Sha512),
            _ => Err(DigestError::UnsupportedAlgorithm(algorithm.into())),
        }
    }
    /// Source algorithm.
    pub fn algorithm(&self) -> DigestAlgorithm {
        match self {
            Self::Md5(_) => DigestAlgorithm::Md5,
            Self::Sha1(_) => DigestAlgorithm::Sha1,
            Self::Sha256(_) => DigestAlgorithm::Sha256,
            Self::Sha512(_) => DigestAlgorithm::Sha512,
        }
    }
    /// Raw digest bytes.
    pub fn bytes(&self) -> &[u8] {
        match self {
            Self::Md5(v) => v,
            Self::Sha1(v) => v,
            Self::Sha256(v) => v,
            Self::Sha512(v) => v,
        }
    }
    /// Canonical lowercase hexadecimal spelling.
    pub fn hex(&self) -> String {
        const HEX: &[u8] = b"0123456789abcdef";
        let mut output = String::with_capacity(self.bytes().len() * 2);
        for &byte in self.bytes() {
            output.push(HEX[(byte >> 4) as usize] as char);
            output.push(HEX[(byte & 15) as usize] as char);
        }
        output
    }
}
fn decode_hex<const N: usize>(hex: &str) -> Result<[u8; N], DigestError> {
    if hex.len() != N * 2 {
        return Err(DigestError::Malformed);
    }
    let mut bytes = [0; N];
    for (i, pair) in hex.as_bytes().as_chunks::<2>().0.iter().enumerate() {
        let digit = |byte: u8| (byte as char).to_digit(16).ok_or(DigestError::Malformed);
        bytes[i] = ((digit(pair[0])? << 4) | digit(pair[1])?) as u8;
    }
    Ok(bytes)
}

/// Nonempty, ordered declarations with at most one value per algorithm.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DigestSet(Vec<ExpectedDigest>);
impl DigestSet {
    /// Reject unknown algorithms and conflicting declarations; identical duplicates collapse.
    pub fn parse<'a>(
        values: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> Result<Self, DigestError> {
        let mut result: Vec<ExpectedDigest> = Vec::new();
        for (algorithm, hex) in values {
            let digest = ExpectedDigest::parse(algorithm, hex)?;
            if let Some(previous) = result
                .iter()
                .find(|value| value.algorithm() == digest.algorithm())
            {
                if previous != &digest {
                    return Err(DigestError::Conflict(digest.algorithm()));
                }
            } else {
                result.push(digest);
            }
        }
        if result.is_empty() {
            return Err(DigestError::Empty);
        }
        result.sort_by_key(ExpectedDigest::algorithm);
        Ok(Self(result))
    }
    /// Declared values in algorithm order.
    pub fn values(&self) -> &[ExpectedDigest] {
        &self.0
    }
    /// Strongest declared algorithm. Internal content addresses are not considered.
    pub fn strongest(&self) -> DigestAlgorithm {
        self.0.last().unwrap().algorithm()
    }
    /// Every declaration must match an observation with the same algorithm.
    pub fn check(&self, actual: &[ExpectedDigest]) -> Result<(), DigestError> {
        for expected in &self.0 {
            let observations: Vec<_> = actual
                .iter()
                .filter(|value| value.algorithm() == expected.algorithm())
                .collect();
            if observations.len() != 1 || observations[0] != expected {
                return Err(DigestError::Mismatch(expected.algorithm()));
            }
        }
        Ok(())
    }
}

/// SHA-256 of actual bytes. This is a content address, not source authentication.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ContentId([u8; 32]);
impl ContentId {
    /// Record a SHA-256 observation made by an acquisition adapter.
    pub fn from_sha256(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
    /// Return the content address.
    pub fn bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// Evidence data. Publication authority requires independent verification elsewhere.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntegrityEvidence {
    /// Actual bytes matched every source declaration.
    MatchedExpected {
        /// Unchanged source evidence, including weaker algorithms.
        expected: DigestSet,
        /// Internal address of the verified bytes.
        actual: ContentId,
    },
    /// Initial bytes were accepted without an independent source declaration.
    ObservedOnly {
        /// Observed address, without a source assurance claim.
        actual: ContentId,
    },
}

/// Malformed or unsatisfied source integrity contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DigestError {
    /// At least one source digest is required.
    Empty,
    /// Unsupported source assertion must not be silently discarded.
    UnsupportedAlgorithm(String),
    /// Wrong width or non-hexadecimal input.
    Malformed,
    /// Conflicting values for one algorithm.
    Conflict(DigestAlgorithm),
    /// Missing, ambiguous or mismatched actual digest.
    Mismatch(DigestAlgorithm),
}
impl fmt::Display for DigestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("Source requires at least one digest"),
            Self::UnsupportedAlgorithm(name) => {
                write!(f, "Unsupported source digest algorithm: {name}")
            }
            Self::Malformed => f.write_str("Invalid source digest width or hexadecimal value"),
            Self::Conflict(algorithm) => {
                write!(f, "Conflicting source {} digests", algorithm.name())
            }
            Self::Mismatch(algorithm) => write!(f, "Source {} digest mismatch", algorithm.name()),
        }
    }
}
impl core::error::Error for DigestError {}
