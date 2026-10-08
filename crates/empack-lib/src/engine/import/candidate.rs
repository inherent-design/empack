//! Interpret verified source evidence without acquiring publication authority.
use super::*;
use crate::engine::{
    documents::DocumentCodec, layout::ProjectLayout, providers::DependencyRelation,
    resources::AdmissionPermit,
};
use empack_core::{
    files::ManagedPath,
    model::*,
    requirements::{Requirement, Requirements},
};

/// The host chooses durable representation; transient locators never silently enter documents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportPersistence {
    Provider,
    Url,
    Local,
}
#[derive(Debug, Clone)]
pub struct ImportFileDecision {
    pub key: DependencyKey,
    pub kind: ContentKind,
    /// Must preserve source participation. Optional defaults and descriptions are explicit here.
    pub requirements: Requirements,
    pub persistence: ImportPersistence,
    /// Required for provider references, which carry no installation destination.
    /// Declared and embedded destinations cannot be changed by this interpretation step.
    pub provider_destination: Option<InstallDestination>,
}
pub struct ImportCandidateOptions {
    pub metadata: PackMetadata,
    /// Index into the source loader declarations. None selects the sole/primary loader or vanilla.
    pub loader: Option<usize>,
    pub layout: BTreeMap<ContentKind, PortableRelPath>,
    pub distribution: DistributionIntent,
    /// Exactly one decision for each acquired file. One provider's files share one logical key.
    pub files: BTreeMap<ImportContentKey, ImportFileDecision>,
    /// Explicit conversion choice; unrecognized archive members remain in retained source evidence.
    pub exclude_auxiliary_members: bool,
}
/// Coherent candidate documents and their exact byte bindings. This is not a publication proof.
pub struct ImportCandidate {
    project: ResolvedProject,
    content: VerifiedImportContent,
    bindings: BTreeMap<(DependencyKey, FileSlot), ImportContentKey>,
    _index: AdmissionPermit,
    publication_bytes: u64,
}
impl ImportCandidate {
    /// Exact document and placed-payload bytes needed by native preparation.
    pub fn publication_bytes(&self) -> u64 {
        self.publication_bytes
    }
    pub fn project(&self) -> &ResolvedProject {
        &self.project
    }
    pub fn source(&self) -> &VerifiedImportContent {
        &self.content
    }
    pub fn bindings(&self) -> &BTreeMap<(DependencyKey, FileSlot), ImportContentKey> {
        &self.bindings
    }
}

