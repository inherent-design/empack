//! Canonical provider requests become one resolved dependency group before native mutation.
use super::*;
use crate::engine::{documents::DocumentCodec, resources::AdmissionPermit};
use anyhow::Context;
use empack_core::{
    addition::AdditionGroup,
    model::*,
    path::{InstallDestination, PortableRelPath},
    requirements::{Requirement, Requirements},
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Default)]
pub enum ProviderFiles {
    /// Use the provider's primary role, including its documented first-file fallback.
    #[default]
    Primary,
    /// Include every declared file only when the caller explicitly requests that representation.
    All,
    Named(BTreeSet<String>),
    /// Per-file destinations and participation, including companion resource-pack roles.
    Placed(BTreeMap<String, NonEmpty<Placement>>),
}
#[derive(Clone)]
pub struct ProviderAddInput {
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
}
impl ProviderAddition {
    pub fn project(&self) -> &ResolvedProject {
        &self.project
    }
    pub fn group(&self) -> &AdditionGroup {
        &self.group
    }
    pub fn evidence(&self) -> &ProviderClosure {
        &self.evidence
    }
}
pub enum ProviderAdditionOutcome {
    Ready(Box<ProviderAddition>),
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
        releases: ReleasePolicy,
        limits: ClosureLimits,
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
                .await?;
            budget = next;
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
                            releases,
                        },
                        limits.selection,
                        budget,
                    )
                    .await?;
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
                    releases,
                },
                limits,
                budget,
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
        let project = normalize(current, &closure, &choices)?;
        let group = AdditionGroup::from_resolved(&project)?;
        Ok(ProviderAdditionOutcome::Ready(Box::new(ProviderAddition {
            project,
            group,
            evidence: closure,
            _documents: reservation,
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
) -> Result<ResolvedProject> {
    let mut intent = current.intent().clone();
    intent.roots.clear();
    let mut keys = BTreeMap::new();
    let mut labels = BTreeSet::new();
    let mut origins = BTreeMap::new();
    for (id, selected) in &closure.selections {
        let key = roots
            .get(id)
            .and_then(|root| root.key.clone())
            .map(Ok)
            .unwrap_or_else(|| DependencyKey::parse(&selected.resolution.project.slug))?;
        ensure!(
            labels.insert(key.clone()),
            "Provider labels collide; choose explicit dependency keys"
        );
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
        let folder = root
            .and_then(|root| root.folder.as_ref())
            .or_else(|| intent.layout.get(&selected.kind))
            .map(|path| path.as_str())
            .or(match selected.kind {
                ContentKind::Mod => Some("mods"),
                ContentKind::ResourcePack => Some("resourcepacks"),
                ContentKind::ShaderPack => Some("shaderpacks"),
                _ => None,
            });
        let declared = selected.resolution.files.as_slice();
        let policy = root
            .map(|root| &root.files)
            .unwrap_or(&ProviderFiles::Primary);
        ensure!(
            declared.iter().all(|file| file.role.is_none())
                || matches!(policy, ProviderFiles::Placed(_)),
            "Companion provider files require explicit per-file placement and participation"
        );
        let files: Vec<_> = match policy {
            ProviderFiles::Primary => vec![
                declared
                    .iter()
                    .find(|file| file.primary)
                    .unwrap_or(&declared[0]),
            ],
            ProviderFiles::All => declared.iter().collect(),
            ProviderFiles::Placed(placements) => {
                ensure!(
                    !placements.is_empty()
                        && placements
                            .keys()
                            .all(|name| declared.iter().any(|file| &file.filename == name)),
                    "Selected provider file is missing"
                );
                ensure!(
                    declared
                        .iter()
                        .filter(|file| file.role.as_deref() == Some("required-resource-pack"))
                        .all(|file| placements.contains_key(&file.filename)),
                    "A required companion resource pack cannot be omitted"
                );
                declared
                    .iter()
                    .filter(|file| placements.contains_key(&file.filename))
                    .collect()
            }
            ProviderFiles::Named(names) => {
                ensure!(
                    !names.is_empty()
                        && names
                            .iter()
                            .all(|name| declared.iter().any(|file| &file.filename == name)),
                    "Selected provider file is missing"
                );
                declared
                    .iter()
                    .filter(|file| names.contains(&file.filename))
                    .collect()
            }
        };
        let files = files
            .into_iter()
            .map(|file| {
                let slot = FileSlot::parse(&file.filename)?;
                let placements = if let ProviderFiles::Placed(placements) = policy {
                    placements[&file.filename].clone()
                } else {
                    NonEmpty::new(vec![Placement {
                        destination: InstallDestination::parse(&format!(
                            "{}/{}",
                            folder.context(
                                "This content kind requires an explicit destination folder"
                            )?,
                            file.filename
                        ))?,
                        layer: ContentLayer::Common,
                        requirements: requirements.clone(),
                    }])?
                };
                Ok::<_, anyhow::Error>(ResolvedFile {
                    slot: slot.clone(),
                    expected: file.expected.clone(),
                    acquisition: AcquisitionSpec::Provider {
                        pin: selected.resolution.pin.clone(),
                        slot,
                        alternatives: vec![],
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
            .collect::<Result<Vec<_>>>()?;
        let dependency = LockedDependency {
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
                    source: SourceIntent::Provider(id.clone()),
                    kind: selected.kind,
                    version: root
                        .pin
                        .clone()
                        .map(VersionIntent::Exact)
                        .unwrap_or(VersionIntent::FollowCompatible),
                    placement: if root.folder.is_some()
                        || !matches!(root.files, ProviderFiles::Primary)
                    {
                        PlacementIntent::Explicit(NonEmpty::new(
                            dependency
                                .files
                                .as_slice()
                                .iter()
                                .flat_map(|file| file.placements.as_slice().iter().cloned())
                                .collect(),
                        )?)
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
