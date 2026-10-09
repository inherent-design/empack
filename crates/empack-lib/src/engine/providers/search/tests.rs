use super::*;
use crate::engine::{
    resources::ResourceGovernor,
    runtime::{OperationOutcome, OperationRuntime},
};
use mockito::{Matcher, Server};
use serde_json::{Value, json};
fn query() -> SearchQuery {
    SearchQuery {
        text: "Terralith".into(),
        providers: NonEmpty::new(vec![ProviderKind::Modrinth, ProviderKind::CurseForge]).unwrap(),
        kind: ContentKind::DataPack,
        game_versions: NonEmpty::new(vec![GameVersion::parse("1.21.1").unwrap()]).unwrap(),
        loader: LoaderKind::Vanilla,
        offset: 0,
    }
}
fn limits() -> SearchLimits {
    SearchLimits {
        catalog: CatalogLimits {
            response_bytes: 4096,
            transfer_bytes: 8192,
            deadline: Duration::from_secs(2),
        },
        page_size: 2,
    }
}
fn hit(id: &str, title: &str) -> Value {
    json!({"project_id":id,"slug":id,"title":title,"project_type":"mod","all_project_types":["mod","datapack"],"categories":["fabric","datapack"],"versions":["1.21.1"]})
}
fn mr() -> Value {
    json!({"hits":[hit("other123","Terralith Extension"),hit("exact123","Terralith")],"offset":0,"limit":2,"total_hits":3})
}
fn cf() -> Value {
    json!({"data":[{"id":123,"gameId":432,"classId":6945,"slug":"terralith","name":"Terralith"}],"pagination":{"index":0,"pageSize":2,"resultCount":1,"totalCount":1}})
}
type Outcome = Arc<OperationOutcome<Result<ProjectSearch>>>;
async fn search(
    server: &Server,
    query: SearchQuery,
    key: Option<String>,
    limits: SearchLimits,
) -> (Outcome, ResourceGovernor) {
    let catalog = ProviderCatalog::for_loopback_tests(&server.url(), key);
    let governor = ResourceGovernor::new(ResourceRequest {
        jobs: 1,
        memory_bytes: 8 << 20,
        open_files: 4,
        ..Default::default()
    });
    let runtime = OperationRuntime::new(governor.clone(), 1);
    let mut handle = runtime
        .start(move |mut scope| async move {
            Ok(catalog.search_projects(&mut scope, query, limits).await)
        })
        .unwrap();
    let outcome = handle.wait().await;
    runtime.shutdown().await;
    (outcome, governor)
}
#[tokio::test]
async fn search_retains_choices_provider_order_and_page_boundaries() {
    let mut server = Server::new_async().await;
    let mr_request = server
        .mock("GET", "/search")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("query".into(), "Terralith".into()),
            Matcher::UrlEncoded(
                "facets".into(),
                r#"[["all_project_types:datapack"],["versions:1.21.1"]]"#.into(),
            ),
        ]))
        .with_body(mr().to_string())
        .create_async()
        .await;
    let cf_request = server
        .mock("GET", "/mods/search")
        .match_header("x-api-key", "fixture-key")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("classId".into(), "6945".into()),
            Matcher::UrlEncoded("gameVersion".into(), "1.21.1".into()),
        ]))
        .with_body(cf().to_string())
        .create_async()
        .await;
    let mut q = query();
    q.providers = NonEmpty::new(vec![ProviderKind::CurseForge, ProviderKind::Modrinth]).unwrap();
    let (outcome, governor) = search(&server, q, Some("fixture-key".into()), limits()).await;
    let OperationOutcome::Completed(Ok(found)) = &*outcome else {
        panic!("search failed")
    };
    assert_eq!(found.pages.len(), 2);
    assert_eq!(found.pages[0].provider, ProviderKind::CurseForge);
    let page = &found.pages[1];
    assert_eq!(page.candidates[0].project.to_string(), "exact123");
    assert_eq!(page.candidates[0].provider_rank, 1);
    assert_eq!(page.candidates[0].similarity, 100);
    assert_eq!(page.next_offset, Some(2));
    assert_eq!(page.total, 3);
    assert_eq!(found.pages[0].next_offset, None);
    mr_request.assert_async().await;
    cf_request.assert_async().await;
    assert!(governor.status().reserved.memory_bytes > 0);
    drop(outcome);
    assert_eq!(governor.status().reserved, ResourceRequest::default());
}
#[tokio::test]
async fn provider_errors_and_cumulative_limits_discard_earlier_choices() {
    let mut server = Server::new_async().await;
    let first = server
        .mock("GET", "/search")
        .match_query(Matcher::Any)
        .with_body(mr().to_string())
        .create_async()
        .await;
    let denied = server
        .mock("GET", "/mods/search")
        .match_query(Matcher::Any)
        .with_status(401)
        .create_async()
        .await;
    let (outcome, governor) = search(&server, query(), Some("fixture-key".into()), limits()).await;
    assert!(matches!(&*outcome, OperationOutcome::Completed(Err(_))));
    first.assert_async().await;
    denied.assert_async().await;
    let OperationOutcome::Completed(Err(error)) = &*outcome else {
        unreachable!()
    };
    assert!(matches!(
        error.downcast_ref::<CatalogError>(),
        Some(CatalogError::Unauthorized)
    ));
    assert_eq!(governor.status().reserved, ResourceRequest::default());
    let mut small = limits();
    small.catalog.transfer_bytes = 10;
    let (outcome, _) = search(&server, query(), Some("fixture-key".into()), small).await;
    assert!(matches!(&*outcome, OperationOutcome::Completed(Err(_))));
}
#[tokio::test]
async fn missing_auth_and_invalid_queries_make_no_requests() {
    let mut server = Server::new_async().await;
    let none = server
        .mock("GET", Matcher::Any)
        .expect(0)
        .create_async()
        .await;
    for text in ["", "\nsecret", &"x".repeat(257)] {
        let mut q = query();
        q.text = text.into();
        assert!(matches!(
            &*search(&server, q, Some("fixture-key".into()), limits())
                .await
                .0,
            OperationOutcome::Completed(Err(_))
        ));
    }
    assert!(matches!(
        &*search(&server, query(), None, limits()).await.0,
        OperationOutcome::Completed(Err(_))
    ));
    none.assert_async().await;
}
#[tokio::test]
async fn unsupported_world_search_is_not_reported_as_provider_not_found() {
    let server = Server::new_async().await;
    let mut q = query();
    q.providers = NonEmpty::new(vec![ProviderKind::Modrinth]).unwrap();
    q.kind = ContentKind::World;
    let (outcome, _) = search(&server, q, None, limits()).await;
    let OperationOutcome::Completed(Ok(found)) = &*outcome else {
        panic!("unsupported capability lost")
    };
    assert_eq!(found.unsupported, vec![ProviderKind::Modrinth]);
    assert!(found.pages.is_empty());
}
#[test]
fn invalid_pagination_duplicates_kinds_and_owners_fail_closed() {
    let q = query();
    let game = q.game_versions.as_slice()[0].clone();
    for (pointer, value) in [
        ("/offset", json!(1)),
        ("/limit", json!(50)),
        ("/total_hits", json!(1)),
        ("/hits/1/project_id", json!("other123")),
        ("/hits/1/all_project_types", json!(["shader"])),
        ("/hits/1/versions", json!(["1.0"])),
    ] {
        let mut response = mr();
        *response.pointer_mut(pointer).unwrap() = value;
        assert!(
            parse_page(
                &serde_json::to_vec(&response).unwrap(),
                ProviderKind::Modrinth,
                game.clone(),
                &q,
                2
            )
            .is_err()
        );
    }
    for (pointer, value) in [
        ("/pagination/index", json!(1)),
        ("/pagination/resultCount", json!(2)),
        ("/data/0/gameId", json!(1)),
        ("/data/0/classId", json!(6)),
    ] {
        let mut response = cf();
        *response.pointer_mut(pointer).unwrap() = value;
        assert!(
            parse_page(
                &serde_json::to_vec(&response).unwrap(),
                ProviderKind::CurseForge,
                game.clone(),
                &q,
                2
            )
            .is_err()
        );
    }
    let empty = json!({"hits":[],"offset":0,"limit":2,"total_hits":0});
    assert!(
        parse_page(
            &serde_json::to_vec(&empty).unwrap(),
            ProviderKind::Modrinth,
            game,
            &q,
            2
        )
        .unwrap()
        .candidates
        .is_empty()
    );
}
#[test]
fn bounded_unicode_ranking_and_loader_queries_preserve_content_kind() {
    assert_eq!(similarity("Terralith", "terralith"), 100);
    assert_eq!(similarity("猫", "猫"), 100);
    assert!(similarity("sodum", "Sodium") > similarity("sodum", "Other Renderer"));
    let mut q = query();
    q.kind = ContentKind::Mod;
    q.loader = LoaderKind::NeoForge;
    let params = parameters(
        &q,
        &q.game_versions.as_slice()[0],
        ProviderKind::CurseForge,
        2,
    )
    .unwrap();
    assert!(params.contains(&("modLoaderType".into(), "6".into())));
    q.kind = ContentKind::DataPack;
    let params = parameters(
        &q,
        &q.game_versions.as_slice()[0],
        ProviderKind::CurseForge,
        2,
    )
    .unwrap();
    assert!(!params.iter().any(|(name, _)| name == "modLoaderType"));
}

