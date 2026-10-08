//! Resolve only unsatisfied authoring roots, then reuse native synchronization publication.
use super::*;
use crate::engine::{
    addition::{DirectFileInput, DirectFileSource, FileAddition, FileEvidence, FileKindPolicy},
    api::SyncRequest,
    documents::{DecodedIntent, DocumentCodec},
    providers::{ClosureLimits, ProviderAdditionOutcome},
    runtime_catalog::{RuntimeCatalog, RuntimeCatalogLimits},
    synchronization::{ResolutionRequired, SynchronizationCandidate},
};
use empack_core::{
    model::*,
    path::InstallDestination,
    synchronization::{affected_roots, rebind_prior_aliases, runtime_satisfies},
};
use std::collections::BTreeMap;
mod materialization;
use materialization::publish;

pub async fn synchronize(session: &dyn Session, materialize: bool) -> Result<()> {
    synchronize_with_services(
        session,
        materialize,
        dependencies::configured_services(session)?,
        RuntimeCatalog::new(HttpAcquisition::new()?),
    )
    .await
}

pub(super) async fn synchronize_with_services(
    session: &dyn Session,
    materialize: bool,
    services: dependencies::AdditionServices,
    runtime_catalog: RuntimeCatalog,
) -> Result<()> {
    let (invocation, project) = project_path(session)?;
    let selected = project.clone();
    let state = state_root(session.config().app_config(), &invocation)?;
    let captured = initialize::discover(session, move |mut scope| async move {
        let work = scope.spawn_blocking(
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
                let workspace = ProjectReader::new(RecoveryReader::new(state)).capture(
                    &selected,
                    &[],
                    SnapshotLimits::default(),
                    &cancel,
                )?;
                let sources: BTreeSet<_> = workspace
                    .intent()
                    .intent()
                    .roots
                    .values()
                    .flat_map(|root| match &root.source {
                        SourceIntent::Local(path) => vec![path.clone()],
                        SourceIntent::LocalFiles(members) => members.values().cloned().collect(),
                        _ => vec![],
                    })
                    .collect();
                // Authored sources must remain beneath their selected project root.
                if !sources.is_empty() {
                    workspace.root().capture(
                        &sources.into_iter().collect::<Vec<_>>(),
                        SnapshotLimits::default(),
                        &cancel,
                    )?;
                }
                Ok::<_, anyhow::Error>((
                    workspace.intent().clone(),
                    workspace.prior_lock().cloned(),
                ))
            },
        )?;
        scope.accept(work.wait().await?)?.transpose()
    })
    .await?;
    let (source, prior) = &*captured;
    if let Some(prior) = prior {
        match SynchronizationCandidate::prepare(source, prior) {
            Ok(candidate) => {
                return publish(
                    session,
                    candidate.project().clone(),
                    None,
                    materialize,
                    services,
                )
                .await;
            }
            Err(error) if error.downcast_ref::<ResolutionRequired>().is_some() => {}
            Err(error) => return Err(error),
        }
    }

    let runtime_intent = source.intent().runtime.clone();
    let old_runtime = prior.as_ref().map(|prior| prior.lock().runtime.clone());
    let runtime_limits = RuntimeCatalogLimits {
        transfer: TransferLimits {
            deadline: Duration::from_secs(session.config().app_config().net_timeout),
            ..RuntimeCatalogLimits::default().transfer
        },
        ..Default::default()
    };
    let runtime =
        if let Some(runtime) = old_runtime.filter(|old| runtime_satisfies(&runtime_intent, old)) {
            runtime
        } else if runtime_intent.loader == LoaderKind::Vanilla
            || runtime_intent.loader_version.is_some()
        {
            RuntimeResolution {
                minecraft: runtime_intent.minecraft,
                loader: runtime_intent.loader,
                loader_version: runtime_intent.loader_version,
            }
        } else {
            initialize::discover(session, move |mut scope| async move {
                let choices = runtime_catalog
                    .loaders(
                        &mut scope,
                        runtime_intent.minecraft,
                        runtime_intent.loader,
                        runtime_limits,
                    )
                    .await?;
                choices.resolve(None)
            })
            .await?
        };
    let previous = prior
        .as_ref()
        .map(|lock| rebind_prior_aliases(source.intent(), lock.lock()))
        .transpose()?;
    let affected = previous.as_ref().map_or_else(
        || source.intent().roots.keys().cloned().collect(),
        |prior| affected_roots(source.intent(), prior, source.semantic_revision()),
    );
    let baseline = resolver_context(source, previous.as_ref(), &runtime, &affected)?;
    let mut providers = Vec::new();
    let mut files = Vec::new();
    for key in &affected {
        let root = &source.intent().roots[key];
        let old = previous
            .as_ref()
            .and_then(|prior| prior.dependencies.get(key));
        match &root.source {
            SourceIntent::Provider(id) => {
                let same_context = previous.as_ref().is_some_and(|prior| {
                    runtime_satisfies(&source.intent().runtime, &prior.runtime)
                        && prior.acceptable_versions == source.intent().runtime.acceptable_versions
                });
                let mut input = provider_input(
                    key,
                    root,
                    ProjectSelector::canonical(id.clone()),
                    old,
                    same_context,
                )?;
                // A policy edit calls for revalidation, not an implicit update of a still-valid pin.
                if input.pin.is_none()
                    && !same_context
                    && let Some(pin) = old
                        .filter(|old| old.kind == root.kind)
                        .and_then(|old| old.selected.as_ref())
                        .filter(|pin| &pin.project == id)
                {
                    let catalog = services.catalog.clone();
                    let pin = pin.clone();
                    let runtime = source.intent().runtime.clone();
                    let kind = root.kind;
                    let limits = crate::engine::providers::CatalogLimits {
                        deadline: Duration::from_secs(session.config().app_config().net_timeout),
                        ..Default::default()
                    };
                    input.pin = initialize::discover(session, move |mut scope| async move {
                        let selected = catalog.resolve_exact(&mut scope, pin.clone(), limits).await?;
                        let mut games = vec![runtime.minecraft];
                        games.extend(runtime.acceptable_versions);
                        match selected.verify_compatibility(kind, &NonEmpty::new(games)?, runtime.loader) {
                            Ok(()) => Ok(Some(pin.selection)),
                            Err(error) if matches!(error.downcast_ref::<crate::engine::providers::CatalogError>(), Some(crate::engine::providers::CatalogError::NoCompatibleSelection)) => Ok(None),
                            Err(error) => Err(error),
                        }
                    }).await?;
                }
                providers.push(input);
            }
            SourceIntent::Search {
                query,
                providers: preference,
            } => {
                let (selector, _) = search_providers(
                    session,
                    &services.catalog,
                    &baseline,
                    query,
                    Some(root.kind),
                    preference.as_slice().to_vec(),
                )
                .await?;
                providers.push(provider_input(key, root, selector, None, false)?);
            }
            SourceIntent::LocalFiles(_) => {
                files.extend(member_inputs(key, root, old, &project, false)?)
            }
            SourceIntent::Local(_) | SourceIntent::Url(_) => {
                files.push(direct_input(key, root, old, &project, source.intent())?)
            }
        }
    }
    let mut limits = ClosureLimits::default();
    limits.selection.catalog.deadline =
        Duration::from_secs(session.config().app_config().net_timeout);
    let source = source.clone();
    let prior = prior.clone();
    let acquisition_services = dependencies::AdditionServices {
        catalog: services.catalog.clone(),
        transport: services.transport.clone(),
        files: services.files,
    };
    let resolved = scoped(session, governor(session.config().app_config()), move |mut scope| async move {
        let provider = if providers.is_empty() { None } else {
            match services.catalog.resolve_addition(&mut scope, &baseline, NonEmpty::new(providers)?, ReleasePolicy::PreferStable, limits).await? {
                ProviderAdditionOutcome::Ready(value) => Some(value),
                ProviderAdditionOutcome::NeedsInput(closure) => anyhow::bail!("Synchronization was not published: dependency evidence requires a decision: {:?}", closure.issues),
            }
        };
        let direct = if files.is_empty() { None } else {
            Some(FileAddition::acquire(&mut scope, &baseline, NonEmpty::new(files)?, &services.transport, SourceEvidencePolicy::Compatibility, services.files).await?)
        };
        let worker = scope.spawn_blocking(ResourceRequest { jobs: 1, memory_bytes: 64 << 20, ..Default::default() }, ResourceRequest { memory_bytes: 64 << 20, ..Default::default() }, move |cancel| {
        cancel.check()?;
        let mut lock = previous.unwrap_or_else(|| ResolutionLock {
            acceptable_versions: source.intent().runtime.acceptable_versions.clone(),
            intent_revision: source.semantic_revision(), resolver: "empack-synchronization-v0.5".into(),
            dependencies: BTreeMap::new(), required_edges: BTreeMap::new(), coverage: BTreeMap::new(), runtime: runtime.clone(),
        });
        lock.acceptable_versions = source.intent().runtime.acceptable_versions.clone();
        lock.runtime = runtime;
        lock.intent_revision = source.semantic_revision();
        let mut seen = BTreeSet::new();
        for group in provider.as_ref().map(|group| group.project()).into_iter().chain(direct.as_ref().map(|group| group.project())) {
            for (key, dependency) in &group.lock().dependencies {
                ensure!(seen.insert(key.clone()), "Resolved groups overlap a logical dependency");
                let mut dependency = dependency.clone();
                preserve_provider_assertions(lock.dependencies.get(key), &mut dependency)?;
                lock.dependencies.insert(key.clone(), dependency);
                lock.coverage.insert(key.clone(), group.lock().coverage[key]);
                lock.required_edges.remove(key);
                if let Some(edges) = group.lock().required_edges.get(key) { lock.required_edges.insert(key.clone(), edges.clone()); }
            }
        }
        let proposed = ResolvedProject::validate(source.intent().clone(), lock, source.semantic_revision())?;
        if let Some(prior) = &prior { SynchronizationCandidate::prepare_resolved(&source, prior, &proposed)?; }
        else { SynchronizationCandidate::prepare_initial(&source, &proposed)?; }
        Ok::<_, anyhow::Error>(proposed)
        })?;
        scope.accept(worker.wait().await?)?.transpose()
    }).await?;
    publish(
        session,
        (*resolved).clone(),
        Some((*resolved).clone()),
        materialize,
        acquisition_services,
    )
    .await
}

