//! An update selects existing logical records; it never reinterprets them as search text.
use super::*;
use crate::engine::addition::{DirectFileInput, DirectFileSource, FileEvidence, FileKindPolicy};
use empack_core::model::{
    AcquisitionSpec, DependencyKey, DependencySelection, ExpectedContent, PlacementIntent,
    ResolvedIdentity, SourceIntent,
};

mod initial;
mod observation;

pub async fn update_command(
    session: &dyn Session,
    options: crate::application::cli::UpdateArgs,
) -> Result<()> {
    ensure!(
        options.all == options.dependencies.is_empty(),
        crate::application::cli::CommandInputRequired(
            "Select dependency names or --all, but not both"
        )
    );
    ensure!(
        !(options.apply && session.config().app_config().dry_run),
        crate::application::cli::CommandInputRequired("--apply cannot be combined with --dry-run")
    );
    ensure!(
        !(options.json && (options.apply || options.details)),
        crate::application::cli::CommandInputRequired(
            "--json previews cannot be combined with --apply or --details"
        )
    );
    let current = current(session).await?;
    let keys = if options.all {
        current
            .intent()
            .roots
            .keys()
            .map(|key| key.as_str().to_owned())
            .collect()
    } else {
        options.dependencies
    };
    if options.all && keys.is_empty() {
        ensure!(
            options.pins.is_empty(),
            crate::application::cli::CommandInputRequired(
                "There are no dependencies for the requested --pin selections"
            )
        );
        dependencies::UpdatePreviewReport::empty().present(
            session,
            dependencies::UpdateOutput {
                json: options.json,
                details: options.details,
                quiet: options.quiet,
            },
        )?;
        return Ok(());
    }
    let (_, project) = project_path(session)?;
    let mut inputs = selections(&current, &project, keys)?.into_vec();
    let pins = requested_pins(&current, &mut inputs, options.pins)?;
    dependencies::update_batch_with_services(
        session,
        NonEmpty::new(inputs)?,
        options.release_policy.map(Into::into),
        SourceEvidencePolicy::Compatibility,
        dependencies::configured_services(session)?,
        dependencies::UpdateExecution {
            batch: if options.continue_independent {
                BatchPolicy::ContinueIndependent
            } else {
                BatchPolicy::AllRequested
            },
            pins,
            output: dependencies::UpdateOutput {
                json: options.json,
                details: options.details,
                quiet: options.quiet,
            },
            mode: if options.apply {
                ApplicationMode::Apply
            } else {
                ApplicationMode::Preview
            },
        },
    )
    .await
}

pub async fn update(session: &dyn Session, keys: Vec<String>) -> Result<()> {
    update_with_policy(session, keys, None, BatchPolicy::AllRequested, vec![]).await
}
pub async fn update_with_policy(
    session: &dyn Session,
    keys: Vec<String>,
    releases: Option<ReleasePolicy>,
    policy: BatchPolicy,
    pins: Vec<String>,
) -> Result<()> {
    update_pinned_with_services(
        session,
        keys,
        releases,
        policy,
        dependencies::configured_services(session)?,
        pins,
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
#[cfg(test)]
pub(super) async fn update_with_services(
    session: &dyn Session,
    keys: Vec<String>,
    releases: Option<ReleasePolicy>,
    policy: BatchPolicy,
    services: dependencies::AdditionServices,
) -> Result<()> {
    update_pinned_with_services(session, keys, releases, policy, services, vec![]).await
}
pub(super) async fn update_pinned_with_services(
    session: &dyn Session,
    keys: Vec<String>,
    releases: Option<ReleasePolicy>,
    policy: BatchPolicy,
    services: dependencies::AdditionServices,
    pins: Vec<String>,
) -> Result<()> {
    let current = current(session).await?;
    let (_, project) = project_path(session)?;
    let mut inputs = selections(&current, &project, keys)?.into_vec();
    let pins = requested_pins(&current, &mut inputs, pins)?;
    dependencies::update_batch_with_services(
        session,
        NonEmpty::new(inputs)?,
        releases,
        SourceEvidencePolicy::Compatibility,
        services,
        dependencies::UpdateExecution {
            batch: policy,
            pins,
            mode: ApplicationMode::Apply,
            output: Default::default(),
        },
    )
    .await
}
/// Exact logical keys select installed identities; version IDs are parsed in that provider's
/// namespace. Nothing here searches, edits documents or promotes transitive selections.
fn requested_pins(
    current: &ResolvedProject,
    inputs: &mut [AddHostInput],
    values: Vec<String>,
) -> Result<std::collections::BTreeMap<DependencyKey, PinSelector>> {
    let mut pins = std::collections::BTreeMap::new();
    for value in values {
        let (key, version) = value.rsplit_once('=').context(UpdatePinInput::Syntax)?;
        let key = DependencyKey::parse(key)?;
        ensure!(
            current.intent().roots.contains_key(&key),
            UpdatePinInput::NotAuthorRoot(key.clone())
        );
        let input = inputs
            .iter_mut()
            .find_map(|input| match input {
                AddHostInput::Provider(input) if input.key.as_ref() == Some(&key) => Some(input),
                _ => None,
            })
            .context(UpdatePinInput::NotRequestedProvider(key.clone()))?;
        let ResolvedIdentity::Provider(project) = &current.lock().dependencies[&key].identity
        else {
            unreachable!("provider host input")
        };
        let pin = project.parse_pin(version)?;
        ensure!(
            pins.insert(key.clone(), pin.clone()).is_none(),
            UpdatePinInput::Duplicate(key.clone())
        );
        input.pin = Some(pin);
    }
    Ok(pins)
}
#[derive(Debug, thiserror::Error)]
pub enum UpdatePinInput {
    #[error("Expected --pin KEY=ID")]
    Syntax,
    #[error("Update pin requires an existing direct dependency in empack.yml: {}", .0.as_str())]
    NotAuthorRoot(DependencyKey),
    #[error("Exact update pin must also name a requested provider dependency: {}", .0.as_str())]
    NotRequestedProvider(DependencyKey),
    #[error("Repeated --pin key: {}; supply one reviewed version per dependency", .0.as_str())]
    Duplicate(DependencyKey),
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
        ensure!(
            seen.insert(key.clone()),
            crate::engine::providers::DependencySelectionError::Duplicate(key.clone())
        );
        let selected = current.lock().dependencies.get(&key).ok_or_else(|| {
            crate::engine::providers::DependencySelectionError::Missing(key.clone())
        })?;
        let root = current.intent().roots.get(&key);
        let primary = &selected.files.as_slice()[0];
        let requirements = root
            .map(|root| root.requirements.clone())
            .unwrap_or_else(|| primary.placements.as_slice()[0].requirements.clone());
        match &selected.identity {
            ResolvedIdentity::Provider(_) => {
                inputs.push(AddHostInput::Provider(ProviderAddInput::for_update(
                    current, &key,
                )?));
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
                        releases: None,
                        source: SourceIntent::LocalFiles(members),
                        kind: selected.kind,
                        selection: DependencySelection::Automatic,
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
                let evidence = match root.map(|root| &root.selection) {
                    Some(DependencySelection::ContentAssertions(digests)) => {
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
