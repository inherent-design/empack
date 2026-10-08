//! Supplied bytes remain the bytes published after provider identification.
use super::*;
use crate::engine::{
    dependency_content::DependencyContent,
    mrpack::{AcquiredBuildFile, LockedFileKey},
    providers::{
        Identification, IdentificationLimits, ProjectSelector, ProviderAddition, ProviderContent,
        ProviderContentChoice, ProviderFiles,
    },
    runtime::WorkScope,
};
use empack_core::{
    identity::ProviderProjectId,
    model::{ProviderKind, ResolvedProject},
};
use std::collections::{BTreeMap, BTreeSet};

type Supplied = BTreeMap<(ProviderProjectId, String), AcquiredBuildFile>;
pub(super) async fn resolve(
    scope: &mut WorkScope,
    current: &ResolvedProject,
    inputs: Vec<(DirectFileInput, NonEmpty<ProviderKind>)>,
    providers: &mut Vec<ProviderAddInput>,
    services: &AdditionServices,
    evidence: SourceEvidencePolicy,
) -> Result<Supplied> {
    let mut supplied = BTreeMap::new();
    // One acquisition allowance covers all supplied files before any identification request.
    if inputs.is_empty() {
        return Ok(supplied);
    }
    let mut choices = BTreeMap::new();
    let mut files = Vec::new();
    for (file, selected) in inputs {
        ensure!(
            choices.insert(file.key.clone(), selected).is_none(),
            "Repeated identification input"
        );
        files.push(file);
    }
    let acquired = FileAddition::acquire(
        scope,
        current,
        NonEmpty::new(files)?,
        &services.transport,
        evidence,
        services.files,
    )
    .await?;
    for (key, dependency) in acquired.project().lock().dependencies.iter() {
        let declared = &acquired.project().intent().roots[key];
        let file = &dependency.files.as_slice()[0];
        let content = acquired
            .content()
            .get(&LockedFileKey {
                dependency: key.clone(),
                slot: file.slot.clone(),
            })
            .context("Supplied identification file is missing")?;
        let DependencyContent::Materialized(content) = content else {
            anyhow::bail!("Identification requires acquired bytes");
        };
        let limits = IdentificationLimits {
            file_bytes: services.files.transfer.file_bytes,
            catalog: crate::engine::providers::CatalogLimits {
                deadline: services.files.transfer.deadline,
                ..Default::default()
            },
            ..Default::default()
        };
        let identified = services
            .catalog
            .identify_file(
                scope,
                content.content.clone(),
                choices
                    .remove(key)
                    .context("Identification provider choice is missing")?,
                limits,
            )
            .await?;
        let identified = match identified {
            Identification::Exact(value) => value,
            Identification::Unknown => anyhow::bail!(
                "No provider identity verified for {}; omit --platform to explicitly retain direct content",
                key.as_str()
            ),
            Identification::Ambiguous(_) => anyhow::bail!(
                "Multiple provider identities match {}; choose one provider or retain direct content",
                key.as_str()
            ),
        };
        ensure!(
            identified.matching_files.as_slice().len() == 1,
            "Provider bytes match multiple file roles; an explicit role choice is required"
        );
        let role = identified.matching_files.as_slice()[0].clone();
        let pin = &identified.resolution.pin;
        ensure!(
            supplied
                .insert((pin.project.clone(), role.clone()), content.clone())
                .is_none(),
            "Repeated supplied provider file"
        );
        providers.push(ProviderAddInput {
            selector: ProjectSelector::canonical(pin.project.clone()),
            key: None,
            kind: Some(dependency.kind),
            pin: Some(pin.selection.clone()),
            requirements: declared.requirements.clone(),
            folder: None,
            files: ProviderFiles::Named(BTreeSet::from([role])),
        });
    }
    Ok(supplied)
}

pub(super) async fn content(
    scope: &mut WorkScope,
    provider: &ProviderAddition,
    mut supplied: Supplied,
    services: &AdditionServices,
    evidence: SourceEvidencePolicy,
) -> Result<ProviderContent> {
    let mut choices = BTreeMap::new();
    for (key, dependency) in &provider.project().lock().dependencies {
        let empack_core::model::ResolvedIdentity::Provider(id) = &dependency.identity else {
            anyhow::bail!("Provider group contains a non-provider identity");
        };
        for file in dependency.files.as_slice() {
            choices.insert(
                LockedFileKey {
                    dependency: key.clone(),
                    slot: file.slot.clone(),
                },
                match supplied.remove(&(id.clone(), file.slot.as_str().to_owned())) {
                    Some(file) => ProviderContentChoice::Supplied(file),
                    None => ProviderContentChoice::Reference,
                },
            );
        }
    }
    ensure!(
        supplied.is_empty(),
        "Identified bytes are absent from the resolved group"
    );
    provider
        .acquire_content(
            scope,
            &services.transport,
            choices,
            evidence,
            services.files.transfer,
        )
        .await
}
