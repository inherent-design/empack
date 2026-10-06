use super::*;
use crate::engine::{
    resources::ResourceGovernor,
    runtime::{OperationOutcome, OperationRuntime},
};
use empack_core::identity::{CurseForgeFileId, ModrinthVersionId};
use mockito::{Matcher, Server};
use serde_json::json;
fn mr_pin() -> PinSelector {
    PinSelector::ModrinthVersion(ModrinthVersionId::parse("version1").unwrap())
}
fn mr_file() -> Value {
    json!({"id":"version1","project_id":"project1","files":[{"filename":"a.jar","primary":true,"size":1,"hashes":{"sha1":"00".repeat(20)},"url":"https://example.com/a.jar"}],"game_versions":["1.21.1"],"loaders":["fabric"],"dependencies":[]})
}
async fn resolve(
    server: &Server,
    pin: PinSelector,
    limit: u64,
) -> Arc<OperationOutcome<Result<RetainedOutput<ProviderResolution>>>> {
    let catalog = ProviderCatalog::for_loopback_tests(&server.url(), Some("fixture-key".into()));
    let governor = ResourceGovernor::new(ResourceRequest {
        jobs: 1,
        memory_bytes: 4 << 20,
        open_files: 4,
        ..Default::default()
    });
    let runtime = OperationRuntime::new(governor, 1);
    let mut handle = runtime
        .start(move |mut scope| async move {
            Ok(catalog
                .resolve_pin(
                    &mut scope,
                    pin,
                    CatalogLimits {
                        response_bytes: 4096,
                        transfer_bytes: limit,
                        deadline: Duration::from_secs(2),
                    },
                )
                .await)
        })
        .unwrap();
    let outcome = handle.wait().await;
    runtime.shutdown().await;
    outcome
}
#[tokio::test]
async fn version_only_modrinth_selector_resolves_owner_with_one_shared_budget() {
    let mut server = Server::new_async().await;
    let file = mr_file().to_string();
    server
        .mock("GET", "/version/version1")
        .with_body(&file)
        .create_async()
        .await;
    let project=json!({"id":"project1","slug":"project","title":"Project","project_type":"mod","loaders":["fabric"]}).to_string();
    server
        .mock("GET", "/project/project1")
        .with_body(&project)
        .create_async()
        .await;
    let outcome = resolve(&server, mr_pin(), 8192).await;
    let OperationOutcome::Completed(Ok(resolved)) = &*outcome else {
        panic!("pin did not resolve")
    };
    assert_eq!(resolved.pin.project.to_string(), "project1");
    assert_eq!(resolved.pin.selection, mr_pin());
    assert!(matches!(
        &*resolve(&server, mr_pin(), (file.len() + project.len() - 1) as u64).await,
        OperationOutcome::Completed(Err(_))
    ));
}
#[tokio::test]
async fn curseforge_file_lookup_preserves_exact_owner_and_restricted_evidence() {
    let mut server = Server::new_async().await;
    let request=server.mock("POST","/mods/files").match_header("x-api-key","fixture-key")
        .match_body(Matcher::Json(json!({"fileIds":[456]})))
        .with_body(json!({"data":[{"id":456,"gameId":432,"modId":123,"fileName":"a.jar","fileLength":1,"hashes":[{"algo":2,"value":"00".repeat(16)}],"downloadUrl":null,"gameVersions":["1.21.1","Fabric"],"dependencies":[]}]}).to_string())
        .create_async().await;
    server
        .mock("GET", "/mods/123")
        .with_body(
            json!({"data":{"id":123,"gameId":432,"classId":6,"slug":"project","name":"Project"}})
                .to_string(),
        )
        .create_async()
        .await;
    let outcome = resolve(
        &server,
        PinSelector::CurseForgeFile(CurseForgeFileId::parse("456").unwrap()),
        8192,
    )
    .await;
    let OperationOutcome::Completed(Ok(resolved)) = &*outcome else {
        panic!("file did not resolve")
    };
    assert_eq!(resolved.pin.project.to_string(), "123");
    assert!(resolved.files.as_slice()[0].alternatives.is_empty());
    request.assert_async().await;
}
#[test]
fn pin_lookup_cannot_substitute_other_records_or_games() {
    let mut file = mr_file();
    file["id"] = json!("different");
    assert!(owned_record(mr_pin(), &serde_json::to_vec(&file).unwrap()).is_err());
    let pin = PinSelector::CurseForgeFile(CurseForgeFileId::parse("456").unwrap());
    for data in [
        json!([]),
        json!([{"id":457,"gameId":432,"modId":123}]),
        json!([{"id":456,"gameId":1,"modId":123}]),
        json!([{"id":456,"gameId":432,"modId":123},{"id":456,"gameId":432,"modId":123}]),
    ] {
        assert!(
            owned_record(
                pin.clone(),
                &serde_json::to_vec(&json!({"data":data})).unwrap()
            )
            .is_err()
        );
    }
}
