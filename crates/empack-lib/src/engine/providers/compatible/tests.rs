use super::*;
use crate::engine::{
    resources::ResourceGovernor,
    runtime::{OperationOutcome, OperationRuntime},
};
use empack_core::identity::CurseForgeProjectId;
use serde_json::json;

fn mr_project() -> CanonicalProject {
    modrinth::project(
        &serde_json::to_vec(
            &json!({"id":"AANobbMI","slug":"assets","title":"Assets","project_type":"mod"}),
        )
        .unwrap(),
    )
    .unwrap()
}
fn request(project: &CanonicalProject, policy: ReleasePolicy) -> CompatibleRequest {
    CompatibleRequest {
        project: project.id.clone(),
        kind: project.kind,
        game_versions: NonEmpty::new(vec![
            GameVersion::parse("1.20.1").unwrap(),
            GameVersion::parse("1.20").unwrap(),
        ])
        .unwrap(),
        loader: LoaderKind::Fabric,
        releases: policy,
    }
}
fn version(id: &str, channel: &str, date: &str, game: &str, loader: &str) -> Value {
    json!({"id":id,"project_id":"AANobbMI","version_type":channel,"date_published":date,"status":"listed",
        "game_versions":[game],"loaders":[loader],"dependencies":[],"files":[{"filename":"assets.jar","primary":true,"size":7,
        "hashes":{"sha512":"11".repeat(64)},"url":"https://example.com/assets.jar"}]})
}
fn page(project: &CanonicalProject, policy: ReleasePolicy, versions: Vec<Value>) -> Page {
    parse_page(
        project,
        &request(project, policy),
        &serde_json::to_vec(&versions).unwrap(),
        0,
        100,
        &Cancellation::default(),
    )
    .unwrap()
}
#[test]
fn selection_orders_instants_and_explicit_policy_not_response_order_or_semver() {
    let project = mr_project();
    for (policy, selected) in [
        (ReleasePolicy::Any, "alpha001"),
        (ReleasePolicy::StableOnly, "stable02"),
        (ReleasePolicy::PreferStable, "stable02"),
    ] {
        let mut parsed = page(
            &project,
            policy,
            vec![
                version(
                    "stable01",
                    "release",
                    "2025-01-01T01:00:00+01:00",
                    "1.20.1",
                    "fabric",
                ),
                version(
                    "wrong001",
                    "release",
                    "2026-01-01T00:00:00Z",
                    "1.20.1",
                    "forge",
                ),
                version(
                    "oldgame1",
                    "release",
                    "2026-01-01T00:00:00Z",
                    "1.20",
                    "fabric",
                ),
                version(
                    "alpha001",
                    "alpha",
                    "2025-02-01T00:00:00Z",
                    "1.20.1",
                    "fabric",
                ),
                version(
                    "stable02",
                    "release",
                    "2025-01-01T00:00:00.001Z",
                    "1.20.1",
                    "fabric",
                ),
            ],
        );
        parsed.candidates.sort_by(|a, b| a.rank.cmp(&b.rank));
        assert_eq!(
            parsed.candidates[0].selected.resolution.pin.selection,
            project.id.parse_pin(selected).unwrap()
        );
        assert_eq!(
            parsed.candidates[0].selected.matched_game.as_str(),
            "1.20.1"
        );
    }
    assert!(
        page(
            &project,
            ReleasePolicy::StableOnly,
            vec![version(
                "alpha001",
                "alpha",
                "2025-01-01T00:00:00Z",
                "1.20.1",
                "fabric"
            )]
        )
        .candidates
        .is_empty()
    );
    assert_eq!(
        page(
            &project,
            ReleasePolicy::PreferStable,
            vec![version(
                "alpha001",
                "alpha",
                "2025-01-01T00:00:00Z",
                "1.20.1",
                "fabric"
            )]
        )
        .candidates
        .len(),
        1
    );
}
#[test]
fn resource_packs_ignore_mod_loader_but_keep_visibility_and_game_checks() {
    let mut project = mr_project();
    project.kind = ContentKind::ResourcePack;
    let mut hidden = version(
        "hidden01",
        "release",
        "2025-01-01T00:00:00Z",
        "1.20.1",
        "minecraft",
    );
    hidden["status"] = json!("unlisted");
    let parsed = page(
        &project,
        ReleasePolicy::Any,
        vec![
            hidden,
            version(
                "oldgame1",
                "release",
                "2025-01-01T00:00:00Z",
                "1.19.4",
                "minecraft",
            ),
            version(
                "assets01",
                "release",
                "2025-01-01T00:00:00Z",
                "1.20.1",
                "minecraft",
            ),
        ],
    );
    assert_eq!(parsed.candidates.len(), 1);
    assert_eq!(
        parsed.candidates[0].selected.resolution.project.kind,
        ContentKind::ResourcePack
    );
    assert_eq!(
        parsed.candidates[0].selected.resolution.files.as_slice()[0]
            .expected
            .size,
        Some(7)
    );
}
#[test]
fn malformed_dates_wrong_ownership_and_entry_overflow_cannot_select_a_file() {
    let project = mr_project();
    let request = request(&project, ReleasePolicy::Any);
    let original = version(
        "assets01",
        "release",
        "2025-01-01T00:00:00Z",
        "1.20.1",
        "fabric",
    );
    for (field, value) in [
        ("date_published", "yesterday"),
        ("project_id", "BADOWNER"),
        ("version_type", "unknown"),
    ] {
        let mut bad = original.clone();
        bad[field] = json!(value);
        assert!(
            parse_page(
                &project,
                &request,
                &serde_json::to_vec(&vec![bad]).unwrap(),
                0,
                1,
                &Cancellation::default()
            )
            .is_err()
        );
    }
    assert!(
        parse_page(
            &project,
            &request,
            &serde_json::to_vec(&vec![original]).unwrap(),
            0,
            0,
            &Cancellation::default()
        )
        .is_err()
    );
}
fn cf_project() -> Value {
    json!({"data":{"id":123,"gameId":432,"slug":"assets","name":"Assets","classId":6}})
}
fn cf_file(id: u64, channel: u64, date: &str) -> Value {
    json!({"id":id,"gameId":432,"modId":123,"releaseType":channel,"fileDate":date,"isAvailable":true,
        "fileName":"assets.jar","fileLength":7,"downloadUrl":null,"hashes":[{"algo":2,"value":"11".repeat(16)}],"gameVersions":["1.20.1","Fabric"],"dependencies":[]})
}
fn cf_page(index: usize, file: Value) -> Value {
    json!({"data":[file],"pagination":{"index":index,"pageSize":1,"resultCount":1,"totalCount":2}})
}
fn limits() -> SelectionLimits {
    SelectionLimits {
        catalog: CatalogLimits {
            response_bytes: 4096,
            transfer_bytes: 16384,
            deadline: Duration::from_secs(3),
        },
        candidates: 10,
        pages: 4,
    }
}
type Outcome = Arc<OperationOutcome<Result<RetainedOutput<CompatibleSelection>>>>;
async fn resolve(
    catalog: ProviderCatalog,
    request: CompatibleRequest,
    limits: SelectionLimits,
) -> (Outcome, ResourceGovernor) {
    let governor = ResourceGovernor::new(ResourceRequest {
        jobs: 1,
        memory_bytes: 1 << 20,
        open_files: 4,
        ..Default::default()
    });
    let runtime = OperationRuntime::new(governor.clone(), 1);
    let mut handle = runtime
        .start(move |mut scope| async move {
            Ok(catalog
                .resolve_compatible(&mut scope, request, limits)
                .await)
        })
        .unwrap();
    let outcome = handle.wait().await;
    runtime.release_completed(handle.id());
    runtime.shutdown().await;
    (outcome, governor)
}
#[tokio::test]
async fn curseforge_pagination_is_complete_or_fails_without_a_partial_selection() {
    for (page_limit, conflicting) in [(4, false), (1, false), (4, true)] {
        let mut server = mockito::Server::new_async().await;
        let project = server
            .mock("GET", "/mods/123")
            .with_body(cf_project().to_string())
            .create_async()
            .await;
        let query = |index: usize| {
            mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("gameVersion".into(), "1.20.1".into()),
                mockito::Matcher::UrlEncoded("index".into(), index.to_string()),
                mockito::Matcher::UrlEncoded("pageSize".into(), "50".into()),
            ])
        };
        let first = server
            .mock("GET", "/mods/123/files")
            .match_query(query(0))
            .with_body(cf_page(0, cf_file(10, 3, "2025-02-01T00:00:00Z")).to_string())
            .create_async()
            .await;
        let second = server
            .mock("GET", "/mods/123/files")
            .match_query(query(1))
            .with_body(
                cf_page(
                    1,
                    cf_file(if conflicting { 10 } else { 11 }, 1, "2025-01-01T00:00:00Z"),
                )
                .to_string(),
            )
            .expect(if page_limit == 1 { 0 } else { 1 })
            .create_async()
            .await;
        let request = CompatibleRequest {
            project: ProviderProjectId::CurseForge(CurseForgeProjectId::parse("123").unwrap()),
            kind: ContentKind::Mod,
            game_versions: NonEmpty::new(vec![GameVersion::parse("1.20.1").unwrap()]).unwrap(),
            loader: LoaderKind::Fabric,
            releases: ReleasePolicy::PreferStable,
        };
        let catalog =
            ProviderCatalog::for_loopback_tests(&server.url(), Some("fixture-key".into()));
        let (outcome, governor) = resolve(
            catalog,
            request,
            SelectionLimits {
                pages: page_limit,
                ..limits()
            },
        )
        .await;
        if page_limit == 1 || conflicting {
            assert!(matches!(&*outcome, OperationOutcome::Completed(Err(_))));
            assert_eq!(governor.status().reserved, ResourceRequest::default());
        } else {
            let OperationOutcome::Completed(Ok(selected)) = &*outcome else {
                panic!("selection failed")
            };
            assert_eq!(
                selected.resolution.pin.selection,
                selected.resolution.project.id.parse_pin("11").unwrap()
            );
            assert_eq!(selected.channel, ReleaseChannel::Release);
            assert!(
                selected.resolution.files.as_slice()[0]
                    .alternatives
                    .is_empty()
            );
            assert!(governor.status().reserved.memory_bytes > 0);
        }
        project.assert_async().await;
        first.assert_async().await;
        second.assert_async().await;
        drop(outcome);
        assert_eq!(governor.status().reserved, ResourceRequest::default());
    }
}
#[tokio::test]
async fn modrinth_query_has_no_changelog_and_enforces_cumulative_byte_limits() {
    for small_limit in [false, true] {
        let mut server = mockito::Server::new_async().await;
        let project = server
            .mock("GET", "/project/AANobbMI")
            .with_body(
                json!({"id":"AANobbMI","slug":"assets","title":"Assets","project_type":"mod"})
                    .to_string(),
            )
            .create_async()
            .await;
        let versions = server
            .mock("GET", "/project/AANobbMI/version")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded(
                    "game_versions".into(),
                    "[\"1.20.1\",\"1.20\"]".into(),
                ),
                mockito::Matcher::UrlEncoded("include_changelog".into(), "false".into()),
            ]))
            .with_body(
                serde_json::to_vec(&vec![
                    version(
                        "assets01",
                        "release",
                        "2025-01-01T00:00:00Z",
                        "1.20.1",
                        "fabric",
                    ),
                    version(
                        "assets02",
                        "release",
                        "2025-02-01T00:00:00Z",
                        "1.20.1",
                        "fabric",
                    ),
                ])
                .unwrap(),
            )
            .create_async()
            .await;
        let mut limits = limits();
        if small_limit {
            limits.catalog.transfer_bytes = 200;
        }
        let (outcome, governor) = resolve(
            ProviderCatalog::for_loopback_tests(&server.url(), None),
            request(&mr_project(), ReleasePolicy::Any),
            limits,
        )
        .await;
        if small_limit {
            assert!(
                matches!(&*outcome, OperationOutcome::Completed(Err(error)) if matches!(error.downcast_ref::<CatalogError>(), Some(CatalogError::Limit)))
            );
        } else {
            let OperationOutcome::Completed(Ok(selected)) = &*outcome else {
                panic!("compatible query failed")
            };
            assert_eq!(
                selected.resolution.pin.selection,
                selected
                    .resolution
                    .project
                    .id
                    .parse_pin("assets02")
                    .unwrap()
            );
        }
        project.assert_async().await;
        versions.assert_async().await;
        drop(outcome);
        assert_eq!(governor.status().reserved, ResourceRequest::default());
    }
}

