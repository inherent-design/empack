//! Canonical provider requests become one resolved dependency group before native mutation.
use super::*;
mod content;
mod world;
use crate::engine::{documents::DocumentCodec, resources::AdmissionPermit};
use anyhow::Context;
pub use content::{
    ProviderContent, ProviderContentChoice, ProviderContentInput, ProviderInputReason,
};
use empack_core::{
    addition::AdditionGroup,
    model::*,
    path::{InstallDestination, PortableRelPath},
    requirements::{Requirement, Requirements},
};
use std::collections::{BTreeMap, BTreeSet};
pub use world::ProviderArchiveAddition;

#[derive(Clone, Default)]
pub enum ProviderFiles {
    /// Use the provider's primary role, including its documented first-file fallback.
    #[default]
    Primary,
    /// Keep the primary file identity while assigning explicit destinations, including renames.
    PrimaryPlaced(NonEmpty<Placement>),
    /// Include every declared file only when the caller explicitly requests that representation.
    All,
    Named(BTreeSet<String>),
    /// Per-file destinations and participation, including companion resource-pack roles.
    Placed(BTreeMap<String, NonEmpty<Placement>>),
}
impl ProviderFiles {
    /// Interpret author placement identically for sync, update, adoption and migration.
    pub(crate) fn from_placement(placement: &PlacementIntent) -> Self {
        match placement {
            PlacementIntent::Automatic => Self::Primary,
            PlacementIntent::Explicit(places) | PlacementIntent::ArchiveRoot(places) => {
                Self::PrimaryPlaced(places.clone())
            }
            PlacementIntent::ByFile(files) => Self::Placed(
                files
                    .iter()
                    .map(|(slot, places)| (slot.as_str().to_owned(), places.clone()))
                    .collect(),
            ),
        }
    }
}
#[derive(Clone)]
pub struct ProviderAddInput {
    /// Authored lookup retained even when execution uses its locked canonical identity.
    pub source_selector: Option<ProviderSelector>,
    /// Authored per-root preference, separate from an operation override.
    pub releases: Option<ReleasePolicy>,
    pub selector: ProjectSelector,
    pub key: Option<DependencyKey>,
    pub kind: Option<ContentKind>,
    /// A requested pin remains exact intent; compatible selection does not manufacture a pin.
    pub pin: Option<PinSelector>,
    pub requirements: Requirements,
    /// Explicit content folder, relative to the common pack root.
    pub folder: Option<PortableRelPath>,
    pub files: ProviderFiles,
}
/// Retains provider evidence and its reservations while the normalized group is in use.
pub struct ProviderAddition {
    project: ResolvedProject,
    group: AdditionGroup,
    evidence: ProviderClosure,
    _documents: AdmissionPermit,
    materialized: crate::engine::dependency_content::DependencyContents,
    _members: Vec<AdmissionPermit>,
}
impl ProviderAddition {
    pub fn project(&self) -> &ResolvedProject {
        &self.project
    }
    pub fn group(&self) -> &AdditionGroup {
        &self.group
    }
    pub fn materialized(&self) -> &crate::engine::dependency_content::DependencyContents {
        &self.materialized
    }
    pub fn evidence(&self) -> &ProviderClosure {
        &self.evidence
    }
}
pub enum ProviderAdditionOutcome {
    Ready(Box<ProviderAddition>),
    /// Exact archive selections still need verified member interpretation before publication.
    Archives(Box<ProviderArchiveAddition>),
    /// Unknown or conflicting required edges remain explicit; no successful subset is normalized.
    NeedsInput(ProviderClosure),
}
impl ProviderCatalog {
    /// Resolve selectors, exact/compatible roots and required closure under one transport budget.
    /// This service has no project writer, payload downloader or publication authority.
    pub async fn resolve_addition(
        &self,
        scope: &mut WorkScope,
        current: &ResolvedProject,
        inputs: NonEmpty<ProviderAddInput>,
        releases: impl Into<Option<ReleasePolicy>> + Send,
        limits: ClosureLimits,
    ) -> Result<ProviderAdditionOutcome> {
        self.resolve_addition_mode(scope, current, inputs, releases.into(), limits, false)
            .await
    }
    /// Runtime changes revalidate implicit libraries while preserving valid exact selections.
    pub(crate) async fn resolve_addition_for_runtime_change(
        &self,
        scope: &mut WorkScope,
        current: &ResolvedProject,
        inputs: NonEmpty<ProviderAddInput>,
        releases: impl Into<Option<ReleasePolicy>> + Send,
        limits: ClosureLimits,
    ) -> Result<ProviderAdditionOutcome> {
        self.resolve_addition_mode(scope, current, inputs, releases.into(), limits, true)
            .await
    }
    async fn resolve_addition_mode(
        &self,
        scope: &mut WorkScope,
        current: &ResolvedProject,
        inputs: NonEmpty<ProviderAddInput>,
        releases: Option<ReleasePolicy>,
        limits: ClosureLimits,
        revalidate_libraries: bool,
    ) -> Result<ProviderAdditionOutcome> {
        ensure!(
            inputs.as_slice().len() <= limits.projects,
            CatalogError::Limit
        );
        let mut games = vec![current.lock().runtime.minecraft.clone()];
        games.extend(current.intent().runtime.acceptable_versions.iter().cloned());
        games.dedup();
        let games = NonEmpty::new(games)?;
        let mut budget = transport::RequestBudget::new(limits.selection.catalog)?;
        let mut roots = Vec::new();
        let mut choices = BTreeMap::new();
        for input in inputs.as_slice() {
            scope.cancellation().check()?;
            let (project, next) = self
                .resolve_selector_budget(
                    scope,
                    input.selector.clone(),
                    limits.selection.catalog,
                    budget,
                )
                .await
                .with_context(|| {
                    format!(
                        "Resolve dependency {} ({})",
                        input
                            .key
                            .as_ref()
                            .map_or("new dependency", |key| key.as_str()),
                        input.selector.value()
                    )
                })?;
            budget = next;
            if let Some(request) = &input.source_selector {
                let bound = current.lock().dependencies.values().any(|locked| {
                    locked.source_selector.as_ref() == Some(request)
                        && locked.identity == ResolvedIdentity::Provider(project.id.clone())
                });
                ensure!(
                    bound
                        || ProjectSelector::parse(request.provider(), request.value())?
                            .matches(&project),
                    "Authored provider selector lacks a matching canonical binding"
                );
            }
            let mut input = input.clone();
            if input.releases.is_none() {
                let key = input
                    .key
                    .as_ref()
                    .filter(|key| current.intent().roots.contains_key(*key))
                    .or_else(|| {
                        current
                            .lock()
                            .dependencies
                            .iter()
                            .find_map(|(key, dependency)| {
                                (dependency.identity
                                    == ResolvedIdentity::Provider(project.id.clone())
                                    && current.intent().roots.contains_key(key))
                                .then_some(key)
                            })
                    });
                input.releases = key
                    .and_then(|key| current.intent().roots.get(key))
                    .and_then(|root| root.releases);
            }
            let kind = input
                .kind
                .or_else(|| {
                    (project.kinds.as_slice().len() == 1).then(|| project.kinds.as_slice()[0])
                })
                .context("Provider content kind requires an explicit choice")?;
            ensure!(
                project.kinds.as_slice().contains(&kind),
                CatalogError::ContentKindMismatch
            );
            let pin = if let Some(pin) = &input.pin {
                let selected = ResolvedPin {
                    project: project.id.clone(),
                    selection: pin.clone(),
                };
                selected.validate()?;
                selected
            } else {
                let (selected, next) = self
                    .resolve_compatible_budget(
                        scope,
                        CompatibleRequest {
                            project: project.id.clone(),
                            kind,
                            game_versions: games.clone(),
                            loader: current.lock().runtime.loader,
                            releases: releases
                                .or(input.releases)
                                .unwrap_or(current.intent().dependency_policy),
                        },
                        limits.selection,
                        budget,
                    )
                    .await.with_context(|| format!(
                        "Select dependency {} (project {}, Minecraft {}, loader {:?}, policy {})",
                        input.key.as_ref().map_or("new dependency", |key| key.as_str()),
                        project.id, current.lock().runtime.minecraft.as_str(), current.lock().runtime.loader,
                        releases.or(input.releases).unwrap_or(current.intent().dependency_policy).as_str()
                    ))?;
                budget = next;
                selected.resolution.pin.clone()
            };
            ensure!(
                choices.insert(project.id.clone(), input.clone()).is_none(),
                "Repeated provider identity in an addition request"
            );
            roots.push(ClosureRoot { pin, kind });
        }
        let closure = self
            .resolve_required_closure_budget(
                scope,
                ClosureRequest {
                    roots: NonEmpty::new(roots)?,
                    game_versions: games,
                    loader: current.lock().runtime.loader,
                    releases: releases.unwrap_or(current.intent().dependency_policy),
                },
                limits,
                budget,
                Some(current),
                revalidate_libraries,
            )
            .await?;
        if !closure.complete_for_required() {
            return Ok(ProviderAdditionOutcome::NeedsInput(closure));
        }
        let count = closure.selections.values().try_fold(0u64, |n, selection| {
            n.checked_add(selection.resolution.files.as_slice().len() as u64)
                .context("Provider file count overflow")
        })?;
        let reservation = scope.reserve_storage(ResourceRequest {
            memory_bytes: count
                .checked_mul(
                    (inputs.as_slice().len() as u64)
                        .checked_mul(512)
                        .and_then(|roots| roots.checked_add(16384))
                        .context("Provider graph size overflow")?,
                )
                .context("Provider document size overflow")?,
            ..Default::default()
        })?;
        if closure.selections.iter().any(|(id, selected)| {
            selected.kind == ContentKind::World
                && (choices.contains_key(id)
                    || !current
                        .lock()
                        .dependencies
                        .values()
                        .any(|dep| dep.identity == ResolvedIdentity::Provider(id.clone())))
        }) {
            return Ok(ProviderAdditionOutcome::Archives(Box::new(
                ProviderArchiveAddition::new(
                    current.clone(),
                    closure,
                    choices,
                    reservation,
                    revalidate_libraries,
                ),
            )));
        }
        let project = normalize(
            current,
            &closure,
            &choices,
            &BTreeMap::new(),
            revalidate_libraries,
        )?;
        let group = AdditionGroup::from_resolved(&project)?;
        Ok(ProviderAdditionOutcome::Ready(Box::new(ProviderAddition {
            project,
            group,
            evidence: closure,
            _documents: reservation,
            materialized: BTreeMap::new(),
            _members: Vec::new(),
        })))
    }
}

