//! Live import acquisition, semantic assembly and publication into temporary projects.
use empack_core::{model::*, path::InstallDestination, projection::BuildTarget, requirements::*};
use empack_lib::{
    application::process_runtime::Cancellation,
    engine::{
        acquisition::HttpAcquisition,
        content::{AcquiredContent, InitialObservation, SourceEvidencePolicy, verify_stream},
        import::{
            ImportCandidateOptions, ImportContentKey, ImportContentLimits, ImportContentOutcome,
            ImportContentPlan, ImportFileDecision, ImportLimits, ImportPersistence,
            ImportedRequirement, VerifiedImportContent, inspect_import,
        },
        providers::ProviderCatalog,
        resources::{ResourceGovernor, ResourceRequest},
        runtime::{OperationOutcome, OperationRuntime},
    },
    networking::rate_budget::HostBudgetRegistry,
};
use std::{collections::BTreeMap, sync::Arc};
#[tokio::test]
#[ignore = "requires explicit local archives and live provider access"]
async fn resolve_and_verify_all_real_import_content() -> anyhow::Result<()> {
    let input = std::env::var_os("EMPACK_TEST_IMPORT_ARCHIVES")
        .ok_or_else(|| anyhow::anyhow!("Set EMPACK_TEST_IMPORT_ARCHIVES to local archive paths"))?;
    let paths: Vec<_> = std::env::split_paths(&input).collect();
    anyhow::ensure!(!paths.is_empty(), "At least one archive is required");
    let catalog = ProviderCatalog::new(
        std::env::var("EMPACK_KEY_CURSEFORGE").ok(),
        Arc::new(HostBudgetRegistry::default()),
    )?;
    let allow_prior = std::env::var("EMPACK_TEST_IMPORT_USE_PRIOR_BYTES").as_deref() == Ok("1");
    let mut prior = BTreeMap::new();
    let governor = ResourceGovernor::new(ResourceRequest {
        jobs: 2,
        memory_bytes: 1 << 30,
        scratch_bytes: 16 << 30,
        open_files: 64,
    });
    for path in paths {
        let file = std::fs::File::open(&path)?;
        let size = file.metadata()?.len();
        let source = verify_stream(
            &mut &file,
            &ExpectedContent {
                digests: None,
                size: Some(size),
                accepted_observation: None,
            },
            ImportLimits::default().archive.compressed_bytes,
            SourceEvidencePolicy::Compatibility,
            InitialObservation::Accepted,
            &Cancellation::default(),
        )?;
        let runtime = OperationRuntime::new(governor.clone(), 1);
        let available: Vec<AcquiredContent> = prior.values().cloned().collect();
        let catalog = catalog.clone();
        let mut handle = runtime.start(move |mut scope| async move {
            let result = async {
                let imported = inspect_import(&mut scope, source, ImportLimits::default()).await?;
                let plan = ImportContentPlan::resolve(
                    &mut scope,
                    imported,
                    &catalog,
                    ImportContentLimits::default(),
                )
                .await?;
                let transport = HttpAcquisition::new()?;
                let mut result = plan.acquire(&mut scope, &transport, BTreeMap::new(), SourceEvidencePolicy::Compatibility).await?;
                let mut supplied = 0;
                if let ImportContentOutcome::NeedsInput { plan, pending, mut provided } = result {
                    anyhow::ensure!(allow_prior, "Import requires explicit input for {} files; no candidate was published", pending.len());
                    // Opt-in fixture association uses original provider assertions, never names.
                    // Production acquisition independently rechecks the bytes and their evidence.
                    for need in pending {
                        let candidates: Vec<_> = available.iter().filter(|candidate| {
                            need.expected.size.is_none_or(|size| size == candidate.lease().len())
                                && need.expected.digests.as_ref().is_some_and(|digests| digests.check(candidate.observed_digests().values()).is_ok())
                        }).collect();
                        anyhow::ensure!(candidates.len() == 1, "Manual request has no unique digest match in explicitly supplied prior archive bytes");
                        provided.insert(need.key, candidates[0].clone());
                        supplied += 1;
                    }
                    result = plan.acquire(&mut scope, &transport, provided, SourceEvidencePolicy::Compatibility).await?;
                }
                match result {
                    ImportContentOutcome::Ready(content) => {
                        let options = fixture_decisions(&content)?;
                        let candidate = content.into_candidate(&mut scope, options)?;
                        Ok((candidate, supplied))
                    },
                    ImportContentOutcome::NeedsInput { pending, .. } => anyhow::bail!("{} import obligations remain", pending.len()),
                }
            }
            .await;
            Ok(result)
        })?;
        let outcome = handle.wait().await;
        runtime.shutdown().await;
        drop(handle);
        drop(runtime);
        match Arc::into_inner(outcome) {
            Some(OperationOutcome::Completed(Ok((candidate, supplied)))) => {
                let published = publish_fixture(candidate, governor.clone()).await?;
                let bytes: u64 = published
                    .files
                    .iter()
                    .map(|value| value.lease().len())
                    .sum();
                println!(
                    "{}: {:?}, {} files, {} bytes, {} explicitly supplied files, {} published roots",
                    path.display(),
                    published.format,
                    published.files.len(),
                    bytes,
                    supplied,
                    published.roots
                );
                if allow_prior {
                    for value in &published.files {
                        prior
                            .entry(value.lease().id())
                            .or_insert_with(|| value.clone());
                    }
                }
            }
            Some(OperationOutcome::Completed(Err(error))) => {
                anyhow::bail!("{}: {error:#}", path.display())
            }
            _ => anyhow::bail!("Import content operation failed"),
        }
    }
    Ok(())
}

