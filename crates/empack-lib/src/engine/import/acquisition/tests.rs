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

pub(in crate::engine) fn source(
    manifest: &str,
    value: Value,
    members: &[(&str, &[u8])],
) -> AcquiredContent {
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
pub(in crate::engine) fn mr(files: Vec<Value>) -> Value {
    json!({"formatVersion":1,"game":"minecraft","name":"Pack","versionId":"1","files":files,"dependencies":{"minecraft":"1.21.1"}})
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|v| format!("{v:02x}")).collect()
}
pub(in crate::engine) fn remote(url: String, path: &str, bytes: &[u8]) -> Value {
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
                ContentLayer::Common => panic!("override was flattened into the base layer"),
                ContentLayer::CommonOverride => b"common",
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
                let mut plan =
                    ImportContentPlan::resolve(&mut scope, imported, &catalog, limits()).await?;
                let revision = plan.resume_revision();
                // Locators are transport facts; slot assertions and source membership are identity.
                let original = plan.needs[1].source.clone();
                plan.needs[1].source = ImportedAcquisition::Downloads(vec![
                    "https://example.invalid/refreshed?signature=private".into(),
                ]);
                assert_eq!(plan.resume_revision(), revision);
                plan.needs[1].source = original;
                let size = plan.needs[1].expected.size;
                plan.needs[1].expected.size = Some(999);
                assert_ne!(plan.resume_revision(), revision);
                plan.needs[1].expected.size = size;
                assert_eq!(plan.resume_revision(), revision);
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

#[tokio::test]
async fn an_allowed_mirror_remains_usable_without_losing_source_alternatives() {
    let mut server = Server::new_async().await;
    let payload = server
        .mock("GET", "/permitted")
        .with_body("payload")
        .create_async()
        .await;
    let mut declaration = remote(
        format!("{}/permitted", server.url()),
        "config/a",
        b"payload",
    );
    declaration["downloads"] = json!([
        "http://example.com/disallowed",
        format!("{}/permitted", server.url())
    ]);
    let (outcome, _) = run(
        source(
            "modrinth.index.json",
            mr(vec![declaration]),
            &[("overrides/config/a", b"shared")],
        ),
        server.url(),
        limits(),
    )
    .await;
    let result = ready(&outcome);
    assert_eq!(result.content().len(), 2);
    assert_eq!(
        bytes(&result.content()[&ImportContentKey::Declared(0)]),
        b"payload"
    );
    assert_eq!(
        bytes(&result.content()[&ImportContentKey::Override(0)]),
        b"shared"
    );
    assert!(
        matches!(&result.plan().imported().files[0].acquisition, ImportedAcquisition::Downloads(urls) if urls.len()==2)
    );
    assert_eq!(
        result.plan().imported().files[0].layer,
        ContentLayer::Common
    );
    assert_eq!(
        result.plan().imported().overrides[0].layer,
        ContentLayer::CommonOverride
    );
    payload.assert_async().await;
}

fn candidate_options(content: &VerifiedImportContent) -> super::super::ImportCandidateOptions {
    use super::super::{ImportCandidateOptions, ImportFileDecision, ImportPersistence};
    use empack_core::{model::*, projection::BuildTarget, requirements::*};
    let requirement = |requirement, key: &str| match requirement {
        ImportedRequirement::Required => Requirement::Required,
        ImportedRequirement::Unsupported => Requirement::Unsupported,
        ImportedRequirement::Optional => Requirement::Optional(OptionalChoice {
            key: ChoiceKey::parse(key).unwrap(),
            default_enabled: false,
            description: Some("Explicit fixture choice".into()),
        }),
    };
    let source = content.plan().imported();
    let files = content
        .content()
        .keys()
        .enumerate()
        .map(|(i, key)| {
            let (requirements, persistence, kind, destination) = match key {
                ImportContentKey::Declared(i) => (
                    &source.files[*i].requirements,
                    ImportPersistence::Local,
                    ContentKind::ResourcePack,
                    None,
                ),
                ImportContentKey::Override(i) => (
                    &source.overrides[*i].requirements,
                    ImportPersistence::Local,
                    ContentKind::Config,
                    None,
                ),
                ImportContentKey::Provider { pin, filename } => (
                    &source
                        .providers
                        .iter()
                        .find(|p| &p.selection == pin)
                        .unwrap()
                        .requirements,
                    ImportPersistence::Provider,
                    ContentKind::Mod,
                    Some(InstallDestination::parse(&format!("mods/{filename}")).unwrap()),
                ),
            };
            let label = format!("item-{i}");
            (
                key.clone(),
                ImportFileDecision {
                    key: DependencyKey::parse(&label).unwrap(),
                    kind,
                    persistence,
                    requirements: Requirements {
                        client: requirement(requirements.client, &label),
                        server: requirement(requirements.server, &label),
                    },
                    provider_destination: destination,
                },
            )
        })
        .collect();
    ImportCandidateOptions {
        metadata: PackMetadata {
            name: "Imported pack".into(),
            version: "1".into(),
            author: None,
            description: None,
        },
        loader: None,
        acceptable_versions: vec![],
        layout: BTreeMap::new(),
        distribution: DistributionIntent {
            targets: NonEmpty::new(vec![BuildTarget::Mrpack]).unwrap(),
            archive: DistributionArchive::Zip,
        },
        files,
        exclude_auxiliary_members: false,
    }
}
pub(in crate::engine) async fn interpret(
    source: AcquiredContent,
    origin: String,
    change: impl FnOnce(&mut super::super::ImportCandidateOptions) + Send + 'static,
) -> Arc<OperationOutcome<Result<super::super::ImportCandidate>>> {
    let (outcome, governor) = run(source, origin, limits()).await;
    let Some(OperationOutcome::Completed(Ok(ImportContentOutcome::Ready(content)))) =
        Arc::into_inner(outcome)
    else {
        panic!("expected acquired content");
    };
    let mut options = candidate_options(&content);
    change(&mut options);
    let runtime = OperationRuntime::new(governor, 1);
    let mut handle = runtime
        .start(
            move |mut scope| async move { Ok(content.into_candidate(&mut scope, options).await) },
        )
        .unwrap();
    let outcome = handle.wait().await;
    runtime.shutdown().await;
    outcome
}
#[tokio::test]
async fn import_candidate_retains_every_layer_and_explicit_local_conversion() {
    use empack_core::model::*;
    let mut server = Server::new_async().await;
    let response = server
        .mock("GET", "/payload")
        .with_body("payload")
        .create_async()
        .await;
    let archive = source(
        "modrinth.index.json",
        mr(vec![remote(
            format!("{}/payload", server.url()),
            "resourcepacks/chosen.zip",
            b"payload",
        )]),
        &[
            ("overrides/resourcepacks/chosen.zip", b"shared"),
            ("client-overrides/resourcepacks/chosen.zip", b"client"),
            ("server-overrides/resourcepacks/chosen.zip", b"server"),
        ],
    );
    let outcome = interpret(archive, server.url(), |_| {}).await;
    let OperationOutcome::Completed(Ok(candidate)) = &*outcome else {
        panic!("candidate failed");
    };
    response.assert_async().await;
    assert_eq!(candidate.bindings().len(), 4);
    let project = candidate.project();
    let codec = crate::engine::documents::DocumentCodec;
    let intent = codec
        .decode_intent(&codec.encode_intent(project.intent()).unwrap(), "candidate")
        .unwrap();
    let lock = codec
        .decode_lock(&codec.encode_lock(project).unwrap(), &intent, "candidate")
        .unwrap();
    assert_eq!(lock.lock(), project.lock());
    let mut layers = BTreeSet::new();
    for ((key, slot), content_key) in candidate.bindings() {
        let root = &project.intent().roots[key];
        let file = project.lock().dependencies[key]
            .files
            .as_slice()
            .iter()
            .find(|file| &file.slot == slot)
            .unwrap();
        layers.insert(file.placements.as_slice()[0].layer);
        assert!(matches!(root.source, SourceIntent::Local(_)));
        assert!(matches!(file.acquisition, AcquisitionSpec::Local(_)));
        if matches!(content_key, ImportContentKey::Declared(_)) {
            assert!(
                file.provenance
                    .conversions
                    .iter()
                    .any(|text| text.contains("local file"))
            );
            assert_eq!(
                file.expected.digests,
                candidate.source().plan().imported().files[0]
                    .expected
                    .digests
            );
            assert_eq!(
                bytes(&candidate.source().content()[content_key]),
                b"payload"
            );
        } else {
            assert!(file.expected.accepted_observation.is_some());
        }
    }
    assert_eq!(
        layers,
        BTreeSet::from([
            ContentLayer::Common,
            ContentLayer::CommonOverride,
            ContentLayer::Client,
            ContentLayer::Server
        ])
    );
}
#[tokio::test]
async fn import_candidate_rejects_inventory_and_semantic_changes() {
    use empack_core::model::DependencyKey;
    use empack_core::requirements::Requirement;
    for case in 0..4 {
        let archive = source(
            "modrinth.index.json",
            mr(vec![]),
            &[("overrides/config/a", b"a"), ("overrides/config/b", b"b")],
        );
        let outcome = interpret(
            archive,
            "http://127.0.0.1:1".into(),
            move |options| match case {
                0 => {
                    options.files.pop_first();
                }
                1 => {
                    options
                        .files
                        .values_mut()
                        .next()
                        .unwrap()
                        .requirements
                        .client = Requirement::Unsupported;
                }
                2 => {
                    for decision in options.files.values_mut() {
                        decision.key = DependencyKey::parse("same").unwrap();
                    }
                }
                _ => {
                    options.loader = Some(0);
                }
            },
        )
        .await;
        assert!(
            matches!(&*outcome, OperationOutcome::Completed(Err(_))),
            "case {case}"
        );
    }
}
#[tokio::test]
async fn import_candidate_requires_a_safe_durable_url_or_explicit_conversion() {
    let mut server = Server::new_async().await;
    server
        .mock("GET", "/payload")
        .with_body("payload")
        .create_async()
        .await;
    let archive = source(
        "modrinth.index.json",
        mr(vec![remote(
            format!("{}/payload", server.url()),
            "mods/chosen.jar",
            b"payload",
        )]),
        &[],
    );
    let outcome = interpret(archive, server.url(), |options| {
        options.files.values_mut().next().unwrap().persistence =
            super::super::ImportPersistence::Url;
    })
    .await;
    assert!(
        matches!(&*outcome, OperationOutcome::Completed(Err(error)) if error.to_string().contains("credential-free HTTPS"))
    );
}
#[tokio::test]
async fn import_candidate_preserves_provider_pins_weak_digests_and_optional_choices() {
    use empack_core::{model::*, requirements::Requirement};
    let mut server = Server::new_async().await;
    let url = format!("{}/payload", server.url());
    provider(&mut server, 123, 456, Some(url.clone())).await;
    provider(&mut server, 124, 457, Some(url)).await;
    server
        .mock("GET", "/payload")
        .with_body("payload")
        .expect(2)
        .create_async()
        .await;
    let outcome = interpret(source("manifest.json", cf(), &[]), server.url(), |_| {}).await;
    let OperationOutcome::Completed(Ok(candidate)) = &*outcome else {
        panic!("provider candidate failed");
    };
    assert_eq!(
        candidate.project().intent().runtime.loader,
        LoaderKind::Fabric
    );
    for dependency in candidate.project().lock().dependencies.values() {
        assert!(matches!(dependency.identity, ResolvedIdentity::Provider(_)));
        let file = &dependency.files.as_slice()[0];
        assert!(matches!(file.acquisition, AcquisitionSpec::Provider { .. }));
        assert_eq!(file.expected.digests, file.provenance.declared_digests);
        assert!(file.expected.accepted_observation.is_none());
        if matches!(&dependency.selected.as_ref().unwrap().selection, empack_core::identity::PinSelector::CurseForgeFile(id) if id.get() == 456)
        {
            assert!(matches!(
                file.placements.as_slice()[0].requirements.client,
                Requirement::Optional(_)
            ));
        }
    }
}

#[tokio::test]
async fn import_candidate_keeps_url_files_as_url_roots_with_original_hashes() {
    use empack_core::model::*;
    let archive = source(
        "modrinth.index.json",
        mr(vec![remote(
            "https://example.org/different-name.bin".into(),
            "resourcepacks/chosen.zip",
            b"payload",
        )]),
        &[],
    );
    let runtime = OperationRuntime::new(governor(), 1);
    let mut handle = runtime
        .start(move |mut scope| async move {
            let result = async {
                let imported = inspect_source(&mut scope, archive).await?;
                let catalog = ProviderCatalog::for_loopback_tests("http://127.0.0.1:1", None);
                let plan =
                    ImportContentPlan::resolve(&mut scope, imported, &catalog, limits()).await?;
                let outcome = plan
                    .acquire(
                        &mut scope,
                        &HttpAcquisition::for_loopback_tests(),
                        BTreeMap::from([(ImportContentKey::Declared(0), observed(b"payload"))]),
                        SourceEvidencePolicy::Compatibility,
                    )
                    .await?;
                let ImportContentOutcome::Ready(content) = outcome else {
                    anyhow::bail!("unexpected input");
                };
                let mut options = candidate_options(&content);
                options.files.values_mut().next().unwrap().persistence =
                    super::super::ImportPersistence::Url;
                content.into_candidate(&mut scope, options).await
            }
            .await;
            Ok(result)
        })
        .unwrap();
    let outcome = handle.wait().await;
    runtime.shutdown().await;
    let OperationOutcome::Completed(Ok(candidate)) = &*outcome else {
        panic!("URL candidate failed");
    };
    let root = candidate.project().intent().roots.values().next().unwrap();
    let locked = candidate
        .project()
        .lock()
        .dependencies
        .values()
        .next()
        .unwrap();
    assert!(
        matches!(&root.source, SourceIntent::Url(urls) if urls.as_slice() == ["https://example.org/different-name.bin"])
    );
    assert!(matches!(root.version, VersionIntent::ContentPinned(_)));
    assert!(matches!(locked.identity, ResolvedIdentity::Url(_)));
    assert!(locked.selected.is_none());
    let file = &locked.files.as_slice()[0];
    assert_eq!(
        file.placements.as_slice()[0]
            .destination
            .relative()
            .as_str(),
        "resourcepacks/chosen.zip"
    );
    assert_eq!(
        file.expected.digests,
        candidate.source().plan().imported().files[0]
            .expected
            .digests
    );
}

#[tokio::test]
async fn auxiliary_member_exclusion_requires_a_decision_and_survives_empty_imports() {
    use empack_core::model::ExtensionValue;
    for exclude in [false, true] {
        let archive = source(
            "modrinth.index.json",
            mr(vec![]),
            &[("modlist.html", b"report")],
        );
        let outcome = interpret(archive, "http://127.0.0.1:1".into(), move |options| {
            options.exclude_auxiliary_members = exclude;
        })
        .await;
        if !exclude {
            assert!(
                matches!(&*outcome, OperationOutcome::Completed(Err(error)) if error.to_string().contains("auxiliary members"))
            );
        } else {
            let OperationOutcome::Completed(Ok(candidate)) = &*outcome else {
                panic!("explicit exclusion failed");
            };
            assert!(candidate.bindings().is_empty());
            let ExtensionValue::Object(provenance) =
                &candidate.project().intent().extensions["empack.import"]
            else {
                panic!("missing provenance");
            };
            assert_eq!(
                provenance["excluded-auxiliary-members"],
                ExtensionValue::List(vec![ExtensionValue::Text("modlist.html".into())])
            );
            assert_eq!(
                provenance["source-archive-sha256"],
                ExtensionValue::Text(
                    empack_core::digest::ExpectedDigest::Sha256(
                        *candidate.source().plan().imported().source_id().bytes()
                    )
                    .hex()
                )
            );
        }
    }
}

#[tokio::test]
async fn default_import_limits_fit_small_archives_under_the_native_host_budget() {
    let source = source(
        "modrinth.index.json",
        mr(vec![]),
        &[("overrides/config/value", b"configuration")],
    );
    let governor = ResourceGovernor::new(ResourceRequest {
        jobs: 1,
        memory_bytes: 512 << 20,
        scratch_bytes: 128 << 30,
        open_files: 512,
    });
    let runtime = OperationRuntime::new(governor.clone(), 1);
    let mut handle = runtime
        .start(move |mut scope| async move {
            let result = async {
                let imported = inspect_import(&mut scope, source, ImportLimits::default()).await?;
                let catalog = ProviderCatalog::for_loopback_tests("http://127.0.0.1:1", None);
                ImportContentPlan::resolve(
                    &mut scope,
                    imported,
                    &catalog,
                    ImportContentLimits::default(),
                )
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
    let content = ready(&outcome);
    assert_eq!(content.content().len(), 1);
    assert_eq!(
        bytes(content.content().values().next().unwrap()),
        b"configuration"
    );
    drop(handle);
    drop(runtime);
    drop(outcome);
    assert_eq!(governor.status().reserved, ResourceRequest::default());
}

#[tokio::test]
async fn local_provider_associations_require_exact_identity_and_original_digests() {
    let mut server = Server::new_async().await;
    for (project, file) in [(123, 456), (124, 457)] {
        server.mock("GET", format!("/mods/{project}").as_str()).with_body(json!({"data":{"id":project,"gameId":432,"classId":6,"slug":format!("mod-{project}"),"name":"Mod"}}).to_string()).create_async().await;
        server.mock("GET", format!("/mods/{project}/files/{file}").as_str())
            .with_body(json!({"data":{"id":file,"gameId":432,"modId":project,"fileName":"shared.jar","fileLength":7,"hashes":[{"algo":2,"value":"321c3cf486ed509164edec1e1981fec8"}],"downloadUrl":null,"gameVersions":["1.21.1","Fabric"],"dependencies":[]}}).to_string())
            .create_async().await;
    }
    let catalog = ProviderCatalog::for_loopback_tests(&server.url(), Some("fixture".into()));
    let archive = source("manifest.json", cf(), &[]);
    let governed = governor();
    let runtime = OperationRuntime::new(governed.clone(), 1);
    let mut handle = runtime
        .start(move |mut scope| async move {
            let result = async {
                let imported = inspect_source(&mut scope, archive).await?;
                let plan =
                    ImportContentPlan::resolve(&mut scope, imported, &catalog, limits()).await?;
                let dir = tempfile::tempdir()?;
                let path = dir.path().join("renamed.zip");
                std::fs::write(&path, b"payload")?;
                for selector in [
                    "shared.jar",
                    "provider:modrinth:123:456:shared.jar",
                    "override:0",
                ] {
                    let result = plan
                        .acquire_local_files(
                            &mut scope,
                            &[ImportLocalFile {
                                selector: selector.into(),
                                source: path.clone(),
                            }],
                            BTreeMap::new(),
                            SourceEvidencePolicy::Compatibility,
                        )
                        .await;
                    assert!(result.is_err(), "ambiguous or foreign selector accepted");
                }
                let files = [
                    "provider:curseforge:123:456:shared.jar",
                    "provider:curseforge:124:457:shared.jar",
                ]
                .map(|selector| ImportLocalFile {
                    selector: selector.into(),
                    source: path.clone(),
                });
                std::fs::write(&path, b"changed")?;
                assert!(
                    plan.acquire_local_files(
                        &mut scope,
                        &files,
                        BTreeMap::new(),
                        SourceEvidencePolicy::Compatibility
                    )
                    .await
                    .is_err()
                );
                std::fs::write(&path, b"payload")?;
                #[cfg(unix)]
                {
                    let link = dir.path().join("linked.jar");
                    std::os::unix::fs::symlink(&path, &link)?;
                    assert!(
                        plan.acquire_local_files(
                            &mut scope,
                            &[ImportLocalFile {
                                selector: files[0].selector.clone(),
                                source: link
                            }],
                            BTreeMap::new(),
                            SourceEvidencePolicy::Compatibility
                        )
                        .await
                        .is_err()
                    );
                }
                let provided = plan
                    .acquire_local_files(
                        &mut scope,
                        &files,
                        BTreeMap::new(),
                        SourceEvidencePolicy::Compatibility,
                    )
                    .await?;
                let outcome = plan
                    .acquire(
                        &mut scope,
                        &HttpAcquisition::for_loopback_tests(),
                        provided,
                        SourceEvidencePolicy::Compatibility,
                    )
                    .await?;
                let ImportContentOutcome::Ready(content) = outcome else {
                    panic!("explicit verified files were not accepted")
                };
                assert_eq!(content.content().len(), 2);
                for value in content.content().values() {
                    assert_eq!(bytes(value), b"payload");
                }
                Ok::<_, anyhow::Error>(())
            }
            .await;
            Ok(result)
        })
        .unwrap();
    let outcome = handle.wait().await;
    runtime.shutdown().await;
    match &*outcome {
        OperationOutcome::Completed(Ok(())) => {}
        OperationOutcome::Completed(Err(error)) => panic!("{error:#}"),
        _ => panic!("import association did not complete"),
    }
    assert_eq!(governed.status().reserved, ResourceRequest::default());
}

#[tokio::test]
async fn saved_import_accumulates_exact_verified_associations_across_restarts() {
    use super::suspension::{load_pending_import, save_pending_import};
    let root = tempfile::tempdir().unwrap();
    let mut server = Server::new_async().await;
    provider(&mut server, 123, 456, None).await;
    provider(&mut server, 124, 457, None).await;
    for step in 0..3 {
        let state = root.path().join("state");
        let target = root.path().join("project");
        let origin = server.url();
        let governor = ResourceGovernor::new(ResourceRequest {
            jobs: 1,
            memory_bytes: 64 << 20,
            scratch_bytes: 1 << 20,
            open_files: 64,
        });
        let runtime = OperationRuntime::new(governor.clone(), 1);
        let mut handle = runtime
            .start(move |mut scope| async move {
                let result: Result<()> = async {
                    if step == 1 {
                        let blob = std::fs::read_dir(state.join("pending-import-content"))?
                            .map(|entry| entry.unwrap().path())
                            .find(|path| path.extension().is_some_and(|value| value == "blob"))
                            .unwrap();
                        let bytes = std::fs::read(&blob)?;
                        std::fs::write(&blob, vec![0; bytes.len()])?;
                        assert!(
                            load_pending_import(&mut scope, state.clone(), target.clone())
                                .await
                                .is_err()
                        );
                        std::fs::remove_file(&blob)?;
                        assert!(
                            load_pending_import(&mut scope, state.clone(), target.clone())
                                .await
                                .is_err()
                        );
                        std::fs::write(&blob, bytes)?;
                    }
                    let mut resumed = if step == 0 {
                        None
                    } else {
                        Some(
                            load_pending_import(&mut scope, state.clone(), target.clone())
                                .await?
                                .unwrap(),
                        )
                    };
                    let archive = resumed
                        .as_ref()
                        .map(|value| value.archive.clone())
                        .unwrap_or_else(|| source("manifest.json", cf(), &[]));
                    let imported = inspect_source(&mut scope, archive).await?;
                    let catalog =
                        ProviderCatalog::for_loopback_tests(&origin, Some("fixture-key".into()));
                    let mut plan =
                        ImportContentPlan::resolve(&mut scope, imported, &catalog, limits())
                            .await?;
                    if step == 1 {
                        let size = plan.needs[0].expected.size;
                        plan.needs[0].expected.size = Some(999);
                        assert!(
                            resumed
                                .take()
                                .unwrap()
                                .restore(
                                    &mut scope,
                                    &plan,
                                    BTreeMap::new(),
                                    SourceEvidencePolicy::Compatibility
                                )
                                .await
                                .is_err()
                        );
                        plan.needs[0].expected.size = size;
                        resumed = Some(
                            load_pending_import(&mut scope, state.clone(), target.clone())
                                .await?
                                .unwrap(),
                        );
                    }
                    let (mut provided, prior) = if let Some(resumed) = resumed {
                        let (provided, saved) = resumed
                            .restore(
                                &mut scope,
                                &plan,
                                BTreeMap::new(),
                                SourceEvidencePolicy::Compatibility,
                            )
                            .await?;
                        (provided, Some(saved))
                    } else {
                        (BTreeMap::new(), None)
                    };
                    assert_eq!(provided.len(), usize::from(step == 2));
                    if step > 0 {
                        provided.insert(plan.needs[step - 1].key.clone(), observed(b"payload"));
                    }
                    match plan
                        .acquire(
                            &mut scope,
                            &HttpAcquisition::for_loopback_tests(),
                            provided,
                            SourceEvidencePolicy::Compatibility,
                        )
                        .await?
                    {
                        ImportContentOutcome::NeedsInput {
                            plan,
                            pending,
                            provided,
                        } => {
                            assert!(step < 2);
                            assert_eq!(pending.len(), 2 - step);
                            save_pending_import(
                                &mut scope,
                                state,
                                target.clone(),
                                plan,
                                provided,
                                SourceEvidencePolicy::Compatibility,
                                prior,
                            )
                            .await?;
                        }
                        ImportContentOutcome::Ready(content) => {
                            assert_eq!(step, 2);
                            assert_eq!(content.content().len(), 2);
                            for file in content.content().values() {
                                assert!(matches!(
                                    file.evidence(),
                                    empack_core::digest::IntegrityEvidence::MatchedExpected { .. }
                                ));
                            }
                        }
                    }
                    assert!(!target.exists());
                    Ok(())
                }
                .await;
                Ok(result)
            })
            .unwrap();
        let outcome = handle.wait().await;
        runtime.shutdown().await;
        match &*outcome {
            OperationOutcome::Completed(Ok(())) => {}
            OperationOutcome::Completed(Err(error)) => panic!("restart {step}: {error:#}"),
            _ => panic!("restart {step} did not complete"),
        }
        assert_eq!(governor.status().reserved, ResourceRequest::default());
    }
}

#[tokio::test]
async fn restricted_import_reuses_verified_cache_without_download_locators() {
    use crate::engine::content::{
        cache::ContentCache,
        store::{ContentStoreLimits, FileContentStore},
    };
    let mut server = Server::new_async().await;
    provider(&mut server, 123, 456, None).await;
    provider(&mut server, 124, 457, None).await;
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("content");
    let store = FileContentStore::open(&path, ContentStoreLimits::default()).unwrap();
    let cache = ContentCache::new(path.clone(), ContentStoreLimits::default()).unwrap();
    let archive = source("manifest.json", cf(), &[]);
    let catalog = ProviderCatalog::for_loopback_tests(&server.url(), Some("fixture-key".into()));
    let selected_governor = governor();
    let runtime = OperationRuntime::new(selected_governor.clone(), 1);
    let mut handle = runtime.start(move |mut scope| async move {
        let result = async {
            let imported = inspect_source(&mut scope, archive.clone()).await?;
            let plan = ImportContentPlan::resolve(&mut scope, imported, &catalog, limits()).await?;
            store.publish_expected(&mut scope, observed(b"payload"), plan.needs[0].expected.clone()).await?;
            let snapshot = || std::fs::read_dir(&path).unwrap().map(|entry| {
                let entry = entry.unwrap();
                (entry.file_name(), (std::fs::read(entry.path()).unwrap(), entry.metadata().unwrap().modified().unwrap()))
            }).collect::<BTreeMap<_, _>>();
            let before = snapshot();
            let outcome = plan.acquire(&mut scope, &HttpAcquisition::for_loopback_tests().with_cache_lookup(cache.clone()), BTreeMap::new(), SourceEvidencePolicy::Compatibility).await?;
            let ImportContentOutcome::Ready(verified) = outcome else { anyhow::bail!("Restricted bytes already cached but import still needs input"); };
            assert_eq!(verified.content().len(), 2);
            for content in verified.content().values() {
                assert!(matches!(content.evidence(), empack_core::digest::IntegrityEvidence::MatchedExpected { expected, .. } if expected.strongest() == empack_core::digest::DigestAlgorithm::Md5));
            }
            assert_eq!(snapshot(), before);
            drop(verified);
            for mode in ["strong", "budget", "corrupt"] {
                if mode == "corrupt" {
                    let blob = std::fs::read_dir(&path)?.map(|entry| entry.unwrap().path()).find(|path| path.extension().is_some_and(|ext| ext == "blob")).unwrap();
                    std::fs::write(blob, b"changed")?;
                }
                let before = snapshot();
                let imported = inspect_source(&mut scope, archive.clone()).await?;
                let mut selected_limits = limits();
                if mode == "budget" { selected_limits.transfer.transfer_bytes = 13; }
                let mut plan = ImportContentPlan::resolve(&mut scope, imported, &catalog, selected_limits).await?;
                if mode == "budget" {
                    // The automatic phase must inherit bytes already consumed by a restricted hit.
                    plan.needs[1].source = ImportedAcquisition::Downloads(vec!["http://127.0.0.1:9/payload".into()]);
                }
                let outcome = plan.acquire(&mut scope, &HttpAcquisition::for_loopback_tests().with_cache_lookup(cache.clone()), BTreeMap::new(),
                    if mode == "strong" { SourceEvidencePolicy::StrongSourceRequired } else { SourceEvidencePolicy::Compatibility }).await;
                if mode == "corrupt" {
                    let ImportContentOutcome::NeedsInput { pending, provided, .. } = outcome? else { anyhow::bail!("corrupt cache satisfied import"); };
                    assert_eq!(pending.len(), 2);
                    assert!(provided.is_empty());
                } else {
                    assert!(outcome.is_err(), "{mode} allowance was bypassed");
                }
                assert_eq!(snapshot(), before);
            }
            Ok::<_, anyhow::Error>(())
        }.await;
        Ok(result)
    }).unwrap();
    let outcome = handle.wait().await;
    let OperationOutcome::Completed(result) = &*outcome else {
        panic!("import worker failed");
    };
    assert!(result.is_ok(), "{result:?}");
    runtime.release_completed(handle.id());
    drop((outcome, handle));
    runtime.shutdown().await;
    assert_eq!(
        selected_governor.status().reserved,
        ResourceRequest::default()
    );
}