fn merge_requirements(values: Vec<&Requirement>) -> Result<Requirement> {
    if values.contains(&&Requirement::Required) {
        return Ok(Requirement::Required);
    }
    let mut result = Requirement::Unsupported;
    for value in values {
        if matches!(value, Requirement::Unsupported) {
            continue;
        }
        ensure!(
            matches!(result, Requirement::Unsupported) || &result == value,
            "A shared dependency needs an explicit conversion of distinct optional choices"
        );
        result = value.clone();
    }
    Ok(result)
}
fn normalize(
    current: &ResolvedProject,
    closure: &ProviderClosure,
    roots: &BTreeMap<ProviderProjectId, ProviderAddInput>,
    worlds: &BTreeMap<ProviderProjectId, world::WorldSelection>,
    revalidate_libraries: bool,
) -> Result<ResolvedProject> {
    let mut intent = current.intent().clone();
    intent.roots.clear();
    let mut keys = BTreeMap::new();
    let mut labels: BTreeMap<_, _> = current
        .lock()
        .dependencies
        .iter()
        .map(|(key, dependency)| (key.clone(), dependency.identity.clone()))
        .collect();
    // Reserve all explicit labels first, regardless of provider traversal order.
    for (id, root) in roots {
        if let Some(key) = &root.key {
            let identity = ResolvedIdentity::Provider(id.clone());
            ensure!(
                labels.get(key).is_none_or(|owner| owner == &identity),
                "Explicit dependency key belongs to another identity"
            );
            labels.insert(key.clone(), identity);
        }
    }
    let mut origins = BTreeMap::new();
    for (id, selected) in &closure.selections {
        let identity = ResolvedIdentity::Provider(id.clone());
        let explicit = roots.get(id).and_then(|root| root.key.clone());
        let retained = current
            .lock()
            .dependencies
            .iter()
            .find(|(_, dependency)| dependency.identity == identity)
            .map(|(key, _)| key.clone());
        let mut key = explicit
            .or(retained)
            .map(Ok)
            .unwrap_or_else(|| DependencyKey::parse(&selected.resolution.project.slug))?;
        if labels.get(&key).is_some_and(|owner| owner != &identity) {
            let base = format!("{}-{id}", selected.resolution.project.slug);
            key = DependencyKey::parse(&base)?;
            let mut suffix = 2usize;
            while labels.get(&key).is_some_and(|owner| owner != &identity) {
                key = DependencyKey::parse(&format!("{base}-{suffix}"))?;
                suffix = suffix
                    .checked_add(1)
                    .context("Dependency label space exhausted")?;
            }
        }
        labels.insert(key.clone(), identity);
        keys.insert(id.clone(), key);
        origins.insert(
            id.clone(),
            if roots.contains_key(id) {
                BTreeSet::from([id.clone()])
            } else {
                BTreeSet::new()
            },
        );
    }
    // Track root reachability before combining choices, so a required root dominates
    // optional roots regardless of traversal order, including through cycles.
    let mut pending: Vec<_> = roots.keys().cloned().collect();
    while let Some(id) = pending.pop() {
        let from = &closure.selections[&id].resolution.pin;
        let incoming = origins[&id].clone();
        for required in closure.required_edges.get(from).into_iter().flatten() {
            let existing = origins
                .get_mut(&required.project)
                .context("Required provider identity was not resolved")?;
            let count = existing.len();
            existing.extend(incoming.iter().cloned());
            if existing.len() != count {
                pending.push(required.project.clone());
            }
        }
    }
    let participation: BTreeMap<_, _> = origins
        .iter()
        .map(|(id, origins)| {
            Ok::<_, anyhow::Error>((
                id.clone(),
                Requirements {
                    client: merge_requirements(
                        origins
                            .iter()
                            .map(|root| &roots[root].requirements.client)
                            .collect(),
                    )?,
                    server: merge_requirements(
                        origins
                            .iter()
                            .map(|root| &roots[root].requirements.server)
                            .collect(),
                    )?,
                },
            ))
        })
        .collect::<Result<_>>()?;
    let mut dependencies = BTreeMap::new();
    let mut coverage = BTreeMap::new();
    let mut edges = BTreeMap::new();
    for (id, selected) in &closure.selections {
        let key = &keys[id];
        let root = roots.get(id);
        let requirements = &participation[id];
        ensure!(
            !matches!(
                (&requirements.client, &requirements.server),
                (Requirement::Unsupported, Requirement::Unsupported)
            ),
            "Dependency has no participating environment"
        );
        if let Some(root) = root {
            ensure!(
                &root.requirements == requirements,
                "Required closure conflicts with an explicit root's environment"
            );
        }
        if root.is_none()
            && let Some(existing) =
                current.lock().dependencies.values().find(|dependency| {
                    dependency.identity == ResolvedIdentity::Provider(id.clone())
                })
            && (!revalidate_libraries
                || existing.selected.as_ref() == Some(&selected.resolution.pin))
        {
            ensure!(
                existing.kind == selected.kind
                    && existing.selected.as_ref() == Some(&selected.resolution.pin),
                "A required selection conflicts with retained content; update it explicitly"
            );
            let mut participation = Vec::new();
            for file in existing.files.as_slice() {
                // Keep every original assertion; matching refreshed records only establishes
                // file role and locator correspondence, not permission to change stored bytes.
                let (slot, expected) = match &file.acquisition {
                    AcquisitionSpec::Provider { slot, .. } => (slot, &file.expected),
                    AcquisitionSpec::ProviderArchiveMember { archive, .. } => {
                        (&archive.slot, &archive.expected)
                    }
                    _ => (&file.slot, &file.expected),
                };
                let provider_file = selected.resolution.file_for_slot(slot, expected)?;
                if !matches!(
                    provider_file.role.as_deref(),
                    Some("required-resource-pack" | "optional-resource-pack")
                ) {
                    participation.extend(
                        file.placements
                            .as_slice()
                            .iter()
                            .map(|placement| &placement.requirements),
                    );
                }
            }
            // A dependency can use distinct main files on each side. Companions retain their
            // narrower requirements and cannot stand in for absent main content.
            ensure!(
                [(&requirements.client, true), (&requirements.server, false)]
                    .into_iter()
                    .all(|(needed, client)| {
                        matches!(needed, Requirement::Unsupported)
                            || participation.iter().any(|requirements| {
                                let actual = if client {
                                    &requirements.client
                                } else {
                                    &requirements.server
                                };
                                actual == needed || matches!(actual, Requirement::Required)
                            })
                    }),
                "Retained dependency participation needs an explicit change"
            );
            let mut retained = existing.clone();
            for assertion in incompatibilities(current, &selected.resolution) {
                if !retained.incompatibilities.contains(&assertion) {
                    retained.incompatibilities.push(assertion);
                }
            }
            dependencies.insert(key.clone(), retained);
            coverage.insert(key.clone(), selected.resolution.coverage);
            if let Some(required) = closure.required_edges.get(&selected.resolution.pin) {
                edges.insert(
                    key.clone(),
                    required
                        .iter()
                        .map(|pin| keys[&pin.project].clone())
                        .collect(),
                );
            }
            continue;
        }
        let files = if selected.kind == ContentKind::World {
            worlds
                .get(id)
                .context("Provider world still requires archive interpretation")?
                .files
                .as_slice()
                .to_vec()
        } else {
            let folder = root
                .and_then(|root| root.folder.as_ref())
                .map(|path| path.as_str())
                .or_else(|| intent.content_folder(selected.kind));
            let declared = selected.resolution.files.as_slice();
            let policy = root
                .map(|root| &root.files)
                .unwrap_or(&ProviderFiles::Primary);
            ensure!(
                declared.iter().all(|file| file.role.is_none())
                    || matches!(policy, ProviderFiles::Placed(_)),
                "Companion provider files require explicit per-file placement and participation for {} (project {}): {}; bind primary and each required companion role in empack.bindings.yml",
                key.as_str(),
                id,
                declared
                    .iter()
                    .map(|file| format!(
                        "{} ({})",
                        file.filename,
                        file.role.as_deref().unwrap_or("primary")
                    ))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            let mut chosen = Vec::new();
            match policy {
                ProviderFiles::Primary | ProviderFiles::PrimaryPlaced(_) => {
                    chosen.push((
                        "primary".to_owned(),
                        selected.resolution.file_for_selector("primary")?,
                    ));
                }
                ProviderFiles::All => {
                    chosen.extend(declared.iter().map(|f| (f.filename.clone(), f)))
                }
                ProviderFiles::Placed(placements) => {
                    ensure!(!placements.is_empty(), "No provider file roles selected");
                    for role in placements.keys() {
                        chosen.push((role.clone(), selected.resolution.file_for_selector(role)?));
                    }
                }
                ProviderFiles::Named(names) => {
                    ensure!(!names.is_empty(), "No provider files selected");
                    for name in names {
                        let file = declared
                            .iter()
                            .find(|file| &file.filename == name)
                            .context("Selected provider file is missing")?;
                        chosen.push((name.clone(), file));
                    }
                }
            }
            let mut unique = BTreeSet::new();
            ensure!(
                chosen.iter().all(|(_, file)| unique.insert(&file.filename)),
                "Multiple file roles select the same provider file"
            );
            ensure!(
                declared
                    .iter()
                    .filter(|file| file.role.as_deref() == Some("required-resource-pack"))
                    .all(|file| chosen
                        .iter()
                        .any(|(_, selected)| selected.filename == file.filename)),
                "A required companion resource pack cannot be omitted"
            );
            chosen.into_iter().map(|(role, file)| {
                let slot = FileSlot::parse(&role)?;
                let placements = if let ProviderFiles::Placed(placements) = policy {
                    placements[&role].clone()
                } else if let ProviderFiles::PrimaryPlaced(placements) = policy {
                    placements.clone()
                } else {
                    NonEmpty::new(vec![Placement {
                        destination: InstallDestination::parse(&format!("{}/{}",
                            folder.context("This content kind requires an explicit destination folder")?, file.filename))?,
                        layer: ContentLayer::Common,
                        requirements: requirements.clone(),
                    }])?
                };
                Ok::<_, anyhow::Error>(ResolvedFile {
                    slot: slot.clone(),
                    expected: file.expected.clone(),
                    acquisition: AcquisitionSpec::Provider {
                        pin: selected.resolution.pin.clone(),
                        slot: FileSlot::parse(&file.filename)?,
                        alternatives: file.persistent_alternatives(),
                    },
                    provenance: Provenance {
                        source: "provider-catalog".into(),
                        location: Some(id.to_string()),
                        declared_digests: file.expected.digests.clone(),
                        conversions: file.role.as_ref().map(|role| format!("Explicit placement and participation for provider file role: {role}")).into_iter().collect(),
                    },
                    placements,
                })
            })
            .collect::<Result<Vec<_>>>()?
        };
        let source = root.map(|root| {
            current
                .intent()
                .roots
                .get(key)
                .filter(|existing| {
                    existing
                        .source
                        .canonical_project(current.lock().dependencies.get(key))
                        == Some(id)
                })
                .map(|existing| existing.source.clone())
                .unwrap_or_else(|| {
                    root.source_selector
                        .clone()
                        .or_else(|| root.selector.authored_selector())
                        .map(SourceIntent::ProviderSelector)
                        .unwrap_or_else(|| SourceIntent::Provider(id.clone()))
                })
        });
        let dependency = LockedDependency {
            incompatibilities: incompatibilities(current, &selected.resolution),
            source_selector: match &source {
                Some(source) => source.selector().cloned(),
                None => current
                    .lock()
                    .dependencies
                    .get(key)
                    .and_then(|locked| locked.source_selector.clone()),
            },
            title: selected.resolution.project.title.clone(),
            kind: selected.kind,
            identity: ResolvedIdentity::Provider(id.clone()),
            selected: Some(selected.resolution.pin.clone()),
            files: NonEmpty::new(files)?,
        };
        if let Some(root) = root {
            intent.roots.insert(
                key.clone(),
                DependencyIntent {
                    releases: if root.pin.is_none() {
                        root.releases
                    } else {
                        None
                    },
                    source: source.expect("Explicit provider root has authored source"),
                    kind: selected.kind,
                    selection: root
                        .pin
                        .clone()
                        .map(DependencySelection::ProviderPin)
                        .unwrap_or(DependencySelection::Automatic),
                    placement: if selected.kind == ContentKind::World {
                        PlacementIntent::ArchiveRoot(worlds[id].roots.clone())
                    } else if root.folder.is_some() || !matches!(root.files, ProviderFiles::Primary)
                    {
                        PlacementIntent::ByFile(
                            dependency
                                .files
                                .as_slice()
                                .iter()
                                .map(|file| (file.slot.clone(), file.placements.clone()))
                                .collect(),
                        )
                    } else {
                        PlacementIntent::Automatic
                    },
                    requirements: requirements.clone(),
                },
            );
        }
        dependencies.insert(key.clone(), dependency);
        coverage.insert(key.clone(), selected.resolution.coverage);
        if let Some(required) = closure.required_edges.get(&selected.resolution.pin) {
            edges.insert(
                key.clone(),
                required
                    .iter()
                    .map(|pin| keys[&pin.project].clone())
                    .collect(),
            );
        }
    }
    let source =
        DocumentCodec.decode_intent(&DocumentCodec.encode_intent(&intent)?, "provider addition")?;
    Ok(ResolvedProject::validate(
        intent,
        ResolutionLock {
            acceptable_versions: source.intent().runtime.acceptable_versions.clone(),
            intent_revision: source.semantic_revision(),
            resolver: "empack-provider-addition-v0.5".into(),
            dependencies,
            required_edges: edges,
            coverage,
            runtime: current.lock().runtime.clone(),
        },
        source.semantic_revision(),
    )?)
}

#[cfg(test)]
mod tests;

fn incompatibilities(
    current: &ResolvedProject,
    resolution: &ProviderResolution,
) -> Vec<ProviderIncompatibility> {
    let mut assertions: Vec<_> = resolution
        .dependencies
        .iter()
        .filter_map(ProviderDependency::incompatibility)
        .collect();
    if let Some(previous) = current
        .lock()
        .dependencies
        .values()
        .find(|dependency| dependency.selected.as_ref() == Some(&resolution.pin))
    {
        for assertion in &previous.incompatibilities {
            if !assertions.contains(assertion) {
                assertions.push(assertion.clone());
            }
        }
    }
    assertions
}
