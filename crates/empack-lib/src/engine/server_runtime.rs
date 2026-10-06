//! Exact vanilla server bytes and launcher checks. Loader installer contracts compose separately.
use super::{
    acquisition::{DownloadRequest, HttpAcquisition, TransferLimits},
    artifacts::{ArchiveLimits, preflight_zip},
    content::{AcquiredContent, InitialObservation, SourceEvidencePolicy},
    documents::validate_download_url,
    mrpack::AcquiredBuildFile,
    resources::ResourceRequest,
    runtime::WorkScope,
};
use crate::application::process_runtime::Cancellation;
use anyhow::{Context, Result, ensure};
use empack_core::{
    digest::{ContentId, DigestSet},
    files::FilePermissions,
    model::{ExpectedContent, LoaderKind, NonEmpty, RuntimeResolution},
    path::{PathSyntax, PortableRelPath},
};
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    io::{Read, Seek},
};

pub mod library;

/// A metadata document bound to the requested Minecraft version and its exact server artifact.
/// Parsing this contract does not grant publication authority or claim bytes were downloaded.
pub struct VanillaServerPlan {
    runtime: RuntimeResolution,
    metadata: ContentId,
    catalog: ContentId,
    url: String,
    expected: ExpectedContent,
    java_major: Option<u16>,
}
#[derive(Deserialize)]
struct VersionMetadata {
    id: String,
    downloads: Downloads,
    #[serde(rename = "javaVersion")]
    java_version: Option<JavaVersion>,
}
#[derive(Deserialize)]
struct JavaVersion {
    #[serde(rename = "majorVersion")]
    major_version: u16,
}
#[derive(Deserialize)]
struct Downloads {
    server: ServerDownload,
}
#[derive(Deserialize)]
struct ServerDownload {
    url: String,
    sha1: String,
    size: u64,
}

#[derive(Deserialize)]
struct VersionManifest {
    versions: Vec<VersionEntry>,
}
#[derive(Deserialize)]
struct VersionEntry {
    id: String,
    url: String,
    sha1: String,
}

