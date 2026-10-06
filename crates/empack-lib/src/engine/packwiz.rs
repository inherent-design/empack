//! Packwiz reference trees derived from an expected game inventory, never backend export output.
use super::{
    backend::{BackendDownload, BackendFile},
    content::{InitialObservation, SourceEvidencePolicy, verify_stream},
    documents::validate_download_url,
    layout::CollisionIndex,
    mrpack::AcquiredBuildFile,
};
use crate::application::process_runtime::Cancellation;
use anyhow::{Context, Result, ensure};
use empack_core::{
    digest::{DigestAlgorithm, ExpectedDigest},
    files::FilePermissions,
    identity::{PinSelector, ProviderProjectId},
    inventory::{BuildInventory, ContentOwner, DownloadOrigins, Representation},
    model::{ExpectedContent, LoaderKind, PackMetadata, RuntimeResolution},
    path::{PathSyntax, PortableRelPath},
    projection::BuildTarget,
    requirements::Environments,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

/// Installer UI behavior is part of the recipe, not an inference from side metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallerInteraction {
    /// A graphical installer can present retained optional choices.
    Interactive,
    /// Headless packwiz accepts every optional entry, so choices must be resolved first.
    Headless,
}

/// A verified reference tree, not a complete launcher/server runtime or publication capability.
pub struct PackwizPlan {
    inventory: BuildInventory,
    files: BTreeMap<PortableRelPath, AcquiredBuildFile>,
}
impl PackwizPlan {
    pub fn inventory(&self) -> &BuildInventory {
        &self.inventory
    }
    pub fn files(&self) -> &BTreeMap<PortableRelPath, AcquiredBuildFile> {
        &self.files
    }

