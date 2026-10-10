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

pub mod producer;
pub(crate) mod signing;
pub mod trust;

/// Independent of the author schema and executable version.
pub const RELEASE_SCHEMA: u32 = 1;
/// Bound wire input before parsing or allocating nested values.
pub const MAX_RELEASE_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ReleaseDocument {
    /// Activation requires explicit root-bound publisher enrollment; this field supplies no keys.
    #[serde(default)]
    pub require_subscription: bool,
    /// Typed Java entry point into required, verified server files; never a host executable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_launch: Option<ReleaseServerLaunch>,
    pub schema: u32,
    pub pack: String,
    pub version: String,
    pub minimum_engine: String,
    pub runtime: ReleaseRuntime,
    pub choices: Vec<ReleaseChoice>,
    pub files: Vec<ReleaseFile>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ReleaseServerLaunch {
    Jar { path: String },
    Arguments { unix: String, windows: String },
}
impl ReleaseServerLaunch {
    fn paths(&self) -> Vec<&str> {
        match self {
            Self::Jar { path } => vec![path],
            Self::Arguments { unix, windows } => vec![unix, windows],
        }
    }
    /// Java is selected locally. Only typed, inventory-bound arguments come from the release.
    pub fn arguments(&self, windows: bool) -> Vec<std::ffi::OsString> {
        match self {
            Self::Jar { path } => vec!["-jar".into(), path.into()],
            Self::Arguments { unix, windows: win } => vec![
                "@user_jvm_args.txt".into(),
                format!("@{}", if windows { win } else { unix }).into(),
            ],
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ReleaseRuntime {
    pub minecraft: String,
    pub loader: ReleaseLoader,
    pub java_major: u16,
}
impl ReleaseRuntime {
    pub(crate) fn resolution(&self) -> Result<empack_core::model::RuntimeResolution> {
        use empack_core::model::{GameVersion, LoaderKind, LoaderVersion, RuntimeResolution};
        let requirements = self;

        let (loader, version) = match &requirements.loader {
            ReleaseLoader::Vanilla => (LoaderKind::Vanilla, None),
            ReleaseLoader::Fabric { version } => (LoaderKind::Fabric, Some(version)),
            ReleaseLoader::Quilt { version } => (LoaderKind::Quilt, Some(version)),
            ReleaseLoader::Forge { version } => (LoaderKind::Forge, Some(version)),
            ReleaseLoader::NeoForge { version } => (LoaderKind::NeoForge, Some(version)),
        };
        Ok(RuntimeResolution {
            minecraft: GameVersion::parse(&requirements.minecraft)?,
            loader,
            loader_version: version
                .map(|value| LoaderVersion::parse(value))
                .transpose()?,
        })
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ReleaseLoader {
    Vanilla,
    Fabric {
        version: String,
    },
    Quilt {
        version: String,
    },
    Forge {
        version: String,
    },
    #[serde(rename = "neoforge")]
    NeoForge {
        version: String,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ReleaseChoice {
    pub key: String,
    pub alternatives: Vec<String>,
    pub default: String,
    pub description: Option<String>,
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
/// Overlay priority is explicit; file order never grants replacement authority.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
pub enum ReleaseLayer {
    Common,
    CommonOverride,
    Client,
    Server,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ReleaseFile {
    /// Stable logical role, independent of the destination and display name.
    pub key: String,
    pub destination: String,
    pub layer: ReleaseLayer,
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
    /// Optional bundled bytes; source attribution remains independent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asset: Option<String>,
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
    #[serde(rename = "curseforge")]
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
    ProviderArchiveMember {
        archive: ReleaseArchiveSource,
        member: String,
    },
    Manual {
        instructions: String,
        selection: Option<ReleaseSelection>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ReleaseSelection {
    pub provider: ReleaseProvider,
    pub project: String,
    pub selection: String,
    pub slot: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ReleaseArchiveSource {
    pub selection: ReleaseSelection,
    pub alternatives: Vec<String>,
    pub assertions: Vec<SourceDigest>,
    pub bytes: Option<u64>,
    pub sha256: Option<String>,
}
impl ReleaseSelection {
    pub(in crate::engine) fn pin(&self) -> Result<empack_core::model::ResolvedPin> {
        use empack_core::identity::*;
        Ok(match self.provider {
            ReleaseProvider::Modrinth => empack_core::model::ResolvedPin {
                project: ProviderProjectId::Modrinth(ModrinthProjectId::parse(&self.project)?),
                selection: PinSelector::ModrinthVersion(ModrinthVersionId::parse(&self.selection)?),
            },
            ReleaseProvider::CurseForge => empack_core::model::ResolvedPin {
                project: ProviderProjectId::CurseForge(CurseForgeProjectId::parse(&self.project)?),
                selection: PinSelector::CurseForgeFile(CurseForgeFileId::parse(&self.selection)?),
            },
        })
    }
    fn validate(&self) -> Result<()> {
        identifier(&self.project)?;
        identifier(&self.selection)?;
        label(&self.slot)?;
        self.pin()?;
        Ok(())
    }
}
impl ReleaseArchiveSource {
    pub fn expected(&self) -> Result<empack_core::model::ExpectedContent> {
        ensure!(
            !self.assertions.is_empty(),
            "Source archive requires original assertions"
        );
        let digests = DigestSet::new(
            self.assertions
                .iter()
                .map(|d| ExpectedDigest::parse(&d.algorithm, &d.value))
                .collect::<std::result::Result<Vec<_>, _>>()?,
        )?;
        ensure!(
            digests.values().len() == self.assertions.len(),
            "Duplicate archive digest algorithm"
        );
        let observed = self.sha256.as_deref().map(decode_hex::<32>).transpose()?;
        if let Some(observed) = observed {
            ensure!(
                digests
                    .values()
                    .iter()
                    .all(|d| !matches!(d, ExpectedDigest::Sha256(value) if *value != observed)),
                "Archive SHA-256 assertion conflicts with address"
            );
        }
        ensure!(self.bytes != Some(0), "Empty source archive");
        Ok(empack_core::model::ExpectedContent {
            digests: Some(digests),
            size: self.bytes,
            accepted_observation: observed.map(ContentId::from_sha256),
        })
    }
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
            if let ReleaseSource::ProviderArchiveMember { archive, .. } = &mut file.source {
                archive
                    .assertions
                    .sort_by(|a, b| a.algorithm.cmp(&b.algorithm));
            }
        }
        Self::decode(&bounded_json(&document, MAX_RELEASE_BYTES)?)
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
            empack_core::requirements::ChoiceKey::parse(&choice.key)?;
            if let Some(description) = &choice.description {
                ensure!(
                    description.len() <= 16 * 1024 && !description.contains('\0'),
                    "Invalid choice description"
                );
            }
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
        if let Some(launch) = &self.server_launch {
            for destination in launch.paths() {
                PortableRelPath::parse(destination, PathSyntax::ProjectContent)?;
                let matching: Vec<_> = self
                    .files
                    .iter()
                    .filter(|file| {
                        file.destination == destination && file.server != Participation::Unsupported
                    })
                    .collect();
                ensure!(
                    matching.len() == 1
                        && matching[0].server == Participation::Required
                        && matching[0].policy == FilePolicy::Managed,
                    "Server launch requires one required managed file at {destination}"
                );
            }
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
            ensure!(
                file.layer != ReleaseLayer::Client || file.server == Participation::Unsupported,
                "Client-layer files cannot participate on servers"
            );
            ensure!(
                file.layer != ReleaseLayer::Server || file.client == Participation::Unsupported,
                "Server-layer files cannot participate on clients"
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
            if let Some(asset) = &file.asset {
                PortableRelPath::parse(asset, PathSyntax::ArchiveMember)?;
            }
            match &file.source {
                ReleaseSource::Asset { path } => {
                    ensure!(
                        file.asset.as_ref().is_none_or(|asset| asset == path),
                        "Authored asset locations disagree"
                    );
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
                    ReleaseSelection {
                        provider: provider.clone(),
                        project: project.clone(),
                        selection: selection.clone(),
                        slot: slot.clone(),
                    }
                    .validate()?;
                    for url in alternatives {
                        https(url)?;
                    }
                }
                ReleaseSource::ProviderArchiveMember { archive, member } => {
                    archive.selection.validate()?;
                    archive.expected()?;
                    PortableRelPath::parse(member, PathSyntax::ArchiveMember)?;
                    for url in &archive.alternatives {
                        https(url)?;
                    }
                    ensure!(
                        file.policy == FilePolicy::Seed,
                        "Provider world members must be seeds"
                    );
                }
                ReleaseSource::Manual {
                    instructions,
                    selection,
                } => {
                    label(instructions)?;
                    if let Some(selection) = selection {
                        selection.validate()?;
                    }
                }
            }
        }
        Ok(())
    }
}
impl ReleaseFile {
    pub fn asset_path(&self) -> Option<&str> {
        self.asset.as_deref().or(match &self.source {
            ReleaseSource::Asset { path } => Some(path.as_str()),
            _ => None,
        })
    }

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

/// Serialization obeys the wire bound while producing bytes, including JSON escaping growth.
fn bounded_json(value: &impl Serialize, maximum: usize) -> Result<Vec<u8>> {
    struct Buffer {
        bytes: Vec<u8>,
        maximum: usize,
    }
    impl std::io::Write for Buffer {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if bytes.len() > self.maximum.saturating_sub(self.bytes.len()) {
                return Err(std::io::Error::other("Release exceeds document byte limit"));
            }
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut buffer = Buffer {
        bytes: Vec::new(),
        maximum,
    };
    serde_json::to_writer(&mut buffer, value)?;
    Ok(buffer.bytes)
}
