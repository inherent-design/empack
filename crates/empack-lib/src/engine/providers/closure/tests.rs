use super::*;
use crate::engine::{
    resources::ResourceGovernor,
    runtime::{OperationOutcome, OperationRuntime},
};
use empack_core::identity::{ModrinthProjectId, ModrinthVersionId};
use mockito::{Matcher, Server};
use serde_json::{Value, json};
fn pin(project: &str, version: &str) -> ResolvedPin {
    ResolvedPin {
        project: ProviderProjectId::Modrinth(ModrinthProjectId::parse(project).unwrap()),
        selection: PinSelector::ModrinthVersion(ModrinthVersionId::parse(version).unwrap()),
    }
}
fn version(project: &str, id: &str, deps: Value) -> Value {
    json!({"id":id,"project_id":project,"files":[{"filename":format!("{project}.jar"),"primary":true,"size":1,"hashes":{"sha1":"00".repeat(20)},"url":format!("https://example.com/{project}.jar")}],"game_versions":["1.21.1"],"loaders":["fabric"],"dependencies":deps,"date_published":"2026-01-01T00:00:00Z","status":"listed","version_type":"release"})
}
async fn record(server: &mut Server, project: &str, id: &str, deps: Value) {
    server.mock("GET",format!("/project/{project}").as_str()).with_body(json!({"id":project,"slug":project,"title":project,"project_type":"mod","loaders":["fabric"]}).to_string()).create_async().await;
    server
        .mock("GET", format!("/version/{id}").as_str())
        .with_body(version(project, id, deps).to_string())
        .create_async()
        .await;
}
fn request(roots: Vec<ResolvedPin>) -> ClosureRequest {
    ClosureRequest {
        roots: NonEmpty::new(
            roots
                .into_iter()
                .map(|pin| ClosureRoot {
                    pin,
                    kind: ContentKind::Mod,
                })
                .collect(),
        )
        .unwrap(),
        game_versions: NonEmpty::new(vec![GameVersion::parse("1.21.1").unwrap()]).unwrap(),
        loader: LoaderKind::Fabric,
        releases: ReleasePolicy::PreferStable,
    }
}
fn limits() -> ClosureLimits {
    ClosureLimits {
        selection: SelectionLimits {
            catalog: CatalogLimits {
                response_bytes: 8192,
                transfer_bytes: 65536,
                deadline: Duration::from_secs(3),
            },
            ..Default::default()
        },
        projects: 8,
        edges: 32,
    }
}
async fn resolve(
    server: &Server,
    request: ClosureRequest,
    limits: ClosureLimits,
) -> (
    Arc<OperationOutcome<Result<ProviderClosure>>>,
    ResourceGovernor,
) {
    let catalog = ProviderCatalog::for_loopback_tests(&server.url(), Some("fixture-key".into()));
    let governor = ResourceGovernor::new(ResourceRequest {
        jobs: 1,
        memory_bytes: 8 << 20,
        open_files: 4,
        ..Default::default()
    });
    let runtime = OperationRuntime::new(governor.clone(), 1);
    let mut handle = runtime
        .start(move |mut scope| async move {
            Ok(catalog
                .resolve_required_closure(&mut scope, request, limits)
                .await)
        })
        .unwrap();
    let result = handle.wait().await;
    runtime.shutdown().await;
    (result, governor)
}
#[tokio::test]
async fn required_version_only_cycle_is_complete_without_installing_optional_content() {
    let mut server = Server::new_async().await;
    record(&mut server,"project1","version1",json!([{"version_id":"version2","dependency_type":"required"},{"project_id":"optional","dependency_type":"optional"}])).await;
    record(
        &mut server,
        "project2",
        "version2",
        json!([{"project_id":"project1","dependency_type":"required"}]),
    )
    .await;
    let none = server
        .mock("GET", "/project/optional")
        .expect(0)
        .create_async()
        .await;
    let (outcome, governor) = resolve(
        &server,
        request(vec![pin("project1", "version1")]),
        limits(),
    )
    .await;
    let OperationOutcome::Completed(Ok(graph)) = &*outcome else {
        panic!("closure failed")
    };
    assert!(graph.complete_for_required());
    assert_eq!(graph.selections.len(), 2);
    assert_eq!(graph.roots.len(), 1);
    assert_eq!(
        graph.required_edges[&pin("project1", "version1")],
        BTreeSet::from([pin("project2", "version2")])
    );
    assert_eq!(
        graph.required_edges[&pin("project2", "version2")],
        BTreeSet::from([pin("project1", "version1")])
    );
    none.assert_async().await;
    assert!(governor.status().reserved.memory_bytes > 0);
    drop(outcome);
    assert_eq!(governor.status().reserved, ResourceRequest::default());
}
#[tokio::test]
async fn project_only_dependency_resolves_compatible_selection_with_shared_budget() {
    let mut server = Server::new_async().await;
    record(
        &mut server,
        "project1",
        "version1",
        json!([{"project_id":"project2","dependency_type":"required"}]),
    )
    .await;
    record(&mut server, "project2", "version2", json!([])).await;
    let catalog = server
        .mock("GET", "/project/project2/version")
        .match_query(Matcher::Any)
        .with_body(json!([version("project2", "version2", json!([]))]).to_string())
        .create_async()
        .await;
    let (outcome, _) = resolve(
        &server,
        request(vec![pin("project1", "version1")]),
        limits(),
    )
    .await;
    let OperationOutcome::Completed(Ok(graph)) = &*outcome else {
        panic!("closure failed")
    };
    assert!(graph.complete_for_required());
    assert_eq!(graph.selections.len(), 2);
    catalog.assert_async().await;
    let mut small = limits();
    small.selection.catalog.transfer_bytes = 100;
    assert!(matches!(
        &*resolve(&server, request(vec![pin("project1", "version1")]), small)
            .await
            .0,
        OperationOutcome::Completed(Err(_))
    ));
}
#[tokio::test]
async fn explicit_roots_and_incompatible_edges_produce_truthful_conflicts() {
    let mut server = Server::new_async().await;
    record(&mut server,"project1","version1",json!([{"project_id":"project2","version_id":"version3","dependency_type":"required"},{"project_id":"project2","dependency_type":"incompatible"}])).await;
    record(&mut server, "project2", "version2", json!([])).await;
    let (outcome, _) = resolve(
        &server,
        request(vec![
            pin("project1", "version1"),
            pin("project2", "version2"),
        ]),
        limits(),
    )
    .await;
    let OperationOutcome::Completed(Ok(graph)) = &*outcome else {
        panic!("conflicts were lost")
    };
    assert!(!graph.complete_for_required());
    assert_eq!(graph.issues.len(), 2);
    assert!(
        graph
            .issues
            .iter()
            .any(|issue| matches!(issue.kind, ClosureIssueKind::SelectionConflict { .. }))
    );
    assert!(
        graph
            .issues
            .iter()
            .any(|issue| matches!(issue.kind, ClosureIssueKind::IncompatibleSelection { .. }))
    );
    assert_eq!(
        graph.selections[&pin("project2", "version2").project]
            .resolution
            .pin,
        pin("project2", "version2")
    );
    assert!(graph.required_edges.is_empty());
}
#[tokio::test]
async fn missing_edges_and_filename_only_requirements_cannot_claim_completeness() {
    let mut server = Server::new_async().await;
    record(
        &mut server,
        "project1",
        "version1",
        json!([{"file_name":"external.jar","dependency_type":"required"}]),
    )
    .await;
    record(&mut server, "project2", "version2", Value::Null).await;
    let (outcome, _) = resolve(
        &server,
        request(vec![
            pin("project1", "version1"),
            pin("project2", "version2"),
        ]),
        limits(),
    )
    .await;
    let OperationOutcome::Completed(Ok(graph)) = &*outcome else {
        panic!("coverage was lost")
    };
    assert!(!graph.complete_for_required());
    assert_eq!(graph.issues.len(), 3);
    assert!(
        graph
            .issues
            .iter()
            .any(|issue| matches!(issue.kind, ClosureIssueKind::MissingIdentity))
    );
    assert!(graph.issues.iter().any(|issue| matches!(
        issue.kind,
        ClosureIssueKind::IncompleteEvidence(Coverage::Unknown)
    )));
}
#[tokio::test]
async fn graph_limits_and_later_failures_return_no_partial_closure() {
    let mut server = Server::new_async().await;
    record(
        &mut server,
        "project1",
        "version1",
        json!([{"project_id":"project2","dependency_type":"required"}]),
    )
    .await;
    server
        .mock("GET", "/project/project2")
        .with_status(401)
        .create_async()
        .await;
    let (outcome, governor) = resolve(
        &server,
        request(vec![pin("project1", "version1")]),
        limits(),
    )
    .await;
    let OperationOutcome::Completed(Err(error)) = &*outcome else {
        panic!("provider failure became a partial graph")
    };
    assert!(matches!(
        error.downcast_ref::<CatalogError>(),
        Some(CatalogError::Unauthorized)
    ));
    assert_eq!(governor.status().reserved, ResourceRequest::default());
    let mut small = limits();
    small.projects = 1;
    let (outcome, governor) =
        resolve(&server, request(vec![pin("project1", "version1")]), small).await;
    let OperationOutcome::Completed(Err(error)) = &*outcome else {
        panic!("bound ignored")
    };
    assert!(matches!(
        error.downcast_ref::<CatalogError>(),
        Some(CatalogError::Limit)
    ));
    assert_eq!(governor.status().reserved, ResourceRequest::default());
}

