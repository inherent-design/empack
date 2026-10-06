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
    let build_catalog = catalog.clone();
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
            let identity = catalog.identify_file(&mut scope, download.clone(), NonEmpty::new(vec![provider])?, empack_lib::engine::providers::IdentificationLimits::default()).await?;
            let matches = |found: &empack_lib::engine::providers::IdentifiedSelection| {
                found.content == download.lease().id() && found.resolution.pin.project == pin.project
                    && found.matching_files.as_slice().contains(&file.filename)
            };
            let identified = match &identity {
                empack_lib::engine::providers::Identification::Exact(found) => matches(found),
                empack_lib::engine::providers::Identification::Ambiguous(found) => found.as_slice().iter().any(|value| matches(value)),
                empack_lib::engine::providers::Identification::Unknown => false,
            };
            anyhow::ensure!(identified, "Official content probe did not retain the downloaded provider identity");
            Ok::<_, anyhow::Error>((pin, expected, file.filename.clone(), resolution.kinds.clone(), resolution.game_versions.first().cloned()))
        }.await;
        Ok(result)
    })?;
    let outcome = handle.wait().await;
    runtime.shutdown().await;
    match &*outcome {
        OperationOutcome::Completed(Ok((pin, expected, filename, kind, game))) => {
            if kind
                .as_slice()
                .contains(&empack_core::model::ContentKind::ResourcePack)
            {
                publish_with_refreshed_locator(
                    build_catalog,
                    pin.clone(),
                    expected.clone(),
                    filename,
                    game.as_deref()
                        .ok_or_else(|| anyhow::anyhow!("Fixture declares no game version"))?,
                )
                .await?;
            }
        }
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

async fn publish_with_refreshed_locator(
    catalog: ProviderCatalog,
    pin: ResolvedPin,
    expected: empack_core::model::ExpectedContent,
    filename: &str,
    game: &str,
) -> anyhow::Result<()> {
    use empack_core::{
        inventory::OptionalPolicy,
        model::*,
        path::{InstallDestination, PathSyntax, PortableRelPath},
        projection::BuildTarget,
    };
    use empack_lib::engine::{
        api::*, artifacts::ArchiveLimits, documents::DocumentCodec, mrpack::OptionalConversion,
        packwiz::InstallerInteraction, server_runtime::installer::InstallerExecution,
        snapshot::SnapshotLimits, templates::TemplateOptions,
    };
    use serde_json::json;
    use sha2::Digest;
    use std::{collections::BTreeMap, fs, io::Read, time::Duration};
    let root = tempfile::tempdir()?;
    let host = tempfile::tempdir()?;
    let selection = match &pin.selection {
        PinSelector::ModrinthVersion(id) => id.to_string(),
        _ => anyhow::bail!("Resource fixture must use Modrinth"),
    };
    let intent_bytes = serde_json::to_vec(
        &json!({"schema":2,"pack":{"name":"Provider smoke","version":"alpha"},"runtime":{"minecraft":game,"loader":{"kind":"vanilla"}},"distribution":{"targets":["client-full"],"archive":"zip"},"dependencies":{"resources":{"source":{"kind":"provider","identity":{"provider":"modrinth","project":pin.project.to_string()}},"content":"resource-pack","version":{"mode":"exact","pin":{"provider":"modrinth","id":selection}},"placement":"automatic","environment":{"client":"required","server":"unsupported"}}}}),
    )?;
    let intent = DocumentCodec.decode_intent(&intent_bytes, "provider-smoke")?;
    let key = DependencyKey::parse("resources")?;
    let slot = FileSlot::parse("primary")?;
    let lock = ResolutionLock {
        intent_revision: intent.semantic_revision(),
        resolver: "provider-smoke".into(),
        runtime: RuntimeResolution {
            minecraft: GameVersion::parse(game)?,
            loader: LoaderKind::Vanilla,
            loader_version: None,
        },
        dependencies: BTreeMap::from([(
            key.clone(),
            LockedDependency {
                title: "Resources".into(),
                kind: ContentKind::ResourcePack,
                identity: ResolvedIdentity::Provider(pin.project.clone()),
                selected: Some(pin.clone()),
                files: NonEmpty::new(vec![ResolvedFile {
                    slot: slot.clone(),
                    acquisition: AcquisitionSpec::Provider {
                        pin,
                        slot,
                        alternatives: vec![],
                    },
                    expected: expected.clone(),
                    provenance: Provenance {
                        source: "modrinth-api-v2".into(),
                        location: None,
                        declared_digests: expected.digests.clone(),
                        conversions: vec![],
                    },
                    placements: NonEmpty::new(vec![Placement {
                        destination: InstallDestination::parse(&format!(
                            "resourcepacks/{filename}"
                        ))?,
                        layer: ContentLayer::Common,
                        requirements: intent.intent().roots[&key].requirements.clone(),
                    }])?,
                }])?,
            },
        )]),
        coverage: BTreeMap::from([(key, Coverage::Unknown)]),
        required_edges: BTreeMap::new(),
    };
    let project =
        ResolvedProject::validate(intent.intent().clone(), lock, intent.semantic_revision())?;
    let lock_bytes = DocumentCodec.encode_lock(&project)?;
    fs::write(root.path().join("empack.yml"), &intent_bytes)?;
    fs::write(root.path().join("empack.lock"), &lock_bytes)?;
    let work = ResourceRequest {
        jobs: 1,
        memory_bytes: 32 << 20,
        scratch_bytes: 512 << 20,
        open_files: 32,
    };
    let retained = ResourceRequest {
        memory_bytes: 1 << 20,
        open_files: 1,
        ..Default::default()
    };
    let governor = ResourceGovernor::new(ResourceRequest {
        jobs: 2,
        memory_bytes: 256 << 20,
        scratch_bytes: 1 << 30,
        open_files: 128,
    });
    let engine = Engine::new(
        EngineConfig {
            state_root: host.path().join("state"),
            retained_operations: 1,
            resources: BuildResources {
                capture: work,
                prepared: retained,
                local_acquisition: work,
                acquired: retained,
                assembly: work,
                receipt: retained,
            },
            snapshot: SnapshotLimits::default(),
            archive: ArchiveLimits::default(),
            transfer: TransferLimits {
                file_bytes: 256 << 20,
                transfer_bytes: 256 << 20,
                ..Default::default()
            },
            installer: InstallerExecution {
                java: "not-needed-for-client-content".into(),
                deadline: Duration::from_secs(10),
                heap_megabytes: 64,
                output: SnapshotLimits::default(),
            },
        },
        governor,
    )?
    .with_provider_catalog(catalog, CatalogLimits::default());
    let request = BuildRequest {
        outputs: NonEmpty::new(vec![BuildOutput {
            target: BuildTarget::ClientFull,
            artifact: PortableRelPath::parse("client.zip", PathSyntax::ArtifactName)?,
        }])?,
        archive: DistributionArchive::Zip,
        optional: OptionalPolicy::Preserve,
        mrpack_optional: OptionalConversion::RejectMetadataLoss,
        templates: TemplateOptions::default(),
        evidence: SourceEvidencePolicy::Compatibility,
        interaction: InstallerInteraction::Headless,
    };
    let prepared = match engine.prepare(root.path().to_owned(), request).await? {
        Preparation::Ready(value) => value,
        _ => anyhow::bail!("Public provider file was not executable"),
    };
    anyhow::ensure!(
        !host.path().join("state").exists(),
        "Preparation wrote host state"
    );
    let grant = ExecutionGrant {
        plan: prepared.view().plan,
        network: NetworkPermission::Allow,
        run_installer: false,
    };
    let mut handle = engine.start(prepared.authorize(grant)?)?;
    let outcome = handle.wait().await;
    engine.shutdown().await;
    match &*outcome {
        OperationOutcome::Completed(BuildOutcome::Completed(_)) => {}
        OperationOutcome::Completed(BuildOutcome::FailedBeforePublication(error)) => {
            anyhow::bail!("Provider-backed build failed: {error:#}")
        }
        _ => anyhow::bail!("Provider-backed build did not publish"),
    }
    anyhow::ensure!(
        fs::read(root.path().join("empack.lock"))? == lock_bytes
            && fs::read(root.path().join("empack.yml"))? == intent_bytes,
        "Locator refresh rewrote project intent"
    );
    let mut archive = zip::ZipArchive::new(fs::File::open(root.path().join("dist/client.zip"))?)?;
    let mut member = archive.by_name(&format!(".minecraft/resourcepacks/{filename}"))?;
    let mut bytes = Vec::new();
    member.read_to_end(&mut bytes)?;
    let sha512 = empack_core::digest::ExpectedDigest::Sha512(sha2::Sha512::digest(&bytes).into());
    anyhow::ensure!(
        expected
            .digests
            .as_ref()
            .unwrap()
            .values()
            .contains(&sha512),
        "Published archive differs from official source digest"
    );
    Ok(())
}

async fn verify_compatible(
    provider: ProviderKind,
    id: &str,
    game: &str,
    loader: empack_core::model::LoaderKind,
    kind: empack_core::model::ContentKind,
) -> anyhow::Result<()> {
    use empack_core::model::GameVersion;
    use empack_lib::engine::providers::{CompatibleRequest, ReleasePolicy, SelectionLimits};
    let key = if provider == ProviderKind::CurseForge {
        Some(
            std::env::var("EMPACK_KEY_CURSEFORGE")
                .map_err(|_| anyhow::anyhow!("Provider smoke requires EMPACK_KEY_CURSEFORGE"))?,
        )
    } else {
        None
    };
    let project = match provider {
        ProviderKind::Modrinth => ProviderProjectId::Modrinth(ModrinthProjectId::parse(id)?),
        ProviderKind::CurseForge => ProviderProjectId::CurseForge(CurseForgeProjectId::parse(id)?),
    };
    let request = CompatibleRequest {
        project,
        kind,
        game_versions: NonEmpty::new(vec![GameVersion::parse(game)?])?,
        loader,
        releases: ReleasePolicy::PreferStable,
    };
    let catalog = ProviderCatalog::new(key, Arc::new(HostBudgetRegistry::new()))?;
    let transport = HttpAcquisition::new()?;
    let governor = ResourceGovernor::new(ResourceRequest {
        jobs: 2,
        memory_bytes: 256 << 20,
        scratch_bytes: 256 << 20,
        open_files: 16,
    });
    let runtime = OperationRuntime::new(governor.clone(), 1);
    let mut handle = runtime.start(move |mut scope| async move {
        Ok(async {
            let selected = catalog.resolve_compatible(&mut scope, request.clone(), SelectionLimits::default()).await?;
            anyhow::ensure!(selected.resolution.pin.project == request.project, "Compatible selection changed project");
            anyhow::ensure!(selected.kind == request.kind && selected.resolution.kinds.as_slice().contains(&request.kind), "Compatible selection changed content kind");
            anyhow::ensure!(selected.matched_game == request.game_versions.as_slice()[0], "Compatible selection changed game");
            let file = selected.resolution.files.as_slice().iter().find(|file|file.primary).unwrap_or(&selected.resolution.files.as_slice()[0]);
            let expected = file.expected.clone();
            let acquired = transport.acquire(&mut scope, DownloadRequest {
                alternatives: NonEmpty::new(file.alternatives.clone())?, expected: expected.clone(),
                limits: TransferLimits { file_bytes: 256 << 20, transfer_bytes: 256 << 20, ..Default::default() },
                evidence: SourceEvidencePolicy::Compatibility, initial: InitialObservation::RequireEvidence,
            }).await?;
            anyhow::ensure!(Some(acquired.lease().len()) == expected.size, "Compatible file changed size");
            anyhow::ensure!(matches!(acquired.evidence(), empack_core::digest::IntegrityEvidence::MatchedExpected { expected: original, .. } if Some(original) == expected.digests.as_ref()), "Compatible file lost source evidence");
            Ok::<_,anyhow::Error>(())
        }.await)
    })?;
    let outcome = handle.wait().await;
    runtime.shutdown().await;
    match &*outcome {
        OperationOutcome::Completed(Ok(())) => {}
        OperationOutcome::Completed(Err(error)) => {
            anyhow::bail!("Compatible provider probe failed: {error:#}")
        }
        _ => anyhow::bail!("Compatible provider operation failed"),
    }
    anyhow::ensure!(
        governor.status().reserved == ResourceRequest::default(),
        "Compatible provider resources remained reserved"
    );
    Ok(())
}
#[tokio::test]
#[ignore = "requires official Modrinth API and CDN"]
async fn modrinth_compatible_selection_and_bytes() -> anyhow::Result<()> {
    verify_compatible(
        ProviderKind::Modrinth,
        "AANobbMI",
        "1.20.1",
        empack_core::model::LoaderKind::Fabric,
        empack_core::model::ContentKind::Mod,
    )
    .await
}
#[tokio::test]
#[ignore = "requires CurseForge API key and CDN"]
async fn curseforge_compatible_selection_and_bytes() -> anyhow::Result<()> {
    verify_compatible(
        ProviderKind::CurseForge,
        "238222",
        "1.21.1",
        empack_core::model::LoaderKind::NeoForge,
        empack_core::model::ContentKind::Mod,
    )
    .await
}

#[tokio::test]
#[ignore = "requires official Modrinth API and CDN"]
async fn modrinth_mixed_project_selects_datapack_bytes() -> anyhow::Result<()> {
    verify_compatible(
        ProviderKind::Modrinth,
        "8oi3bsk5",
        "1.21.1",
        empack_core::model::LoaderKind::Vanilla,
        empack_core::model::ContentKind::DataPack,
    )
    .await
}