/// Fresh locators do not replace source assertions for an unchanged provider selection.
fn preserve_provider_assertions(
    previous: Option<&LockedDependency>,
    next: &mut LockedDependency,
) -> Result<()> {
    let Some(previous) =
        previous.filter(|old| old.selected.is_some() && old.selected == next.selected)
    else {
        return Ok(());
    };
    let files = next
        .files
        .clone()
        .into_vec()
        .into_iter()
        .map(|mut file| {
            if let Some(old) = previous
                .files
                .as_slice()
                .iter()
                .find(|old| old.slot == file.slot)
            {
                ensure!(
                    old.expected
                        .size
                        .zip(file.expected.size)
                        .is_none_or(|(a, b)| a == b),
                    "Provider file size changed for an unchanged selection"
                );
                if let (Some(old), Some(new)) = (&old.expected.digests, &file.expected.digests) {
                    ensure!(
                        old.values().iter().all(|a| new
                            .values()
                            .iter()
                            .find(|b| a.algorithm() == b.algorithm())
                            .is_none_or(|b| a == b)),
                        "Provider file assertions changed for an unchanged selection"
                    );
                }
                file.expected = old.expected.clone();
                file.provenance = old.provenance.clone();
            }
            Ok(file)
        })
        .collect::<Result<Vec<_>>>()?;
    next.files = NonEmpty::new(files)?;
    Ok(())
}