// These are explicit smoke-host choices, not production importer defaults. Every source path
// and requirement is preserved; optional items start disabled and provider kinds must be unique.
fn fixture_decisions(content: &VerifiedImportContent) -> anyhow::Result<ImportCandidateOptions> {
    let source = content.plan().imported();
    let mut files = BTreeMap::new();
    for key in content.content().keys() {
        let (label, requirements, persistence, kind, destination) = match key {
            ImportContentKey::Declared(i) => {
                let file = &source.files[*i];
                let kind = if file.destination.relative().as_str().starts_with("mods/") {
                    ContentKind::Mod
                } else if file
                    .destination
                    .relative()
                    .as_str()
                    .starts_with("resourcepacks/")
                {
                    ContentKind::ResourcePack
                } else {
                    ContentKind::OtherFile
                };
                (
                    format!("declared-{i}"),
                    &file.requirements,
                    ImportPersistence::Url,
                    kind,
                    None,
                )
            }
            ImportContentKey::Override(i) => (
                format!("override-{i}"),
                &source.overrides[*i].requirements,
                ImportPersistence::Local,
                ContentKind::OtherFile,
                None,
            ),
            ImportContentKey::Provider { pin, filename } => {
                let i = source
                    .providers
                    .iter()
                    .position(|p| &p.selection == pin)
                    .unwrap();
                let record = &content.plan().providers().records()[pin];
                anyhow::ensure!(
                    record.kinds.as_slice().len() == 1,
                    "Fixture needs an explicit choice for ambiguous provider content kind"
                );
                let kind = record.kinds.as_slice()[0];
                let folder = match kind {
                    ContentKind::Mod => "mods",
                    ContentKind::ResourcePack => "resourcepacks",
                    ContentKind::ShaderPack => "shaderpacks",
                    _ => anyhow::bail!("Fixture needs an explicit provider layout choice"),
                };
                (
                    format!("provider-{i}"),
                    &source.providers[i].requirements,
                    ImportPersistence::Provider,
                    kind,
                    Some(InstallDestination::parse(&format!("{folder}/{filename}"))?),
                )
            }
        };
        let requirement = |value| -> anyhow::Result<Requirement> {
            Ok(match value {
                ImportedRequirement::Required => Requirement::Required,
                ImportedRequirement::Unsupported => Requirement::Unsupported,
                ImportedRequirement::Optional => Requirement::Optional(OptionalChoice {
                    key: ChoiceKey::parse(&label)?,
                    default_enabled: false,
                    description: Some("Optional imported content".into()),
                }),
            })
        };
        files.insert(
            key.clone(),
            ImportFileDecision {
                key: DependencyKey::parse(&label)?,
                kind,
                persistence,
                provider_destination: destination,
                requirements: Requirements {
                    client: requirement(requirements.client)?,
                    server: requirement(requirements.server)?,
                },
            },
        );
    }
    Ok(ImportCandidateOptions {
        metadata: PackMetadata {
            name: source
                .metadata
                .name
                .clone()
                .ok_or_else(|| anyhow::anyhow!("Fixture name missing"))?,
            version: source
                .metadata
                .version
                .clone()
                .ok_or_else(|| anyhow::anyhow!("Fixture version missing"))?,
            author: source.metadata.author.clone(),
            description: source.metadata.summary.clone(),
        },
        loader: None,
        layout: BTreeMap::new(),
        files,
        distribution: DistributionIntent {
            targets: NonEmpty::new(vec![BuildTarget::Mrpack])?,
            archive: DistributionArchive::Zip,
        },
        // The known generated CurseForge report is not game content. Reject any additional
        // unknown member; record this exact exclusion in the durable import provenance.
        exclude_auxiliary_members: source.auxiliary_members.len() == 1
            && source.auxiliary_members[0].as_str() == "modlist.html",
    })
}

