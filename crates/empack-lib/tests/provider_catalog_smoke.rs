//! Opt-in official API probes: canonical identity, exact file ownership and declared byte hashes.
use empack_core::{
    identity::{
        CurseForgeFileId, CurseForgeProjectId, ModrinthProjectId, ModrinthVersionId, PinSelector,
        ProviderProjectId,
    },
    model::{NonEmpty, ProviderKind, ResolvedPin},
};
use empack_lib::{
    engine::{
        acquisition::{DownloadRequest, HttpAcquisition, TransferLimits},
        content::{InitialObservation, SourceEvidencePolicy},
        providers::{CatalogLimits, ProjectSelector, ProviderCatalog},
        resources::{ResourceGovernor, ResourceRequest},
        runtime::{OperationOutcome, OperationRuntime},
    },
    networking::rate_budget::HostBudgetRegistry,
};
use std::sync::Arc;

async fn verify(
    provider: ProviderKind,
    slug: &str,
    project: &str,
    selection: &str,
) -> anyhow::Result<()> {
    let key = if provider == ProviderKind::CurseForge {
        Some(
            std::env::var("EMPACK_KEY_CURSEFORGE")
                .map_err(|_| anyhow::anyhow!("Provider smoke requires EMPACK_KEY_CURSEFORGE"))?,
        )
    } else {
        None
    };
    let catalog = ProviderCatalog::new(key, Arc::new(HostBudgetRegistry::new()))?;
    let transport = HttpAcquisition::new()?;
    let pin = match provider {
        ProviderKind::Modrinth => ResolvedPin {
            project: ProviderProjectId::Modrinth(ModrinthProjectId::parse(project)?),
            selection: PinSelector::ModrinthVersion(ModrinthVersionId::parse(selection)?),
        },
        ProviderKind::CurseForge => ResolvedPin {
            project: ProviderProjectId::CurseForge(CurseForgeProjectId::parse(project)?),
            selection: PinSelector::CurseForgeFile(CurseForgeFileId::parse(selection)?),
        },
    };
    let selector = ProjectSelector::parse(provider, slug)?;
    let governor = ResourceGovernor::new(ResourceRequest {
        jobs: 2,
        memory_bytes: 256 << 20,
        scratch_bytes: 256 << 20,
        open_files: 16,
    });
    let runtime = OperationRuntime::new(governor.clone(), 1);
    let mut handle = runtime.start(move |mut scope| async move {
        let result = async {
            let project = catalog.resolve_selector(&mut scope, selector, CatalogLimits::default()).await?;
            anyhow::ensure!(project.id == pin.project, "Official selector resolved a different project");
            drop(project);
            let resolution = catalog.resolve_exact(&mut scope, pin.clone(), CatalogLimits::default()).await?;
            anyhow::ensure!(resolution.pin == pin, "Official API returned another selection");
            let file = resolution.files.as_slice().iter().find(|file| file.primary).unwrap_or(&resolution.files.as_slice()[0]);
            anyhow::ensure!(!file.alternatives.is_empty(), "Fixture now requires manual acquisition");
            let expected = file.expected.clone();
            let download = transport.acquire(&mut scope, DownloadRequest {
                alternatives: NonEmpty::new(file.alternatives.clone())?, expected: expected.clone(),
                limits: TransferLimits { file_bytes: 256 << 20, transfer_bytes: 256 << 20, ..Default::default() },
                evidence: SourceEvidencePolicy::Compatibility, initial: InitialObservation::RequireEvidence,
            }).await?;
            anyhow::ensure!(Some(download.lease().len()) == expected.size, "Acquired file changed size");
            anyhow::ensure!(matches!(download.evidence(), empack_core::digest::IntegrityEvidence::MatchedExpected { expected: original, .. } if Some(original) == expected.digests.as_ref()), "Source evidence was lost");
            Ok::<_, anyhow::Error>(())
        }.await;
        Ok(result)
    })?;
    let outcome = handle.wait().await;
    runtime.shutdown().await;
    match &*outcome {
        OperationOutcome::Completed(Ok(())) => {}
        OperationOutcome::Completed(Err(error)) => {
            anyhow::bail!("Official provider probe failed: {error:#}")
        }
        _ => anyhow::bail!("Official provider operation failed"),
    }
    anyhow::ensure!(
        governor.status().reserved == ResourceRequest::default(),
        "Provider resources remained reserved"
    );
    Ok(())
}
#[tokio::test]
#[ignore = "requires official Modrinth API and CDN"]
async fn modrinth_mod_identity_and_exact_bytes() -> anyhow::Result<()> {
    verify(ProviderKind::Modrinth, "sodium", "AANobbMI", "NM9w06Kr").await
}
#[tokio::test]
#[ignore = "requires official Modrinth API and CDN"]
async fn modrinth_resource_pack_identity_and_exact_bytes() -> anyhow::Result<()> {
    verify(
        ProviderKind::Modrinth,
        "faithful-32x",
        "w0TnApzs",
        "lDYpMiqk",
    )
    .await
}
#[tokio::test]
#[ignore = "requires CurseForge API key and CDN"]
async fn curseforge_identity_and_exact_bytes() -> anyhow::Result<()> {
    verify(ProviderKind::CurseForge, "jei", "238222", "7364663").await
}