#[tokio::test]
async fn curseforge_required_dependencies_resolve_without_promoting_optional_relations() {
    use empack_core::identity::{CurseForgeFileId, CurseForgeProjectId};
    let mut server = Server::new_async().await;
    for id in [123, 456] {
        server.mock("GET",format!("/mods/{id}").as_str()).with_body(json!({"data":{"id":id,"gameId":432,"classId":6,"slug":format!("project-{id}"),"name":"Project"}}).to_string()).create_async().await;
    }
    let file = |owner, id, deps: Value| json!({"id":id,"modId":owner,"gameId":432,"fileName":format!("{owner}.jar"),"fileLength":1,"hashes":[{"algo":2,"value":"00".repeat(16)}],"downloadUrl":null,"gameVersions":["1.21.1","Fabric"],"dependencies":deps,"releaseType":1,"fileDate":"2026-01-01T00:00:00Z","isAvailable":true});
    server.mock("GET","/mods/123/files/1").with_body(json!({"data":file(123,1,json!([{"modId":456,"relationType":3},{"modId":789,"relationType":2}]))}).to_string()).create_async().await;
    let child=server.mock("GET","/mods/456/files").match_query(Matcher::Any).with_body(json!({"data":[file(456,2,json!([]))],"pagination":{"index":0,"pageSize":50,"resultCount":1,"totalCount":1}}).to_string()).create_async().await;
    let root = ResolvedPin {
        project: ProviderProjectId::CurseForge(CurseForgeProjectId::parse("123").unwrap()),
        selection: PinSelector::CurseForgeFile(CurseForgeFileId::parse("1").unwrap()),
    };
    let (outcome, _) = resolve(&server, request(vec![root.clone()]), limits()).await;
    let OperationOutcome::Completed(Ok(graph)) = &*outcome else {
        panic!("CurseForge closure failed")
    };
    assert!(graph.complete_for_required());
    assert_eq!(graph.selections.len(), 2);
    assert_eq!(
        graph.required_edges[&root]
            .iter()
            .next()
            .unwrap()
            .project
            .to_string(),
        "456"
    );
    child.assert_async().await;
}