#[tokio::test]
async fn accepted_game_alternatives_keep_separate_bounded_windows() {
    let mut server = Server::new_async().await;
    let first = server
        .mock("GET", "/search")
        .match_query(Matcher::UrlEncoded(
            "facets".into(),
            r#"[["all_project_types:datapack"],["versions:1.21.1"]]"#.into(),
        ))
        .with_body(mr().to_string())
        .expect(1)
        .create_async()
        .await;
    let second = server
        .mock("GET", "/search")
        .match_query(Matcher::UrlEncoded(
            "facets".into(),
            r#"[["all_project_types:datapack"],["versions:1.20.1"]]"#.into(),
        ))
        .with_body(json!({"hits":[],"offset":0,"limit":2,"total_hits":0}).to_string())
        .expect(1)
        .create_async()
        .await;
    let mut q = query();
    q.providers = NonEmpty::new(vec![ProviderKind::Modrinth]).unwrap();
    q.game_versions = NonEmpty::new(vec![
        GameVersion::parse("1.21.1").unwrap(),
        GameVersion::parse("1.20.1").unwrap(),
        GameVersion::parse("1.21.1").unwrap(),
    ])
    .unwrap();
    let (outcome, _) = search(&server, q, None, limits()).await;
    let OperationOutcome::Completed(Ok(found)) = &*outcome else {
        panic!("search failed")
    };
    assert_eq!(found.pages.len(), 2);
    assert_eq!(found.pages[0].game.as_str(), "1.21.1");
    assert_eq!(found.pages[1].game.as_str(), "1.20.1");
    first.assert_async().await;
    second.assert_async().await;
}
#[test]
fn paging_ceiling_retains_truncation_and_out_of_range_empty_is_valid() {
    let mut q = query();
    q.offset = 9998;
    let mut response = mr();
    response["offset"] = json!(9998);
    response["total_hits"] = json!(10001);
    let page = parse_page(
        &serde_json::to_vec(&response).unwrap(),
        ProviderKind::Modrinth,
        q.game_versions.as_slice()[0].clone(),
        &q,
        2,
    )
    .unwrap();
    assert!(page.has_more);
    assert!(page.next_offset.is_none());
    response["hits"] = json!([]);
    response["total_hits"] = json!(10);
    let page = parse_page(
        &serde_json::to_vec(&response).unwrap(),
        ProviderKind::Modrinth,
        q.game_versions.as_slice()[0].clone(),
        &q,
        2,
    )
    .unwrap();
    assert!(!page.has_more);
    assert!(page.next_offset.is_none());
}
