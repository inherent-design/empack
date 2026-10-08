//! An update selects existing logical records; it never reinterprets them as search text.
use super::*;
use crate::engine::addition::{DirectFileInput, DirectFileSource, FileEvidence, FileKindPolicy};
use empack_core::model::{
    AcquisitionSpec, DependencyKey, ExpectedContent, PlacementIntent, ResolvedIdentity,
    SourceIntent, VersionIntent,
};

pub async fn update(session: &dyn Session, keys: Vec<String>) -> Result<()> {
    update_with_services(session, keys, dependencies::configured_services(session)?).await
}
/// Accept changed tracked local bytes after a document-only preview. Provider-owned bytes
/// remain constrained by their original assertions; provider selection drift needs catalog evidence.
pub async fn adopt(session: &dyn Session, keys: Vec<String>) -> Result<()> {
    let current = current(session).await?;
    let (_, project) = project_path(session)?;
    let inputs = selections(&current, &project, keys)?;
    ensure!(
        inputs.as_slice().iter().all(|input| matches!(
            input,
            AddHostInput::File(DirectFileInput {
                source: DirectFileSource::Local(_),
                ..
            })
        )),
        "Observed adoption requires tracked local files; provider or URL selection changes need explicit verified evidence"
    );
    dependencies::adopt_with_services(session, inputs, dependencies::configured_services(session)?)
        .await
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
                let files = if root
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
                ensure!(
                    selected.files.as_slice().len() == 1,
                    "Multi-file content requires an explicit member update: {value}"
                );
                let source = match root.map(|root| &root.source) {
                    Some(SourceIntent::Local(path)) => {
                        DirectFileSource::Local(project.join(path.as_str()))
                    }
                    Some(SourceIntent::Url(urls)) => DirectFileSource::Download {
                        origins: urls.clone(),
                        alternatives: urls.clone(),
                    },
                    _ => match &primary.acquisition {
                        AcquisitionSpec::Local(path) => {
                            DirectFileSource::Local(project.join(path.as_str()))
                        }
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
