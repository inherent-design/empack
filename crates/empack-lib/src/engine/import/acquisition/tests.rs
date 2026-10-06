use super::*;
use crate::engine::{
    resources::ResourceGovernor,
    runtime::{OperationOutcome, OperationRuntime},
};
use mockito::Server;
use serde_json::{Value, json};
use sha2::Digest;
use std::{
    io::{Cursor, Write},
    sync::Arc,
};

fn source(manifest: &str, value: Value, members: &[(&str, &[u8])]) -> AcquiredContent {
    let bytes = serde_json::to_vec(&value).unwrap();
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in
        std::iter::once((manifest, bytes.as_slice())).chain(members.iter().copied())
    {
        zip.start_file(
            name,
            zip::write::SimpleFileOptions::default().unix_permissions(0o755),
        )
        .unwrap();
        zip.write_all(bytes).unwrap();
    }
    observed(&zip.finish().unwrap().into_inner())
}
fn observed(bytes: &[u8]) -> AcquiredContent {
    verify_stream(
        &mut &*bytes,
        &ExpectedContent {
            digests: None,
            size: Some(bytes.len() as u64),
            accepted_observation: None,
        },
        bytes.len() as u64,
        SourceEvidencePolicy::Compatibility,
        InitialObservation::Accepted,
        &Cancellation::default(),
    )
    .unwrap()
}
fn mr(files: Vec<Value>) -> Value {
    json!({"formatVersion":1,"game":"minecraft","name":"Pack","versionId":"1","files":files,"dependencies":{"minecraft":"1.21.1"}})
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|v| format!("{v:02x}")).collect()
}
fn remote(url: String, path: &str, bytes: &[u8]) -> Value {
    json!({"path":path,"fileSize":bytes.len(),"downloads":[url],"hashes":{"sha1":hex(&sha1::Sha1::digest(bytes)),"sha512":hex(&sha2::Sha512::digest(bytes))},"env":{"client":"optional","server":"unsupported"}})
}
fn limits() -> ImportContentLimits {
    ImportContentLimits {
        catalog: CatalogLimits {
            response_bytes: 4096,
            transfer_bytes: 32768,
            deadline: std::time::Duration::from_secs(3),
        },
        archive: ArchiveLimits {
            compressed_bytes: 32768,
            file_bytes: 4096,
            total_bytes: 32768,
            entries: 32,
            depth: 16,
        },
        transfer: TransferLimits {
            file_bytes: 4096,
            transfer_bytes: 32768,
            deadline: std::time::Duration::from_secs(3),
            redirects: 2,
        },
        records: 32,
        total_bytes: 32768,
    }
}
fn governor() -> ResourceGovernor {
    ResourceGovernor::new(ResourceRequest {
        jobs: 1,
        memory_bytes: 8 << 20,
        scratch_bytes: 1 << 20,
        open_files: 32,
    })
}
async fn inspect_source(
    scope: &mut WorkScope,
    source: AcquiredContent,
) -> Result<RetainedOutput<ImportedProject>> {
    inspect_import(
        scope,
        source,
        ImportLimits {
            archive: limits().archive,
            manifest_bytes: 4096,
            records: 32,
        },
    )
    .await
}
async fn run(
    source: AcquiredContent,
    origin: String,
    limits: ImportContentLimits,
) -> (
    Arc<OperationOutcome<Result<ImportContentOutcome>>>,
    ResourceGovernor,
) {
    let governor = governor();
    let runtime = OperationRuntime::new(governor.clone(), 1);
    let mut handle = runtime
        .start(move |mut scope| async move {
            let result = async {
                let imported = inspect_source(&mut scope, source).await?;
                let catalog =
                    ProviderCatalog::for_loopback_tests(&origin, Some("fixture-key".into()));
                ImportContentPlan::resolve(&mut scope, imported, &catalog, limits)
                    .await?
                    .acquire(
                        &mut scope,
                        &HttpAcquisition::for_loopback_tests(),
                        BTreeMap::new(),
                        SourceEvidencePolicy::Compatibility,
                    )
                    .await
            }
            .await;
            Ok(result)
        })
        .unwrap();
    let outcome = handle.wait().await;
    runtime.shutdown().await;
    (outcome, governor)
}
fn ready(outcome: &OperationOutcome<Result<ImportContentOutcome>>) -> &VerifiedImportContent {
    match outcome {
        OperationOutcome::Completed(Ok(ImportContentOutcome::Ready(value))) => value,
        OperationOutcome::Completed(Err(error)) => panic!("{error:#}"),
        _ => panic!("expected complete verified content"),
    }
}
fn bytes(content: &AcquiredContent) -> Vec<u8> {
    let mut result = vec![];
    content.lease().open().read_to_end(&mut result).unwrap();
    result
}
#[tokio::test]
async fn imported_bytes_preserve_declared_names_optional_sides_and_all_layers() {
    let mut server = Server::new_async().await;
    let response = server
        .mock("GET", "/unrelated-name.bin")
        .with_body("payload")
        .create_async()
        .await;
    let source = source(
        "modrinth.index.json",
        mr(vec![remote(
            format!("{}/unrelated-name.bin", server.url()),
            "resourcepacks/chosen.zip",
            b"payload",
        )]),
        &[
            ("overrides/config/a", b"common"),
            ("client-overrides/config/a", b"client"),
            ("server-overrides/config/a", b"server"),
        ],
    );
    let (outcome, governor) = run(source, server.url(), limits()).await;
    let result = ready(&outcome);
    assert_eq!(result.content().len(), 4);
    let declaration = &result.plan().imported().files[0];
    assert_eq!(
        declaration.destination.relative().as_str(),
        "resourcepacks/chosen.zip"
    );
    assert_eq!(
        declaration.requirements.client,
        ImportedRequirement::Optional
    );
    assert_eq!(
        declaration.requirements.server,
        ImportedRequirement::Unsupported
    );
    assert_eq!(
        declaration
            .expected
            .digests
            .as_ref()
            .unwrap()
            .values()
            .len(),
        2
    );
    assert_eq!(
        bytes(&result.content()[&ImportContentKey::Declared(0)]),
        b"payload"
    );
    for (index, file) in result.plan().imported().overrides.iter().enumerate() {
        let key = ImportContentKey::Override(index);
        assert_eq!(
            bytes(&result.content()[&key]),
            match file.layer {
                ContentLayer::Common => b"common",
                ContentLayer::Client => b"client",
                ContentLayer::Server => b"server",
            }
        );
        assert!(result.permissions()[&key].executable);
    }
    let retained = result.content()[&ImportContentKey::Override(0)].clone();
    drop(outcome);
    assert!(governor.status().reserved.scratch_bytes > 0);
    assert!(!bytes(&retained).is_empty());
    drop(retained);
    assert_eq!(governor.status().reserved, ResourceRequest::default());
    response.assert_async().await;
}
#[tokio::test]
async fn a_later_digest_failure_returns_no_successful_import_subset() {
    let mut server = Server::new_async().await;
    let good = server
        .mock("GET", "/first")
        .with_body("payload")
        .create_async()
        .await;
    let bad = server
        .mock("GET", "/second")
        .with_body("changed")
        .create_async()
        .await;
    let source = source(
        "modrinth.index.json",
        mr(vec![
            remote(
                format!("{}/first", server.url()),
                "mods/first.jar",
                b"payload",
            ),
            remote(
                format!("{}/second", server.url()),
                "mods/second.jar",
                b"payload",
            ),
        ]),
        &[],
    );
    let (outcome, governor) = run(source, server.url(), limits()).await;
    assert!(matches!(&*outcome, OperationOutcome::Completed(Err(_))));
    assert_eq!(governor.status().reserved, ResourceRequest::default());
    good.assert_async().await;
    bad.assert_async().await;
}
fn cf() -> Value {
    json!({"manifestVersion":1,"manifestType":"minecraftModpack","name":"Pack","version":"1","files":[{"projectID":123,"fileID":456,"required":false},{"projectID":124,"fileID":457,"required":true}],"minecraft":{"version":"1.21.1","modLoaders":[{"id":"fabric-0.16.0","primary":true}]},"overrides":"overrides"})
}
async fn provider(server: &mut Server, project: u64, file: u64, url: Option<String>) {
    server.mock("GET", format!("/mods/{project}").as_str()).with_body(json!({"data":{"id":project,"gameId":432,"classId":6,"slug":format!("mod-{project}"),"name":"Mod"}}).to_string()).create_async().await;
    server.mock("GET", format!("/mods/{project}/files/{file}").as_str()).with_body(json!({"data":{"id":file,"gameId":432,"modId":project,"fileName":format!("mod-{project}.jar"),"fileLength":7,"hashes":[{"algo":2,"value":"321c3cf486ed509164edec1e1981fec8"}],"downloadUrl":url,"gameVersions":["1.21.1","Fabric"],"dependencies":[]}}).to_string()).create_async().await;
}
#[tokio::test]
async fn restricted_obligations_precede_downloads_and_explicit_bytes_are_reverified() {
    let mut server = Server::new_async().await;
    provider(&mut server, 123, 456, None).await;
    let url = format!("{}/public", server.url());
    provider(&mut server, 124, 457, Some(url)).await;
    let payload = server
        .mock("GET", "/public")
        .with_body("payload")
        .create_async()
        .await;
    let archive = source("manifest.json", cf(), &[]);
    let catalog = ProviderCatalog::for_loopback_tests(&server.url(), Some("fixture-key".into()));
    let runtime = OperationRuntime::new(governor(), 1);
    let mut handle = runtime
        .start(move |mut scope| async move {
            let result = async {
                let imported = inspect_source(&mut scope, archive).await?;
                let plan =
                    ImportContentPlan::resolve(&mut scope, imported, &catalog, limits()).await?;
                let transport = HttpAcquisition::for_loopback_tests();
                let ImportContentOutcome::NeedsInput {
                    plan,
                    pending,
                    mut provided,
                } = plan
                    .acquire(
                        &mut scope,
                        &transport,
                        BTreeMap::new(),
                        SourceEvidencePolicy::Compatibility,
                    )
                    .await?
                else {
                    panic!("missing manual request")
                };
                assert_eq!(pending.len(), 1);
                assert_eq!(pending[0].reason, ImportInputReason::RestrictedDownload);
                assert!(
                    !payload.matched_async().await,
                    "unrelated download preceded manual input"
                );
                provided.insert(pending[0].key.clone(), observed(b"payload"));
                let result = plan
                    .acquire(
                        &mut scope,
                        &transport,
                        provided,
                        SourceEvidencePolicy::Compatibility,
                    )
                    .await?;
                payload.assert_async().await;
                Ok::<_, anyhow::Error>(result)
            }
            .await;
            Ok(result)
        })
        .unwrap();
    let outcome = handle.wait().await;
    runtime.shutdown().await;
    let OperationOutcome::Completed(Ok(ImportContentOutcome::Ready(result))) = &*outcome else {
        panic!("manual import did not complete")
    };
    assert_eq!(result.content().len(), 2);
    assert_eq!(result.plan().providers().records().len(), 2);
    assert_eq!(
        result.plan().imported().providers[0].requirements.client,
        ImportedRequirement::Optional
    );
    for content in result.content().values() {
        assert!(
            matches!(content.evidence(), empack_core::digest::IntegrityEvidence::MatchedExpected { expected, .. } if expected.strongest() == empack_core::digest::DigestAlgorithm::Md5)
        );
    }
}
#[tokio::test]
async fn total_content_and_shared_network_allowances_are_distinct_and_enforced() {
    let mut server = Server::new_async().await;
    let first = server
        .mock("GET", "/one")
        .with_body("payload")
        .create_async()
        .await;
    let second = server
        .mock("GET", "/two")
        .with_body("payload")
        .create_async()
        .await;
    let make = || {
        source(
            "modrinth.index.json",
            mr(vec![
                remote(format!("{}/one", server.url()), "mods/a.jar", b"payload"),
                remote(format!("{}/two", server.url()), "mods/b.jar", b"payload"),
            ]),
            &[],
        )
    };
    let mut bounds = limits();
    bounds.total_bytes = 13;
    let (outcome, _) = run(make(), server.url(), bounds).await;
    assert!(matches!(&*outcome, OperationOutcome::Completed(Err(_))));
    assert!(!first.matched_async().await);
    bounds.total_bytes = 14;
    bounds.transfer.transfer_bytes = 13;
    let (outcome, governor) = run(make(), server.url(), bounds).await;
    assert!(
        matches!(&*outcome, OperationOutcome::Completed(Err(error)) if error.is::<crate::engine::acquisition::TransferError>())
    );
    assert_eq!(governor.status().reserved, ResourceRequest::default());
    first.assert_async().await;
    second.assert_async().await;
}
#[tokio::test]
async fn insecure_declarations_return_choices_instead_of_dropping_records() {
    let server = Server::new_async().await;
    let archive = source(
        "modrinth.index.json",
        mr(vec![remote(
            "http://example.com/old".into(),
            "mods/a.jar",
            b"payload",
        )]),
        &[],
    );
    let (outcome, _) = run(archive, server.url(), limits()).await;
    let OperationOutcome::Completed(Ok(ImportContentOutcome::NeedsInput { plan, pending, .. })) =
        &*outcome
    else {
        panic!("unsupported transport did not remain an input obligation")
    };
    assert_eq!(pending[0].reason, ImportInputReason::UnsupportedTransport);
    assert_eq!(plan.imported().files.len(), 1);
}

