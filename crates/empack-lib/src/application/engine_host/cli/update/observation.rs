//! Provider adoption uses byte evidence, never metadata filenames or a latest-version guess.
use super::*;
use crate::engine::{
    acquisition::{LocalFileRequest, acquire_local_file},
    content::InitialObservation,
    providers::{Identification, IdentificationLimits},
};
use empack_core::{identity::ProviderProjectId, model::FileSlot, path::PortableRelPath};

pub(super) async fn identify(
    session: &dyn Session,
    project: PathBuf,
    identity: ProviderProjectId,
    members: Vec<(PortableRelPath, Option<FileSlot>)>,
    services: &dependencies::AdditionServices,
) -> Result<PinSelector> {
    ensure!(
        members.len() <= services.files.files,
        "Adoption observation count limit exceeded"
    );
    let catalog = services.catalog.clone();
    let limits = services.files;
    initialize::discover(session, move |mut scope| async move {
        let deadline = std::time::Instant::now()
            .checked_add(limits.transfer.deadline)
            .context("Adoption observation deadline overflow")?;
        let provider = match &identity {
            ProviderProjectId::Modrinth(_) => ProviderKind::Modrinth,
            ProviderProjectId::CurseForge(_) => ProviderKind::CurseForge,
        };
        let selected = project.clone();
        let paths: Vec<_> = members.iter().map(|(path, _)| path.clone()).collect();
        let capture = scope.spawn_blocking(
            ResourceRequest {
                jobs: 1,
                memory_bytes: 64 << 20,
                open_files: 16,
                ..Default::default()
            },
            ResourceRequest::default(),
            move |cancel| {
                crate::engine::snapshot::ProjectReadRoot::open(&selected)?.capture(
                    &paths,
                    SnapshotLimits::default(),
                    &cancel,
                )?;
                Ok::<_, anyhow::Error>(())
            },
        )?;
        scope.accept(capture.wait().await?)?.transpose()?;
        let mut pin = None;
        let mut bytes = 0u64;
        for (path, slot) in members {
            scope.cancellation().check()?;
            let content = acquire_local_file(
                &mut scope,
                LocalFileRequest {
                    source: project.join(path.as_str()),
                    expected: ExpectedContent {
                        digests: None,
                        size: None,
                        accepted_observation: None,
                    },
                    maximum: limits
                        .transfer
                        .file_bytes
                        .min(limits.transfer.transfer_bytes.saturating_sub(bytes)),
                    evidence: SourceEvidencePolicy::Compatibility,
                    initial: InitialObservation::Accepted,
                },
            )
            .await?;
            bytes = bytes
                .checked_add(content.content.lease().len())
                .context("Adoption observation size overflow")?;
            let identification_limits = IdentificationLimits {
                file_bytes: limits.transfer.file_bytes,
                catalog: crate::engine::providers::CatalogLimits {
                    deadline: deadline
                        .checked_duration_since(std::time::Instant::now())
                        .context("Adoption observation deadline exceeded")?,
                    ..Default::default()
                },
                ..Default::default()
            };
            let identified = catalog
                .identify_file(
                    &mut scope,
                    content.content,
                    NonEmpty::new(vec![provider])?,
                    identification_limits,
                )
                .await?;
            let Identification::Exact(identified) = identified else {
                anyhow::bail!("Observed provider content has no unique verified selection");
            };
            ensure!(
                identified.resolution.pin.project == identity,
                "Observed content belongs to another provider project"
            );
            ensure!(
                slot.as_ref().is_none_or(|slot| identified
                    .matching_files
                    .as_slice()
                    .iter()
                    .any(|role| role == slot.as_str())),
                "Observed content belongs to another provider file role"
            );
            let selected = &identified.resolution.pin.selection;
            ensure!(
                pin.as_ref().is_none_or(|pin| pin == selected),
                "Observed members disagree about their provider selection"
            );
            pin = Some(selected.clone());
        }
        pin.context("Adoption requires at least one observed provider file")
    })
    .await
}