/// Resolver context reserves retained labels. It is never a publication candidate.
fn resolver_context(
    source: &DecodedIntent,
    prior: Option<&ResolutionLock>,
    runtime: &RuntimeResolution,
    affected: &BTreeSet<DependencyKey>,
) -> Result<ResolvedProject> {
    let mut intent = source.intent().clone();
    intent.roots.clear();
    let revision = DocumentCodec
        .decode_intent(
            &DocumentCodec.encode_intent(&intent)?,
            "sync resolver context",
        )?
        .semantic_revision();
    let mut lock = prior.cloned().unwrap_or_else(|| ResolutionLock {
        acceptable_versions: source.intent().runtime.acceptable_versions.clone(),
        intent_revision: revision,
        resolver: "empack-synchronization-v0.5".into(),
        dependencies: BTreeMap::new(),
        required_edges: BTreeMap::new(),
        coverage: BTreeMap::new(),
        runtime: runtime.clone(),
    });
    let replaced: BTreeSet<_> = affected
        .iter()
        .filter(|key| {
            let root = &source.intent().roots[*key];
            lock.dependencies
                .get(*key)
                .is_some_and(|old| match (&root.source, &old.identity) {
                    (SourceIntent::Provider(a), ResolvedIdentity::Provider(b)) => a != b,
                    (
                        SourceIntent::Local(_) | SourceIntent::LocalFiles(_),
                        ResolvedIdentity::Local(_),
                    )
                    | (SourceIntent::Url(_), ResolvedIdentity::Url(_)) => false,
                    (SourceIntent::Search { .. }, _) => true,
                    _ => true,
                })
        })
        .cloned()
        .collect();
    for key in &replaced {
        lock.dependencies.remove(key);
        lock.coverage.remove(key);
        lock.required_edges.remove(key);
    }
    for edges in lock.required_edges.values_mut() {
        edges.retain(|key| !replaced.contains(key));
    }
    lock.acceptable_versions = source.intent().runtime.acceptable_versions.clone();
    lock.runtime = runtime.clone();
    lock.intent_revision = revision;
    Ok(ResolvedProject::validate(intent, lock, revision)?)
}

