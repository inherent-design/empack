use super::*;
use crate::engine::{
    addition::tests::fixture,
    documents::DocumentCodec,
    resources::ResourceGovernor,
    runtime::{OperationOutcome, OperationRuntime},
};
use mockito::{Matcher, Server};
use serde_json::{Value, json};

fn version(project: &str, id: &str, game: &str, deps: Value) -> Value {
    json!({"id":id,"project_id":project,"version_number":id,"version_type":"release","date_published":"2026-01-01T00:00:00Z","status":"listed","game_versions":[game],"loaders":["minecraft"],"dependencies":deps,"files":[{"filename":"assets.zip","primary":true,"size":7,"hashes":{"sha512":"11".repeat(64)},"url":"https://example.com/assets.zip"}]})
}
async fn records(server: &mut Server, project: &str, id: &str, deps: Value) {
    server
        .mock("GET", format!("/project/{project}").as_str())
        .with_body(
            json!({"id":project,"slug":project,"title":project,"project_type":"resourcepack"})
                .to_string(),
        )
        .create_async()
        .await;
    let value = version(project, id, "1.20.1", deps);
    server
        .mock("GET", format!("/project/{project}/version").as_str())
        .match_query(Matcher::Any)
        .with_body(json!([value]).to_string())
        .create_async()
        .await;
    server
        .mock("GET", format!("/version/{id}").as_str())
        .with_body(value.to_string())
        .create_async()
        .await;
}
async fn assess(server: &Server, project: ResolvedProject) -> UpdateAssessment {
    let catalog = ProviderCatalog::for_loopback_tests(&server.url(), None);
    let runtime = OperationRuntime::new(
        ResourceGovernor::new(ResourceRequest {
            jobs: 2,
            memory_bytes: 64 << 20,
            open_files: 8,
            ..Default::default()
        }),
        1,
    );
    let mut handle = runtime
        .start(move |mut scope| async move {
            Ok(catalog
                .assess_updates(
                    &mut scope,
                    &project,
                    ReleasePolicy::PreferStable,
                    ClosureLimits::default(),
                )
                .await)
        })
        .unwrap();
    let outcome = handle.wait().await;
    runtime.shutdown().await;
    runtime.release_completed(handle.id());
    drop(handle);
    let OperationOutcome::Completed(report) =
        Arc::try_unwrap(outcome).ok().expect("retired outcome")
    else {
        panic!("assessment failed")
    };
    report.unwrap()
}
#[tokio::test]
async fn changed_rows_require_joint_validation_and_pins_remain_exact() {
    let mut server = Server::new_async().await;
    records(&mut server, "Project1", "Version2", json!([])).await;
    server
        .mock("GET", "/version/Version1")
        .with_body(version("Project1", "Version1", "1.20.1", json!([])).to_string())
        .create_async()
        .await;
    let project = fixture(&[("root", "Project1", "Version1")], &["root"], &[], true);
    let report = assess(&server, project.clone()).await;
    assert_eq!(report.dependencies[0].state, "change-available");
    assert_eq!(
        report.dependencies[0].permitted.as_ref().unwrap().selection,
        "Version2"
    );
    assert_eq!(report.joint.state, "validated");
    let mut intent = project.intent().clone();
    intent.roots.values_mut().next().unwrap().version = VersionIntent::Exact(
        project
            .lock()
            .dependencies
            .values()
            .next()
            .unwrap()
            .selected
            .as_ref()
            .unwrap()
            .selection
            .clone(),
    );
    let revision = DocumentCodec
        .decode_intent(&DocumentCodec.encode_intent(&intent).unwrap(), "pinned")
        .unwrap()
        .semantic_revision();
    let mut lock = project.lock().clone();
    lock.intent_revision = revision;
    let report = assess(
        &server,
        ResolvedProject::validate(intent, lock, revision).unwrap(),
    )
    .await;
    assert_eq!(report.dependencies[0].state, "pinned");
    assert_eq!(
        report.dependencies[0].permitted.as_ref().unwrap().selection,
        "Version1"
    );
    assert_eq!(
        report.dependencies[0].upstream.as_ref().unwrap().selection,
        "Version2"
    );
    assert_eq!(report.joint.state, "validated");
}
#[tokio::test]
async fn individually_eligible_updates_with_incompatible_edges_are_blocked() {
    let mut server = Server::new_async().await;
    records(
        &mut server,
        "Project1",
        "Version3",
        json!([{"project_id":"Project2","version_id":"Version4","dependency_type":"incompatible"}]),
    )
    .await;
    records(&mut server, "Project2", "Version4", json!([])).await;
    let report = assess(
        &server,
        fixture(
            &[("a", "Project1", "Version1"), ("b", "Project2", "Version2")],
            &["a", "b"],
            &[],
            true,
        ),
    )
    .await;
    assert!(
        report
            .dependencies
            .iter()
            .all(|row| row.state == "change-available")
    );
    assert_eq!(report.joint.state, "blocked");
    assert!(
        report
            .joint
            .reasons
            .contains(&UpdateReason::DependencyConflict)
    );
}
#[tokio::test]
async fn throttled_catalog_is_unknown_and_does_not_reset_shared_budget() {
    let mut server = Server::new_async().await;
    server
        .mock("GET", "/project/Project1")
        .with_status(429)
        .create_async()
        .await;
    let skipped = server
        .mock("GET", "/project/Project2")
        .expect(0)
        .create_async()
        .await;
    let report = assess(
        &server,
        fixture(
            &[("a", "Project1", "Version1"), ("b", "Project2", "Version2")],
            &["a", "b"],
            &[],
            true,
        ),
    )
    .await;
    assert_eq!(report.dependencies[0].state, "unknown");
    assert_eq!(
        report.dependencies[0].reasons,
        vec![UpdateReason::RateLimited]
    );
    assert!(!report.dependencies[0].catalog_complete);
    assert_eq!(
        report.dependencies[1].reasons,
        vec![UpdateReason::NotAssessedAfterFailure]
    );
    assert_eq!(report.joint.state, "not-assessed");
    skipped.assert_async().await;
}
#[tokio::test]
async fn manual_sources_are_not_reported_as_current_provider_versions() {
    let server = Server::new_async().await;
    let report = assess(&server, crate::engine::mrpack::tests::project(false, false)).await;
    assert_eq!(report.dependencies[0].state, "manual");
    assert_eq!(
        report.dependencies[0].reasons,
        vec![UpdateReason::ManualSource]
    );
    assert!(report.dependencies[0].permitted.is_none());
    assert_eq!(report.joint.state, "not-applicable");
}
