//! Explicit live import acquisition. No project is created or replaced by this probe.
use empack_core::model::ExpectedContent;
use empack_lib::{
    application::process_runtime::Cancellation,
    engine::{
        acquisition::HttpAcquisition,
        content::{AcquiredContent, InitialObservation, SourceEvidencePolicy, verify_stream},
        import::{
            ImportContentLimits, ImportContentOutcome, ImportContentPlan, ImportLimits,
            inspect_import,
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
                    ImportContentOutcome::Ready(content) => Ok((content, supplied)),
                    ImportContentOutcome::NeedsInput { pending, .. } => anyhow::bail!("{} import obligations remain", pending.len()),
                }
            }
            .await;
            Ok(result)
        })?;
        let outcome = handle.wait().await;
        runtime.shutdown().await;
        match &*outcome {
            OperationOutcome::Completed(Ok((content, supplied))) => {
                let bytes: u64 = content
                    .content()
                    .values()
                    .map(|value| value.lease().len())
                    .sum();
                println!(
                    "{}: {:?}, {} files, {} bytes, {} explicitly supplied files",
                    path.display(),
                    content.plan().imported().format,
                    content.content().len(),
                    bytes,
                    supplied
                );
                if allow_prior {
                    for value in content.content().values() {
                        prior
                            .entry(value.lease().id())
                            .or_insert_with(|| value.clone());
                    }
                }
            }
            OperationOutcome::Completed(Err(error)) => {
                anyhow::bail!("{}: {error:#}", path.display())
            }
            _ => anyhow::bail!("Import content operation failed"),
        }
    }
    Ok(())
}
