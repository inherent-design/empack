use super::*;
use crate::engine::{
    resources::ResourceGovernor,
    runtime::{OperationOutcome, OperationRuntime},
};
use empack_core::{
    digest::DigestAlgorithm,
    identity::{CurseForgeFileId, CurseForgeProjectId, ModrinthProjectId, ModrinthVersionId},
    model::ProviderKind,
};
use serde_json::{Value, json};

fn mr_project() -> Value {
    json!({"id":"AANobbMI","slug":"sodium","title":"Sodium","project_type":"mod","client_side":"required","server_side":"unsupported","loaders":["fabric"]})
}
fn mr_version() -> Value {
    json!({"id":"abcdefgh","project_id":"AANobbMI","game_versions":["1.20.1"],"loaders":["fabric"],"environment":"client_only",
        "files":[{"filename":"sodium.jar","primary":true,"size":7,"hashes":{"sha1":"11".repeat(20),"sha512":"22".repeat(64)},"url":"https://cdn.modrinth.com/content.jar"}],
        "dependencies":[{"project_id":"P7dR8mSH","version_id":null,"file_name":null,"dependency_type":"required"}]})
}
#[test]
fn provider_export_origins_exclude_execution_only_credentials() {
    let result = modrinth::selection(
        modrinth::project(&bytes(&mr_project())).unwrap(),
        &pin(ProviderKind::Modrinth),
        &bytes(&mr_version()),
    )
    .unwrap();
    let mut file = result.files.into_vec().remove(0);
    file.alternatives = vec![
        "https://cdn.modrinth.com/content.jar".into(),
        "https://cdn.example.com/file?X-Amz-Signature=fixture-secret".into(),
        "https://cdn.example.com/file?%61pi_key=fixture-secret".into(),
        "https://user:fixture-secret@cdn.example.com/file".into(),
        "http://cdn.example.com/file".into(),
        "https://cdn.example.com/file#fixture-secret".into(),
        "https://mirror.example.com/file?version=123".into(),
    ];
    assert_eq!(
        file.persistent_alternatives(),
        vec![
            "https://cdn.modrinth.com/content.jar",
            "https://mirror.example.com/file?version=123",
        ]
    );
    assert_eq!(
        file.alternatives.len(),
        7,
        "Execution alternatives remain intact"
    );
}
fn cf_project() -> Value {
    json!({"data":{"id":394468,"gameId":432,"slug":"sodium","name":"Sodium","classId":6}})
}
fn cf_file() -> Value {
    json!({"data":{"id":456,"gameId":432,"modId":394468,"fileName":"sodium.jar","fileLength":7,"downloadUrl":null,
        "hashes":[{"value":"33".repeat(16),"algo":2}],"gameVersions":["1.20.1","Fabric"],"dependencies":[{"modId":306612,"relationType":3}]}})
}
fn pin(provider: ProviderKind) -> ResolvedPin {
    match provider {
        ProviderKind::Modrinth => ResolvedPin {
            project: ProviderProjectId::Modrinth(ModrinthProjectId::parse("AANobbMI").unwrap()),
            selection: PinSelector::ModrinthVersion(ModrinthVersionId::parse("abcdefgh").unwrap()),
        },
        ProviderKind::CurseForge => ResolvedPin {
            project: ProviderProjectId::CurseForge(CurseForgeProjectId::parse("394468").unwrap()),
            selection: PinSelector::CurseForgeFile(CurseForgeFileId::parse("456").unwrap()),
        },
    }
}
fn bytes(value: &Value) -> Vec<u8> {
    serde_json::to_vec(value).unwrap()
}
fn limits() -> CatalogLimits {
    CatalogLimits {
        response_bytes: 4096,
        transfer_bytes: 8192,
        deadline: Duration::from_secs(3),
    }
}
fn governor() -> ResourceGovernor {
    ResourceGovernor::new(ResourceRequest {
        jobs: 2,
        memory_bytes: 1 << 20,
        open_files: 4,
        ..Default::default()
    })
}
fn catalog(origin: &str) -> ProviderCatalog {
    ProviderCatalog {
        transport: transport::CatalogTransport::test(origin, Some("fixture-secret".into())),
    }
}
type ExactOutcome = Arc<OperationOutcome<Result<RetainedOutput<ProviderResolution>>>>;
async fn resolve(
    catalog: ProviderCatalog,
    pin: ResolvedPin,
    limits: CatalogLimits,
) -> (ExactOutcome, ResourceGovernor) {
    let governor = governor();
    let runtime = OperationRuntime::new(governor.clone(), 2);
    let mut handle =
        runtime
            .start(move |mut scope| async move {
                Ok(catalog.resolve_exact(&mut scope, pin, limits).await)
            })
            .unwrap();
    let result = handle.wait().await;
    runtime.release_completed(handle.id());
    runtime.shutdown().await;
    (result, governor)
}
fn exact(
    result: &OperationOutcome<Result<RetainedOutput<ProviderResolution>>>,
) -> &ProviderResolution {
    match result {
        OperationOutcome::Completed(Ok(value)) => value,
        OperationOutcome::Completed(Err(e)) => panic!("resolution failed: {e:#}"),
        _ => panic!("operation failed"),
    }
}
fn error(result: &OperationOutcome<Result<RetainedOutput<ProviderResolution>>>) -> &anyhow::Error {
    match result {
        OperationOutcome::Completed(Err(value)) => value,
        _ => panic!("expected classified catalog error"),
    }
}

