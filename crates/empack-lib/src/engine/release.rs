//! Portable exact release documents. Decoding establishes syntax, never publisher trust.
use anyhow::{Context, Result, ensure};
use empack_core::{
    digest::{ContentId, DigestSet, ExpectedDigest},
    files::{FileContent, FilePermissions},
    path::{PathSyntax, PortableRelPath},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub mod trust;

/// Independent of the author schema and executable version.
pub const RELEASE_SCHEMA: u32 = 1;
/// Bound wire input before parsing or allocating nested values.
pub const MAX_RELEASE_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ReleaseDocument {
    pub schema: u32,
    pub pack: String,
    pub version: String,
    pub minimum_engine: String,
    pub runtime: ReleaseRuntime,
    pub choices: Vec<ReleaseChoice>,
    pub files: Vec<ReleaseFile>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ReleaseRuntime {
    pub minecraft: String,
    pub loader: ReleaseLoader,
    pub java_major: u16,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ReleaseLoader {
    Vanilla,
    Fabric { version: String },
    Quilt { version: String },
    Forge { version: String },
    NeoForge { version: String },
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ReleaseChoice {
    pub key: String,
    pub alternatives: Vec<String>,
    pub default: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Participation {
    Required,
    Unsupported,
    Choice { key: String, value: String },
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum FilePolicy {
    Managed,
    Seed,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ReleaseFile {
    /// Stable logical role, independent of the destination and display name.
    pub key: String,
    pub destination: String,
    pub policy: FilePolicy,
    pub client: Participation,
    pub server: Participation,
    /// Address of exact selected bytes, distinct from original source evidence.
    pub sha256: String,
    pub bytes: u64,
    pub readonly: bool,
    pub executable: bool,
    pub assertions: Vec<SourceDigest>,
    pub source: ReleaseSource,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SourceDigest {
    pub algorithm: String,
    pub value: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ReleaseProvider {
    Modrinth,
    CurseForge,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ReleaseSource {
    /// Relative to immutable release assets, never relative to an author's workspace.
    Asset {
        path: String,
    },
    Url {
        alternatives: Vec<String>,
    },
    Provider {
        provider: ReleaseProvider,
        project: String,
        selection: String,
        slot: String,
        alternatives: Vec<String>,
    },
    Manual {
        instructions: String,
    },
}

/// Exact checked bytes and identity. This value alone does not authorize installation.
#[derive(Debug, Clone)]
pub struct DecodedRelease {
    document: ReleaseDocument,
    bytes: Vec<u8>,
    id: String,
}
impl DecodedRelease {
    /// Read an explicitly selected local snapshot through bounded no-follow handles.
    pub fn read(
        path: &std::path::Path,
        cancel: &crate::application::process_runtime::Cancellation,
    ) -> Result<Self> {
        ensure!(path.is_absolute(), "Release input must be absolute");
        let root = super::snapshot::ProjectReadRoot::open(
            path.parent().context("Release has no parent")?,
        )?;
        let leaf = path
            .file_name()
            .and_then(|v| v.to_str())
            .context("Invalid release filename")?;
        let relative = PortableRelPath::parse(leaf, PathSyntax::ProjectContent)?;
        let limits = super::snapshot::SnapshotLimits {
            file_bytes: MAX_RELEASE_BYTES as u64,
            total_bytes: MAX_RELEASE_BYTES as u64,
            ..Default::default()
        };
        let snapshot = root.capture(&[relative], limits, cancel)?;
        let bytes = super::project::read_document(&root, &snapshot, leaf, cancel)?
            .context("Selected release does not exist")?;
        root.revalidate(&snapshot, cancel)?;
        Self::decode(&bytes)
    }

    pub fn document(&self) -> &ReleaseDocument {
        &self.document
    }
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        ensure!(
            bytes.len() <= MAX_RELEASE_BYTES,
            "Release exceeds document byte limit"
        );
        let document: ReleaseDocument =
            serde_json::from_slice(bytes).context("Invalid release document")?;
        document.validate()?;
        Ok(Self {
            document,
            bytes: bytes.to_vec(),
            id: hash(bytes),
        })
    }
    /// Deterministic writer. Verification always hashes received bytes, not a reserialization.
    pub fn encode(mut document: ReleaseDocument) -> Result<Self> {
        document.validate()?;
        document.choices.sort_by(|a, b| a.key.cmp(&b.key));
        for choice in &mut document.choices {
            choice.alternatives.sort();
        }
        document.files.sort_by(|a, b| a.key.cmp(&b.key));
        for file in &mut document.files {
            file.assertions
                .sort_by(|a, b| a.algorithm.cmp(&b.algorithm));
        }
        Self::decode(&serde_json::to_vec(&document)?)
    }
}
impl ReleaseDocument {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.schema == RELEASE_SCHEMA, "Unsupported release schema");
        identifier(&self.pack)?;
        label(&self.version)?;
        semver::VersionReq::parse(&self.minimum_engine)
            .context("Invalid minimum engine requirement")?;
        identifier(&self.runtime.minecraft)?;
        ensure!(self.runtime.java_major >= 8, "Invalid Java requirement");
        match &self.runtime.loader {
            ReleaseLoader::Vanilla => {}
            ReleaseLoader::Fabric { version }
            | ReleaseLoader::Quilt { version }
            | ReleaseLoader::Forge { version }
            | ReleaseLoader::NeoForge { version } => identifier(version)?,
        }
        let mut keys = BTreeSet::new();
        for choice in &self.choices {
            identifier(&choice.key)?;
            ensure!(keys.insert(&choice.key), "Duplicate release choice");
            let mut values = BTreeSet::new();
            for value in &choice.alternatives {
                identifier(value)?;
                ensure!(values.insert(value), "Duplicate choice alternative");
            }
            ensure!(
                values.contains(&choice.default),
                "Choice default is not an alternative"
            );
        }
        let mut files = BTreeSet::new();
        // Side-disjoint variants may share a destination; selected projection checks collisions.
        for file in &self.files {
            identifier(&file.key)?;
            ensure!(files.insert(&file.key), "Duplicate release file key");
            let path = PortableRelPath::parse(&file.destination, PathSyntax::ProjectContent)?;
            let first = path.components().next().unwrap().to_ascii_lowercase();
            ensure!(
                !matches!(first.as_str(), ".empack" | "empack.yml" | "empack.lock"),
                "Release cannot own instance control files"
            );
            ensure!(
                !matches!(
                    first.as_str(),
                    "saves" | "world" | "world_nether" | "world_the_end"
                ) || file.policy == FilePolicy::Seed,
                "World content must be initial-only seeds"
            );
            file.content()?;
            file.expected()?;
            let assertions = file
                .assertions
                .iter()
                .map(|v| (v.algorithm.as_str(), v.value.as_str()));
            if !file.assertions.is_empty() {
                DigestSet::parse(assertions)?;
            }
            let mut algorithms = BTreeSet::new();
            for assertion in &file.assertions {
                ensure!(
                    algorithms.insert(&assertion.algorithm),
                    "Duplicate source digest algorithm"
                );
            }
            for participation in [&file.client, &file.server] {
                if let Participation::Choice { key, value } = participation {
                    ensure!(self.choices.iter().any(|choice| choice.key == *key && choice.alternatives.contains(value)), "File references unknown choice alternative");
                }
            }
            ensure!(
                file.client != Participation::Unsupported
                    || file.server != Participation::Unsupported,
                "File is unsupported on both sides"
            );
            match &file.source {
                ReleaseSource::Asset { path } => {
                    PortableRelPath::parse(path, PathSyntax::ArchiveMember)?;
                }
                ReleaseSource::Url { alternatives } => {
                    ensure!(!alternatives.is_empty(), "URL content requires a locator");
                    for url in alternatives {
                        https(url)?;
                    }
                }
                ReleaseSource::Provider {
                    provider,
                    project,
                    selection,
                    slot,
                    alternatives,
                } => {
                    identifier(project)?;
                    identifier(selection)?;
                    label(slot)?;
                    if *provider == ReleaseProvider::CurseForge {
                        ensure!(
                            project.parse::<u64>().is_ok_and(|n| n > 0)
                                && selection.parse::<u64>().is_ok_and(|n| n > 0),
                            "CurseForge references require positive numeric identities"
                        );
                    }
                    for url in alternatives {
                        https(url)?;
                    }
                }
                ReleaseSource::Manual { instructions } => label(instructions)?,
            }
        }
        Ok(())
    }
}
impl ReleaseFile {
    pub fn content(&self) -> Result<FileContent> {
        let digest = decode_hex::<32>(&self.sha256)?;
        Ok(FileContent {
            content: ContentId::from_sha256(digest),
            bytes: self.bytes,
            permissions: FilePermissions {
                readonly: self.readonly,
                executable: self.executable,
            },
        })
    }
    /// Acquisition must check both the release address and every original assertion.
    pub fn expected(&self) -> Result<empack_core::model::ExpectedContent> {
        let digests = self
            .assertions
            .iter()
            .map(|v| ExpectedDigest::parse(&v.algorithm, &v.value))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let address = decode_hex(&self.sha256)?;
        for digest in &digests {
            if let ExpectedDigest::Sha256(value) = digest {
                ensure!(
                    *value == address,
                    "Source SHA-256 differs from selected content"
                );
            }
        }
        Ok(empack_core::model::ExpectedContent {
            digests: if digests.is_empty() {
                None
            } else {
                Some(DigestSet::new(digests)?)
            },
            size: Some(self.bytes),
            accepted_observation: Some(ContentId::from_sha256(address)),
        })
    }
}
pub(super) fn identifier(value: &str) -> Result<()> {
    ensure!(
        !value.is_empty()
            && value.len() <= 128
            && value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.+".contains(&b)),
        "Invalid portable identifier"
    );
    Ok(())
}
fn label(value: &str) -> Result<()> {
    ensure!(
        !value.trim().is_empty() && value.len() <= 4096 && !value.chars().any(char::is_control),
        "Invalid release label"
    );
    Ok(())
}
pub(super) fn https(value: &str) -> Result<reqwest::Url> {
    let url = reqwest::Url::parse(value).context("Invalid release URL")?;
    ensure!(
        url.scheme() == "https"
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && url.fragment().is_none(),
        "Release URLs require HTTPS without credentials or fragments"
    );
    Ok(url)
}
pub(super) fn hash(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}
pub(super) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
pub(super) fn decode_hex<const N: usize>(value: &str) -> Result<[u8; N]> {
    ensure!(
        value.len() == N * 2
            && value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "Expected canonical lowercase hexadecimal value"
    );
    let mut result = [0; N];
    for (i, byte) in result.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[i * 2..i * 2 + 2], 16)?;
    }
    Ok(result)
}

#[cfg(test)]
mod tests;
