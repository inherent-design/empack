//! Portable release projection from resolved author intent and verified bytes.
//! This adapter has no publisher, network client, workspace discovery or backend observer.
use super::*;
use crate::{
    application::process_runtime::Cancellation,
    engine::{
        artifacts::{ArchiveLimits, VerifiedArchive, write_archive},
        documents::DocumentCodec,
        mrpack::{AcquiredBuildFile, LockedFileKey, SourceFile},
        snapshot::SnapshotLimits,
        staging::MutableStage,
    },
};
use empack_core::{
    distribution::Delivery,
    identity::{PinSelector, ProviderProjectId},
    model::{
        AcquisitionSpec, ContentKind, ContentLayer, DistributionArchive, ExpectedContent,
        LoaderKind, ResolvedPin, ResolvedProject,
    },
    requirements::Requirement,
};
use std::{collections::BTreeMap, fs::File};

/// Publisher identity and runtime requirements are explicit inputs, never derived from display text.
pub struct NativeReleaseOptions {
    pub pack: String,
    pub minimum_engine: String,
    pub java_major: u16,
    pub delivery: Delivery,
    /// Exact destination policies; unspecified configuration/world files are seeds.
    pub policies: BTreeMap<PortableRelPath, FilePolicy>,
}
/// Verified immutable assets remain leased through writing and independent archive verification.
pub struct NativeReleasePlan {
    release: DecodedRelease,
    assets: BTreeMap<PortableRelPath, AcquiredBuildFile>,
    expected: BTreeMap<PortableRelPath, FileContent>,
}
impl NativeReleasePlan {
    pub fn release(&self) -> &DecodedRelease {
        &self.release
    }
    /// Immutable asset bytes for embedding the release in a consumer distribution.
    pub fn assets(&self) -> &BTreeMap<PortableRelPath, AcquiredBuildFile> {
        &self.assets
    }
    pub fn archive_inventory(&self) -> &BTreeMap<PortableRelPath, FileContent> {
        &self.expected
    }
    pub fn prepare(
        project: &ResolvedProject,
        acquired: &BTreeMap<LockedFileKey, AcquiredBuildFile>,
        sources: Vec<SourceFile>,
        options: NativeReleaseOptions,
    ) -> Result<Self> {
        let mut files = Vec::new();
        let mut choices = BTreeMap::new();
        let mut assets = BTreeMap::new();
        let mut used = BTreeSet::new();
        for (key, dependency) in &project.lock().dependencies {
            for file in dependency.files.as_slice() {
                let logical = LockedFileKey {
                    dependency: key.clone(),
                    slot: file.slot.clone(),
                };
                let supplied = acquired.get(&logical).with_context(|| {
                    format!(
                        "Native release needs verified bytes for {}:{}",
                        key.as_str(),
                        file.slot.as_str()
                    )
                })?;
                used.insert(logical);
                check_expected(supplied, &file.expected)?;
                let assertions =
                    assertions(&file.expected, file.provenance.declared_digests.as_ref())?;
                if !assertions.is_empty() {
                    DigestSet::parse(
                        assertions
                            .iter()
                            .map(|d| (d.algorithm.as_str(), d.value.as_str())),
                    )?
                    .check(supplied.content.observed_digests().values())?;
                }
                let address = hex_address(supplied.content.lease().id().bytes());
                let asset = format!("assets/{address}");
                let source = file_source(file, &asset)?;
                let embed = options.delivery == Delivery::Bundled
                    || matches!(source, ReleaseSource::Asset { .. });
                if embed {
                    assets.insert(
                        PortableRelPath::parse(&asset, PathSyntax::ArchiveMember)?,
                        asset_file(supplied),
                    );
                }
                for placement in file.placements.as_slice() {
                    let destination = placement.destination.relative();
                    let policy = policy(&options, destination, dependency.kind)?;
                    let layer = layer(placement.layer);
                    files.push(ReleaseFile {
                        key: logical_key(&[
                            "dependency",
                            key.as_str(),
                            file.slot.as_str(),
                            layer_name(layer),
                            destination.as_str(),
                        ])?,
                        destination: destination.as_str().into(),
                        layer,
                        policy,
                        client: participation(&placement.requirements.client, &mut choices)?,
                        server: participation(&placement.requirements.server, &mut choices)?,
                        sha256: address.clone(),
                        bytes: supplied.content.lease().len(),
                        readonly: supplied.permissions.readonly,
                        executable: supplied.permissions.executable,
                        assertions: assertions.clone(),
                        source: source.clone(),
                        asset: if embed && !matches!(source, ReleaseSource::Asset { .. }) {
                            Some(asset.clone())
                        } else {
                            None
                        },
                    });
                }
            }
        }
        ensure!(
            used.len() == acquired.len(),
            "Native release contains an unrelated acquisition"
        );
        for source in sources {
            let destination = source.destination.relative();
            let policy = policy(&options, destination, ContentKind::OtherFile)?;
            let address = hex_address(source.content.lease().id().bytes());
            let asset = format!("assets/{address}");
            assets.insert(
                PortableRelPath::parse(&asset, PathSyntax::ArchiveMember)?,
                AcquiredBuildFile {
                    content: source.content.clone(),
                    permissions: FilePermissions {
                        readonly: false,
                        executable: false,
                    },
                },
            );
            let layer = layer(source.layer);
            files.push(ReleaseFile {
                key: logical_key(&["source", layer_name(layer), destination.as_str()])?,
                destination: destination.as_str().into(),
                layer,
                policy,
                client: participation(&source.requirements.client, &mut choices)?,
                server: participation(&source.requirements.server, &mut choices)?,
                sha256: address,
                bytes: source.content.lease().len(),
                readonly: source.permissions.readonly,
                executable: source.permissions.executable,
                assertions: Vec::new(),
                source: ReleaseSource::Asset { path: asset },
                asset: None,
            });
        }
        Self::finish(project, files, assets, choices, options)
    }
    /// Project one consumer's verified game inventory without acquiring excluded files.
    /// The original lock still supplies provider identity and source assertions.
    pub fn prepare_selected(
        game: &crate::engine::build::materialized::PreparedGameContent,
        options: NativeReleaseOptions,
    ) -> Result<Self> {
        use empack_core::inventory::{ContentOwner, Representation};
        let project = game.project();
        let inventory = game.inventory();
        let content = game.files();
        ensure!(
            inventory.entries().len() == content.len(),
            "Selected release has unrelated content"
        );
        let mut files = Vec::new();
        let mut assets = BTreeMap::new();
        let mut choices = BTreeMap::new();
        for entry in inventory.entries() {
            let destination = entry.destination.relative();
            let supplied = content
                .get(destination)
                .context("Selected release needs verified content")?;
            let Representation::Embedded {
                content: expected,
                bytes,
                permissions,
            } = &entry.representation
            else {
                anyhow::bail!("Selected native release requires acquired content");
            };
            ensure!(
                *expected == supplied.content.lease().id()
                    && *bytes == supplied.content.lease().len()
                    && *permissions == supplied.permissions,
                "Selected release content differs from the verified inventory"
            );
            let address = hex_address(supplied.content.lease().id().bytes());
            let asset = format!("assets/{address}");
            let (key, kind, source, assertions) = match &entry.owner {
                ContentOwner::Dependency { key, slot } => {
                    let dependency = project
                        .lock()
                        .dependencies
                        .get(key)
                        .context("Selected release owner is not in the lock")?;
                    let file = dependency
                        .files
                        .as_slice()
                        .iter()
                        .find(|file| &file.slot == slot)
                        .context("Selected release role is not in the lock")?;
                    ensure!(
                        file.placements
                            .as_slice()
                            .iter()
                            .any(|placement| placement.destination == entry.destination),
                        "Selected release destination is not a locked placement"
                    );
                    check_expected(supplied, &file.expected)?;
                    let assertions =
                        assertions(&file.expected, file.provenance.declared_digests.as_ref())?;
                    if !assertions.is_empty() {
                        DigestSet::parse(
                            assertions
                                .iter()
                                .map(|d| (d.algorithm.as_str(), d.value.as_str())),
                        )?
                        .check(supplied.content.observed_digests().values())?;
                    }
                    (
                        logical_key(&[
                            "dependency",
                            key.as_str(),
                            slot.as_str(),
                            "common",
                            destination.as_str(),
                        ])?,
                        dependency.kind,
                        file_source(file, &asset)?,
                        assertions,
                    )
                }
                ContentOwner::Source(_) => (
                    logical_key(&["source", "common", destination.as_str()])?,
                    ContentKind::OtherFile,
                    ReleaseSource::Asset {
                        path: asset.clone(),
                    },
                    Vec::new(),
                ),
                ContentOwner::Runtime(_) => {
                    anyhow::bail!("Runtime content requires a runtime release recipe")
                }
            };
            let embed = options.delivery == Delivery::Bundled
                || matches!(source, ReleaseSource::Asset { .. });
            if embed {
                assets.insert(
                    PortableRelPath::parse(&asset, PathSyntax::ArchiveMember)?,
                    asset_file(supplied),
                );
            }
            files.push(ReleaseFile {
                key,
                destination: destination.as_str().into(),
                layer: ReleaseLayer::Common,
                policy: policy(&options, destination, kind)?,
                client: participation(&entry.requirements.client, &mut choices)?,
                server: participation(&entry.requirements.server, &mut choices)?,
                sha256: address,
                bytes: *bytes,
                readonly: permissions.readonly,
                executable: permissions.executable,
                assertions,
                asset: (embed && !matches!(source, ReleaseSource::Asset { .. })).then_some(asset),
                source,
            });
        }
        Self::finish(project, files, assets, choices, options)
    }
    fn finish(
        project: &ResolvedProject,
        files: Vec<ReleaseFile>,
        assets: BTreeMap<PortableRelPath, AcquiredBuildFile>,
        choices: BTreeMap<String, ReleaseChoice>,
        options: NativeReleaseOptions,
    ) -> Result<Self> {
        // Reapply stable-locator and strict semantic checks to programmatically supplied models.
        DocumentCodec.encode_lock(project)?;
        let runtime = &project.lock().runtime;
        let version = || {
            runtime
                .loader_version
                .as_ref()
                .map(|v| v.as_str().to_owned())
                .context("Native release requires an exact loader version")
        };
        let loader = match runtime.loader {
            LoaderKind::Vanilla => ReleaseLoader::Vanilla,
            LoaderKind::Fabric => ReleaseLoader::Fabric {
                version: version()?,
            },
            LoaderKind::Quilt => ReleaseLoader::Quilt {
                version: version()?,
            },
            LoaderKind::Forge => ReleaseLoader::Forge {
                version: version()?,
            },
            LoaderKind::NeoForge => ReleaseLoader::NeoForge {
                version: version()?,
            },
        };
        let destinations: BTreeSet<_> = files
            .iter()
            .map(|file| PortableRelPath::parse(&file.destination, PathSyntax::ProjectContent))
            .collect::<std::result::Result<_, _>>()?;
        ensure!(
            options
                .policies
                .keys()
                .all(|path| destinations.contains(path)),
            "File policy names no selected release destination"
        );
        // Check every preserved layer, not only the default side/choice projection.
        let mut collisions = BTreeMap::new();
        for file in &files {
            collisions
                .entry(file.layer)
                .or_insert_with(crate::engine::layout::CollisionIndex::default)
                .insert_file(&PortableRelPath::parse(
                    &file.destination,
                    PathSyntax::ProjectContent,
                )?)?;
        }
        let release = DecodedRelease::encode(ReleaseDocument {
            schema: RELEASE_SCHEMA,
            pack: options.pack,
            version: project.intent().metadata.version.clone(),
            minimum_engine: options.minimum_engine,
            runtime: ReleaseRuntime {
                minecraft: runtime.minecraft.as_str().into(),
                loader,
                java_major: options.java_major,
            },
            choices: choices.into_values().collect(),
            files,
        })?;
        let permissions = FilePermissions {
            readonly: false,
            executable: false,
        };
        let mut expected = BTreeMap::new();
        for (path, lease) in &assets {
            expected.insert(
                path.clone(),
                FileContent {
                    content: lease.content.lease().id(),
                    bytes: lease.content.lease().len(),
                    permissions,
                },
            );
        }
        expected.insert(
            PortableRelPath::parse("release.json", PathSyntax::ArchiveMember)?,
            FileContent {
                content: ContentId::from_sha256(decode_hex(release.id())?),
                bytes: release.bytes().len() as u64,
                permissions,
            },
        );
        Ok(Self {
            release,
            assets,
            expected,
        })
    }
    pub fn write(
        &self,
        candidate: &mut File,
        format: DistributionArchive,
        limits: ArchiveLimits,
        cancel: &Cancellation,
    ) -> Result<VerifiedArchive> {
        ensure!(
            self.expected.len() <= limits.entries,
            "Native release exceeds archive entry budget"
        );
        let mut bytes = 0u64;
        for (path, file) in &self.expected {
            ensure!(
                file.bytes <= limits.file_bytes,
                "Native release asset exceeds file budget"
            );
            ensure!(
                path.as_str().split('/').count() <= limits.depth,
                "Native release exceeds path depth"
            );
            bytes = bytes
                .checked_add(file.bytes)
                .context("Native release size overflow")?;
            ensure!(
                bytes <= limits.total_bytes,
                "Native release exceeds decoded byte budget"
            );
        }
        let mut stage = MutableStage::empty()?;
        for (path, lease) in &self.assets {
            stage.write(
                path,
                &mut lease.content.lease().open(),
                lease.content.lease().len(),
                cancel,
            )?;
        }
        stage.write(
            &PortableRelPath::parse("release.json", PathSyntax::ArchiveMember)?,
            &mut self.release.bytes(),
            self.release.bytes().len() as u64,
            cancel,
        )?;
        let mut frozen = stage.freeze(
            SnapshotLimits {
                entries: limits.entries,
                depth: limits.depth,
                file_bytes: limits.file_bytes,
                total_bytes: limits.total_bytes,
            },
            cancel,
        )?;
        write_archive(
            &mut frozen,
            candidate,
            format,
            &self.expected,
            limits,
            cancel,
        )
    }
}
/// Capture author-owned bytes without observing any external package-manager metadata.
pub(in crate::engine) fn capture(
    workspace: &crate::engine::project::WorkspaceSnapshot,
    cancel: &Cancellation,
) -> Result<NativeReleasePlan> {
    use crate::engine::{
        content::{ContentPool, SourceEvidencePolicy},
        layout::ProjectLayout,
        snapshot::Observation,
    };
    use empack_core::files::ManagedPath;
    let project = workspace.require_resolved()?;
    let native = project
        .intent()
        .distribution
        .native
        .as_ref()
        .context("Native export requires distribution.native settings")?;
    let options = NativeReleaseOptions {
        pack: native.pack_id.clone(),
        minimum_engine: ">=0.6.0-beta".into(),
        java_major: native.java_major,
        delivery: native.delivery,
        policies: native
            .policies
            .iter()
            .map(|(p, policy)| {
                (
                    p.clone(),
                    match policy {
                        empack_core::instance::FilePolicy::Managed => FilePolicy::Managed,
                        empack_core::instance::FilePolicy::Seed => FilePolicy::Seed,
                    },
                )
            })
            .collect(),
    };
    let maximum = workspace
        .observations()
        .entries()
        .values()
        .try_fold(0u64, |sum, entry| {
            sum.checked_add(match entry {
                Observation::File(file) => file.bytes,
                _ => 0,
            })
            .context("Captured release input size overflow")
        })?;
    let mut pool = ContentPool::new(maximum)?;
    let mut occupied = BTreeSet::new();
    let mut acquired = BTreeMap::new();
    for (key, dep) in &project.lock().dependencies {
        for file in dep.files.as_slice() {
            cancel.check()?;
            let mut paths = BTreeSet::new();
            match &file.acquisition {
                AcquisitionSpec::Local(path) => {
                    occupied.insert(path.clone());
                    paths.insert(path.clone());
                }
                AcquisitionSpec::Embedded { archive, .. } => {
                    occupied.insert(archive.clone());
                }
                _ => {}
            }
            for placement in file.placements.as_slice() {
                let path = ProjectLayout::path(&ManagedPath::Content {
                    layer: placement.layer,
                    path: placement.destination.relative().clone(),
                })?;
                occupied.insert(path.clone());
                paths.insert(path);
            }
            let mut selected: Option<AcquiredBuildFile> = None;
            for path in paths {
                match workspace.observations().entries().get(&path) {
                    Some(Observation::File(_)) => {
                        let (content, permissions) = workspace.acquire_file(
                            &path,
                            Some(&file.expected),
                            SourceEvidencePolicy::Compatibility,
                            cancel,
                        )?;
                        let content = pool.insert(content, cancel)?;
                        if let Some(old) = &selected {
                            ensure!(
                                old.content.lease().id() == content.lease().id()
                                    && old.permissions == permissions,
                                "Native release placements disagree"
                            );
                        } else {
                            selected = Some(AcquiredBuildFile {
                                content,
                                permissions,
                            });
                        }
                    }
                    Some(Observation::Absent) | None => {}
                    _ => anyhow::bail!("Native release input is not a regular file"),
                }
            }
            acquired.insert(LockedFileKey { dependency: key.clone(), slot: file.slot.clone() }, selected.with_context(|| format!("Native release needs exact materialized content for {}:{}; run sync --materialize or supply its declared local source", key.as_str(), file.slot.as_str()))?);
        }
    }
    let mut sources = Vec::new();
    for source in workspace.source_entries(cancel)? {
        if occupied.contains(&source.path) {
            continue;
        }
        let (content, permissions) = workspace.acquire_file(
            &source.path,
            None,
            SourceEvidencePolicy::Compatibility,
            cancel,
        )?;
        sources.push(SourceFile {
            label: source.path.as_str().into(),
            destination: source.destination,
            layer: source.layer,
            requirements: empack_core::requirements::Requirements {
                client: if source.layer == ContentLayer::Server {
                    Requirement::Unsupported
                } else {
                    Requirement::Required
                },
                server: if source.layer == ContentLayer::Client {
                    Requirement::Unsupported
                } else {
                    Requirement::Required
                },
            },
            content: pool.insert(content, cancel)?,
            permissions,
        });
    }
    NativeReleasePlan::prepare(&project, &acquired, sources, options)
}

