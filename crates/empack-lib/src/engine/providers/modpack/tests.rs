use super::*;
use crate::engine::{
    resources::ResourceGovernor,
    runtime::{OperationOutcome, OperationRuntime},
};
use mockito::{Matcher, Server};
use serde_json::json;
use std::sync::Arc;

fn mr(id: &str, release: &str, date: &str) -> Value {
    json!({"id":id,"project_id":"Pack0001","version_number":"1.0","version_type":release,
        "status":"listed","date_published":date,"game_versions":["1.21.1"],"loaders":["fabric"],
        "files":[{"filename":"pack.mrpack","primary":true,"size":7,"url":"https://example.com/pack.mrpack",
        "hashes":{"sha512":empack_core::digest::ExpectedDigest::Sha512(sha2::Sha512::digest(b"payload").into()).hex()}}],"dependencies":[]})
}
fn cf(id: u64, release: u64, date: &str) -> Value {
    json!({"id":id,"modId":1001,"gameId":432,"fileName":"pack.zip","fileLength":7,
        "hashes":[{"algo":2,"value":"321c3cf486ed509164edec1e1981fec8"}],"downloadUrl":null,
        "gameVersions":["1.21.1"],"dependencies":[],"releaseType":release,"fileDate":date,"isAvailable":true,"isServerPack":false})
}
fn limits() -> SelectionLimits {
    SelectionLimits {
        catalog: CatalogLimits {
            response_bytes: 8192,
            transfer_bytes: 65536,
            deadline: std::time::Duration::from_secs(3),
        },
        candidates: 10,
        pages: 4,
    }
}
async fn run(
    origin: String,
    selector: ModpackSelector,
    releases: ReleasePolicy,
    limits: SelectionLimits,
) -> (
    Arc<OperationOutcome<Result<ModpackArchive>>>,
    ResourceGovernor,
) {
    let governor = ResourceGovernor::new(ResourceRequest {
        jobs: 1,
        memory_bytes: 4 << 20,
        open_files: 8,
        ..Default::default()
    });
    let runtime = OperationRuntime::new(governor.clone(), 1);
    let mut handle = runtime
        .start(move |mut scope| async move {
            Ok(
                ProviderCatalog::for_loopback_tests(&origin, Some("fixture-key".into()))
                    .resolve_modpack_archive(&mut scope, selector, releases, limits)
                    .await,
            )
        })
        .unwrap();
    let outcome = handle.wait().await;
    runtime.shutdown().await;
    (outcome, governor)
}
fn ready(outcome: &OperationOutcome<Result<ModpackArchive>>) -> &ModpackArchive {
    match outcome {
        OperationOutcome::Completed(Ok(value)) => value,
        OperationOutcome::Completed(Err(error)) => panic!("Archive resolution failed: {error:#}"),
        _ => panic!("Archive worker failed"),
    }
}
fn failed(outcome: &OperationOutcome<Result<ModpackArchive>>) -> &anyhow::Error {
    match outcome {
        OperationOutcome::Completed(Err(error)) => error,
        _ => panic!("Invalid archive selection did not fail"),
    }
}
async fn project(server: &mut Server) {
    server
        .mock("GET", "/project/a-pack")
        .with_body(
            json!({"id":"Pack0001","slug":"a-pack","title":"A pack","project_type":"modpack"})
                .to_string(),
        )
        .create_async()
        .await;
}
#[test]
fn modpack_selectors_accept_only_the_selected_provider_namespace() {
    assert!(
        ModpackSelector::parse(
            ProviderKind::CurseForge,
            "https://curseforge.com/minecraft/modpacks/123"
        )
        .unwrap()
        .project
        .is_slug()
    );
    assert!(
        !ModpackSelector::parse(ProviderKind::CurseForge, "123")
            .unwrap()
            .project
            .is_slug()
    );
    for (provider, input) in [
        (
            ProviderKind::Modrinth,
            "https://modrinth.com/modpack/a-pack",
        ),
        (
            ProviderKind::Modrinth,
            "https://modrinth.com/modpack/a-pack/version/1.0",
        ),
        (
            ProviderKind::CurseForge,
            "https://www.curseforge.com/minecraft/modpacks/a-pack/files/123",
        ),
        (ProviderKind::CurseForge, "1001"),
    ] {
        assert!(ModpackSelector::parse(provider, input).is_ok(), "{input}");
    }
    for input in [
        "https://evil.test/modrinth.com/modpack/a-pack",
        "https://modrinth.com.evil.test/modpack/a-pack",
        "https://modrinth.com/mod/a-pack",
        "https://user@modrinth.com/modpack/a-pack",
        "https://modrinth.com//modpack/a-pack",
        "https://modrinth.com/modpack/a-pack?token=secret",
        "https://modrinth.com/modpack/a-pack/version/a%2Fb",
    ] {
        assert!(ModpackSelector::parse(ProviderKind::Modrinth, input).is_err());
    }
    let selected = ModpackSelector::parse(
        ProviderKind::Modrinth,
        "https://modrinth.com/modpack/a-pack/version/1.0",
    )
    .unwrap();
    assert!(selected.with_version("2.0").is_err());
    assert!(
        ModpackSelector::parse(ProviderKind::CurseForge, "a-pack")
            .unwrap()
            .with_version("not-a-file-id")
            .is_err()
    );
}
#[tokio::test]
async fn latest_archive_selection_is_deterministic_and_release_policy_is_explicit() {
    let mut server = Server::new_async().await;
    project(&mut server).await;
    let versions = server
        .mock("GET", "/project/Pack0001/version")
        .with_body(
            json!([
                mr("Old00001", "release", "2025-01-01T00:00:00Z"),
                mr("Beta0003", "beta", "2025-03-01T00:00:00Z"),
                mr("New00002", "release", "2025-02-01T00:00:00Z")
            ])
            .to_string(),
        )
        .expect(3)
        .create_async()
        .await;
    for (policy, expected) in [
        (ReleasePolicy::PreferStable, "New00002"),
        (ReleasePolicy::Any, "Beta0003"),
        (ReleasePolicy::StableOnly, "New00002"),
    ] {
        let (outcome, governor) = run(
            server.url(),
            ModpackSelector::parse(
                ProviderKind::Modrinth,
                "https://modrinth.com/modpack/a-pack",
            )
            .unwrap(),
            policy,
            limits(),
        )
        .await;
        let selected = ready(&outcome);
        assert_eq!(selected.project().id.to_string(), "Pack0001");
        assert_eq!(
            selected.pin().selection,
            selected.project().id.parse_pin(expected).unwrap()
        );
        assert_eq!(selected.file().filename, "pack.mrpack");
        assert_eq!(selected.file().expected.size, Some(7));
        assert!(governor.status().reserved.memory_bytes > 0);
        drop(outcome);
        assert_eq!(governor.status().reserved, ResourceRequest::default());
    }
    versions.assert_async().await;
}
#[tokio::test]
async fn exact_named_archive_versions_keep_canonical_pins_and_primary_file_evidence() {
    let mut server = Server::new_async().await;
    project(&mut server).await;
    let mut version = mr("Beta0003", "beta", "2025-03-01T00:00:00Z");
    version["files"].as_array_mut().unwrap().insert(0, json!({"filename":"signature.txt","primary":false,"size":1,"url":"https://example.com/signature","hashes":{"md5":"00000000000000000000000000000000"}}));
    let exact = server
        .mock("GET", "/project/Pack0001/version/1.0")
        .with_body(version.to_string())
        .create_async()
        .await;
    let (outcome, governor) = run(
        server.url(),
        ModpackSelector::parse(
            ProviderKind::Modrinth,
            "https://modrinth.com/modpack/a-pack/version/1.0",
        )
        .unwrap(),
        ReleasePolicy::StableOnly,
        limits(),
    )
    .await;
    assert_eq!(
        ready(&outcome).pin().selection,
        ready(&outcome).project().id.parse_pin("Beta0003").unwrap()
    );
    assert_eq!(ready(&outcome).file().filename, "pack.mrpack");
    exact.assert_async().await;
    drop(outcome);
    assert_eq!(governor.status().reserved, ResourceRequest::default());
}
#[tokio::test]
async fn curseforge_pages_preserve_restricted_archive_evidence_and_skip_server_packs() {
    let mut server = Server::new_async().await;
    let query = server.mock("GET", "/mods/search").match_query(Matcher::AllOf(vec![Matcher::UrlEncoded("classId".into(), "4471".into()), Matcher::UrlEncoded("gameId".into(), "432".into()), Matcher::UrlEncoded("slug".into(), "a-pack".into()), Matcher::UrlEncoded("pageSize".into(), "2".into())])).with_body(json!({"data":[{"id":1001,"slug":"a-pack","name":"A pack","classId":4471,"gameId":432}]}).to_string()).create_async().await;
    let mut server_pack = cf(2003, 1, "2025-03-01T00:00:00Z");
    server_pack["isServerPack"] = json!(true);
    let first = server.mock("GET", "/mods/1001/files").match_query(Matcher::AllOf(vec![Matcher::UrlEncoded("index".into(), "0".into()), Matcher::UrlEncoded("pageSize".into(), "50".into())])).with_body(json!({"data":[cf(2001,1,"2025-01-01T00:00:00Z"),server_pack],"pagination":{"index":0,"pageSize":2,"resultCount":2,"totalCount":3}}).to_string()).create_async().await;
    let second = server.mock("GET", "/mods/1001/files").match_query(Matcher::AllOf(vec![Matcher::UrlEncoded("index".into(), "2".into()), Matcher::UrlEncoded("pageSize".into(), "50".into())])).with_body(json!({"data":[cf(2002,1,"2025-02-01T00:00:00Z")],"pagination":{"index":2,"pageSize":2,"resultCount":1,"totalCount":3}}).to_string()).create_async().await;
    let (outcome, governor) = run(
        server.url(),
        ModpackSelector::parse(
            ProviderKind::CurseForge,
            "https://curseforge.com/minecraft/modpacks/a-pack",
        )
        .unwrap(),
        ReleasePolicy::PreferStable,
        limits(),
    )
    .await;
    let selected = ready(&outcome);
    assert_eq!(
        selected.pin().selection,
        selected.project().id.parse_pin("2002").unwrap()
    );
    assert!(
        selected.file().alternatives.is_empty(),
        "restricted archives must remain manual obligations"
    );
    assert_eq!(
        selected.file().expected.digests.as_ref().unwrap().values()[0].algorithm(),
        empack_core::digest::DigestAlgorithm::Md5
    );
    for mock in [query, first, second] {
        mock.assert_async().await;
    }
    drop(outcome);
    assert_eq!(governor.status().reserved, ResourceRequest::default());
}
#[tokio::test]
async fn archive_resolution_rejects_response_identity_limits_and_bad_evidence() {
    for mode in [
        "owner",
        "kind",
        "digest",
        "duplicate-primary",
        "empty-files",
        "wrong-version",
        "limit",
        "deadline",
        "duplicate-record",
    ] {
        let mut server = Server::new_async().await;
        let kind = if mode == "kind" { "mod" } else { "modpack" };
        server
            .mock("GET", "/project/a-pack")
            .with_body(
                json!({"id":"Pack0001","slug":"a-pack","title":"Pack","project_type":kind})
                    .to_string(),
            )
            .create_async()
            .await;
        let mut version = mr("Old00001", "release", "2025-01-01T00:00:00Z");
        match mode {
            "owner" => version["project_id"] = json!("Wrong001"),
            "digest" => version["files"][0]["hashes"] = json!({"sha512":"invalid"}),
            "duplicate-primary" => {
                let file = version["files"][0].clone();
                version["files"].as_array_mut().unwrap().push(file);
            }
            "empty-files" => version["files"] = json!([]),
            "wrong-version" => version["version_number"] = json!("wrong"),
            _ => {}
        }
        let mut selected = ModpackSelector::parse(ProviderKind::Modrinth, "a-pack").unwrap();
        let mut bounded = limits();
        if mode == "limit" {
            bounded.catalog.response_bytes = 1;
        }
        if mode == "deadline" {
            bounded.catalog.deadline = std::time::Duration::ZERO;
        }
        if mode == "wrong-version" {
            selected = selected.with_version("2.0").unwrap();
            server
                .mock("GET", "/project/Pack0001/version/2.0")
                .with_body(version.to_string())
                .create_async()
                .await;
        } else {
            let data = if mode == "duplicate-record" {
                json!([version.clone(), version])
            } else {
                json!([version])
            };
            server
                .mock("GET", "/project/Pack0001/version")
                .with_body(data.to_string())
                .create_async()
                .await;
        }
        let (outcome, governor) = run(server.url(), selected, ReleasePolicy::Any, bounded).await;
        let _ = failed(&outcome);
        drop(outcome);
        assert_eq!(
            governor.status().reserved,
            ResourceRequest::default(),
            "{mode}"
        );
    }
}