async fn capacity_issue(version_only: bool) {
    let mut server = Server::new_async().await;
    let dependency = if version_only {
        json!({"version_id":"version2","dependency_type":"required"})
    } else {
        json!({"file_name":"external.jar","dependency_type":"required"})
    };
    record(&mut server, "project1", "version1", json!([dependency])).await;
    if version_only {
        server
            .mock("GET", "/version/version2")
            .with_body(version("project1", "version2", json!([])).to_string())
            .create_async()
            .await;
    }
    let mut bounded = limits();
    bounded.projects = 1;
    let (outcome, _) = resolve(&server, request(vec![pin("project1", "version1")]), bounded).await;
    let OperationOutcome::Completed(Ok(graph)) = &*outcome else {
        panic!("project capacity suppressed a dependency issue without adding a project")
    };
    assert_eq!(graph.selections.len(), 1);
    assert!(!graph.complete_for_required());
    assert!(graph.issues.iter().any(|issue| if version_only {
        matches!(issue.kind, ClosureIssueKind::SelectionConflict { .. })
    } else {
        matches!(issue.kind, ClosureIssueKind::MissingIdentity)
    }));
}

#[tokio::test]
async fn full_project_capacity_still_classifies_missing_identity() {
    capacity_issue(false).await;
}
#[tokio::test]
async fn full_project_capacity_still_classifies_version_conflict() {
    capacity_issue(true).await;
}
#[tokio::test]
async fn project_capacity_counts_distinct_root_owners() {
    let mut server = Server::new_async().await;
    record(&mut server, "project1", "version1", json!([])).await;
    let mut bounded = limits();
    bounded.projects = 1;
    let (outcome, _) = resolve(
        &server,
        request(vec![
            pin("project1", "version1"),
            pin("project1", "version1"),
        ]),
        bounded,
    )
    .await;
    let OperationOutcome::Completed(Ok(graph)) = &*outcome else {
        panic!("duplicate root consumed another project slot")
    };
    assert!(graph.complete_for_required());
    assert_eq!(graph.selections.len(), 1);
}
