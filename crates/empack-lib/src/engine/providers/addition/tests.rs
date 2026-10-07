use super::*;
use crate::engine::{
    dependency_content::{DependencyContent, DependencyContents},
    project::ProjectReader,
    publication::{Publisher, RecoveryReader},
    resources::{ResourceGovernor, ResourceRequest},
    runtime::{OperationOutcome, OperationRuntime},
    snapshot::SnapshotLimits,
};
use empack_core::{
    identity::{ModrinthProjectId, ModrinthVersionId},
    requirements::{ChoiceKey, OptionalChoice},
};
use mockito::{Matcher, Server};
use serde_json::{Value, json};
use std::fs;
fn current() -> ResolvedProject {
    let base = crate::engine::mrpack::tests::project(false, false);
    let mut intent = base.intent().clone();
    intent.roots.clear();
    let source = DocumentCodec
        .decode_intent(&DocumentCodec.encode_intent(&intent).unwrap(), "current")
        .unwrap();
    let mut lock = base.lock().clone();
    lock.intent_revision = source.semantic_revision();
    lock.dependencies.clear();
    lock.required_edges.clear();
    lock.coverage.clear();
    ResolvedProject::validate(intent, lock, source.semantic_revision()).unwrap()
}
fn input(selector: &str, pinned: bool) -> ProviderAddInput {
    ProviderAddInput {
        selector: ProjectSelector::parse(ProviderKind::Modrinth, selector).unwrap(),
        key: Some(DependencyKey::parse("chosen-alias").unwrap()),
        kind: None,
        pin: pinned
            .then(|| PinSelector::ModrinthVersion(ModrinthVersionId::parse("version1").unwrap())),
        requirements: Requirements {
            client: Requirement::Required,
            server: Requirement::Unsupported,
        },
        folder: None,
        files: ProviderFiles::Primary,
    }
}
fn version(project: &str, id: &str, dependencies: Value) -> Value {
    json!({"id":id,"project_id":project,"game_versions":["1.20.1"],"loaders":["fabric"],"files":[{"filename":format!("{project}.jar"),"primary":true,"size":7,"hashes":{"sha1":"f07e5a815613c5abeddc4b682247a4c42d8a95df"},"url":format!("https://example.com/{project}.jar")}],"dependencies":dependencies,"date_published":"2026-01-01T00:00:00Z","status":"listed","version_type":"release"})
}
async fn records(server: &mut Server, project: &str, slug: &str, kind: &str, version: &Value) {
    let record =
        json!({"id":project,"slug":slug,"title":slug,"project_type":kind,"loaders":["fabric"]});
    for selector in [project, slug] {
        server
            .mock("GET", format!("/project/{selector}").as_str())
            .with_body(record.to_string())
            .create_async()
            .await;
    }
    server
        .mock(
            "GET",
            format!("/version/{}", version["id"].as_str().unwrap()).as_str(),
        )
        .with_body(version.to_string())
        .create_async()
        .await;
    server
        .mock("GET", format!("/project/{project}/version").as_str())
        .match_query(Matcher::Any)
        .with_body(json!([version]).to_string())
        .create_async()
        .await;
}
fn limits() -> ClosureLimits {
    ClosureLimits {
        selection: SelectionLimits {
            catalog: CatalogLimits {
                response_bytes: 8192,
                transfer_bytes: 131072,
                deadline: Duration::from_secs(5),
            },
            ..Default::default()
        },
        projects: 8,
        edges: 16,
    }
}
type Outcome = Arc<OperationOutcome<Result<ProviderAdditionOutcome>>>;
async fn resolve(
    server: &Server,
    inputs: Vec<ProviderAddInput>,
    limits: ClosureLimits,
) -> (Outcome, ResourceGovernor) {
    let catalog = ProviderCatalog::for_loopback_tests(&server.url(), None);
    let governor = ResourceGovernor::new(ResourceRequest {
        jobs: 2,
        memory_bytes: 8 << 20,
        open_files: 4,
        ..Default::default()
    });
    let runtime = OperationRuntime::new(governor.clone(), 1);
    let mut handle = runtime
        .start(move |mut scope| async move {
            Ok(catalog
                .resolve_addition(
                    &mut scope,
                    &current(),
                    NonEmpty::new(inputs).unwrap(),
                    ReleasePolicy::PreferStable,
                    limits,
                )
                .await)
        })
        .unwrap();
    let result = handle.wait().await;
    runtime.release_completed(handle.id());
    runtime.shutdown().await;
    (result, governor)
}
fn ready(result: &OperationOutcome<Result<ProviderAdditionOutcome>>) -> &ProviderAddition {
    match result {
        OperationOutcome::Completed(Ok(ProviderAdditionOutcome::Ready(value))) => value,
        OperationOutcome::Completed(Err(error)) => panic!("{error:#}"),
        _ => panic!("addition did not resolve"),
    }
}
fn references(project: &ResolvedProject) -> DependencyContents {
    project
        .lock()
        .dependencies
        .iter()
        .flat_map(|(key, dependency)| {
            dependency.files.as_slice().iter().map(|file| {
                (
                    crate::engine::mrpack::LockedFileKey {
                        dependency: key.clone(),
                        slot: file.slot.clone(),
                    },
                    DependencyContent::Reference,
                )
            })
        })
        .collect()
}