#[tokio::test]
#[ignore = "requires the public Modrinth API"]
async fn live_modrinth_modpack_archive_catalog() {
    let governor = ResourceGovernor::new(ResourceRequest {
        jobs: 1,
        memory_bytes: 512 << 20,
        open_files: 16,
        ..Default::default()
    });
    let runtime = OperationRuntime::new(governor.clone(), 1);
    let mut handle = runtime
        .start(move |mut scope| async move {
            let catalog = ProviderCatalog::new(
                None,
                Arc::new(crate::networking::rate_budget::HostBudgetRegistry::new()),
            )
            .unwrap();
            Ok(catalog
                .resolve_modpack_archive(
                    &mut scope,
                    ModpackSelector::parse(
                        ProviderKind::Modrinth,
                        "https://modrinth.com/modpack/fabulously-optimized",
                    )
                    .unwrap(),
                    ReleasePolicy::PreferStable,
                    SelectionLimits::default(),
                )
                .await)
        })
        .unwrap();
    let outcome = handle.wait().await;
    let archive = ready(&outcome);
    assert_eq!(archive.project().slug, "fabulously-optimized");
    assert!(archive.file().filename.ends_with(".mrpack"));
    assert!(archive.file().expected.size.unwrap() > 0);
    assert!(!archive.file().alternatives.is_empty());
    assert!(archive.file().expected.digests.is_some());
    runtime.shutdown().await;
    drop((handle, runtime, outcome));
    assert_eq!(governor.status().reserved, ResourceRequest::default());
}

