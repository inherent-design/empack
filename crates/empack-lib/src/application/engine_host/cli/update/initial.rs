//! First-lock adoption resolves declared roles from installed evidence, never from latest versions.
use super::*;
use crate::engine::documents::DecodedIntent;
use empack_core::model::{DependencyIntent, FileSlot};

pub(super) async fn adopt(
    session: &dyn Session,
    project: &Path,
    source: &DecodedIntent,
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
                    let identify = provider_observation(root)?;
                    input.pin = Some(
                        observation::identify(
                            session,
                            project.to_path_buf(),
                            identity.clone(),
                            identify,
                            &services,
                        )
                        .await?,
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

type ObservationInputs = Vec<(empack_core::path::PortableRelPath, Option<FileSlot>)>;
fn provider_observation(root: &DependencyIntent) -> Result<ObservationInputs> {
    let roles: Vec<_> = match &root.placement {
        PlacementIntent::Automatic => anyhow::bail!(
            "First-lock provider adoption requires explicit file placements or an exact version"
        ),
        PlacementIntent::ArchiveRoot(_) => {
            anyhow::bail!("Provider world adoption requires its exact archive selection")
        }
        PlacementIntent::Explicit(places) => vec![(None, places)],
        PlacementIntent::ByFile(files) => files
            .iter()
            .map(|(slot, places)| (Some(slot.clone()), places))
            .collect(),
    };
    roles
        .into_iter()
        .map(|(slot, placements)| {
            let placement = &placements.as_slice()[0];
            let path = crate::engine::layout::ProjectLayout::path(
                &empack_core::files::ManagedPath::Content {
                    layer: placement.layer,
                    path: placement.destination.relative().clone(),
                },
            )?;
            Ok((path, slot))
        })
        .collect()
}
