//! Explicit live discovery probes. Run with --ignored; no project or persistent cache is used.
use empack_core::model::{GameVersion, LoaderKind};
use empack_lib::engine::{
    acquisition::HttpAcquisition,
    resources::{ResourceGovernor, ResourceRequest},
    runtime::{OperationOutcome, OperationRuntime},
    runtime_catalog::{RuntimeCatalog, RuntimeCatalogLimits},
};
#[tokio::test]
#[ignore = "requires official runtime catalogs"]
async fn live_runtime_catalogs_preserve_supported_loader_families_and_historical_coordinates() {
    let governor = ResourceGovernor::new(ResourceRequest {
        jobs: 2,
        memory_bytes: 512 << 20,
        scratch_bytes: 32 << 20,
        open_files: 64,
    });
    let runtime = OperationRuntime::new(governor.clone(), 1);
    let mut operation = runtime
        .start(move |mut scope| async move {
            let catalog = RuntimeCatalog::new(HttpAcquisition::new().unwrap());
            let limits = RuntimeCatalogLimits::default();
            let games = catalog.games(&mut scope, limits).await.unwrap();
            assert!(games.resolve(None).is_ok());
            for (game, family) in [
                ("1.21.1", LoaderKind::Fabric),
                ("1.21.1", LoaderKind::Quilt),
                ("1.21.1", LoaderKind::Forge),
                ("1.7.10", LoaderKind::Forge),
                ("1.20.1", LoaderKind::NeoForge),
                ("1.21.1", LoaderKind::NeoForge),
            ] {
                let selected = games
                    .resolve(Some(&GameVersion::parse(game).unwrap()))
                    .unwrap();
                let loaders = catalog
                    .loaders(&mut scope, selected, family, limits)
                    .await
                    .unwrap();
                let resolution = loaders.resolve(None).unwrap();
                assert_eq!(resolution.minecraft.as_str(), game);
                assert_eq!(resolution.loader, family);
                assert!(!loaders.versions().is_empty());
            }
            Ok(())
        })
        .unwrap();
    let outcome = operation.wait().await;
    assert!(
        matches!(&*outcome, OperationOutcome::Completed(())),
        "Live catalog resolution failed"
    );
    runtime.shutdown().await;
    assert_eq!(governor.status().reserved, ResourceRequest::default());
}