#[tokio::test]
async fn later_catalog_failure_and_cumulative_catalog_limits_return_no_inventory() {
    let mut server = Server::new_async().await;
    let url = format!("{}/public", server.url());
    provider(&mut server, 123, 456, Some(url.clone())).await;
    provider(&mut server, 124, 457, Some(url)).await;
    let payload = server.mock("GET", "/public").expect(0).create_async().await;
    let mut bounds = limits();
    bounds.catalog.transfer_bytes = 500;
    let (outcome, governor) = run(source("manifest.json", cf(), &[]), server.url(), bounds).await;
    assert!(
        matches!(&*outcome, OperationOutcome::Completed(Err(error)) if matches!(error.downcast_ref::<crate::engine::providers::CatalogError>(),Some(crate::engine::providers::CatalogError::Limit)))
    );
    assert_eq!(governor.status().reserved, ResourceRequest::default());
    server
        .mock("GET", "/mods/124")
        .with_status(401)
        .create_async()
        .await;
    let (outcome, governor) = run(source("manifest.json", cf(), &[]), server.url(), limits()).await;
    assert!(
        matches!(&*outcome, OperationOutcome::Completed(Err(error)) if matches!(error.downcast_ref::<crate::engine::providers::CatalogError>(),Some(crate::engine::providers::CatalogError::Unauthorized)))
    );
    assert_eq!(governor.status().reserved, ResourceRequest::default());
    payload.assert_async().await;
}
#[tokio::test]
async fn conflicting_import_pins_are_rejected_before_provider_lookup() {
    let mut server = Server::new_async().await;
    let no_lookup = server
        .mock("GET", "/mods/123")
        .expect(0)
        .create_async()
        .await;
    use empack_core::identity::{
        CurseForgeFileId, CurseForgeProjectId, PinSelector, ProviderProjectId,
    };
    let pins: Vec<_> = ["456", "457"]
        .into_iter()
        .map(|file| ResolvedPin {
            project: ProviderProjectId::CurseForge(CurseForgeProjectId::parse("123").unwrap()),
            selection: PinSelector::CurseForgeFile(CurseForgeFileId::parse(file).unwrap()),
        })
        .collect();
    let catalog = ProviderCatalog::for_loopback_tests(&server.url(), Some("fixture-key".into()));
    let runtime = OperationRuntime::new(governor(), 1);
    let mut handle = runtime
        .start(move |mut scope| async move {
            Ok(catalog
                .resolve_exact_batch(&mut scope, &pins, 32, limits().catalog)
                .await)
        })
        .unwrap();
    let outcome = handle.wait().await;
    runtime.shutdown().await;
    assert!(
        matches!(&*outcome, OperationOutcome::Completed(Err(error)) if error.to_string().contains("conflicting project pins"))
    );
    no_lookup.assert_async().await;
}
#[tokio::test]
async fn explicitly_supplied_wrong_bytes_fail_before_unrelated_downloads() {
    let mut server = Server::new_async().await;
    provider(&mut server, 123, 456, None).await;
    let url = format!("{}/public", server.url());
    provider(&mut server, 124, 457, Some(url)).await;
    let payload = server.mock("GET", "/public").expect(0).create_async().await;
    let archive = source("manifest.json", cf(), &[]);
    let catalog = ProviderCatalog::for_loopback_tests(&server.url(), Some("fixture-key".into()));
    let runtime = OperationRuntime::new(governor(), 1);
    let mut handle = runtime
        .start(move |mut scope| async move {
            let result = async {
                let imported = inspect_source(&mut scope, archive).await?;
                let plan =
                    ImportContentPlan::resolve(&mut scope, imported, &catalog, limits()).await?;
                let transport = HttpAcquisition::for_loopback_tests();
                let ImportContentOutcome::NeedsInput { plan, pending, .. } = plan
                    .acquire(
                        &mut scope,
                        &transport,
                        BTreeMap::new(),
                        SourceEvidencePolicy::Compatibility,
                    )
                    .await?
                else {
                    panic!("missing manual obligation")
                };
                plan.acquire(
                    &mut scope,
                    &transport,
                    BTreeMap::from([(pending[0].key.clone(), observed(b"changed"))]),
                    SourceEvidencePolicy::Compatibility,
                )
                .await
            }
            .await;
            Ok(result)
        })
        .unwrap();
    let outcome = handle.wait().await;
    runtime.shutdown().await;
    assert!(matches!(&*outcome, OperationOutcome::Completed(Err(_))));
    payload.assert_async().await;
}