fn hex_address(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn logical_key(parts: &[&str]) -> Result<String> {
    Ok(format!("file-{}", hash(&serde_json::to_vec(parts)?)))
}
fn layer(value: ContentLayer) -> ReleaseLayer {
    match value {
        ContentLayer::Common => ReleaseLayer::Common,
        ContentLayer::CommonOverride => ReleaseLayer::CommonOverride,
        ContentLayer::Client => ReleaseLayer::Client,
        ContentLayer::Server => ReleaseLayer::Server,
    }
}
fn asset_file(file: &AcquiredBuildFile) -> AcquiredBuildFile {
    AcquiredBuildFile {
        content: file.content.clone(),
        permissions: FilePermissions {
            readonly: false,
            executable: false,
        },
    }
}
fn file_source(file: &empack_core::model::ResolvedFile, asset: &str) -> Result<ReleaseSource> {
    Ok(match &file.acquisition {
        AcquisitionSpec::Local(_) | AcquisitionSpec::Embedded { .. } => ReleaseSource::Asset {
            path: asset.to_owned(),
        },
        AcquisitionSpec::Url(urls) => ReleaseSource::Url {
            alternatives: urls.as_slice().to_vec(),
        },
        AcquisitionSpec::Provider {
            pin,
            slot,
            alternatives,
        } => {
            let selected = selection(pin, slot.as_str())?;
            ReleaseSource::Provider {
                provider: selected.provider,
                project: selected.project,
                selection: selected.selection,
                slot: selected.slot,
                alternatives: alternatives.clone(),
            }
        }
        AcquisitionSpec::Manual { pin, instructions } => ReleaseSource::Manual {
            instructions: instructions.clone(),
            selection: pin
                .as_ref()
                .map(|p| selection(p, file.slot.as_str()))
                .transpose()?,
        },
        AcquisitionSpec::ProviderArchiveMember { archive, member } => {
            ReleaseSource::ProviderArchiveMember {
                archive: ReleaseArchiveSource {
                    selection: selection(&archive.pin, archive.slot.as_str())?,
                    alternatives: archive.alternatives.clone(),
                    assertions: assertions_from(&archive.expected),
                    bytes: archive.expected.size,
                    sha256: archive
                        .expected
                        .accepted_observation
                        .as_ref()
                        .map(|id| hex_address(id.bytes())),
                },
                member: member.as_str().into(),
            }
        }
    })
}
fn layer_name(value: ReleaseLayer) -> &'static str {
    match value {
        ReleaseLayer::Common => "common",
        ReleaseLayer::CommonOverride => "common-override",
        ReleaseLayer::Client => "client",
        ReleaseLayer::Server => "server",
    }
}
fn policy(
    options: &NativeReleaseOptions,
    path: &PortableRelPath,
    kind: ContentKind,
) -> Result<FilePolicy> {
    let world = kind == ContentKind::World
        || matches!(
            path.as_str().split('/').next(),
            Some("world" | "world_nether" | "world_the_end" | "saves")
        );
    let policy = options.policies.get(path).copied().unwrap_or(
        if world || kind == ContentKind::Config || path.as_str().starts_with("config/") {
            FilePolicy::Seed
        } else {
            FilePolicy::Managed
        },
    );
    ensure!(
        !world || policy == FilePolicy::Seed,
        "World content cannot be managed replacement data"
    );
    Ok(policy)
}
fn selection(pin: &ResolvedPin, slot: &str) -> Result<ReleaseSelection> {
    pin.validate()?;
    Ok(ReleaseSelection {
        provider: match pin.project {
            ProviderProjectId::Modrinth(_) => ReleaseProvider::Modrinth,
            ProviderProjectId::CurseForge(_) => ReleaseProvider::CurseForge,
        },
        project: pin.project.to_string(),
        selection: match &pin.selection {
            PinSelector::ModrinthVersion(v) => v.as_str().into(),
            PinSelector::CurseForgeFile(v) => v.to_string(),
        },
        slot: slot.into(),
    })
}
fn assertions_from(expected: &ExpectedContent) -> Vec<SourceDigest> {
    expected
        .digests
        .as_ref()
        .map(|set| {
            set.values()
                .iter()
                .map(|d| SourceDigest {
                    algorithm: d.algorithm().name().into(),
                    value: d.hex(),
                })
                .collect()
        })
        .unwrap_or_default()
}
fn assertions(
    expected: &ExpectedContent,
    provenance: Option<&DigestSet>,
) -> Result<Vec<SourceDigest>> {
    let mut all = expected
        .digests
        .as_ref()
        .map(|v| v.values().to_vec())
        .unwrap_or_default();
    if let Some(declared) = provenance {
        all.extend_from_slice(declared.values());
    }
    if all.is_empty() {
        return Ok(Vec::new());
    }
    Ok(DigestSet::new(all)?
        .values()
        .iter()
        .map(|d| SourceDigest {
            algorithm: d.algorithm().name().into(),
            value: d.hex(),
        })
        .collect())
}
fn check_expected(file: &AcquiredBuildFile, expected: &ExpectedContent) -> Result<()> {
    if let Some(digests) = &expected.digests {
        digests.check(file.content.observed_digests().values())?;
    }
    ensure!(
        expected
            .size
            .is_none_or(|n| n == file.content.lease().len()),
        "Release byte count differs from lock"
    );
    ensure!(
        expected
            .accepted_observation
            .as_ref()
            .is_none_or(|id| *id == file.content.lease().id()),
        "Release content differs from accepted lock observation"
    );
    Ok(())
}