impl VanillaServerPlan {
    /// Resolve only through the official catalog; the per-version response must match its
    /// catalog digest before any server URL or byte assertion becomes usable.
    pub async fn resolve(
        transport: &HttpAcquisition,
        scope: &mut WorkScope,
        runtime: RuntimeResolution,
        limits: TransferLimits,
    ) -> Result<Self> {
        ensure!(
            runtime.loader == LoaderKind::Vanilla && runtime.loader_version.is_none(),
            "Vanilla catalog cannot satisfy a loader"
        );
        let metadata_limits = TransferLimits {
            file_bytes: limits.file_bytes.min(16 << 20),
            transfer_bytes: limits.transfer_bytes.min(16 << 20),
            ..limits
        };
        let manifest = transport
            .acquire(
                scope,
                DownloadRequest {
                    alternatives: NonEmpty::new(vec![
                        "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json".into(),
                    ])?,
                    expected: ExpectedContent {
                        digests: None,
                        size: None,
                        accepted_observation: None,
                    },
                    limits: metadata_limits,
                    evidence: SourceEvidencePolicy::Compatibility,
                    // This is an observed HTTPS catalog snapshot, not accepted server byte evidence.
                    initial: InitialObservation::Accepted,
                },
            )
            .await?;
        let catalog = manifest.lease().id();
        let requested = runtime.minecraft.as_str().to_owned();
        let worker = scope.spawn_blocking(
            metadata_work(),
            ResourceRequest::default(),
            move |_| -> Result<VersionEntry> {
                let parsed: VersionManifest = serde_json::from_reader(manifest.lease().open())?;
                let mut entries = parsed
                    .versions
                    .into_iter()
                    .filter(|entry| entry.id == requested);
                let entry = entries
                    .next()
                    .context("Minecraft version is absent from official catalog")?;
                ensure!(
                    entries.next().is_none(),
                    "Catalog repeats the selected version"
                );
                validate_download_url(&entry.url)?;
                Ok(entry)
            },
        )?;
        let entry = scope
            .accept(worker.wait().await?)?
            .transpose()?
            .into_parts()
            .0;
        let metadata = transport
            .acquire(
                scope,
                DownloadRequest {
                    alternatives: NonEmpty::new(vec![entry.url])?,
                    expected: ExpectedContent {
                        digests: Some(DigestSet::parse([("sha1", entry.sha1.as_str())])?),
                        size: None,
                        accepted_observation: None,
                    },
                    limits: metadata_limits,
                    evidence: SourceEvidencePolicy::Compatibility,
                    initial: InitialObservation::RequireEvidence,
                },
            )
            .await?;
        let worker =
            scope.spawn_blocking(metadata_work(), ResourceRequest::default(), move |_| {
                Self::from_metadata(runtime, &metadata, catalog)
            })?;
        let plan = scope
            .accept(worker.wait().await?)?
            .transpose()?
            .into_parts()
            .0;
        Ok(plan)
    }
    pub fn download(
        &self,
        limits: TransferLimits,
        evidence: SourceEvidencePolicy,
    ) -> Result<DownloadRequest> {
        Ok(DownloadRequest {
            alternatives: NonEmpty::new(vec![self.url.clone()])?,
            expected: self.expected.clone(),
            limits,
            evidence,
            initial: InitialObservation::RequireEvidence,
        })
    }
    /// Download and independently inspect the selected runtime before returning its file set.
    pub async fn acquire(
        self,
        transport: &HttpAcquisition,
        scope: &mut WorkScope,
        transfer: TransferLimits,
        archive: ArchiveLimits,
        policy: SourceEvidencePolicy,
    ) -> Result<PreparedServerRuntime> {
        let server = transport
            .acquire(scope, self.download(transfer, policy)?)
            .await?;
        let worker =
            scope.spawn_blocking(metadata_work(), ResourceRequest::default(), move |cancel| {
                self.verify(server, archive, &cancel)
            })?;
        Ok(scope
            .accept(worker.wait().await?)?
            .transpose()?
            .into_parts()
            .0)
    }