    /// Project one selected side. Preserve reference optionality, or require a resolved selection
    /// for semantics packwiz cannot express (optional local bytes and grouped choices).
    pub fn prepare(
        inventory: BuildInventory,
        metadata: &PackMetadata,
        runtime: &RuntimeResolution,
        embedded: &BTreeMap<ContentOwner, AcquiredBuildFile>,
        interaction: InstallerInteraction,
        cancel: &Cancellation,
    ) -> Result<Self> {
        ensure!(
            matches!(
                inventory.target(),
                BuildTarget::Client | BuildTarget::Server
            ),
            "Packwiz bootstrap projection needs a client or server target"
        );
        let mut files = BTreeMap::new();
        let mut collisions = CollisionIndex::default();
        let mut installed = CollisionIndex::default();
        let mut choices = BTreeSet::new();
        collisions.insert_file(&path("pack.toml")?)?;
        collisions.insert_file(&path("index.toml")?)?;
        let mut index = Vec::new();
        for entry in inventory.entries() {
            cancel.check()?;
            let destination = entry.destination.relative();
            installed.insert_file(destination)?;
            let requirement = entry.requirements.uniform()?;
            if let Some(choice) = requirement.choice {
                ensure!(
                    interaction == InstallerInteraction::Interactive,
                    "Resolve optional choices before headless bootstrap execution"
                );
                ensure!(
                    choices.insert(choice.key.as_str()),
                    "Packwiz cannot preserve a grouped optional choice; select it explicitly"
                );
            }
            match &entry.representation {
                Representation::Unacquired { .. } => anyhow::bail!("Incomplete game inventory"),
                Representation::Embedded {
                    content,
                    bytes,
                    permissions,
                } => {
                    ensure!(
                        requirement.choice.is_none(),
                        "Packwiz cannot preserve optional embedded bytes; select them explicitly"
                    );
                    collisions.insert_file(destination)?;
                    let mut file = embedded
                        .get(&entry.owner)
                        .context("Missing packwiz payload lease")?
                        .clone();
                    ensure!(
                        file.content.lease().id() == *content
                            && file.content.lease().len() == *bytes,
                        "Packwiz payload differs from expected inventory"
                    );
                    file.permissions = *permissions;
                    index.push(json!({"file": destination.as_str(), "hash": ExpectedDigest::Sha256(*content.bytes()).hex(), "metafile": false}));
                    files.insert(destination.clone(), file);
                }
                Representation::Download { expected, allowed } => {
                    let digest = reference_digest(expected)?;
                    let name = destination
                        .components()
                        .last()
                        .context("Empty destination")?;
                    let parent = destination
                        .as_str()
                        .rsplit_once('/')
                        .map(|(parent, _)| format!("{parent}/"))
                        .unwrap_or_default();
                    let metadata_path = path(&format!(
                        "{parent}.empack-{}.pw.toml",
                        ExpectedDigest::Sha256(
                            Sha256::digest(destination.as_str().as_bytes()).into()
                        )
                        .hex()
                    ))?;
                    collisions.insert_file(&metadata_path)?;
                    let side = match requirement.environments {
                        Environments::Both => "both",
                        Environments::Client => "client",
                        Environments::Server => "server",
                    };
                    let mut record = json!({"name": name, "filename": name, "side": side,
                        "download": {"hash-format": digest.algorithm().name(), "hash": digest.hex()}});
                    let selection = match allowed {
                        DownloadOrigins::Urls(urls) => {
                            // A wire format with one locator chooses the first approved alternative.
                            for url in urls.as_slice() {
                                validate_download_url(url)?;
                            }
                            record["download"]["url"] = json!(&urls.as_slice()[0]);
                            None
                        }
                        DownloadOrigins::Provider { pin, .. } => {
                            pin.validate()?;
                            let (
                                ProviderProjectId::CurseForge(project),
                                PinSelector::CurseForgeFile(file),
                            ) = (&pin.project, &pin.selection)
                            else {
                                anyhow::bail!("Packwiz needs a URL for this provider reference");
                            };
                            let project = i32::try_from(project.get())
                                .context("CurseForge project exceeds installer integer range")?;
                            let file = i32::try_from(file.get())
                                .context("CurseForge file exceeds installer integer range")?;
                            record["download"]["mode"] = json!("metadata:curseforge");
                            record["update"] =
                                json!({"curseforge": {"project-id": project, "file-id": file}});
                            Some(pin)
                        }
                    };
                    if let Some(choice) = requirement.choice {
                        record["option"] = json!({"optional": true, "default": choice.default_enabled,
                            "description": choice.description.as_deref().unwrap_or("")});
                    }
                    let bytes = toml::to_string(&record)?.into_bytes();
                    // Parse with the shared backend reader and compare semantic facts independently.
                    let parsed = BackendFile::parse(metadata_path.clone(), &bytes)?;
                    ensure!(
                        parsed.destination == entry.destination
                            && parsed.digest == digest
                            && parsed.matches_selection_and_requirements(
                                selection,
                                &entry.requirements
                            )?,
                        "Generated packwiz metadata differs from expected content"
                    );
                    match (&parsed.download, allowed) {
                        (BackendDownload::Url(actual), DownloadOrigins::Urls(urls)) => {
                            ensure!(actual == &urls.as_slice()[0], "Changed download origin")
                        }
                        (BackendDownload::CurseForgeMetadata, DownloadOrigins::Provider { .. }) => {
                        }
                        _ => anyhow::bail!("Generated packwiz origin differs"),
                    }
                    let file = generated(&bytes, cancel)?;
                    index.push(json!({"file": metadata_path.as_str(), "hash": ExpectedDigest::Sha256(*file.content.lease().id().bytes()).hex(), "metafile": true}));
                    files.insert(metadata_path, file);
                }
            }
        }
        // Generated metadata must not occupy another installed payload destination.
        for entry in inventory.entries() {
            if matches!(entry.representation, Representation::Download { .. }) {
                collisions.insert_file(entry.destination.relative())?;
            }
        }
        let index_bytes =
            toml::to_string(&json!({"hash-format": "sha256", "files": index}))?.into_bytes();
        let index_file = generated(&index_bytes, cancel)?;
        let mut versions = BTreeMap::from([("minecraft", runtime.minecraft.as_str())]);
        if runtime.loader != LoaderKind::Vanilla {
            let key = match runtime.loader {
                LoaderKind::Fabric => "fabric",
                LoaderKind::Quilt => "quilt",
                LoaderKind::Forge => "forge",
                LoaderKind::NeoForge => "neoforge",
                LoaderKind::Vanilla => unreachable!(),
            };
            versions.insert(
                key,
                runtime
                    .loader_version
                    .as_ref()
                    .context("Loader needs exact version")?
                    .as_str(),
            );
        }
        let mut pack = json!({"name": metadata.name, "version": metadata.version, "pack-format": "packwiz:1.1.0",
            "index": {"file": "index.toml", "hash-format": "sha256", "hash": ExpectedDigest::Sha256(*index_file.content.lease().id().bytes()).hex()},
            "versions": versions});
        if let Some(author) = &metadata.author {
            pack["author"] = json!(author);
        }
        if let Some(description) = &metadata.description {
            pack["description"] = json!(description);
        }
        let pack_file = generated(toml::to_string(&pack)?.as_bytes(), cancel)?;
        files.insert(path("index.toml")?, index_file);
        files.insert(path("pack.toml")?, pack_file);
        Ok(Self { inventory, files })
    }
}
fn path(value: &str) -> Result<PortableRelPath> {
    Ok(PortableRelPath::parse(value, PathSyntax::ArchiveMember)?)
}
fn generated(bytes: &[u8], cancel: &Cancellation) -> Result<AcquiredBuildFile> {
    Ok(AcquiredBuildFile {
        content: verify_stream(
            &mut &*bytes,
            &ExpectedContent {
                digests: None,
                size: Some(bytes.len() as u64),
                accepted_observation: None,
            },
            16 << 20,
            SourceEvidencePolicy::Compatibility,
            InitialObservation::Accepted,
            cancel,
        )?,
        permissions: FilePermissions {
            readonly: false,
            executable: false,
        },
    })
}
fn reference_digest(expected: &ExpectedContent) -> Result<ExpectedDigest> {
    let digests = expected
        .digests
        .as_ref()
        .context("Packwiz reference requires declared digests or acquired bytes")?;
    if let Some(observation) = &expected.accepted_observation {
        ensure!(
            digests
                .values()
                .iter()
                .any(|digest| digest.algorithm() == DigestAlgorithm::Sha256
                    && digest.bytes() == observation.bytes()),
            "Acquire reference to bind its accepted observation"
        );
    }
    digests
        .values()
        .iter()
        .max_by_key(|digest| digest.algorithm())
        .cloned()
        .context("Missing reference digest")
}

#[cfg(test)]
mod tests;
