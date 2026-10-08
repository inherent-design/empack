//! Supplied bytes remain the bytes published after provider identification.
use super::*;
use crate::engine::{
    acquisition::{DownloadRequest, LocalFileRequest, acquire_local_file},
    content::InitialObservation,
    mrpack::{AcquiredBuildFile, LockedFileKey},
    providers::{
        Identification, IdentificationLimits, ProjectSelector, ProviderAddition, ProviderContent,
        ProviderContentChoice, ProviderFiles,
    },
    runtime::WorkScope,
};
use empack_core::{
    files::FilePermissions,
    identity::ProviderProjectId,
    model::{ContentKind, ExpectedContent, ProviderKind},
    requirements::{Requirement, Requirements},
};
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct Input {
    pub source: DirectFileSource,
    pub kind: Option<ContentKind>,
    pub providers: NonEmpty<ProviderKind>,
}

type Supplied = BTreeMap<(ProviderProjectId, String), AcquiredBuildFile>;
pub(super) async fn resolve(
    scope: &mut WorkScope,
    inputs: Vec<Input>,
    providers: &mut Vec<ProviderAddInput>,
    services: &AdditionServices,
    evidence: SourceEvidencePolicy,
) -> Result<Supplied> {
    let mut supplied = BTreeMap::new();
    ensure!(
        inputs.len() <= services.files.files,
        "Identification file count limit exceeded"
    );
    let deadline = std::time::Instant::now()
        .checked_add(services.files.transfer.deadline)
        .context("Identification acquisition deadline overflow")?;
    // Remote alternatives share the same transfer budget, including failed attempts.
    let mut requests = Vec::new();
    let mut request_keys = Vec::new();
    for (index, input) in inputs.iter().enumerate() {
        let alternatives = match &input.source {
            DirectFileSource::Local(_) => continue,
            DirectFileSource::Download {
                origins,
                alternatives,
            } => {
                for origin in origins.as_slice() {
                    crate::engine::documents::validate_download_url(origin)?;
                }
                alternatives
            }
            DirectFileSource::DownloadAsLocal { alternatives } => alternatives,
        };
        request_keys.push(index);
        requests.push(DownloadRequest {
            alternatives: alternatives.clone(),
            expected: ExpectedContent {
                digests: None,
                size: None,
                accepted_observation: None,
            },
            initial: InitialObservation::Accepted,
            evidence,
            limits: services.files.transfer,
        });
    }
    let mut downloaded: BTreeMap<_, _> = if requests.is_empty() {
        BTreeMap::new()
    } else {
        request_keys
            .into_iter()
            .zip(
                services
                    .transport
                    .acquire_batch(scope, requests, services.files.transfer)
                    .await?,
            )
            .collect()
    };
    let mut total = downloaded.values().try_fold(0u64, |sum, file| {
        sum.checked_add(file.lease().len())
            .context("Identification size overflow")
    })?;
    for (index, input) in inputs.into_iter().enumerate() {
        let content = match input.source {
            DirectFileSource::Local(source) => {
                let content = acquire_local_file(
                    scope,
                    LocalFileRequest {
                        source,
                        expected: ExpectedContent {
                            digests: None,
                            size: None,
                            accepted_observation: None,
                        },
                        maximum: services
                            .files
                            .transfer
                            .file_bytes
                            .min(services.files.transfer.transfer_bytes.saturating_sub(total)),
                        evidence,
                        initial: InitialObservation::Accepted,
                    },
                )
                .await?;
                total = total
                    .checked_add(content.content.lease().len())
                    .context("Identification size overflow")?;
                content
            }
            _ => AcquiredBuildFile {
                content: downloaded
                    .remove(&index)
                    .context("Missing identified download")?,
                permissions: FilePermissions {
                    readonly: false,
                    executable: false,
                },
            },
        };
        ensure!(
            total <= services.files.transfer.transfer_bytes,
            "Identification batch exceeds byte limit"
        );
        let limits = IdentificationLimits {
            file_bytes: services.files.transfer.file_bytes,
            catalog: crate::engine::providers::CatalogLimits {
                deadline: deadline
                    .checked_duration_since(std::time::Instant::now())
                    .context("Identification deadline exceeded")?,
                ..Default::default()
            },
            ..Default::default()
        };
        let identified = services
            .catalog
            .identify_file(scope, content.content.clone(), input.providers, limits)
            .await?;
        let identified = match identified {
            Identification::Exact(value) => value,
            Identification::Unknown => anyhow::bail!(
                "No provider identity verified; omit --platform to explicitly retain direct content"
            ),
            Identification::Ambiguous(_) => anyhow::bail!(
                "Multiple provider identities match; choose one provider or retain direct content"
            ),
        };
        ensure!(
            identified.matching_files.as_slice().len() == 1,
            "Provider bytes match multiple file roles; an explicit role choice is required"
        );
        let kinds = identified.resolution.kinds.as_slice();
        let kind = match input.kind {
            Some(kind) => {
                ensure!(
                    kinds.contains(&kind),
                    "Verified provider file does not match requested content kind"
                );
                kind
            }
            None => {
                ensure!(
                    kinds.len() == 1,
                    "Verified file supports several kinds; supply --type"
                );
                kinds[0]
            }
        };
        ensure!(
            kind != ContentKind::World,
            "World additions require explicit member interpretation"
        );
        crate::engine::addition::validate_file_kind(
            scope,
            &content,
            kind,
            crate::engine::addition::FileKindPolicy::RequireRecognized,
            services.files.archive,
        )
        .await?;
        let requirements = Requirements {
            client: Requirement::Required,
            server: if matches!(kind, ContentKind::ResourcePack | ContentKind::ShaderPack) {
                Requirement::Unsupported
            } else {
                Requirement::Required
            },
        };
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
            kind: Some(kind),
            pin: Some(pin.selection.clone()),
            requirements,
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