struct PublishedProbe {
    format: empack_lib::engine::import::ImportFormat,
    files: Vec<AcquiredContent>,
    roots: usize,
}
async fn publish_fixture(
    candidate: empack_lib::engine::import::ImportCandidate,
    governor: ResourceGovernor,
) -> anyhow::Result<PublishedProbe> {
    use empack_core::files::ManagedPath;
    use empack_lib::engine::{
        acquisition::TransferLimits,
        api::{
            Engine, EngineConfig, ExecutionGrant, ExecutionOutcome, ExecutionReceipt,
            ImportRequest, NetworkPermission, OperationResources, Preparation, ProjectTarget,
        },
        artifacts::ArchiveLimits,
        layout::ProjectLayout,
        project::ProjectReader,
        project_change::ProjectReplacementPolicy,
        publication::RecoveryReader,
        server_runtime::installer::InstallerExecution,
        snapshot::SnapshotLimits,
    };
    let bytes = candidate.publication_bytes();
    let work = ResourceRequest {
        jobs: 1,
        memory_bytes: 64 << 20,
        scratch_bytes: bytes * 3 + (8 << 20),
        open_files: 16,
    };
    let retained = ResourceRequest {
        memory_bytes: 8 << 20,
        ..Default::default()
    };
    let temp = tempfile::tempdir()?;
    let project = temp.path().join("project");
    let host = temp.path().join("state");
    let format = candidate.source().plan().imported().format;
    let files = candidate.source().content().values().cloned().collect();
    let engine = Engine::new(
        EngineConfig {
            state_root: host.clone(),
            retained_operations: 1,
            resources: OperationResources {
                capture: work,
                prepared: retained,
                local_acquisition: work,
                acquired: retained,
                assembly: work,
                receipt: retained,
            },
            snapshot: SnapshotLimits::default(),
            archive: ArchiveLimits::default(),
            transfer: TransferLimits::default(),
            installer: InstallerExecution {
                java: "must-not-run".into(),
                deadline: std::time::Duration::from_secs(1),
                heap_megabytes: 64,
                output: SnapshotLimits::default(),
            },
        },
        governor,
    )?;
    let prepared = match engine
        .prepare(
            ProjectTarget::New(project.clone()),
            ImportRequest {
                candidate,
                replacement: ProjectReplacementPolicy::RejectExisting,
            },
        )
        .await?
    {
        Preparation::Ready(prepared) => prepared,
        Preparation::NeedsInput(_) => anyhow::bail!("Verified import unexpectedly needs input"),
    };
    anyhow::ensure!(
        !project.exists() && !host.exists() && std::fs::read_dir(temp.path())?.count() == 0,
        "Preparation changed the project or host state"
    );
    let grant = ExecutionGrant {
        plan: prepared.view().plan(),
        network: NetworkPermission::Offline,
        run_installer: false,
        replacement: prepared.view().import().and_then(|view| view.replacement),
    };
    let mut handle = engine.start(prepared.authorize(grant)?)?;
    let outcome = handle.wait().await;
    engine.shutdown().await;
    let receipt = match &*outcome {
        OperationOutcome::Completed(ExecutionOutcome::Completed(ExecutionReceipt::Import(
            receipt,
        ))) => receipt,
        OperationOutcome::Completed(ExecutionOutcome::FailedBeforePublication(error)) => {
            anyhow::bail!("{error:#}")
        }
        _ => anyhow::bail!("Import publication did not complete"),
    };
    let cancel = Cancellation::default();
    let reader = ProjectReader::new(RecoveryReader::new(host));
    let snapshot = reader.capture_build(&project, &[], SnapshotLimits::default(), &cancel)?;
    anyhow::ensure!(
        snapshot.require_resolved()?.lock() == receipt.project.lock(),
        "Published resolution differs from candidate"
    );
    for dependency in receipt.project.lock().dependencies.values() {
        for file in dependency.files.as_slice() {
            for placement in file.placements.as_slice() {
                let path = ProjectLayout::path(&ManagedPath::Content {
                    layer: placement.layer,
                    path: placement.destination.relative().clone(),
                })?;
                snapshot.acquire_file(
                    &path,
                    Some(&file.expected),
                    SourceEvidencePolicy::Compatibility,
                    &cancel,
                )?;
            }
        }
    }
    Ok(PublishedProbe {
        format,
        files,
        roots: receipt.project.intent().roots.len(),
    })
}