fn provider_input(
    key: &DependencyKey,
    root: &DependencyIntent,
    selector: ProjectSelector,
    old: Option<&LockedDependency>,
    same_runtime: bool,
) -> Result<ProviderAddInput> {
    let retained = old.filter(|old| same_runtime && old.kind == root.kind && matches!((&root.source, &old.identity), (SourceIntent::Provider(a), ResolvedIdentity::Provider(b)) if a == b));
    let pin = match &root.version {
        VersionIntent::Exact(pin) => Some(pin.clone()),
        VersionIntent::FollowCompatible => {
            retained.and_then(|old| old.selected.as_ref().map(|pin| pin.selection.clone()))
        }
        VersionIntent::ContentPinned(_) => anyhow::bail!("Provider roots require provider pins"),
    };
    let files = match &root.placement {
        PlacementIntent::Automatic => ProviderFiles::Primary,
        PlacementIntent::ByFile(files) => ProviderFiles::Placed(
            files
                .iter()
                .map(|(slot, places)| (slot.as_str().to_owned(), places.clone()))
                .collect(),
        ),
        PlacementIntent::Explicit(placements) => ProviderFiles::PrimaryPlaced(placements.clone()),
    };
    Ok(ProviderAddInput {
        selector,
        key: Some(key.clone()),
        kind: Some(root.kind),
        pin,
        requirements: root.requirements.clone(),
        folder: None,
        files,
    })
}

