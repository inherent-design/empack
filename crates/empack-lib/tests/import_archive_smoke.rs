//! Explicit local archive probes. No credentials, provider calls or project writes.
use empack_core::model::ExpectedContent;
use empack_lib::{
    application::process_runtime::Cancellation,
    engine::{
        archive_source::ZipContentSource,
        content::{InitialObservation, SourceEvidencePolicy, verify_stream},
        import::{ImportLimits, ImportedAcquisition, inspect_import},
        resources::{ResourceGovernor, ResourceRequest},
        runtime::{OperationOutcome, OperationRuntime},
    },
};
#[tokio::test]
#[ignore = "requires EMPACK_TEST_IMPORT_ARCHIVES containing local pack archives"]
async fn inspect_real_archives_and_verify_every_embedded_member() -> anyhow::Result<()> {
    let input = std::env::var_os("EMPACK_TEST_IMPORT_ARCHIVES")
        .ok_or_else(|| anyhow::anyhow!("Set EMPACK_TEST_IMPORT_ARCHIVES to local archive paths"))?;
    let paths: Vec<_> = std::env::split_paths(&input).collect();
    anyhow::ensure!(!paths.is_empty(), "At least one archive is required");
    for path in paths {
        let limits = ImportLimits::default();
        let file = std::fs::File::open(&path)?;
        let bytes = file.metadata()?.len();
        anyhow::ensure!(
            bytes <= limits.archive.compressed_bytes,
            "Archive is too large"
        );
        let source = verify_stream(
            &mut &file,
            &ExpectedContent {
                digests: None,
                size: Some(bytes),
                accepted_observation: None,
            },
            bytes,
            SourceEvidencePolicy::Compatibility,
            InitialObservation::Accepted,
            &Cancellation::default(),
        )?;
        let runtime = OperationRuntime::new(
            ResourceGovernor::new(ResourceRequest {
                jobs: 2,
                memory_bytes: 1 << 30,
                scratch_bytes: 16 << 30,
                open_files: 16,
            }),
            1,
        );
        let mut handle = runtime.start(move |mut scope| async move {
            let result = async {
                let project = inspect_import(&mut scope, source, limits).await?;
                let worker = scope.spawn_blocking(
                    ResourceRequest {
                        jobs: 1,
                        open_files: 4,
                        scratch_bytes: limits.archive.file_bytes,
                        ..Default::default()
                    },
                    ResourceRequest::default(),
                    move |cancel| {
                        let mut archive =
                            ZipContentSource::open(project.archive(), limits.archive, &cancel)?;
                        let mut count = 0;
                        for file in project.files.iter().chain(&project.overrides) {
                            if let ImportedAcquisition::Embedded(member) = &file.acquisition {
                                let (content, _) = archive.acquire(
                                    member,
                                    &file.expected,
                                    SourceEvidencePolicy::Compatibility,
                                    InitialObservation::Accepted,
                                    &cancel,
                                )?;
                                anyhow::ensure!(
                                    Some(content.lease().len()) == file.expected.size,
                                    "Embedded size mismatch"
                                );
                                count += 1;
                            }
                        }
                        Ok::<_, anyhow::Error>((
                            project.format,
                            project.files.len(),
                            project.providers.len(),
                            count,
                        ))
                    },
                )?;
                scope.accept(worker.wait().await?)?.transpose()
            }
            .await;
            Ok(result)
        })?;
        let result = handle.wait().await;
        runtime.shutdown().await;
        match &*result {
            OperationOutcome::Completed(Ok(counts)) => {
                println!("{}: {:?}", path.display(), **counts)
            }
            OperationOutcome::Completed(Err(error)) => {
                anyhow::bail!("{}: {error:#}", path.display())
            }
            _ => anyhow::bail!("Import smoke operation failed"),
        }
    }
    Ok(())
}
