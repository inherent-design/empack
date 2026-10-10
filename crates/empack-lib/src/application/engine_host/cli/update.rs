//! An update selects existing logical records; it never reinterprets them as search text.
use super::*;
use crate::engine::addition::{DirectFileInput, DirectFileSource, FileEvidence, FileKindPolicy};
use empack_core::model::{
    AcquisitionSpec, DependencyKey, ExpectedContent, Placement, PlacementIntent, ResolvedIdentity,
    SourceIntent, VersionIntent,
};

mod initial;
mod observation;

pub async fn update(session: &dyn Session, keys: Vec<String>) -> Result<()> {
    update_with_services(session, keys, dependencies::configured_services(session)?).await
}
pub async fn update_with_policy(
    session: &dyn Session,
    keys: Vec<String>,
    policy: BatchPolicy,
) -> Result<()> {
    let services = dependencies::configured_services(session)?;
    let current = current(session).await?;
    let (_, project) = project_path(session)?;
    let inputs = selections(&current, &project, keys)?;
    dependencies::update_batch_with_services(
        session,
        inputs,
        ReleasePolicy::PreferStable,
        SourceEvidencePolicy::Compatibility,
        services,
        policy,
    )
    .await
}
/// Adopt selected installed content. Provider pins come from verified byte identification,
/// not from a latest-version query; final native preparation verifies the observed bytes.
pub async fn adopt(session: &dyn Session, keys: Vec<String>) -> Result<()> {
    adopt_with_services(session, keys, dependencies::configured_services(session)?).await
}
pub(super) async fn adopt_with_services(
    session: &dyn Session,
    keys: Vec<String>,
    services: dependencies::AdditionServices,
) -> Result<()> {
    let (invocation, project) = project_path(session)?;
    let state = state_root(session.config().app_config(), &invocation)?;
    let selected = project.clone();
    let parsed = NonEmpty::new(
        keys.iter()
            .map(|key| DependencyKey::parse(key))
            .collect::<std::result::Result<Vec<_>, _>>()?,
    )?;
    let captured = initialize::discover(session, move |mut scope| async move {
        let worker = scope.spawn_blocking(
            ResourceRequest {
                jobs: 1,
                memory_bytes: 64 << 20,
                open_files: 16,
                ..Default::default()
            },
            ResourceRequest {
                memory_bytes: 64 << 20,
                ..Default::default()
            },
            move |cancel| {
                let reader = ProjectReader::new(RecoveryReader::new(state));
                let documents =
                    reader.capture(&selected, &[], SnapshotLimits::default(), &cancel)?;
                if documents.prior_lock().is_none() {
                    return Ok::<_, anyhow::Error>((None, documents.intent().clone()));
                }
                let snapshot = reader.capture_observed_dependencies(
                    &selected,
                    &parsed,
                    SnapshotLimits::default(),
                    &cancel,
                )?;
                let workspace = snapshot;
                Ok::<_, anyhow::Error>((
                    Some(workspace.require_resolved()?),
                    workspace.intent().clone(),
                ))
            },
        )?;
        scope.accept(worker.wait().await?)?.transpose()
    })
    .await?;
    let (current, source) = &*captured;
    let Some(current) = current else {
        return initial::adopt(session, &project, source, keys, services).await;
    };
    let mut inputs = selections(current, &project, keys)?.into_vec();
    for input in &mut inputs {
        if let AddHostInput::File(input) = input {
            if let DirectFileSource::Download { origins, .. } = &input.source {
                let placement = &input.placements.as_slice()[0];
                let relative = crate::engine::layout::ProjectLayout::path(
                    &empack_core::files::ManagedPath::Content {
                        layer: placement.layer,
                        path: placement.destination.relative().clone(),
                    },
                )?;
                input.source = DirectFileSource::ObservedUrl {
                    path: project.join(relative.as_str()),
                    origins: origins.clone(),
                };
            }
            continue;
        }
        let AddHostInput::Provider(input) = input else {
            continue;
        };
        let key = input
            .key
            .as_ref()
            .context("Adoption requires an exact logical key")?;
        let dependency = &current.lock().dependencies[key];
        let ResolvedIdentity::Provider(identity) = &dependency.identity else {
            unreachable!("provider input selected above")
        };
        if dependency.kind == ContentKind::World
            && dependency.files.as_slice().iter().all(|file| {
                matches!(
                    file.acquisition,
                    AcquisitionSpec::ProviderArchiveMember { .. }
                )
            })
        {
            input.pin = Some(
                dependency
                    .selected
                    .as_ref()
                    .context("Provider world has no exact archive selection")?
                    .selection
                    .clone(),
            );
            continue;
        }
        let identify = dependency
            .files
            .as_slice()
            .iter()
            .map(|file| {
                let placement = &file.placements.as_slice()[0];
                let path = crate::engine::layout::ProjectLayout::path(
                    &empack_core::files::ManagedPath::Content {
                        layer: placement.layer,
                        path: placement.destination.relative().clone(),
                    },
                )?;
                Ok((path, Some(file.slot.clone())))
            })
            .collect::<Result<Vec<_>>>()?;
        input.pin = Some(
            observation::identify(
                session,
                project.clone(),
                identity.clone(),
                identify,
                &services,
            )
            .await?,
        );
    }
    dependencies::adopt_with_services(session, NonEmpty::new(inputs)?, services).await
}
pub(super) async fn update_with_services(
    session: &dyn Session,
    keys: Vec<String>,
    services: dependencies::AdditionServices,
) -> Result<()> {
    let current = current(session).await?;
    let (_, project) = project_path(session)?;
    let inputs = selections(&current, &project, keys)?;
    dependencies::update_with_services(
        session,
        inputs,
        ReleasePolicy::PreferStable,
        SourceEvidencePolicy::Compatibility,
        services,
    )
    .await
}
fn selections(
    current: &ResolvedProject,
    project: &Path,
    keys: Vec<String>,
) -> Result<NonEmpty<AddHostInput>> {
    let mut seen = BTreeSet::new();
    let mut inputs = Vec::new();
    for value in keys {
        let key = DependencyKey::parse(&value)?;
        ensure!(seen.insert(key.clone()), "Repeated update key: {value}");
        let selected = current
            .lock()
            .dependencies
            .get(&key)
            .with_context(|| format!("Unknown installed dependency key: {value}"))?;
        let root = current.intent().roots.get(&key);
        let primary = &selected.files.as_slice()[0];
        let requirements = root
            .map(|root| root.requirements.clone())
            .unwrap_or_else(|| primary.placements.as_slice()[0].requirements.clone());
        let pin = root.and_then(|root| match &root.version {
            VersionIntent::Exact(pin) => Some(pin.clone()),
            _ => None,
        });
        match &selected.identity {
            ResolvedIdentity::Provider(id) => {
                let files = if let Some(root) = root
                    && let PlacementIntent::ArchiveRoot(roots) = &root.placement
                {
                    ProviderFiles::PrimaryPlaced(roots.clone())
                } else if matches!(
                    primary.acquisition,
                    AcquisitionSpec::ProviderArchiveMember { .. }
                ) {
                    let suffix = format!("/{}", primary.slot.as_str());
                    let roots = primary
                        .placements
                        .as_slice()
                        .iter()
                        .map(|placement| {
                            let base = placement
                                .destination
                                .relative()
                                .as_str()
                                .strip_suffix(&suffix)
                                .context("World member has no consistent destination root")?;
                            Ok(Placement {
                                destination: empack_core::path::InstallDestination::parse(base)?,
                                layer: placement.layer,
                                requirements: placement.requirements.clone(),
                            })
                        })
                        .collect::<Result<Vec<_>>>()?;
                    ProviderFiles::PrimaryPlaced(NonEmpty::new(roots)?)
                } else if root
                    .is_some_and(|root| matches!(root.placement, PlacementIntent::Automatic))
                    && selected.files.as_slice().len() == 1
                {
                    ProviderFiles::Primary
                } else {
                    // Filename identity is deliberate for multi-file/explicit placements. A provider
                    // changing those roles requires a new placement choice, never an inferred move.
                    ProviderFiles::Placed(
                        selected
                            .files
                            .as_slice()
                            .iter()
                            .map(|file| (file.slot.as_str().to_owned(), file.placements.clone()))
                            .collect(),
                    )
                };
                inputs.push(AddHostInput::Provider(ProviderAddInput {
                    selector: ProjectSelector::canonical(id.clone()),
                    key: Some(key),
                    kind: Some(selected.kind),
                    pin,
                    requirements,
                    folder: None,
                    files,
                }));
            }
            ResolvedIdentity::Local(_) | ResolvedIdentity::Url(_) => {
                // A retained group keeps its exact member map even after its root is forgotten.
                // This temporary request does not authorize promoting it back into intent.
                let retained = if root.is_none()
                    && selected.files.as_slice().len() > 1
                    && matches!(selected.identity, ResolvedIdentity::Local(_))
                {
                    let members = selected.files.as_slice().iter().map(|file| {
                        let AcquisitionSpec::Local(path) = &file.acquisition else {
                            anyhow::bail!("Retained member requires an explicit acquisition choice: {value}");
                        };
                        Ok((file.slot.clone(), path.clone()))
                    }).collect::<Result<_>>()?;
                    Some(empack_core::model::DependencyIntent {
                        source: SourceIntent::LocalFiles(members),
                        kind: selected.kind,
                        version: VersionIntent::FollowCompatible,
                        requirements: requirements.clone(),
                        placement: PlacementIntent::ByFile(
                            selected
                                .files
                                .as_slice()
                                .iter()
                                .map(|file| (file.slot.clone(), file.placements.clone()))
                                .collect(),
                        ),
                    })
                } else {
                    None
                };
                if let Some(root) = root
                    .or(retained.as_ref())
                    .filter(|root| matches!(root.source, SourceIntent::LocalFiles(_)))
                {
                    inputs.extend(
                        super::synchronization::member_inputs(
                            &key,
                            root,
                            Some(selected),
                            project,
                            true,
                        )?
                        .into_iter()
                        .map(AddHostInput::File),
                    );
                    continue;
                }
                ensure!(
                    selected.files.as_slice().len() == 1,
                    "Multi-file content requires an explicit member update: {value}"
                );
                let source = match root.map(|root| &root.source) {
                    Some(SourceIntent::Local(path)) => DirectFileSource::TrackedLocal {
                        path: project.join(path.as_str()),
                        source: path.clone(),
                    },
                    Some(SourceIntent::Url(urls)) => DirectFileSource::Download {
                        origins: urls.clone(),
                        alternatives: urls.clone(),
                    },
                    _ => match &primary.acquisition {
                        AcquisitionSpec::Local(path) => DirectFileSource::TrackedLocal {
                            path: project.join(path.as_str()),
                            source: path.clone(),
                        },
                        AcquisitionSpec::Url(urls) => DirectFileSource::Download {
                            origins: urls.clone(),
                            alternatives: urls.clone(),
                        },
                        _ => anyhow::bail!(
                            "Dependency requires an explicit acquisition choice: {value}"
                        ),
                    },
                };
                let evidence = match root.map(|root| &root.version) {
                    Some(VersionIntent::ContentPinned(digests)) => {
                        FileEvidence::Declared(ExpectedContent {
                            digests: Some(digests.clone()),
                            size: None,
                            accepted_observation: None,
                        })
                    }
                    _ => FileEvidence::AcceptObserved,
                };
                inputs.push(AddHostInput::File(DirectFileInput {
                    role: crate::engine::addition::FileInputRole::Named(primary.slot.clone()),
                    key,
                    title: selected.title.clone(),
                    source,
                    evidence,
                    kind: selected.kind,
                    kind_policy: FileKindPolicy::AcceptUnrecognized,
                    requirements,
                    placements: primary.placements.clone(),
                }));
            }
        }
    }
    NonEmpty::new(inputs).map_err(Into::into)
}