    /// The catalog adapter verifies the metadata response against its selected manifest entry.
    /// This boundary checks its semantic identity and retains the exact document address.
    fn from_metadata(
        runtime: RuntimeResolution,
        metadata: &AcquiredContent,
        catalog: ContentId,
    ) -> Result<Self> {
        ensure!(
            runtime.loader == LoaderKind::Vanilla && runtime.loader_version.is_none(),
            "Vanilla runtime cannot satisfy a loader selection"
        );
        ensure!(
            metadata.lease().len() <= 16 << 20,
            "Runtime metadata exceeds limit"
        );
        let parsed: VersionMetadata = serde_json::from_reader(metadata.lease().open())?;
        ensure!(
            parsed.id == runtime.minecraft.as_str(),
            "Server metadata belongs to another Minecraft version"
        );
        validate_download_url(&parsed.downloads.server.url)?;
        ensure!(
            parsed.downloads.server.size > 0,
            "Server artifact has no bytes"
        );
        let java_major = parsed.java_version.map(|value| value.major_version);
        ensure!(
            java_major.is_none_or(|value| value > 0),
            "Invalid Java requirement"
        );
        Ok(Self {
            runtime,
            metadata: metadata.lease().id(),
            catalog,
            url: parsed.downloads.server.url,
            expected: ExpectedContent {
                digests: Some(DigestSet::parse([(
                    "sha1",
                    parsed.downloads.server.sha1.as_str(),
                )])?),
                size: Some(parsed.downloads.server.size),
                accepted_observation: None,
            },
            java_major,
        })
    }
    pub fn url(&self) -> &str {
        &self.url
    }
    pub fn expected(&self) -> &ExpectedContent {
        &self.expected
    }
    pub fn runtime(&self) -> &RuntimeResolution {
        &self.runtime
    }
    /// Run in an admitted worker. Verify the source assertions and bounded launcher metadata;
    /// merely finding a filename or observing installer exit status is insufficient.
    pub fn verify(
        self,
        server: AcquiredContent,
        limits: ArchiveLimits,
        cancel: &Cancellation,
    ) -> Result<PreparedServerRuntime> {
        cancel.check()?;
        ensure!(
            Some(server.lease().len()) == self.expected.size,
            "Server size differs from selected metadata"
        );
        self.expected
            .digests
            .as_ref()
            .context("Server digest is missing")?
            .check(server.observed_digests().values())?;
        let mut jar = JarReader::open(&server, limits, cancel)?;
        let attributes = jar.main_attributes()?;
        let main_class = attributes
            .get("main-class")
            .context("Server JAR has no main class")?;
        ensure!(
            !main_class.is_empty()
                && main_class.split('.').all(|part| !part.is_empty()
                    && part
                        .bytes()
                        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, b'_' | b'$'))),
            "Invalid server main class"
        );
        ensure!(
            attributes
                .get("class-path")
                .is_none_or(|value| value.trim().is_empty()),
            "Vanilla server requires unaccounted external libraries"
        );
        let class_path = format!("{}.class", main_class.replace('.', "/"));
        let mut class = jar
            .archive
            .by_name(&class_path)
            .context("Server JAR lacks its declared main class")?;
        ensure!(
            !class.is_dir() && !class.encrypted(),
            "Main class is not readable"
        );
        let maximum = limits.file_bytes.min(16 << 20);
        ensure!(
            class.size() >= 8 && class.size() <= maximum,
            "Main class exceeds inspection limits"
        );
        let mut header = [0; 8];
        class.read_exact(&mut header)?;
        ensure!(
            header[..4] == [0xca, 0xfe, 0xba, 0xbe],
            "Main class is not a Java class file"
        );
        let count = std::io::copy(&mut class.take(maximum - 8 + 1), &mut std::io::sink())?;
        ensure!(count <= maximum - 8, "Main class exceeds inspection limits");
        cancel.check()?;
        let evidence = ServerRuntimeEvidence {
            metadata: self.metadata,
            catalog: self.catalog,
            expected: self.expected,
            actual: server.lease().id(),
            java_major: self.java_major,
            main_class: main_class.clone(),
            loader: None,
        };
        let files = BTreeMap::from([(
            PortableRelPath::parse("server.jar", PathSyntax::ProjectContent)?,
            AcquiredBuildFile {
                content: server,
                permissions: FilePermissions {
                    readonly: false,
                    executable: false,
                },
            },
        )]);
        Ok(PreparedServerRuntime {
            runtime: self.runtime,
            files,
            evidence,
        })
    }
}
#[derive(Debug, Clone)]
pub struct ServerRuntimeEvidence {
    /// Exact Minecraft version metadata, independently bound to its catalog entry.
    pub metadata: ContentId,
    pub catalog: ContentId,
    /// Original Minecraft server assertions and observed bytes, even when a loader wraps them.
    pub expected: ExpectedContent,
    pub actual: ContentId,
    pub java_major: Option<u16>,
    pub main_class: String,
    pub loader: Option<LoaderRuntimeEvidence>,
}
/// Evidence for the selected additional loader; absent for a vanilla runtime.
#[derive(Debug, Clone)]
pub enum LoaderRuntimeEvidence {
    Libraries(library::LibraryRuntimeEvidence),
}
/// A verified runtime file set, still separate from game content, templates and publication.
#[derive(Clone)]
pub struct PreparedServerRuntime {
    runtime: RuntimeResolution,
    files: BTreeMap<PortableRelPath, AcquiredBuildFile>,
    evidence: ServerRuntimeEvidence,
}
impl PreparedServerRuntime {
    pub fn runtime(&self) -> &RuntimeResolution {
        &self.runtime
    }
    pub fn launcher_main_class(&self) -> &str {
        match &self.evidence.loader {
            Some(LoaderRuntimeEvidence::Libraries(loader)) => &loader.main_class,
            None => &self.evidence.main_class,
        }
    }
    pub fn files(&self) -> &BTreeMap<PortableRelPath, AcquiredBuildFile> {
        &self.files
    }
    pub fn evidence(&self) -> &ServerRuntimeEvidence {
        &self.evidence
    }
}
fn metadata_work() -> ResourceRequest {
    ResourceRequest {
        jobs: 1,
        memory_bytes: 32 << 20,
        open_files: 1,
        ..ResourceRequest::default()
    }
}