#[tokio::test]
async fn provider_closure_publishes_canonical_roots_and_preserves_required_content_on_sync() {
    let mut server = Server::new_async().await;
    records(
        &mut server,
        "project1",
        "renderer",
        "mod",
        &version(
            "project1",
            "version1",
            json!([{"project_id":"project2","version_id":"version2","dependency_type":"required"}]),
        ),
    )
    .await;
    records(
        &mut server,
        "project2",
        "library",
        "mod",
        &version("project2", "version2", json!([])),
    )
    .await;
    let (outcome, governor) = resolve(&server, vec![input("renderer", true)], limits()).await;
    let addition = ready(&outcome);
    assert_eq!(addition.project().intent().roots.len(), 1);
    assert_eq!(addition.project().lock().dependencies.len(), 2);
    let key = DependencyKey::parse("chosen-alias").unwrap();
    assert_eq!(
        addition.project().lock().required_edges[&key],
        BTreeSet::from([DependencyKey::parse("library").unwrap()])
    );
    assert!(matches!(
        addition.project().intent().roots[&key].version,
        VersionIntent::Exact(_)
    ));
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("empack.yml"),
        DocumentCodec.encode_intent(current().intent()).unwrap(),
    )
    .unwrap();
    fs::write(
        root.path().join("empack.lock"),
        DocumentCodec.encode_lock(&current()).unwrap(),
    )
    .unwrap();
    let cancel = crate::application::process_runtime::Cancellation::default();
    let reader = ProjectReader::new(RecoveryReader::new(host.path().join("state")));
    let snapshot = reader
        .capture_addition(
            root.path(),
            addition.group(),
            SnapshotLimits::default(),
            &cancel,
        )
        .unwrap();
    crate::engine::addition::plan_addition(
        snapshot,
        addition.group(),
        references(addition.project()),
        &cancel,
    )
    .unwrap()
    .stage(&cancel)
    .unwrap()
    .publish(
        &Publisher::open(&host.path().join("state")).unwrap(),
        &cancel,
    )
    .unwrap();
    for _ in 0..2 {
        let snapshot = reader
            .capture_synchronization(root.path(), SnapshotLimits::default(), &cancel)
            .unwrap();
        let planned = crate::engine::synchronization::plan_synchronization_with_resolution(
            snapshot,
            references(addition.project()),
            None,
            &cancel,
        )
        .unwrap();
        assert!(planned.stage(&cancel).unwrap().files().changes().is_empty());
    }
    drop(outcome);
    assert_eq!(governor.status().reserved, ResourceRequest::default());
}

#[tokio::test]
async fn equivalent_resource_pack_selectors_remain_unpinned_and_use_kind_specific_placement() {
    let mut server = Server::new_async().await;
    records(
        &mut server,
        "project1",
        "assets",
        "resourcepack",
        &version("project1", "version1", json!([])),
    )
    .await;
    for selector in [
        "assets",
        "project1",
        "https://modrinth.com/resourcepack/assets",
    ] {
        let (outcome, governor) = resolve(&server, vec![input(selector, false)], limits()).await;
        let project = ready(&outcome).project();
        let key = DependencyKey::parse("chosen-alias").unwrap();
        let root = &project.intent().roots[&key];
        assert_eq!(
            root.source,
            SourceIntent::Provider(ProviderProjectId::Modrinth(
                ModrinthProjectId::parse("project1").unwrap()
            ))
        );
        assert_eq!(root.version, VersionIntent::FollowCompatible);
        assert_eq!(root.placement, PlacementIntent::Automatic);
        assert_eq!(root.kind, ContentKind::ResourcePack);
        assert_eq!(
            project.lock().dependencies[&key].files.as_slice()[0]
                .placements
                .as_slice()[0]
                .destination
                .relative()
                .as_str(),
            "resourcepacks/project1.jar"
        );
        drop(outcome);
        assert_eq!(governor.status().reserved, ResourceRequest::default());
    }
}

#[tokio::test]
async fn unresolved_required_identity_returns_input_without_a_publishable_subset() {
    let mut server = Server::new_async().await;
    records(
        &mut server,
        "project1",
        "renderer",
        "mod",
        &version(
            "project1",
            "version1",
            json!([{"file_name":"unknown.jar","dependency_type":"required"}]),
        ),
    )
    .await;
    let (outcome, governor) = resolve(&server, vec![input("renderer", true)], limits()).await;
    let OperationOutcome::Completed(Ok(ProviderAdditionOutcome::NeedsInput(evidence))) = &*outcome
    else {
        panic!("missing identity was lost")
    };
    assert!(!evidence.issues.is_empty());
    drop(outcome);
    assert_eq!(governor.status().reserved, ResourceRequest::default());
}