impl VerifiedImportContent {
    /// No network, backend or project writer is available. Failure returns no candidate subset.
    pub fn into_candidate(
        self,
        scope: &mut WorkScope,
        options: ImportCandidateOptions,
    ) -> Result<ImportCandidate> {
        ensure!(
            self.content().keys().eq(options.files.keys()),
            "Import decisions must cover exactly the verified file inventory"
        );
        let source = self.plan().imported();
        ensure!(
            source.auxiliary_members.is_empty() || options.exclude_auxiliary_members,
            "Import contains auxiliary members; an explicit preservation or exclusion decision is required"
        );
        let index = scope.reserve_storage(ResourceRequest {
            memory_bytes: (options.files.len() as u64)
                .checked_mul(16384)
                .context("Import candidate bookkeeping overflow")?,
            ..Default::default()
        })?;
        let runtime = select_runtime(&source.runtime, options.loader)?;
        let mut intent = ProjectIntent {
            metadata: options.metadata,
            runtime: RuntimeIntent {
                minecraft: runtime.minecraft.clone(),
                acceptable_versions: vec![],
                loader: runtime.loader,
                loader_version: runtime.loader_version.clone(),
            },
            roots: BTreeMap::new(),
            layout: options.layout,
            distribution: options.distribution,
            extensions: BTreeMap::from([(
                "empack.import".into(),
                ExtensionValue::Object(BTreeMap::from([
                    (
                        "format".into(),
                        ExtensionValue::Text(
                            match source.format {
                                ImportFormat::Modrinth => "mrpack",
                                ImportFormat::CurseForge => "curseforge-zip",
                            }
                            .into(),
                        ),
                    ),
                    (
                        "source-archive-sha256".into(),
                        ExtensionValue::Text(
                            empack_core::digest::ExpectedDigest::Sha256(
                                *source.source_id().bytes(),
                            )
                            .hex(),
                        ),
                    ),
                    (
                        "excluded-auxiliary-members".into(),
                        ExtensionValue::List(
                            source
                                .auxiliary_members
                                .iter()
                                .map(|path| ExtensionValue::Text(path.as_str().into()))
                                .collect(),
                        ),
                    ),
                ])),
            )]),
        };
        let mut dependencies = BTreeMap::<DependencyKey, LockedDependency>::new();
        let mut bindings = BTreeMap::new();
        let mut collisions = CollisionIndex::default();
        let mut provider_keys = BTreeMap::new();
        let provider_sources: BTreeMap<_, _> = source
            .providers
            .iter()
            .map(|reference| (&reference.selection, reference))
            .collect();
        let provider_files: BTreeMap<_, _> = self
            .plan()
            .providers()
            .records()
            .iter()
            .flat_map(|(pin, record)| {
                record
                    .files
                    .as_slice()
                    .iter()
                    .map(move |file| ((pin, file.filename.as_str()), (record, file)))
            })
            .collect();
        let cancel = scope.cancellation();
        for (content_key, decision) in &options.files {
            cancel.check()?;
            let content = &self.content()[content_key];
            let (declared, provider) = match content_key {
                ImportContentKey::Declared(i) => (Some(&source.files[*i]), None),
                ImportContentKey::Override(i) => (Some(&source.overrides[*i]), None),
                ImportContentKey::Provider { pin, filename } => {
                    let (record, file) = provider_files
                        .get(&(pin, filename.as_str()))
                        .context("Verified provider file has no source record")?;
                    (None, Some((*record, *file)))
                }
            };
            let (requirements, location) = if let Some(file) = declared {
                (&file.requirements, &file.source)
            } else {
                let (record, _) = provider.unwrap();
                let reference = provider_sources
                    .get(&record.pin)
                    .context("Provider selection has no imported reference")?;
                (&reference.requirements, &reference.source)
            };
            preserve_requirements(requirements, &decision.requirements)?;
            let placement = if let Some(file) = declared {
                ensure!(
                    decision.provider_destination.is_none(),
                    "Declared destinations cannot be reassigned"
                );
                Placement {
                    destination: file.destination.clone(),
                    layer: file.layer,
                    requirements: decision.requirements.clone(),
                }
            } else {
                Placement {
                    destination: decision
                        .provider_destination
                        .clone()
                        .context("Provider file requires an explicit installation destination")?,
                    layer: ContentLayer::Common,
                    requirements: decision.requirements.clone(),
                }
            };
            let managed = ManagedPath::Content {
                layer: placement.layer,
                path: placement.destination.relative().clone(),
            };
            let local = ProjectLayout::path(&managed)?;
            ensure!(
                ProjectLayout::classify(&local)? == managed,
                "Imported content occupies a reserved backend document path"
            );
            collisions.insert_file(&local)?;
            let mut conversions = vec![];
            if options.exclude_auxiliary_members && !source.auxiliary_members.is_empty() {
                conversions.push("Explicitly excluded auxiliary archive members".into());
            }
            let (source_intent, identity, selection, acquisition, slot, title, expected) =
                if let Some((record, file)) = provider {
                    ensure!(
                        decision.persistence == ImportPersistence::Provider,
                        "Provider imports must preserve canonical identity"
                    );
                    ensure!(
                        record.kinds.as_slice().contains(&decision.kind),
                        "Chosen kind is not supported by the exact provider selection"
                    );
                    if let Some(previous) =
                        provider_keys.insert(record.pin.project.clone(), decision.key.clone())
                    {
                        ensure!(
                            previous == decision.key,
                            "One provider identity cannot use multiple logical keys"
                        );
                    }
                    let slot = FileSlot::parse(&file.filename)?;
                    // Preserve safe export origins; signed locators stay execution-only.
                    (
                        SourceIntent::Provider(record.pin.project.clone()),
                        ResolvedIdentity::Provider(record.pin.project.clone()),
                        Some(record.pin.clone()),
                        AcquisitionSpec::Provider {
                            pin: record.pin.clone(),
                            slot: slot.clone(),
                            alternatives: file.persistent_alternatives(),
                        },
                        slot,
                        record.project.title.clone(),
                        file.expected.clone(),
                    )
                } else {
                    let file = declared.unwrap();
                    let (source_intent, identity, acquisition) =
                        match (&file.acquisition, decision.persistence) {
                            (ImportedAcquisition::Downloads(urls), ImportPersistence::Url) => {
                                for url in urls {
                                    crate::engine::documents::validate_download_url(url)?;
                                }
                                let urls = NonEmpty::new(urls.clone())?;
                                (
                                    SourceIntent::Url(urls.clone()),
                                    ResolvedIdentity::Url(decision.key.clone()),
                                    AcquisitionSpec::Url(urls),
                                )
                            }
                            (_, ImportPersistence::Local) => {
                                if matches!(file.acquisition, ImportedAcquisition::Downloads(_)) {
                                    conversions.push(
                                        "Explicitly retained downloaded bytes as a local file"
                                            .into(),
                                    );
                                }
                                (
                                    SourceIntent::Local(local.clone()),
                                    ResolvedIdentity::Local(decision.key.clone()),
                                    AcquisitionSpec::Local(local),
                                )
                            }
                            _ => anyhow::bail!(
                                "Import persistence does not represent the original source"
                            ),
                        };
                    let mut expected = file.expected.clone();
                    if expected.digests.is_none() && expected.accepted_observation.is_none() {
                        expected.accepted_observation = Some(content.lease().id());
                    }
                    (
                        source_intent,
                        identity,
                        None,
                        acquisition,
                        FileSlot::parse("content")?,
                        decision.key.as_str().to_owned(),
                        expected,
                    )
                };
            let version = if let Some(pin) = &selection {
                VersionIntent::Exact(pin.selection.clone())
            } else if let Some(digests) = &expected.digests {
                VersionIntent::ContentPinned(digests.clone())
            } else {
                VersionIntent::FollowCompatible
            };
            let file = ResolvedFile {
                slot: slot.clone(),
                acquisition,
                provenance: Provenance {
                    source: match source.format {
                        ImportFormat::Modrinth => "mrpack",
                        ImportFormat::CurseForge => "curseforge-zip",
                    }
                    .into(),
                    location: Some(format!(
                        "{}{}",
                        location.member.as_str(),
                        location.pointer.as_deref().unwrap_or("")
                    )),
                    declared_digests: expected.digests.clone(),
                    conversions,
                },
                expected,
                placements: NonEmpty::new(vec![placement.clone()])?,
            };
            ensure!(
                bindings
                    .insert((decision.key.clone(), slot), content_key.clone())
                    .is_none(),
                "Import logical file slot is repeated"
            );
            if let Some(previous) = dependencies.get_mut(&decision.key) {
                ensure!(
                    selection.is_some()
                        && previous.selected == selection
                        && previous.kind == decision.kind,
                    "Import logical key refers to different sources or kinds"
                );
                let root = intent
                    .roots
                    .get_mut(&decision.key)
                    .context("Missing candidate root")?;
                ensure!(
                    root.requirements == decision.requirements,
                    "Provider files disagree on imported participation"
                );
                previous.files.push(file);
                let PlacementIntent::Explicit(placements) = &mut root.placement else {
                    unreachable!()
                };
                placements.push(placement);
            } else {
                intent.roots.insert(
                    decision.key.clone(),
                    DependencyIntent {
                        source: source_intent,
                        kind: decision.kind,
                        version,
                        placement: PlacementIntent::Explicit(NonEmpty::new(vec![placement])?),
                        requirements: decision.requirements.clone(),
                    },
                );
                dependencies.insert(
                    decision.key.clone(),
                    LockedDependency {
                        title,
                        kind: decision.kind,
                        identity,
                        selected: selection,
                        files: NonEmpty::new(vec![file])?,
                    },
                );
            }
        }
        let mut required_edges = BTreeMap::new();
        for (pin, record) in self.plan().providers().records() {
            cancel.check()?;
            let from = &provider_keys[&pin.project];
            for edge in &record.dependencies {
                if edge.relation != DependencyRelation::Required {
                    continue;
                }
                if let Some(project) = &edge.project
                    && let Some(to) = provider_keys.get(project)
                    && edge.pin.as_ref().is_none_or(|pin| {
                        dependencies[to]
                            .selected
                            .as_ref()
                            .is_some_and(|selected| &selected.selection == pin)
                    })
                {
                    required_edges
                        .entry(from.clone())
                        .or_insert_with(BTreeSet::new)
                        .insert(to.clone());
                }
            }
        }
        cancel.check()?;
        let codec = DocumentCodec;
        let intent_bytes = codec.encode_intent(&intent)?;
        let decoded = codec.decode_intent(&intent_bytes, "import candidate")?;
        let lock = ResolutionLock {
            intent_revision: decoded.semantic_revision(),
            resolver: "empack-import-v0.5".into(),
            coverage: dependencies
                .keys()
                .map(|key| (key.clone(), Coverage::Partial))
                .collect(),
            dependencies,
            required_edges,
            runtime,
        };
        let project = ResolvedProject::validate(intent, lock, decoded.semantic_revision())?;
        // Exercise the durable boundary now, rather than discovering a lossy representation at publication.
        let document_bytes = intent_bytes.len() as u64 + codec.encode_lock(&project)?.len() as u64;
        let publication_bytes =
            bindings
                .iter()
                .try_fold(document_bytes, |sum, ((key, slot), source)| {
                    let file = project.lock().dependencies[key]
                        .files
                        .as_slice()
                        .iter()
                        .find(|file| &file.slot == slot)
                        .context("Import binding has no locked file")?;
                    sum.checked_add(
                        self.content()[source]
                            .lease()
                            .len()
                            .checked_mul(file.placements.as_slice().len() as u64)
                            .context("Import staging size overflow")?,
                    )
                    .context("Import staging size overflow")
                })?;
        Ok(ImportCandidate {
            project,
            content: self,
            bindings,
            _index: index,
            publication_bytes,
        })
    }
}
fn preserve_requirements(source: &ImportedRequirements, chosen: &Requirements) -> Result<()> {
    for (source, chosen) in [
        (source.client, &chosen.client),
        (source.server, &chosen.server),
    ] {
        ensure!(
            matches!(
                (source, chosen),
                (ImportedRequirement::Required, Requirement::Required)
                    | (ImportedRequirement::Unsupported, Requirement::Unsupported)
                    | (ImportedRequirement::Optional, Requirement::Optional(_))
            ),
            "Import decisions cannot change required, optional or unsupported participation"
        );
    }
    Ok(())
}
fn select_runtime(source: &ImportedRuntime, selected: Option<usize>) -> Result<RuntimeResolution> {
    let loader = if let Some(index) = selected {
        Some(
            source
                .loaders
                .get(index)
                .context("Selected imported loader does not exist")?,
        )
    } else if source.loaders.is_empty() {
        None
    } else if source.loaders.len() == 1 {
        source.loaders.first()
    } else {
        let primary: Vec<_> = source
            .loaders
            .iter()
            .filter(|loader| loader.primary)
            .collect();
        ensure!(
            primary.len() == 1,
            "Import requires an explicit runtime selection"
        );
        Some(primary[0])
    };
    Ok(RuntimeResolution {
        minecraft: source.minecraft.clone(),
        loader: loader.map_or(LoaderKind::Vanilla, |loader| loader.kind),
        loader_version: loader.map(|loader| loader.version.clone()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn competing_runtimes_require_a_selection_and_preserve_exact_versions() {
        let mut runtime = ImportedRuntime {
            minecraft: GameVersion::parse("1.21.1").unwrap(),
            loaders: vec![
                ImportedLoader {
                    kind: LoaderKind::Fabric,
                    version: LoaderVersion::parse("0.16.0").unwrap(),
                    primary: false,
                },
                ImportedLoader {
                    kind: LoaderKind::Quilt,
                    version: LoaderVersion::parse("0.27.0").unwrap(),
                    primary: false,
                },
            ],
        };
        assert!(select_runtime(&runtime, None).is_err());
        assert_eq!(
            select_runtime(&runtime, Some(1)).unwrap().loader,
            LoaderKind::Quilt
        );
        runtime.loaders[0].primary = true;
        let selected = select_runtime(&runtime, None).unwrap();
        assert_eq!(selected.loader_version.unwrap().as_str(), "0.16.0");
        runtime.loaders[1].primary = true;
        assert!(select_runtime(&runtime, None).is_err());
        assert!(select_runtime(&runtime, Some(2)).is_err());
    }
}