fn direct_input(
    key: &DependencyKey,
    root: &DependencyIntent,
    old: Option<&LockedDependency>,
    project: &Path,
    intent: &ProjectIntent,
) -> Result<DirectFileInput> {
    let (source, filename) = match &root.source {
        SourceIntent::Local(path) => (
            DirectFileSource::TrackedLocal {
                path: project.join(path.as_str()),
                source: path.clone(),
            },
            path.components()
                .last()
                .context("Local source lacks filename")?
                .to_owned(),
        ),
        SourceIntent::Url(urls) => {
            let url = reqwest::Url::parse(&urls.as_slice()[0])?;
            let filename = percent_encoding::percent_decode_str(
                url.path_segments()
                    .and_then(|mut parts| parts.next_back())
                    .context("URL lacks filename")?,
            )
            .decode_utf8()?
            .into_owned();
            (
                DirectFileSource::Download {
                    origins: urls.clone(),
                    alternatives: urls.clone(),
                },
                filename,
            )
        }
        _ => unreachable!("direct source matched by caller"),
    };
    let placements = match &root.placement {
        PlacementIntent::ByFile(files) => {
            ensure!(
                files.len() == 1,
                "A single source requires one explicit file role"
            );
            files
                .values()
                .next()
                .context("Missing standalone file role")?
                .clone()
        }
        PlacementIntent::Explicit(placements) => placements.clone(),
        PlacementIntent::Automatic => {
            let folder = intent
                .content_folder(root.kind)
                .context("This content kind requires an explicit placement or configured folder")?;
            NonEmpty::new(vec![Placement {
                destination: InstallDestination::parse(&format!("{folder}/{filename}"))?,
                layer: ContentLayer::Common,
                requirements: root.requirements.clone(),
            }])?
        }
    };
    let evidence = match &root.version {
        VersionIntent::ContentPinned(digests) => FileEvidence::Declared(ExpectedContent {
            digests: Some(digests.clone()),
            size: None,
            accepted_observation: None,
        }),
        VersionIntent::FollowCompatible => {
            let same = old
                .and_then(|old| {
                    (old.files.as_slice().len() == 1).then_some(&old.files.as_slice()[0])
                })
                .filter(|file| match (&root.source, &file.acquisition) {
                    (SourceIntent::Local(a), AcquisitionSpec::Local(b)) => a == b,
                    (SourceIntent::Url(a), AcquisitionSpec::Url(b)) => a == b,
                    _ => false,
                });
            same.map_or(FileEvidence::AcceptObserved, |file| {
                FileEvidence::Declared(file.expected.clone())
            })
        }
        VersionIntent::Exact(_) => anyhow::bail!("Direct content cannot use a provider pin"),
    };
    Ok(DirectFileInput {
        role: match &root.placement {
            PlacementIntent::ByFile(files) => crate::engine::addition::FileInputRole::Named(
                files.keys().next().context("Missing file role")?.clone(),
            ),
            _ => old
                .and_then(|old| {
                    (old.files.as_slice().len() == 1).then(|| {
                        crate::engine::addition::FileInputRole::Named(
                            old.files.as_slice()[0].slot.clone(),
                        )
                    })
                })
                .unwrap_or(crate::engine::addition::FileInputRole::Primary),
        },
        key: key.clone(),
        title: old.map_or_else(|| key.as_str().to_owned(), |old| old.title.clone()),
        source,
        evidence,
        kind: root.kind,
        kind_policy: FileKindPolicy::AcceptUnrecognized,
        requirements: root.requirements.clone(),
        placements,
    })
}

/// Member slots retain one dependency identity while each source keeps its own assertions.
pub(super) fn member_inputs(
    key: &DependencyKey,
    root: &DependencyIntent,
    old: Option<&LockedDependency>,
    project: &Path,
    accept_changes: bool,
) -> Result<Vec<DirectFileInput>> {
    let SourceIntent::LocalFiles(members) = &root.source else {
        anyhow::bail!("Expected tracked local members")
    };
    members.iter().map(|(slot,path)| {
        let prior = old.and_then(|old| old.files.as_slice().iter().find(|file| &file.slot==slot));
        let selected: Vec<_> = match &root.placement {
        PlacementIntent::ByFile(files) => files.get(slot).context("Missing explicit member placement")?.as_slice().to_vec(),
        PlacementIntent::Explicit(_) | PlacementIntent::Automatic => anyhow::bail!("Members require named file placements"),
        };
        ensure!(!selected.is_empty(), "Member source needs a corresponding explicit placement");
        let evidence=if accept_changes { FileEvidence::AcceptObserved } else {
            prior.filter(|file| matches!(&file.acquisition, AcquisitionSpec::Local(source) if source==path))
                .map_or(FileEvidence::AcceptObserved, |file|FileEvidence::Declared(file.expected.clone()))
        };
        Ok(DirectFileInput {
            role:crate::engine::addition::FileInputRole::Member(slot.clone()),key:key.clone(),title:old.map_or_else(||key.as_str().to_owned(),|old|old.title.clone()),
            source:DirectFileSource::TrackedLocal { path: project.join(path.as_str()), source: path.clone() },evidence,kind:root.kind,
            kind_policy:FileKindPolicy::AcceptUnrecognized,requirements:root.requirements.clone(),placements:NonEmpty::new(selected)?,
        })
    }).collect()
}

#[cfg(test)]
mod tests;