fn participation(
    value: &Requirement,
    choices: &mut BTreeMap<String, ReleaseChoice>,
) -> Result<Participation> {
    Ok(match value {
        Requirement::Required => Participation::Required,
        Requirement::Unsupported => Participation::Unsupported,
        Requirement::Optional(choice) => {
            let key = choice.key.as_str().to_owned();
            let definition = ReleaseChoice {
                key: key.clone(),
                alternatives: vec!["disabled".into(), "enabled".into()],
                default: if choice.default_enabled {
                    "enabled"
                } else {
                    "disabled"
                }
                .into(),
                description: choice.description.clone(),
            };
            if let Some(prior) = choices.get(&key) {
                ensure!(
                    prior == &definition,
                    "Optional group has conflicting defaults or descriptions: {key}"
                );
            } else {
                choices.insert(key.clone(), definition);
            }
            Participation::Choice {
                key,
                value: "enabled".into(),
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::content::{InitialObservation, SourceEvidencePolicy, verify_stream};
    use empack_core::{path::InstallDestination, requirements::*};
    use std::io::{Read, Seek};
    fn acquired(mut bytes: &[u8]) -> AcquiredBuildFile {
        AcquiredBuildFile {
            content: verify_stream(
                &mut bytes,
                &ExpectedContent {
                    digests: None,
                    size: None,
                    accepted_observation: None,
                },
                100,
                SourceEvidencePolicy::Compatibility,
                InitialObservation::Accepted,
                &Cancellation::default(),
            )
            .unwrap(),
            permissions: FilePermissions {
                readonly: false,
                executable: false,
            },
        }
    }
    fn options(delivery: Delivery) -> NativeReleaseOptions {
        NativeReleaseOptions {
            pack: "stable-id".into(),
            minimum_engine: ">=0.6.0-beta".into(),
            java_major: 17,
            delivery,
            policies: BTreeMap::new(),
        }
    }
    fn acquisitions(project: &ResolvedProject) -> BTreeMap<LockedFileKey, AcquiredBuildFile> {
        project
            .lock()
            .dependencies
            .iter()
            .flat_map(|(key, dep)| {
                dep.files.as_slice().iter().map(|f| {
                    (
                        LockedFileKey {
                            dependency: key.clone(),
                            slot: f.slot.clone(),
                        },
                        acquired(b"payload"),
                    )
                })
            })
            .collect()
    }
    fn source(bytes: &[u8], layer: ContentLayer) -> SourceFile {
        let file = acquired(bytes);
        SourceFile {
            label: "/host/private/never-publish".into(),
            destination: InstallDestination::parse("config/test.toml").unwrap(),
            layer,
            requirements: Requirements {
                client: Requirement::Required,
                server: if layer == ContentLayer::Client {
                    Requirement::Unsupported
                } else {
                    Requirement::Required
                },
            },
            content: file.content,
            permissions: file.permissions,
        }
    }
    #[test]
    fn native_reference_and_bundled_exports_preserve_choices_layers_and_original_assertions() {
        let project = crate::engine::mrpack::tests::project(true, true);
        let content = acquisitions(&project);
        for delivery in [Delivery::References, Delivery::Bundled] {
            let plan = NativeReleasePlan::prepare(
                &project,
                &content,
                vec![
                    source(b"common", ContentLayer::Common),
                    source(b"client", ContentLayer::Client),
                ],
                options(delivery),
            )
            .unwrap();
            let document = plan.release().document();
            assert_eq!(document.pack, "stable-id");
            assert_eq!(document.choices[0].key, "extra");
            assert_eq!(document.choices[0].default, "disabled");
            assert_eq!(
                document.choices[0].description.as_deref(),
                Some("Extra content")
            );
            assert_eq!(document.files.len(), 5);
            for file in &document.files {
                if matches!(file.source, ReleaseSource::Url { .. }) {
                    assert_eq!(file.assertions.len(), 1);
                    assert_eq!(file.assertions[0].algorithm, "md5");
                    assert_eq!(file.asset.is_some(), delivery == Delivery::Bundled);
                } else {
                    assert_eq!(file.policy, FilePolicy::Seed);
                }
            }
            assert!(!String::from_utf8_lossy(plan.release().bytes()).contains("/host/private"));
            let mut output = tempfile::tempfile().unwrap();
            let verified = plan
                .write(
                    &mut output,
                    DistributionArchive::Zip,
                    ArchiveLimits::default(),
                    &Cancellation::default(),
                )
                .unwrap();
            assert!(!verified.is_empty());
            output.rewind().unwrap();
            let mut archive = zip::ZipArchive::new(output).unwrap();
            assert_eq!(
                (0..archive.len())
                    .filter(|&i| archive.by_index(i).unwrap().is_file())
                    .count(),
                if delivery == Delivery::Bundled { 4 } else { 3 }
            );
            let mut bytes = Vec::new();
            archive
                .by_name("release.json")
                .unwrap()
                .read_to_end(&mut bytes)
                .unwrap();
            assert_eq!(bytes, plan.release().bytes());
        }
    }
    #[test]
    fn export_rejects_missing_wrong_and_unrelated_content_and_conflicting_choices() {
        let project = crate::engine::mrpack::tests::project(true, true);
        let mut content = acquisitions(&project);
        assert!(
            NativeReleasePlan::prepare(
                &project,
                &BTreeMap::new(),
                vec![],
                options(Delivery::References)
            )
            .is_err()
        );
        let key = content.keys().next().unwrap().clone();
        content.insert(key, acquired(b"changed"));
        assert!(
            NativeReleasePlan::prepare(&project, &content, vec![], options(Delivery::Bundled))
                .is_err()
        );
        let mut conflicting = source(b"source", ContentLayer::Client);
        conflicting.requirements.client = Requirement::Optional(OptionalChoice {
            key: ChoiceKey::parse("extra").unwrap(),
            default_enabled: true,
            description: None,
        });
        assert!(
            NativeReleasePlan::prepare(
                &project,
                &acquisitions(&project),
                vec![conflicting],
                options(Delivery::References)
            )
            .is_err()
        );
        let mut option = options(Delivery::References);
        option.policies.insert(
            PortableRelPath::parse("missing", PathSyntax::ProjectContent).unwrap(),
            FilePolicy::Seed,
        );
        assert!(
            NativeReleasePlan::prepare(&project, &acquisitions(&project), vec![], option).is_err()
        );
    }
    #[test]
    fn archive_assertions_are_distinct_from_member_assertions_and_bundled_bytes() {
        let mut document = super::super::tests::document();
        document.files[0].source = ReleaseSource::ProviderArchiveMember {
            archive: ReleaseArchiveSource {
                selection: ReleaseSelection {
                    provider: ReleaseProvider::Modrinth,
                    project: "AANobbMI".into(),
                    selection: "abcdefgh".into(),
                    slot: "world.zip".into(),
                },
                alternatives: vec!["https://example.com/world.zip".into()],
                assertions: vec![SourceDigest {
                    algorithm: "sha1".into(),
                    value: "aa".repeat(20),
                }],
                bytes: Some(100),
                sha256: None,
            },
            member: "world/level.dat".into(),
        };
        document.files[0].asset = Some("assets/member".into());
        let release = DecodedRelease::encode(document.clone()).unwrap();
        assert!(
            release.document().files[0]
                .expected()
                .unwrap()
                .digests
                .is_none()
        );
        assert_eq!(
            release.document().files[0].asset_path(),
            Some("assets/member")
        );
        document.files[0].policy = FilePolicy::Managed;
        assert!(DecodedRelease::encode(document).is_err());
    }
}