#[tokio::test]
async fn alternate_game_queries_share_identity_evidence_and_never_guess_from_a_partial_catalog() {
    for conflicting in [false, true] {
        let mut server = mockito::Server::new_async().await;
        let project = server
            .mock("GET", "/mods/123")
            .with_body(cf_project().to_string())
            .create_async()
            .await;
        let mut file = cf_file(10, 1, "2025-01-01T00:00:00Z");
        file["gameVersions"] = json!(["1.20.1", "1.20", "Fabric"]);
        let first = server.mock("GET", "/mods/123/files").match_query(mockito::Matcher::UrlEncoded("gameVersion".into(), "1.20.1".into()))
            .with_body(json!({"data":[file.clone()],"pagination":{"index":0,"pageSize":50,"resultCount":1,"totalCount":1}}).to_string()).create_async().await;
        file["downloadCount"] = json!(9876);
        if conflicting {
            file["hashes"][0]["value"] = json!("22".repeat(16));
        }
        let second = server.mock("GET", "/mods/123/files").match_query(mockito::Matcher::UrlEncoded("gameVersion".into(), "1.20".into()))
            .with_body(json!({"data":[file],"pagination":{"index":0,"pageSize":50,"resultCount":1,"totalCount":1}}).to_string()).create_async().await;
        let canonical =
            curseforge::project(&serde_json::to_vec(&cf_project()).unwrap(), false).unwrap();
        let (outcome, governor) = resolve(
            ProviderCatalog::for_loopback_tests(&server.url(), Some("fixture-key".into())),
            request(&canonical, ReleasePolicy::Any),
            limits(),
        )
        .await;
        if conflicting {
            assert!(matches!(&*outcome, OperationOutcome::Completed(Err(_))));
        } else {
            let OperationOutcome::Completed(Ok(selected)) = &*outcome else {
                panic!("alternate query failed")
            };
            assert_eq!(selected.matched_game.as_str(), "1.20.1");
        }
        first.assert_async().await;
        second.assert_async().await;
        project.assert_async().await;
        drop(outcome);
        assert_eq!(governor.status().reserved, ResourceRequest::default());
    }
}
