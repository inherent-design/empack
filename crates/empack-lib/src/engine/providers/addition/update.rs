//! Existing logical selections retain their author policy and file participation during updates.
use super::*;

impl ProviderAddInput {
    /// Construct semantic provider choices, without opening project paths or granting writes.
    pub fn for_update(current: &ResolvedProject, key: &DependencyKey) -> Result<Self> {
        let selected = current
            .lock()
            .dependencies
            .get(key)
            .with_context(|| format!("Unknown installed dependency key: {}", key.as_str()))?;
        let ResolvedIdentity::Provider(id) = &selected.identity else {
            anyhow::bail!("Not a provider dependency: {}", key.as_str());
        };
        let root = current.intent().roots.get(key);
        let primary = &selected.files.as_slice()[0];
        let files = if let Some(root) = root {
            ProviderFiles::from_placement(&root.placement)
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
                        destination: InstallDestination::parse(base)?,
                        layer: placement.layer,
                        requirements: placement.requirements.clone(),
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            ProviderFiles::PrimaryPlaced(NonEmpty::new(roots)?)
        } else {
            ProviderFiles::Placed(
                selected
                    .files
                    .as_slice()
                    .iter()
                    .map(|file| (file.slot.as_str().to_owned(), file.placements.clone()))
                    .collect(),
            )
        };
        Ok(Self {
            source_selector: root.and_then(|root| root.source.selector()).cloned(),
            releases: root.and_then(|root| root.releases),
            selector: ProjectSelector::canonical(id.clone()),
            key: Some(key.clone()),
            kind: Some(selected.kind),
            pin: root.and_then(|root| match &root.selection {
                DependencySelection::ProviderPin(pin) => Some(pin.clone()),
                _ => None,
            }),
            requirements: root
                .map(|root| root.requirements.clone())
                .unwrap_or_else(|| primary.placements.as_slice()[0].requirements.clone()),
            folder: None,
            files,
        })
    }
}
