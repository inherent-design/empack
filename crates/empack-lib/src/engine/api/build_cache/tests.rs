use super::*;
use crate::engine::{
    api::tests::{engine, put, request},
    content::store::ContentStoreLimits,
    documents::DocumentCodec,
    mrpack::{LockedFileKey, tests::project},
};
use std::fs;

#[tokio::test]
async fn approved_build_populates_cache_and_next_build_verifies_it_offline_without_preview_writes()
{
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    let cache_path = host.path().join("content");
    let resolved = project(true, false);
    put(
        root.path(),
        "empack.yml",
        &DocumentCodec.encode_intent(resolved.intent()).unwrap(),
    );
    put(
        root.path(),
        "empack.lock",
        &DocumentCodec.encode_lock(&resolved).unwrap(),
    );
    let payload = host.path().join("payload");
    fs::write(&payload, b"payload").unwrap();
    let supplied: BTreeMap<_, _> = resolved
        .lock()
        .dependencies
        .iter()
        .flat_map(|(key, dependency)| {
            dependency.files.as_slice().iter().map(|file| {
                (
                    AcquisitionKey::Locked(LockedFileKey {
                        dependency: key.clone(),
                        slot: file.slot.clone(),
                    }),
                    payload.clone(),
                )
            })
        })
        .collect();
    for first in [true, false] {
        let (engine, governor) = engine(host.path().join("state"));
        let engine = engine.with_content_cache(
            ContentCache::new(cache_path.clone(), ContentStoreLimits::default()).unwrap(),
        );
        let input = || {
            if first {
                request()
                    .with_content(Default::default())
                    .with_local_files(supplied.clone())
            } else {
                request().with_content(Default::default())
            }
        };
        let before = if first {
            None
        } else {
            Some(footprint(&cache_path))
        };
        let prepared = match engine
            .prepare(root.path().to_owned(), input())
            .await
            .unwrap()
        {
            Preparation::Ready(value) => value,
            Preparation::NeedsInput(_) => {
                panic!("verified supplied/cache bytes must satisfy preparation")
            }
        };
        let view = prepared.view().build().unwrap();
        assert!(view.content.is_empty());
        assert!(!view.needs_network);
        if let Some(before) = before {
            assert_eq!(footprint(&cache_path), before);
        } else {
            assert!(!cache_path.exists());
        }
        let grant = ExecutionGrant {
            plan: prepared.view().plan(),
            network: NetworkPermission::Offline,
            run_installer: false,
            run_runtime: false,
            replacement: prepared.view().replacement(),
        };
        let mut handle = engine.start(prepared.authorize(grant).unwrap()).unwrap();
        let outcome = handle.wait().await;
        match &*outcome {
            OperationOutcome::Completed(ExecutionOutcome::Completed(ExecutionReceipt::Build(
                receipt,
            ))) => assert_eq!(receipt.artifacts.len(), 2),
            OperationOutcome::Completed(ExecutionOutcome::FailedBeforePublication(error)) => {
                panic!("{error:#}")
            }
            _ => panic!("cached build did not complete"),
        }
        assert!(
            cache_path
                .join("md5-321c3cf486ed509164edec1e1981fec8.hint")
                .is_file()
        );
        engine.release_completed(handle.id());
        drop((handle, outcome));
        engine.shutdown().await;
        assert_eq!(governor.status().reserved, ResourceRequest::default());
    }
    let (engine, governor) = engine(host.path().join("state"));
    let engine = engine.with_content_cache(
        ContentCache::new(cache_path.clone(), ContentStoreLimits::default()).unwrap(),
    );
    let hint = cache_path.join("md5-321c3cf486ed509164edec1e1981fec8.hint");
    fs::write(hint, b"invalid").unwrap();
    let before = footprint(&cache_path);
    let prepared = match engine
        .prepare(root.path().to_owned(), request())
        .await
        .unwrap()
    {
        Preparation::Ready(value) => value,
        Preparation::NeedsInput(_) => {
            panic!("URL content should remain an explicit download obligation")
        }
    };
    assert!(!prepared.view().build().unwrap().content.is_empty());
    assert!(prepared.view().build().unwrap().needs_network);
    let plan = prepared.view().plan();
    assert!(
        prepared
            .authorize(ExecutionGrant {
                plan,
                network: NetworkPermission::Offline,
                run_installer: false,
                run_runtime: false,
                replacement: None
            })
            .is_err()
    );
    assert_eq!(footprint(&cache_path), before);
    engine.shutdown().await;
    assert_eq!(governor.status().reserved, ResourceRequest::default());
}

fn footprint(
    path: &std::path::Path,
) -> BTreeMap<std::ffi::OsString, (Vec<u8>, std::time::SystemTime)> {
    fs::read_dir(path)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            (
                entry.file_name(),
                (
                    fs::read(entry.path()).unwrap(),
                    entry.metadata().unwrap().modified().unwrap(),
                ),
            )
        })
        .collect()
}

#[tokio::test]
async fn provider_cache_satisfies_exact_files_without_catalog_access_or_credentials() {
    use crate::engine::api::tests::provider_fixture;
    use empack_core::model::{DependencyKey, FileSlot};
    for curseforge in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let host = tempfile::tempdir().unwrap();
        provider_fixture(root.path(), curseforge);
        let payload = host.path().join("payload");
        fs::write(&payload, b"payload").unwrap();
        let cache =
            ContentCache::new(host.path().join("cache"), ContentStoreLimits::default()).unwrap();
        for first in [true, false] {
            let (engine, governor) = engine(host.path().join("state"));
            let engine = engine
                .with_content_cache(cache.clone())
                .with_provider_catalog(
                    ProviderCatalog::for_loopback_tests("http://127.0.0.1:1", None),
                    CatalogLimits::default(),
                );
            let mut input = request().with_content(Default::default());
            if first {
                input = input.with_local_files(BTreeMap::from([(
                    AcquisitionKey::Locked(LockedFileKey {
                        dependency: DependencyKey::parse("assets").unwrap(),
                        slot: FileSlot::parse("primary").unwrap(),
                    }),
                    payload.clone(),
                )]));
            }
            let prepared = match engine.prepare(root.path().to_owned(), input).await.unwrap() {
                Preparation::Ready(value) => value,
                Preparation::NeedsInput(_) => {
                    panic!("verified cache must satisfy provider content")
                }
            };
            assert!(prepared.view().build().unwrap().content.is_empty());
            assert!(!prepared.view().build().unwrap().needs_network);
            let grant = ExecutionGrant {
                plan: prepared.view().plan(),
                network: NetworkPermission::Offline,
                run_installer: false,
                run_runtime: false,
                replacement: prepared.view().replacement(),
            };
            let mut handle = engine.start(prepared.authorize(grant).unwrap()).unwrap();
            let outcome = handle.wait().await;
            assert!(matches!(
                &*outcome,
                OperationOutcome::Completed(ExecutionOutcome::Completed(ExecutionReceipt::Build(
                    _
                )))
            ));
            engine.release_completed(handle.id());
            drop((handle, outcome));
            engine.shutdown().await;
            assert_eq!(governor.status().reserved, ResourceRequest::default());
        }
    }
}