#[test]
fn archive_primary_fallback_and_server_pack_types_follow_provider_contracts() {
    let id = ProviderProjectId::Modrinth(ModrinthProjectId::parse("Pack0001").unwrap());
    let mut version = mr("Old00001", "release", "2025-01-01T00:00:00Z");
    version["files"][0]["primary"] = json!(false);
    let page = parse_page(
        &id,
        None,
        ReleasePolicy::Any,
        &serde_json::to_vec(&vec![version]).unwrap(),
        0,
        1,
        &crate::application::process_runtime::Cancellation::default(),
    )
    .unwrap();
    assert_eq!(page.candidates[0].file.filename, "pack.mrpack");
    let id = ProviderProjectId::CurseForge(CurseForgeProjectId::parse("1001").unwrap());
    let mut file = cf(2001, 1, "2025-01-01T00:00:00Z");
    file["isServerPack"] = json!("true");
    assert!(
        parse_page(
            &id,
            Some("2001"),
            ReleasePolicy::Any,
            &serde_json::to_vec(&json!({"data":file})).unwrap(),
            0,
            1,
            &crate::application::process_runtime::Cancellation::default()
        )
        .is_err()
    );
}
#[tokio::test]
async fn incomplete_or_inconsistent_archive_pages_cannot_publish_an_earlier_choice() {
    for mode in [
        "page-limit",
        "record-limit",
        "changed-total",
        "empty-page",
        "overlap",
    ] {
        let mut server = Server::new_async().await;
        server
            .mock("GET", "/mods/1001")
            .with_body(
                json!({"data":{"id":1001,"gameId":432,"classId":4471,"slug":"pack","name":"Pack"}})
                    .to_string(),
            )
            .create_async()
            .await;
        server.mock("GET", "/mods/1001/files").match_query(Matcher::AllOf(vec![Matcher::UrlEncoded("index".into(),"0".into()),Matcher::UrlEncoded("pageSize".into(),"50".into())])).with_body(json!({"data":[cf(2001,1,"2025-01-01T00:00:00Z")],"pagination":{"index":0,"pageSize":1,"resultCount":1,"totalCount":2}}).to_string()).create_async().await;
        let entries = if mode == "empty-page" {
            vec![]
        } else {
            vec![cf(
                if mode == "overlap" { 2001 } else { 2002 },
                1,
                "2025-01-01T00:00:00Z",
            )]
        };
        server.mock("GET", "/mods/1001/files").match_query(Matcher::AllOf(vec![Matcher::UrlEncoded("index".into(),"1".into()),Matcher::UrlEncoded("pageSize".into(),"50".into())])).with_body(json!({"data":entries,"pagination":{"index":1,"pageSize":1,"resultCount":entries.len(),"totalCount":if mode == "changed-total" { 3 } else { 2 }}}).to_string()).expect(if mode == "page-limit" { 0 } else { 1 }).create_async().await;
        let mut bounded = limits();
        if mode == "page-limit" {
            bounded.pages = 1;
        }
        if mode == "record-limit" {
            bounded.candidates = 1;
        }
        let (outcome, governor) = run(
            server.url(),
            ModpackSelector::parse(ProviderKind::CurseForge, "1001").unwrap(),
            ReleasePolicy::Any,
            bounded,
        )
        .await;
        let _ = failed(&outcome);
        drop(outcome);
        assert_eq!(
            governor.status().reserved,
            ResourceRequest::default(),
            "{mode}"
        );
    }
}