#[tokio::test]
async fn equivalent_modrinth_selectors_resolve_canonical_identity_and_kind() {
    let mut server = mockito::Server::new_async().await;
    let mut source = mr_project();
    source["project_type"] = json!("resourcepack");
    let slug = server
        .mock("GET", "/project/sodium")
        .match_header("x-api-key", mockito::Matcher::Missing)
        .with_body(bytes(&source))
        .expect(2)
        .create_async()
        .await;
    let id = server
        .mock("GET", "/project/AANobbMI")
        .with_body(bytes(&source))
        .create_async()
        .await;
    let runtime = OperationRuntime::new(governor(), 4);
    for input in [
        "sodium",
        "AANobbMI",
        "https://modrinth.com/resourcepack/sodium",
    ] {
        let catalog = catalog(&server.url());
        let selector = ProjectSelector::parse(ProviderKind::Modrinth, input).unwrap();
        let mut handle = runtime
            .start(move |mut scope| async move {
                Ok(catalog
                    .resolve_selector(&mut scope, selector, limits())
                    .await)
            })
            .unwrap();
        let result = handle.wait().await;
        match &*result {
            OperationOutcome::Completed(Ok(project)) => {
                assert_eq!(project.id, pin(ProviderKind::Modrinth).project);
                assert_eq!(project.kinds.as_slice(), &[ContentKind::ResourcePack]);
            }
            _ => panic!("selector resolution failed"),
        }
        runtime.release_completed(handle.id());
    }
    runtime.shutdown().await;
    slug.assert_async().await;
    id.assert_async().await;
}
#[tokio::test]
async fn exact_files_keep_all_assertions_environment_edges_and_admission() {
    let mut server = mockito::Server::new_async().await;
    let project = server
        .mock("GET", "/project/AANobbMI")
        .with_body(bytes(&mr_project()))
        .create_async()
        .await;
    let mut version = mr_version();
    let mut extra = version["files"][0].clone();
    extra["filename"] = json!("resources.zip");
    extra["primary"] = json!(false);
    extra["file_type"] = json!("required-resource-pack");
    extra["url"] = json!("https://cdn.example/resources.zip?token=sensitive");
    version["files"].as_array_mut().unwrap().push(extra);
    let file = server
        .mock("GET", "/version/abcdefgh")
        .with_body(bytes(&version))
        .create_async()
        .await;
    let (result, governor) = resolve(
        catalog(&server.url()),
        pin(ProviderKind::Modrinth),
        limits(),
    )
    .await;
    let resolution = exact(&result);
    assert_eq!(resolution.files.as_slice().len(), 2);
    assert_eq!(
        resolution.files.as_slice()[0]
            .expected
            .digests
            .as_ref()
            .unwrap()
            .strongest(),
        DigestAlgorithm::Sha512
    );
    assert_eq!(
        resolution.files.as_slice()[1].role.as_deref(),
        Some("required-resource-pack")
    );
    assert_eq!(
        resolution.environment.version.as_deref(),
        Some("client_only")
    );
    assert_eq!(
        resolution.dependencies[0].relation,
        DependencyRelation::Required
    );
    assert_eq!(resolution.coverage, Coverage::CompleteForSelection);
    assert_eq!(
        governor.status().reserved.memory_bytes,
        ((bytes(&mr_project()).len() + bytes(&version).len()) * 16 + 4096) as u64
    );
    assert_eq!(governor.status().reserved.jobs, 0);
    drop(result);
    assert_eq!(governor.status().reserved, ResourceRequest::default());
    project.assert_async().await;
    file.assert_async().await;
}
#[tokio::test]
async fn curseforge_restricted_files_keep_md5_and_project_ownership() {
    let mut server = mockito::Server::new_async().await;
    let project = server
        .mock("GET", "/mods/394468")
        .match_header("x-api-key", "fixture-secret")
        .with_body(bytes(&cf_project()))
        .create_async()
        .await;
    let file = server
        .mock("GET", "/mods/394468/files/456")
        .match_header("x-api-key", "fixture-secret")
        .with_body(bytes(&cf_file()))
        .create_async()
        .await;
    let (result, _) = resolve(
        catalog(&server.url()),
        pin(ProviderKind::CurseForge),
        limits(),
    )
    .await;
    let result = exact(&result);
    assert_eq!(result.pin, pin(ProviderKind::CurseForge));
    let content = &result.files.as_slice()[0];
    assert!(content.alternatives.is_empty());
    assert_eq!(content.expected.size, Some(7));
    assert_eq!(
        content.expected.digests.as_ref().unwrap().strongest(),
        DigestAlgorithm::Md5
    );
    assert_eq!(
        result.dependencies[0].project.as_ref().unwrap().to_string(),
        "306612"
    );
    assert_eq!(result.loaders, ["fabric"]);
    assert!(result.environment.client.is_none());
    project.assert_async().await;
    file.assert_async().await;
}
#[test]
fn explicit_ids_wrong_owners_and_malformed_files_fail_before_resolution() {
    let selector = ProjectSelector::canonical(pin(ProviderKind::Modrinth).project);
    let mut project = mr_project();
    project["id"] = json!("XXXXXXXX");
    assert!(decode_project(&selector, &bytes(&project)).is_err());
    for (field, value) in [
        ("id", json!("XXXXXXXX")),
        ("project_id", json!("YYYYYYYY")),
        ("files", json!([])),
    ] {
        let mut version = mr_version();
        version[field] = value;
        assert!(
            modrinth::selection(
                modrinth::project(&bytes(&mr_project())).unwrap(),
                &pin(ProviderKind::Modrinth),
                &bytes(&version)
            )
            .is_err()
        );
    }
    for (field, value) in [
        ("filename", json!("../escape.jar")),
        ("hashes", json!({"sha1":"short"})),
        ("url", json!("https://user:secret@cdn.example/file")),
    ] {
        let mut version = mr_version();
        version["files"][0][field] = value;
        assert!(
            modrinth::selection(
                modrinth::project(&bytes(&mr_project())).unwrap(),
                &pin(ProviderKind::Modrinth),
                &bytes(&version)
            )
            .is_err()
        );
    }
    let mut version = mr_version();
    version["files"]
        .as_array_mut()
        .unwrap()
        .push(mr_version()["files"][0].clone());
    assert!(
        modrinth::selection(
            modrinth::project(&bytes(&mr_project())).unwrap(),
            &pin(ProviderKind::Modrinth),
            &bytes(&version)
        )
        .is_err()
    );
    let mut file = cf_file();
    file["data"]["modId"] = json!(999);
    assert!(
        curseforge::selection(
            curseforge::project(&bytes(&cf_project()), false).unwrap(),
            &pin(ProviderKind::CurseForge),
            &bytes(&file)
        )
        .is_err()
    );
}
#[test]
fn absent_and_partial_dependency_evidence_never_claims_a_complete_closure() {
    let mut version = mr_version();
    version.as_object_mut().unwrap().remove("dependencies");
    let result = modrinth::selection(
        modrinth::project(&bytes(&mr_project())).unwrap(),
        &pin(ProviderKind::Modrinth),
        &bytes(&version),
    )
    .unwrap();
    assert_eq!(result.coverage, Coverage::Unknown);
    version["dependencies"] = json!([{"dependency_type":"required","file_name":"manual.jar"}, {"dependency_type":"optional","project_id":"P7dR8mSH"}]);
    let result = modrinth::selection(
        modrinth::project(&bytes(&mr_project())).unwrap(),
        &pin(ProviderKind::Modrinth),
        &bytes(&version),
    )
    .unwrap();
    assert_eq!(result.coverage, Coverage::Partial);
    assert_eq!(
        result.dependencies[0].filename.as_deref(),
        Some("manual.jar")
    );
    assert_eq!(
        result.dependencies[1].relation,
        DependencyRelation::Optional
    );
}
#[test]
fn selectors_reject_credentialed_foreign_and_ambiguous_urls() {
    for input in [
        "",
        "..",
        "a/b",
        "sodium?token=secret",
        "https://evil.example/mod/sodium",
        "https://modrinth.com/mod/sodium/version/abcdefgh",
        "https://user:secret@modrinth.com/mod/sodium",
        "https://modrinth.com/mod/sodium?token=secret",
    ] {
        assert!(
            ProjectSelector::parse(ProviderKind::Modrinth, input).is_err(),
            "accepted {input}"
        );
    }
    assert!(ProjectSelector::parse(ProviderKind::CurseForge, "0394468").is_err());
    let selector = ProjectSelector::parse(
        ProviderKind::CurseForge,
        "https://www.curseforge.com/minecraft/mc-mods/sodium",
    )
    .unwrap();
    assert!(selector.is_slug());
    assert_eq!(selector.value(), "sodium");
}
#[tokio::test]
async fn curseforge_slug_uses_exact_search_and_rejects_ambiguous_results() {
    let mut server = mockito::Server::new_async().await;
    let response = server
        .mock("GET", "/mods/search")
        .match_query(mockito::Matcher::AllOf(vec![
            mockito::Matcher::UrlEncoded("slug".into(), "sodium".into()),
            mockito::Matcher::UrlEncoded("gameId".into(), "432".into()),
            mockito::Matcher::UrlEncoded("pageSize".into(), "2".into()),
        ]))
        .with_body(bytes(&json!({"data":[cf_project()["data"].clone()]})))
        .create_async()
        .await;
    let runtime = OperationRuntime::new(governor(), 1);
    let catalog = catalog(&server.url());
    let mut handle = runtime
        .start(move |mut scope| async move {
            Ok(catalog
                .resolve_selector(
                    &mut scope,
                    ProjectSelector::parse(ProviderKind::CurseForge, "sodium").unwrap(),
                    limits(),
                )
                .await)
        })
        .unwrap();
    let outcome = handle.wait().await;
    assert!(
        matches!(&*outcome, OperationOutcome::Completed(Ok(value)) if value.id == pin(ProviderKind::CurseForge).project)
    );
    runtime.shutdown().await;
    response.assert_async().await;
    assert!(
        curseforge::project(
            &bytes(&json!({"data":[cf_project()["data"].clone(), cf_project()["data"].clone()]})),
            true
        )
        .is_err()
    );
}
#[tokio::test]
async fn transport_classifies_failures_and_never_follows_a_redirect_with_credentials() {
    let mut server = mockito::Server::new_async().await;
    let mut elsewhere = mockito::Server::new_async().await;
    let no_redirect = elsewhere
        .mock("GET", "/stolen")
        .expect(0)
        .create_async()
        .await;
    for status in [302, 401, 403, 404, 429, 503, 206] {
        let response = server
            .mock("GET", "/mods/394468")
            .with_status(status)
            .with_header("location", &format!("{}/stolen", elsewhere.url()))
            .with_header("retry-after", "0")
            .with_body("secret response body")
            .expect(if status == 429 || status == 503 { 3 } else { 1 })
            .create_async()
            .await;
        let (result, governor) = resolve(
            catalog(&server.url()),
            pin(ProviderKind::CurseForge),
            limits(),
        )
        .await;
        let error = error(&result);
        let classified = error.downcast_ref::<CatalogError>().unwrap();
        assert!(match status {
            302 => matches!(classified, CatalogError::Redirect),
            401 | 403 => matches!(classified, CatalogError::Unauthorized),
            404 => matches!(classified, CatalogError::NotFound),
            429 => matches!(classified, CatalogError::RateLimited),
            503 => matches!(classified, CatalogError::Server(503)),
            206 => matches!(classified, CatalogError::Status(206)),
            _ => false,
        });
        assert!(!format!("{error:#}").contains("secret"));
        assert_eq!(governor.status().reserved, ResourceRequest::default());
        response.assert_async().await;
    }
    no_redirect.assert_async().await;
}
#[tokio::test]
async fn response_and_total_limits_apply_to_chunked_bodies_before_parsing() {
    let mut server = mockito::Server::new_async().await;
    let response = server
        .mock("GET", "/project/AANobbMI")
        .with_chunked_body(|writer| {
            for _ in 0..16 {
                if writer.write_all(&[b'x'; 128]).is_err() {
                    break;
                }
            }
            Ok(())
        })
        .create_async()
        .await;
    let (result, _) = resolve(
        catalog(&server.url()),
        pin(ProviderKind::Modrinth),
        CatalogLimits {
            response_bytes: 512,
            ..limits()
        },
    )
    .await;
    assert!(matches!(
        error(&result).downcast_ref::<CatalogError>(),
        Some(CatalogError::Limit)
    ));
    response.assert_async().await;
    let project = server
        .mock("GET", "/project/AANobbMI")
        .with_body(bytes(&mr_project()))
        .create_async()
        .await;
    let file = server
        .mock("GET", "/version/abcdefgh")
        .with_body(bytes(&mr_version()))
        .create_async()
        .await;
    let (result, _) = resolve(
        catalog(&server.url()),
        pin(ProviderKind::Modrinth),
        CatalogLimits {
            transfer_bytes: (bytes(&mr_project()).len() + bytes(&mr_version()).len() - 1) as u64,
            ..limits()
        },
    )
    .await;
    assert!(matches!(
        error(&result).downcast_ref::<CatalogError>(),
        Some(CatalogError::Limit)
    ));
    project.assert_async().await;
    file.assert_async().await;
}
#[tokio::test]
async fn retry_waits_remain_inside_the_original_deadline() {
    let mut server = mockito::Server::new_async().await;
    let response = server
        .mock("GET", "/project/AANobbMI")
        .with_status(429)
        .with_header("retry-after", "10")
        .create_async()
        .await;
    let start = std::time::Instant::now();
    let (result, _) = resolve(
        catalog(&server.url()),
        pin(ProviderKind::Modrinth),
        CatalogLimits {
            deadline: Duration::from_millis(100),
            ..limits()
        },
    )
    .await;
    assert!(matches!(
        error(&result).downcast_ref::<CatalogError>(),
        Some(CatalogError::Deadline)
    ));
    assert!(start.elapsed() < Duration::from_secs(2));
    response.assert_async().await;
}
#[tokio::test]
async fn cancellation_retires_network_and_all_reservations() {
    let mut server = mockito::Server::new_async().await;
    let response = server
        .mock("GET", "/project/AANobbMI")
        .with_status(429)
        .with_header("retry-after", "10")
        .create_async()
        .await;
    let catalog = catalog(&server.url());
    let governor = governor();
    let runtime = OperationRuntime::new(governor.clone(), 1);
    let mut handle = runtime
        .start(move |mut scope| async move {
            Ok(catalog
                .resolve_exact(&mut scope, pin(ProviderKind::Modrinth), limits())
                .await)
        })
        .unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while !response.matched_async().await {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    handle.cancel();
    tokio::time::timeout(Duration::from_secs(1), handle.wait())
        .await
        .unwrap();
    runtime.shutdown().await;
    assert_eq!(governor.status().reserved, ResourceRequest::default());
    response.assert_async().await;
}

#[tokio::test]
async fn missing_authentication_has_no_request_and_wrong_namespace_has_no_lookup() {
    let mut server = mockito::Server::new_async().await;
    let no_request = server
        .mock("GET", mockito::Matcher::Any)
        .expect(0)
        .create_async()
        .await;
    let catalog = ProviderCatalog {
        transport: transport::CatalogTransport::test(&server.url(), None),
    };
    let (outcome, _) = resolve(catalog.clone(), pin(ProviderKind::CurseForge), limits()).await;
    assert!(matches!(
        error(&outcome).downcast_ref::<CatalogError>(),
        Some(CatalogError::Unauthorized)
    ));
    let mut invalid = pin(ProviderKind::CurseForge);
    invalid.selection = pin(ProviderKind::Modrinth).selection;
    let (outcome, _) = resolve(catalog, invalid, limits()).await;
    assert!(matches!(&*outcome, OperationOutcome::Completed(Err(_))));
    no_request.assert_async().await;
}
#[test]
fn provider_classification_and_all_dependency_relations_survive_normalization() {
    for (class, kind) in [
        (6, ContentKind::Mod),
        (12, ContentKind::ResourcePack),
        (17, ContentKind::World),
        (6552, ContentKind::ShaderPack),
        (6945, ContentKind::DataPack),
    ] {
        let mut project = cf_project();
        project["data"]["classId"] = json!(class);
        assert_eq!(
            curseforge::project(&bytes(&project), false)
                .unwrap()
                .kinds
                .as_slice(),
            &[kind]
        );
    }
    let mut project = mr_project();
    project["loaders"] = json!(["datapack"]);
    assert_eq!(
        modrinth::project(&bytes(&project))
            .unwrap()
            .kinds
            .as_slice(),
        &[ContentKind::DataPack]
    );
    let mut file = cf_file();
    file["data"]["dependencies"] = json!(
        (1..=6)
            .map(|kind| json!({"modId":100+kind,"relationType":kind}))
            .collect::<Vec<_>>()
    );
    let result = curseforge::selection(
        curseforge::project(&bytes(&cf_project()), false).unwrap(),
        &pin(ProviderKind::CurseForge),
        &bytes(&file),
    )
    .unwrap();
    assert_eq!(
        result
            .dependencies
            .into_iter()
            .map(|dep| dep.relation)
            .collect::<Vec<_>>(),
        [
            DependencyRelation::Embedded,
            DependencyRelation::Optional,
            DependencyRelation::Required,
            DependencyRelation::Tool,
            DependencyRelation::Incompatible,
            DependencyRelation::Include
        ]
    );
    for (field, value) in [
        ("gameId", json!(999)),
        ("id", json!(999)),
        ("hashes", json!([])),
        ("dependencies", json!([{"modId":123,"relationType":99}])),
    ] {
        let mut file = cf_file();
        file["data"][field] = value;
        assert!(
            curseforge::selection(
                curseforge::project(&bytes(&cf_project()), false).unwrap(),
                &pin(ProviderKind::CurseForge),
                &bytes(&file)
            )
            .is_err()
        );
    }
}
#[tokio::test]
async fn safe_retry_can_complete_and_malformed_response_does_not_disclose_its_values() {
    let mut server = mockito::Server::new_async().await;
    let retry = server
        .mock("GET", "/project/AANobbMI")
        .with_status(503)
        .with_header("retry-after", "0")
        .expect(1)
        .create_async()
        .await;
    let project = server
        .mock("GET", "/project/AANobbMI")
        .with_body(bytes(&mr_project()))
        .expect(1)
        .create_async()
        .await;
    let file = server
        .mock("GET", "/version/abcdefgh")
        .with_body(bytes(&mr_version()))
        .create_async()
        .await;
    let (outcome, _) = resolve(
        catalog(&server.url()),
        pin(ProviderKind::Modrinth),
        limits(),
    )
    .await;
    assert_eq!(exact(&outcome).pin, pin(ProviderKind::Modrinth));
    retry.assert_async().await;
    project.assert_async().await;
    file.assert_async().await;
    let invalid = server
        .mock("GET", "/project/AANobbMI")
        .with_body(r#"{"id":{"secret":"must-not-leak"}}"#)
        .create_async()
        .await;
    let (outcome, _) = resolve(
        catalog(&server.url()),
        pin(ProviderKind::Modrinth),
        limits(),
    )
    .await;
    assert!(matches!(
        error(&outcome).downcast_ref::<CatalogError>(),
        Some(CatalogError::InvalidRecord)
    ));
    assert!(!format!("{:#}", error(&outcome)).contains("must-not-leak"));
    invalid.assert_async().await;
}

#[test]
fn locator_refresh_keeps_locked_assertions_and_refuses_ambiguous_or_changed_roles() {
    use empack_core::{
        digest::{ContentId, DigestSet},
        model::FileSlot,
    };
    let mut version = mr_version();
    let mut extra = version["files"][0].clone();
    extra["filename"] = json!("extra.zip");
    extra["primary"] = json!(false);
    extra["hashes"]["sha1"] = json!("99".repeat(20));
    extra["hashes"]["sha512"] = json!("88".repeat(64));
    version["files"].as_array_mut().unwrap().push(extra);
    let result = modrinth::selection(
        modrinth::project(&bytes(&mr_project())).unwrap(),
        &pin(ProviderKind::Modrinth),
        &bytes(&version),
    )
    .unwrap();
    let role = |name| FileSlot::parse(name).unwrap();
    let expected = result.files.as_slice()[1].expected.clone();
    assert_eq!(
        result
            .download_alternatives(&role("extra.zip"), &expected)
            .unwrap(),
        result.files.as_slice()[1].alternatives
    );
    assert_eq!(
        result
            .download_alternatives(&role("arbitrary-logical-role"), &expected)
            .unwrap(),
        result.files.as_slice()[1].alternatives
    );
    assert!(
        result
            .download_alternatives(&role("primary"), &expected)
            .is_err()
    );
    let mut unknown = expected.clone();
    unknown.digests = None;
    unknown.accepted_observation = Some(ContentId::from_sha256([1; 32]));
    assert!(
        result
            .download_alternatives(&role("arbitrary-logical-role"), &unknown)
            .is_err()
    );
    assert!(
        result
            .download_alternatives(&role("extra.zip"), &unknown)
            .is_ok()
    );
    let mut conflict = expected.clone();
    conflict.digests = Some(DigestSet::parse([("sha1", "00".repeat(20).as_str())]).unwrap());
    assert!(
        result
            .download_alternatives(&role("extra.zip"), &conflict)
            .is_err()
    );
    let mut wrong_size = expected.clone();
    wrong_size.size = Some(99);
    assert!(
        result
            .download_alternatives(&role("extra.zip"), &wrong_size)
            .is_err()
    );
    assert_eq!(result.files.as_slice()[1].expected, expected);
}

#[test]
fn exact_version_kinds_do_not_inherit_the_whole_project_union() {
    let mut project = mr_project();
    project["loaders"] = json!(["datapack", "fabric", "forge"]);
    let project = modrinth::project(&bytes(&project)).unwrap();
    assert_eq!(
        project.kinds.as_slice(),
        &[ContentKind::Mod, ContentKind::DataPack]
    );
    for (loaders, expected) in [
        (vec!["fabric"], vec![ContentKind::Mod]),
        (vec!["datapack"], vec![ContentKind::DataPack]),
        (
            vec!["datapack", "fabric"],
            vec![ContentKind::Mod, ContentKind::DataPack],
        ),
    ] {
        let mut file = mr_version();
        file["loaders"] = json!(loaders);
        let pin = ResolvedPin {
            project: project.id.clone(),
            selection: project.id.parse_pin(file["id"].as_str().unwrap()).unwrap(),
        };
        let resolved = modrinth::selection(project.clone(), &pin, &bytes(&file)).unwrap();
        assert_eq!(resolved.kinds.as_slice(), expected.as_slice());
        assert_eq!(
            resolved.project.kinds.as_slice(),
            &[ContentKind::Mod, ContentKind::DataPack]
        );
    }
}