/// Inspect an opaque JAR without extracting its members as native filesystem paths.
/// Java resources can differ by case, so archive member names are not native path identities.
struct JarReader {
    archive: zip::ZipArchive<super::content::ContentReader>,
}
impl JarReader {
    fn open(
        content: &AcquiredContent,
        limits: ArchiveLimits,
        cancel: &Cancellation,
    ) -> Result<Self> {
        ensure!(
            content.lease().len() <= limits.compressed_bytes,
            "Runtime JAR exceeds byte limit"
        );
        let mut reader = content.lease().open();
        preflight_zip(&mut reader, content.lease().len(), limits, cancel)?;
        reader.rewind()?;
        Ok(Self {
            archive: zip::ZipArchive::new(reader)?,
        })
    }
    fn main_attributes(&mut self) -> Result<BTreeMap<String, String>> {
        let entry = self
            .archive
            .by_name("META-INF/MANIFEST.MF")
            .context("Runtime JAR lacks a manifest")?;
        ensure!(
            !entry.is_dir() && !entry.encrypted() && entry.size() <= 1 << 20,
            "Runtime manifest is not bounded readable data"
        );
        let mut bytes = Vec::new();
        entry.take((1 << 20) + 1).read_to_end(&mut bytes)?;
        ensure!(bytes.len() <= 1 << 20, "Runtime manifest exceeds limit");
        parse_main_attributes(&bytes)
    }
}
fn parse_main_attributes(bytes: &[u8]) -> Result<BTreeMap<String, String>> {
    // Unfold bytes before decoding: a UTF-8 code point can cross a physical line boundary.
    ensure!(!bytes.contains(&0), "NUL in JAR manifest");
    let mut unfolded: Vec<Vec<u8>> = Vec::new();
    let mut cursor = 0;
    while cursor < bytes.len() {
        let end = bytes[cursor..]
            .iter()
            .position(|ch| matches!(ch, b'\r' | b'\n'))
            .map(|offset| cursor + offset)
            .context("Unterminated JAR manifest line")?;
        let line = &bytes[cursor..end];
        cursor = end + 1;
        if bytes[end] == b'\r' && bytes.get(cursor) == Some(&b'\n') {
            cursor += 1;
        }
        if line.is_empty() {
            break;
        }
        if let Some(value) = line.strip_prefix(b" ") {
            unfolded
                .last_mut()
                .context("Manifest continuation has no header")?
                .extend_from_slice(value);
        } else {
            unfolded.push(line.to_vec());
        }
    }
    let mut fields = BTreeMap::new();
    for header in unfolded {
        let text = std::str::from_utf8(&header)?;
        let (name, value) = text
            .split_once(": ")
            .context("Malformed JAR manifest header")?;
        ensure!(
            !name.is_empty()
                && name
                    .bytes()
                    .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, b'-' | b'_')),
            "Invalid manifest header name"
        );
        ensure!(
            fields
                .insert(name.to_ascii_lowercase(), value.to_owned())
                .is_none(),
            "Duplicate main manifest attribute"
        );
    }
    ensure!(
        fields
            .get("manifest-version")
            .is_some_and(|version| version == "1.0"),
        "Unsupported JAR manifest version"
    );
    Ok(fields)
}

#[cfg(test)]
pub(super) mod tests;
