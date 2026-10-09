//! First-lock adoption resolves declared roles from installed evidence, never from latest versions.
use super::*;
use crate::engine::{backend::BackendFile, documents::DecodedIntent};
use empack_core::model::{ContentLayer, DependencyIntent, FileSlot};

pub(super) async fn adopt(
    session: &dyn Session,
    project: &Path,
    source: &DecodedIntent,
    records: &[BackendFile],
    keys: Vec<String>,
    services: dependencies::AdditionServices,
) -> Result<()> {
    let selected: BTreeSet<_> = keys
        .iter()
        .map(|key| DependencyKey::parse(key))
        .collect::<std::result::Result<_, _>>()?;
    ensure!(selected.len() == keys.len(), "Repeated adoption key");
    ensure!(
        selected.iter().eq(source.intent().roots.keys()),
        "First-lock adoption must select every declared dependency root"
    );
    // Validate runtime evidence before contacting providers or reading selected payloads.
    dependencies::adoption_context(source)?;
    let mut inputs = Vec::new();
    for key in selected {
        let root = &source.intent().roots[&key];
        match &root.source {
            SourceIntent::Provider(identity) => {
                let mut input = super::super::synchronization::provider_input(
                    &key,
                    root,
                    ProjectSelector::canonical(identity.clone()),
                    None,
                    false,
                )?;
                if input.pin.is_none() {
                    let (observed, identify) = provider_observation(root, identity, records)?;
                    let identified = if identify.is_empty() {
                        None
                    } else {
                        Some(
                            observation::identify(
                                session,
                                project.to_path_buf(),
                                identity.clone(),
                                identify,
                                &services,
                            )
                            .await?,
                        )
                    };
                    ensure!(
                        observed
                            .as_ref()
                            .zip(identified.as_ref())
                            .is_none_or(|(a, b)| a == b),
                        "Observed metadata and bytes disagree about the provider selection"
                    );
                    input.pin = observed.or(identified);
                    ensure!(
                        input.pin.is_some(),
                        "First-lock adoption needs an observed exact provider selection or explicit file placements: {}",
                        key.as_str()
                    );
                }
                inputs.push(AddHostInput::Provider(input));
            }
            SourceIntent::LocalFiles(_) => {
                inputs.extend(
                    super::super::synchronization::member_inputs(&key, root, None, project, true)?
                        .into_iter()
                        .map(AddHostInput::File),
                );
            }
            SourceIntent::Local(_) | SourceIntent::Url(_) => {
                let mut input = super::super::synchronization::direct_input(
                    &key,
                    root,
                    None,
                    project,
                    source.intent(),
                )?;
                if let DirectFileSource::Download { origins, .. } = &input.source {
                    let placement = &input.placements.as_slice()[0];
                    let path = crate::engine::layout::ProjectLayout::path(
                        &empack_core::files::ManagedPath::Content {
                            layer: placement.layer,
                            path: placement.destination.relative().clone(),
                        },
                    )?;
                    input.source = DirectFileSource::ObservedUrl {
                        path: project.join(path.as_str()),
                        origins: origins.clone(),
                    };
                }
                inputs.push(AddHostInput::File(input));
            }
            SourceIntent::Search { .. } => anyhow::bail!(
                "Adoption needs a declared canonical identity instead of a search query: {}",
                key.as_str()
            ),
        }
    }
    dependencies::adopt_with_services(session, NonEmpty::new(inputs)?, services).await
}

type ObservationInputs = (
    Option<PinSelector>,
    Vec<(empack_core::path::PortableRelPath, Option<FileSlot>)>,
);
fn provider_observation(
    root: &DependencyIntent,
    identity: &empack_core::identity::ProviderProjectId,
    records: &[BackendFile],
) -> Result<ObservationInputs> {
    let mut pin = None;
    let mut identify = Vec::new();
    let mut record_pin = |record: &BackendFile| -> Result<()> {
        let owner = record
            .provider
            .as_ref()
            .context("Observed metadata has no provider identity")?;
        ensure!(
            &owner.project == identity,
            "Observed metadata names another provider identity"
        );
        let selected = owner
            .selection
            .as_ref()
            .context("Observed metadata has no exact provider pin")?;
        ensure!(
            pin.as_ref().is_none_or(|pin| pin == selected),
            "Observed files disagree about their provider selection"
        );
        pin = Some(selected.clone());
        Ok(())
    };
    let roles: Vec<_> = match &root.placement {
        PlacementIntent::Automatic => {
            for record in records.iter().filter(|record| {
                record
                    .provider
                    .as_ref()
                    .is_some_and(|owner| &owner.project == identity)
            }) {
                record_pin(record)?;
            }
            Vec::new()
        }
        PlacementIntent::ArchiveRoot(_) => {
            anyhow::bail!("Provider world adoption requires its exact archive selection")
        }
        PlacementIntent::Explicit(places) => vec![(None, places)],
        PlacementIntent::ByFile(files) => files
            .iter()
            .map(|(slot, places)| (Some(slot.clone()), places))
            .collect(),
    };
    for (slot, placements) in roles {
        let mut described = false;
        for placement in placements.as_slice() {
            if placement.layer != ContentLayer::Common {
                continue;
            }
            let matches: Vec<_> = records
                .iter()
                .filter(|record| record.destination == placement.destination)
                .collect();
            ensure!(
                matches.len() <= 1,
                "Multiple metadata records claim the selected destination"
            );
            if let Some(record) = matches.first() {
                record_pin(record)?;
                described = true;
            }
        }
        if !described {
            let placement = &placements.as_slice()[0];
            let path = crate::engine::layout::ProjectLayout::path(
                &empack_core::files::ManagedPath::Content {
                    layer: placement.layer,
                    path: placement.destination.relative().clone(),
                },
            )?;
            identify.push((path, slot));
        }
    }
    Ok((pin, identify))
}
