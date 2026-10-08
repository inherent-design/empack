//! Mrpack planning from normalized intent and exact selections, independent of exporter output.
use super::{
    artifacts::{ArchiveLimits, VerifiedArchive, write_archive},
    content::{AcquiredContent, ContentLease},
    documents::DocumentCodec,
    layout::CollisionIndex,
    snapshot::SnapshotLimits,
    staging::MutableStage,
};
use crate::application::process_runtime::Cancellation;
use anyhow::{Context, Result, ensure};
use empack_core::{
    digest::{ContentId, DigestAlgorithm},
    files::{FileContent, FilePermissions},
    inventory::{
        BuildInventory, ContentOwner, DownloadOrigins, InventoryInput, OptionalPolicy,
        Representation,
    },
    model::{
        AcquisitionSpec, ContentLayer, DependencyKey, DistributionArchive, ExpectedContent,
        FileSlot, LoaderKind, NonEmpty, ResolutionLock, ResolvedProject,
    },
    path::{InstallDestination, PathSyntax, PortableRelPath},
    projection::BuildTarget,
    requirements::{Requirement, Requirements},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
};

mod observed;
pub use observed::{ObservedFile, ObservedFileEvidence};

/// Acquisition is associated with an exact logical file, never a guessed filename.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct LockedFileKey {
    pub dependency: DependencyKey,
    pub slot: FileSlot,
}
/// Verified bytes plus the source's portable output attributes.
#[derive(Clone)]
pub struct AcquiredBuildFile {
    pub content: AcquiredContent,
    pub permissions: FilePermissions,
}
/// Explicitly enumerated source/override input. The snapshot owner accounts for completeness.
pub struct SourceFile {
    pub label: String,
    pub destination: InstallDestination,
    pub layer: ContentLayer,
    pub requirements: Requirements,
    pub permissions: FilePermissions,
    pub content: AcquiredContent,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptionalConversion {
    /// Mrpack has optional participation but no choice key, default or description fields.
    RejectMetadataLoss,
    /// Host explicitly accepted loss of choice metadata; participation remains optional.
    AcknowledgedMetadataLoss,
}
/// Immutable format plan retains all embedded bytes until the candidate is written.
pub struct MrpackPlan {
    resolution: ResolutionLock,
    pub(super) backend_comparisons: Vec<super::backend::BackendDigestComparison>,
    observed: Vec<ObservedFileEvidence>,
    inventory: BuildInventory,
    index: Vec<u8>,
    embedded: BTreeMap<PortableRelPath, ContentLease>,
    expected: BTreeMap<PortableRelPath, FileContent>,
    conversions: Vec<String>,
}
impl MrpackPlan {
    /// Every locked file and placement contributes an obligation. Missing acquisition fails before writing.
    pub fn prepare(
        project: &ResolvedProject,
        acquired: &BTreeMap<LockedFileKey, AcquiredBuildFile>,
        sources: Vec<SourceFile>,
        optional: OptionalConversion,
    ) -> Result<Self> {
        Self::prepare_with_observed(project, acquired, sources, Vec::new(), optional)
    }
    /// Include verified observed content without manufacturing roots or replacing locked intent.
    pub fn prepare_with_observed(
        project: &ResolvedProject,
        acquired: &BTreeMap<LockedFileKey, AcquiredBuildFile>,
        sources: Vec<SourceFile>,
        observed: Vec<ObservedFile>,
        optional: OptionalConversion,
    ) -> Result<Self> {
        // Apply the wire boundary's stable-locator rules even for programmatically built values.
        DocumentCodec.encode_lock(project)?;
        let mut inputs = Vec::new();
        let mut leases = BTreeMap::new();
        let mut used = BTreeSet::new();
        for (key, dependency) in &project.lock().dependencies {
            for file in dependency.files.as_slice() {
                let file_key = LockedFileKey {
                    dependency: key.clone(),
                    slot: file.slot.clone(),
                };
                let supplied = acquired.get(&file_key);
                let acquired = supplied.map(|file| &file.content);
                if let Some(acquired) = acquired {
                    used.insert(file_key);
                    if let Some(expected) = &file.expected.digests {
                        expected.check(acquired.observed_digests().values())?;
                    }
                    ensure!(
                        file.expected
                            .size
                            .is_none_or(|size| size == acquired.lease().len()),
                        "Acquired size differs from locked file"
                    );
                    ensure!(
                        file.expected
                            .accepted_observation
                            .as_ref()
                            .is_none_or(|id| *id == acquired.lease().id()),
                        "Acquired bytes differ from locked observation"
                    );
                } else if let Some(observation) = &file.expected.accepted_observation {
                    ensure!(
                        file.expected.digests.as_ref().is_some_and(|digests| digests
                            .values()
                            .iter()
                            .any(|digest| digest.algorithm() == DigestAlgorithm::Sha256
                                && digest.bytes() == observation.bytes())),
                        "Acquire the file to bind its accepted observation before export"
                    );
                }
                let urls = match &file.acquisition {
                    AcquisitionSpec::Provider { alternatives, .. } => alternatives.clone(),
                    AcquisitionSpec::Url(urls) => urls.as_slice().to_vec(),
                    _ => Vec::new(),
                };
                // Download entries have no portable permission fields. Verified files with
                // nondefault attributes must be embedded so those attributes are not discarded.
                let representation = if !urls.is_empty()
                    && supplied.is_none_or(|file| {
                        !file.permissions.readonly && !file.permissions.executable
                    }) {
                    let digests = acquired
                        .map(AcquiredContent::observed_digests)
                        .or(file.expected.digests.as_ref())
                        .with_context(|| {
                            format!(
                                "Acquire {} / {} before reference export",
                                key.as_str(),
                                file.slot.as_str()
                            )
                        })?;
                    for algorithm in [DigestAlgorithm::Sha1, DigestAlgorithm::Sha512] {
                        ensure!(
                            digests
                                .values()
                                .iter()
                                .any(|digest| digest.algorithm() == algorithm),
                            "Mrpack requires {} for {} / {}; acquire and verify the file first",
                            algorithm.name(),
                            key.as_str(),
                            file.slot.as_str()
                        );
                    }
                    let bytes = acquired
                        .map(|content| content.lease().len())
                        .or(file.expected.size)
                        .context("Mrpack reference requires an exact byte length")?;
                    Representation::Download {
                        expected: ExpectedContent {
                            digests: Some(digests.clone()),
                            size: Some(bytes),
                            accepted_observation: file.expected.accepted_observation.clone(),
                        },
                        allowed: DownloadOrigins::Urls(NonEmpty::new(urls)?),
                    }
                } else {
                    let acquired = acquired.with_context(|| {
                        format!(
                            "Acquire {} / {} before embedding",
                            key.as_str(),
                            file.slot.as_str()
                        )
                    })?;
                    leases.insert(acquired.lease().id(), acquired.lease().clone());
                    Representation::Embedded {
                        content: acquired.lease().id(),
                        bytes: acquired.lease().len(),
                        permissions: supplied.unwrap().permissions,
                    }
                };
                for placement in file.placements.as_slice() {
                    inputs.push(InventoryInput {
                        owner: ContentOwner::Dependency {
                            key: key.clone(),
                            slot: file.slot.clone(),
                        },
                        destination: placement.destination.clone(),
                        layer: placement.layer,
                        requirements: placement.requirements.clone(),
                        representation: representation.clone(),
                    });
                }
            }
        }
        ensure!(
            used.len() == acquired.len(),
            "Acquisition contains a file outside the exact lock"
        );
        for source in sources {
            let lease = source.content.lease();
            leases.insert(lease.id(), lease.clone());
            inputs.push(InventoryInput {
                owner: ContentOwner::Source(source.label),
                destination: source.destination,
                layer: source.layer,
                requirements: source.requirements,
                representation: Representation::Embedded {
                    content: lease.id(),
                    bytes: lease.len(),
                    permissions: source.permissions,
                },
            });
        }
        let mut observed_acquired = BTreeMap::new();
        let mut observed_evidence = Vec::new();
        for file in observed {
            let ContentOwner::Source(label) = &file.input.owner else {
                unreachable!("observed constructor assigns source ownership");
            };
            ensure!(
                observed_acquired
                    .insert(label.clone(), file.acquired)
                    .is_none(),
                "Duplicate observed backend record"
            );
            inputs.push(file.input);
            observed_evidence.push(file.evidence);
        }
        for file in observed_acquired.values() {
            leases.insert(file.content.lease().id(), file.content.lease().clone());
        }
        // The index has one path namespace and no overlay precedence. Materialize downloads
        // that share a destination across layers, then project the effective bytes for each side.
        // A duplicate reference must never be delegated to installer-specific overwrite ordering.
        let mut destinations: BTreeMap<_, Vec<usize>> = BTreeMap::new();
        for (index, input) in inputs.iter().enumerate() {
            destinations
                .entry(input.destination.relative().clone())
                .or_default()
                .push(index);
        }
        let layered: BTreeSet<_> = destinations
            .iter()
            .filter_map(|(path, indices)| {
                (indices.len() > 1
                    && indices.iter().any(|index| {
                        inputs[*index].layer == ContentLayer::CommonOverride
                            || matches!(
                                inputs[*index].representation,
                                Representation::Download { .. }
                            )
                    }))
                .then_some(path.clone())
            })
            .collect();
        for input in &mut inputs {
            if layered.contains(input.destination.relative())
                && matches!(input.representation, Representation::Download { .. })
            {
                let file = match &input.owner {
                    ContentOwner::Dependency { key, slot } => acquired.get(&LockedFileKey {
                        dependency: key.clone(),
                        slot: slot.clone(),
                    }),
                    ContentOwner::Source(label) => observed_acquired.get(label),
                    ContentOwner::Runtime(_) => None,
                }
                .context("Acquire exact bytes to preserve layered mrpack content")?;
                let lease = file.content.lease();
                leases.insert(lease.id(), lease.clone());
                input.representation = Representation::Embedded {
                    content: lease.id(),
                    bytes: lease.len(),
                    permissions: file.permissions,
                };
            }
        }
        let inventory =
            BuildInventory::project(&inputs, BuildTarget::Mrpack, &OptionalPolicy::Preserve)?;
        let mut archive_entries: Vec<_> = inventory
            .entries()
            .iter()
            .filter(|entry| !layered.contains(entry.destination.relative()))
            .cloned()
            .collect();
        for path in &layered {
            let group: Vec<_> = destinations[path]
                .iter()
                .map(|index| inputs[*index].clone())
                .collect();
            for target in [BuildTarget::Client, BuildTarget::Server] {
                let view = BuildInventory::project(&group, target, &OptionalPolicy::Preserve)?;
                archive_entries.extend_from_slice(view.entries());
            }
        }
        let mut embedded = BTreeMap::new();
        let mut expected = BTreeMap::new();
        let mut references = Vec::new();
        let mut reference_paths = CollisionIndex::default();
        let mut member_paths = CollisionIndex::default();
        let mut installation_paths = CollisionIndex::default();
        let mut spelled_destinations = BTreeSet::new();
        let mut conversions = BTreeSet::new();
        for entry in &archive_entries {
            // Exact spellings may intentionally appear in multiple layers, but portable aliases
            // and file/ancestor collisions cannot depend on the installer's host filesystem.
            if spelled_destinations.insert(entry.destination.relative().clone()) {
                installation_paths.insert_file(entry.destination.relative())?;
            }
            match &entry.representation {
                Representation::Unacquired { .. } => {
                    anyhow::bail!("Mrpack inventory contains unacquired content")
                }
                Representation::Download { expected, allowed } => {
                    let DownloadOrigins::Urls(urls) = allowed else {
                        anyhow::bail!("Mrpack needs direct URL references")
                    };
                    let digests = expected
                        .digests
                        .as_ref()
                        .context("Mrpack needs reference digests")?;
                    let bytes = expected.size.context("Mrpack needs exact reference size")?;
                    for algorithm in [DigestAlgorithm::Sha1, DigestAlgorithm::Sha512] {
                        ensure!(
                            digests
                                .values()
                                .iter()
                                .any(|digest| digest.algorithm() == algorithm),
                            "Mrpack reference is missing {}",
                            algorithm.name()
                        );
                    }
                    reference_paths.insert_file(entry.destination.relative())?;
                    let mut hashes = BTreeMap::new();
                    for digest in digests.values() {
                        if matches!(
                            digest.algorithm(),
                            DigestAlgorithm::Sha1 | DigestAlgorithm::Sha512
                        ) {
                            hashes.insert(digest.algorithm().name(), digest.hex());
                        }
                    }
                    references.push(json!({"path":entry.destination.relative().as_str(), "hashes":hashes, "fileSize":bytes, "downloads":urls.as_slice(), "env":{
                        "client":requirement(&entry.requirements.client, optional, &mut conversions)?,
                        "server":requirement(&entry.requirements.server, optional, &mut conversions)?}}));
                }
                Representation::Embedded {
                    content,
                    bytes,
                    permissions,
                } => {
                    let prefix = override_prefix(&entry.requirements)?;
                    let path = PortableRelPath::parse(
                        &format!("{prefix}/{}", entry.destination.relative().as_str()),
                        PathSyntax::ArchiveMember,
                    )?;
                    member_paths.insert_file(&path)?;
                    embedded.insert(
                        path.clone(),
                        leases
                            .get(content)
                            .context("Missing retained embedded input")?
                            .clone(),
                    );
                    expected.insert(
                        path,
                        FileContent {
                            content: content.clone(),
                            bytes: *bytes,
                            permissions: *permissions,
                        },
                    );
                }
            }
        }
        let runtime = &project.lock().runtime;
        let mut dependencies = BTreeMap::from([("minecraft", runtime.minecraft.as_str())]);
        let loader = match runtime.loader {
            LoaderKind::Vanilla => None,
            LoaderKind::Fabric => Some("fabric-loader"),
            LoaderKind::Quilt => Some("quilt-loader"),
            LoaderKind::Forge => Some("forge"),
            LoaderKind::NeoForge => Some("neoforge"),
        };
        if let Some(loader) = loader {
            dependencies.insert(
                loader,
                runtime
                    .loader_version
                    .as_ref()
                    .context("Missing exact loader version")?
                    .as_str(),
            );
        }
        let metadata = &project.intent().metadata;
        let mut index = json!({"formatVersion":1,"game":"minecraft","versionId":metadata.version,"name":metadata.name,"files":references,"dependencies":dependencies});
        if let Some(description) = &metadata.description {
            index["summary"] = Value::String(description.clone());
        }
        let index = serde_json::to_vec_pretty(&index)?;
        ensure!(
            index.len() <= 16 * 1024 * 1024,
            "Mrpack index exceeds 16 MiB limit"
        );
        let index_path = PortableRelPath::parse("modrinth.index.json", PathSyntax::ArchiveMember)?;
        member_paths.insert_file(&index_path)?;
        expected.insert(
            index_path,
            FileContent {
                content: ContentId::from_sha256(Sha256::digest(&index).into()),
                bytes: index.len() as u64,
                permissions: FilePermissions {
                    readonly: false,
                    executable: false,
                },
            },
        );
        Ok(Self {
            resolution: project.lock().clone(),
            backend_comparisons: Vec::new(),
            observed: observed_evidence,
            inventory,
            index,
            embedded,
            expected,
            conversions: conversions.into_iter().collect(),
        })
    }
    pub fn archive_inventory(&self) -> &BTreeMap<PortableRelPath, FileContent> {
        &self.expected
    }
    pub fn inventory(&self) -> &BuildInventory {
        &self.inventory
    }
    /// Original declarations and provenance remain distinct from hashes observed for export.
    pub fn resolution(&self) -> &ResolutionLock {
        &self.resolution
    }
    pub fn backend_comparisons(&self) -> &[super::backend::BackendDigestComparison] {
        &self.backend_comparisons
    }
    pub fn observed(&self) -> &[ObservedFileEvidence] {
        &self.observed
    }
    pub fn conversions(&self) -> &[String] {
        &self.conversions
    }
    /// Produce and independently verify a private container; this grants no publication authority.
    pub fn write(&self, candidate: &mut File, cancel: &Cancellation) -> Result<VerifiedArchive> {
        let mut stage = MutableStage::empty()?;
        for (path, lease) in &self.embedded {
            stage.write(path, &mut lease.open(), lease.len(), cancel)?;
        }
        stage.write(
            &PortableRelPath::parse("modrinth.index.json", PathSyntax::ArchiveMember)?,
            &mut self.index.as_slice(),
            self.index.len() as u64,
            cancel,
        )?;
        let mut frozen = stage.freeze(SnapshotLimits::default(), cancel)?;
        write_archive(
            &mut frozen,
            candidate,
            DistributionArchive::Zip,
            &self.expected,
            ArchiveLimits::default(),
            cancel,
        )
    }
}
fn unsupported_conversion(object: Option<&str>, message: String) -> anyhow::Error {
    let mut diagnostic = super::diagnostics::Diagnostic::new(
        super::diagnostics::DiagnosticCode::UnsupportedConversion,
        super::diagnostics::DiagnosticPhase::Preparation,
    );
    diagnostic.object = object.map(str::to_owned);
    anyhow::Error::new(diagnostic).context(message)
}
fn requirement(
    value: &Requirement,
    policy: OptionalConversion,
    conversions: &mut BTreeSet<String>,
) -> Result<&'static str> {
    Ok(match value {
        Requirement::Required => "required",
        Requirement::Unsupported => "unsupported",
        Requirement::Optional(choice) => {
            ensure!(
                policy == OptionalConversion::AcknowledgedMetadataLoss,
                unsupported_conversion(
                    Some(choice.key.as_str()),
                    format!(
                        "Mrpack cannot encode optional choice metadata for {}; conversion needs acknowledgement",
                        choice.key.as_str()
                    )
                )
            );
            conversions.insert(format!(
                "{}: retain optional participation; omit choice key, default and description",
                choice.key.as_str()
            ));
            "optional"
        }
    })
}
fn override_prefix(requirements: &Requirements) -> Result<&'static str> {
    Ok(match (&requirements.client, &requirements.server) {
        (Requirement::Required, Requirement::Required) => "overrides",
        (Requirement::Required, Requirement::Unsupported) => "client-overrides",
        (Requirement::Unsupported, Requirement::Required) => "server-overrides",
        _ => return Err(unsupported_conversion(None, "Embedded mrpack files cannot express optional participation; select an explicit conversion or a verified download reference".into())),
    })
}

#[cfg(test)]
pub(super) mod tests;