#[test]
fn required_participation_dominates_multiple_optional_roots_independently_of_order() {
    let a = Requirement::Optional(OptionalChoice {
        key: ChoiceKey::parse("a").unwrap(),
        default_enabled: false,
        description: None,
    });
    let b = Requirement::Optional(OptionalChoice {
        key: ChoiceKey::parse("b").unwrap(),
        default_enabled: true,
        description: None,
    });
    assert!(merge_requirements(vec![&a, &b]).is_err());
    assert_eq!(
        merge_requirements(vec![&a, &b, &Requirement::Required]).unwrap(),
        Requirement::Required
    );
    assert_eq!(
        merge_requirements(vec![&Requirement::Required, &b, &a]).unwrap(),
        Requirement::Required
    );
}

#[tokio::test]
async fn provider_addition_shares_budget_and_rejects_foreign_pins_and_duplicate_roots() {
    for failure in ["budget", "owner", "duplicate"] {
        let mut server = Server::new_async().await;
        let record = version(
            if failure == "owner" {
                "foreign1"
            } else {
                "project1"
            },
            "version1",
            json!([]),
        );
        records(&mut server, "project1", "renderer", "mod", &record).await;
        let inputs = if failure == "duplicate" {
            vec![input("renderer", true), input("project1", true)]
        } else {
            vec![input("renderer", failure != "budget")]
        };
        let mut limits = limits();
        if failure == "budget" {
            limits.selection.catalog.transfer_bytes = 800;
        }
        let (outcome, governor) = resolve(&server, inputs, limits).await;
        assert!(
            matches!(&*outcome, OperationOutcome::Completed(Err(_))),
            "{failure}"
        );
        drop(outcome);
        assert_eq!(governor.status().reserved, ResourceRequest::default());
    }
}

#[tokio::test]
async fn companion_files_require_complete_explicit_placement_without_losing_requirements() {
    let mut server = Server::new_async().await;
    let mut record = version("project1", "version1", json!([]));
    let mut companion = record["files"][0].clone();
    companion["filename"] = json!("resources.zip");
    companion["primary"] = json!(false);
    companion["file_type"] = json!("required-resource-pack");
    record["files"].as_array_mut().unwrap().push(companion);
    records(&mut server, "project1", "renderer", "mod", &record).await;
    let (outcome, _) = resolve(&server, vec![input("renderer", true)], limits()).await;
    assert!(matches!(&*outcome, OperationOutcome::Completed(Err(_))));
    let placement = |destination: &str| {
        NonEmpty::new(vec![Placement {
            destination: InstallDestination::parse(destination).unwrap(),
            layer: ContentLayer::Common,
            requirements: input("renderer", true).requirements,
        }])
        .unwrap()
    };
    for complete in [false, true] {
        let mut selected = input("renderer", true);
        let mut placements = BTreeMap::from([("project1.jar".into(), placement("mods/main.jar"))]);
        if complete {
            placements.insert(
                "resources.zip".into(),
                placement("resourcepacks/companion.zip"),
            );
        }
        selected.files = ProviderFiles::Placed(placements);
        let (outcome, _) = resolve(&server, vec![selected], limits()).await;
        if !complete {
            assert!(matches!(&*outcome, OperationOutcome::Completed(Err(_))));
            continue;
        }
        let project = ready(&outcome).project();
        let dependency = project.lock().dependencies.values().next().unwrap();
        assert_eq!(dependency.files.as_slice().len(), 2);
        assert!(
            !dependency.files.as_slice()[1]
                .provenance
                .conversions
                .is_empty()
        );
        assert_eq!(
            dependency.files.as_slice()[1].placements.as_slice()[0]
                .destination
                .relative()
                .as_str(),
            "resourcepacks/companion.zip"
        );
        assert_eq!(
            dependency.files.as_slice()[1].placements.as_slice()[0]
                .requirements
                .server,
            Requirement::Unsupported
        );
        assert_eq!(
            dependency.files.as_slice()[1].provenance.declared_digests,
            dependency.files.as_slice()[1].expected.digests
        );
    }
}

#[tokio::test]
async fn explicit_file_selection_retains_placement_intent() {
    let mut server = Server::new_async().await;
    records(
        &mut server,
        "project1",
        "renderer",
        "mod",
        &version("project1", "version1", json!([])),
    )
    .await;
    for files in [
        ProviderFiles::All,
        ProviderFiles::Named(BTreeSet::from(["project1.jar".into()])),
    ] {
        let mut selected = input("renderer", true);
        selected.files = files;
        let (outcome, _) = resolve(&server, vec![selected], limits()).await;
        let project = ready(&outcome).project();
        let root = project.intent().roots.values().next().unwrap();
        let PlacementIntent::Explicit(placements) = &root.placement else {
            panic!("explicit selection became automatic")
        };
        assert_eq!(
            placements.as_slice(),
            project
                .lock()
                .dependencies
                .values()
                .next()
                .unwrap()
                .files
                .as_slice()[0]
                .placements
                .as_slice()
        );
    }
}
