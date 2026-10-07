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
    resolve_with_current(server, current(), inputs, limits).await
}
async fn resolve_with_current(
    server: &Server,
    current: ResolvedProject,
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
                    &current,
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

#[tokio::test]
async fn required_dependencies_reuse_compatible_locked_pins_aliases_and_placements() {
    let mut server = Server::new_async().await;
    records(
        &mut server,
        "project2",
        "library",
        "mod",
        &version("project2", "version2", json!([])),
    )
    .await;
    let mut library = input("library", false);
    library.key = Some(DependencyKey::parse("kept-alias").unwrap());
    library.pin = Some(PinSelector::ModrinthVersion(
        ModrinthVersionId::parse("version2").unwrap(),
    ));
    library.folder = Some(
        PortableRelPath::parse(
            "custom/libraries",
            empack_core::path::PathSyntax::ProjectContent,
        )
        .unwrap(),
    );
    library.requirements.server = Requirement::Required;
    let (prior, _) = resolve(&server, vec![library], limits()).await;
    let prior = ready(&prior).project().clone();
    let retained_key = DependencyKey::parse("kept-alias").unwrap();
    let before = prior.lock().dependencies[&retained_key].clone();
    server.reset();
    let mut newest = version("project2", "version3", json!([]));
    newest["date_published"] = json!("2026-07-01T00:00:00Z");
    records(&mut server, "project2", "library", "mod", &newest).await;
    server
        .mock("GET", "/version/version2")
        .with_body(version("project2", "version2", json!([])).to_string())
        .expect(1)
        .create_async()
        .await;
    records(
        &mut server,
        "project1",
        "renderer",
        "mod",
        &version(
            "project1",
            "version1",
            json!([{"project_id":"project2","dependency_type":"required"}]),
        ),
    )
    .await;
    let (outcome, _) = resolve_with_current(
        &server,
        prior.clone(),
        vec![input("renderer", true)],
        limits(),
    )
    .await;
    let addition = ready(&outcome);
    let plan = empack_core::addition::AdditionPlan::prepare(&prior, addition.group()).unwrap();
    let source = DocumentCodec
        .decode_intent(
            &DocumentCodec.encode_intent(plan.intent()).unwrap(),
            "merged",
        )
        .unwrap();
    let merged = plan.resolve(source.semantic_revision()).unwrap();
    assert_eq!(merged.lock().dependencies[&retained_key], before);
    assert_eq!(
        merged.intent().roots[&retained_key],
        prior.intent().roots[&retained_key]
    );
    assert_eq!(
        merged.lock().required_edges[&DependencyKey::parse("chosen-alias").unwrap()],
        BTreeSet::from([retained_key])
    );
    assert_eq!(merged.lock().dependencies.len(), 2);
}

#[tokio::test]
async fn transitive_labels_do_not_collide_with_explicit_roots() {
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
    let mut root = input("renderer", true);
    root.key = Some(DependencyKey::parse("library").unwrap());
    let (outcome, _) = resolve(&server, vec![root], limits()).await;
    let project = ready(&outcome).project();
    let key = DependencyKey::parse("library").unwrap();
    assert_eq!(
        project.intent().roots.keys().collect::<Vec<_>>(),
        vec![&key]
    );
    let required = &project.lock().required_edges[&key];
    assert_eq!(required.len(), 1);
    assert!(!required.contains(&key));
    let dependency = &project.lock().dependencies[required.first().unwrap()];
    assert_eq!(
        dependency.identity,
        ResolvedIdentity::Provider(ProviderProjectId::Modrinth(
            ModrinthProjectId::parse("project2").unwrap()
        ))
    );
}

#[tokio::test]
async fn explicit_root_participation_cannot_be_expanded_by_another_root() {
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
    let mut parent = input("renderer", true);
    parent.requirements = Requirements {
        client: Requirement::Unsupported,
        server: Requirement::Required,
    };
    let mut child = input("library", false);
    child.key = Some(DependencyKey::parse("library").unwrap());
    child.pin = Some(PinSelector::ModrinthVersion(
        ModrinthVersionId::parse("version2").unwrap(),
    ));
    let (outcome, _) = resolve(&server, vec![parent.clone(), child.clone()], limits()).await;
    let OperationOutcome::Completed(Err(error)) = &*outcome else {
        panic!("unsupported side was silently enabled")
    };
    assert!(error.to_string().contains("explicit root's environment"));
    // The host can explicitly authorize both sides; it must not make that choice itself.
    child.requirements.server = Requirement::Required;
    let (outcome, _) = resolve(&server, vec![parent, child], limits()).await;
    assert_eq!(
        ready(&outcome).project().intent().roots[&DependencyKey::parse("library").unwrap()]
            .requirements
            .server,
        Requirement::Required
    );
}

#[tokio::test]
async fn retained_dependency_reuse_refuses_incompatible_pins_assertions_and_participation() {
    let mut server = Server::new_async().await;
    records(
        &mut server,
        "project2",
        "library",
        "mod",
        &version("project2", "version2", json!([])),
    )
    .await;
    let mut child = input("library", false);
    child.key = Some(DependencyKey::parse("library").unwrap());
    child.pin = Some(PinSelector::ModrinthVersion(
        ModrinthVersionId::parse("version2").unwrap(),
    ));
    let (prior, _) = resolve(&server, vec![child], limits()).await;
    let prior = ready(&prior).project().clone();
    for fault in ["compatibility", "assertions", "participation", "exact pin"] {
        server.reset();
        let mut existing = version("project2", "version2", json!([]));
        if fault == "compatibility" {
            existing["game_versions"] = json!(["1.21.1"]);
        }
        if fault == "assertions" {
            existing["files"][0]["hashes"]["sha1"] =
                json!("0000000000000000000000000000000000000000");
        }
        if fault == "exact pin" {
            existing["id"] = json!("version3");
        }
        records(&mut server, "project2", "library", "mod", &existing).await;
        let mut edge = json!({"project_id":"project2","dependency_type":"required"});
        if fault == "exact pin" {
            edge["version_id"] = json!("version3");
        }
        records(
            &mut server,
            "project1",
            "renderer",
            "mod",
            &version("project1", "version1", json!([edge])),
        )
        .await;
        let mut root = input("renderer", true);
        if fault == "participation" {
            root.requirements.server = Requirement::Required;
        }
        let (outcome, governor) =
            resolve_with_current(&server, prior.clone(), vec![root], limits()).await;
        match &*outcome {
            OperationOutcome::Completed(Ok(ProviderAdditionOutcome::NeedsInput(evidence)))
                if fault == "compatibility" =>
            {
                assert!(evidence.issues.iter().any(|issue| matches!(
                    issue.kind,
                    ClosureIssueKind::IncompatibleRequirement { .. }
                )));
            }
            OperationOutcome::Completed(Err(error)) => {
                let expected = match fault {
                    "assertions" => "assertions changed",
                    "participation" => "participation needs",
                    "exact pin" => "conflicts with retained",
                    _ => panic!("unexpected error: {error}"),
                };
                assert!(error.to_string().contains(expected), "{error}");
            }
            _ => panic!("{fault} was silently accepted"),
        }
        drop(outcome);
        assert_eq!(governor.status().reserved, ResourceRequest::default());
    }
}

#[tokio::test]
async fn generated_labels_preserve_unrelated_retained_records() {
    let mut server = Server::new_async().await;
    records(
        &mut server,
        "project3",
        "unrelated",
        "mod",
        &version("project3", "version3", json!([])),
    )
    .await;
    let mut unrelated = input("unrelated", false);
    unrelated.key = Some(DependencyKey::parse("library").unwrap());
    let (prior, _) = resolve(&server, vec![unrelated], limits()).await;
    let prior = ready(&prior).project().clone();
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
    let (outcome, _) = resolve_with_current(
        &server,
        prior.clone(),
        vec![input("renderer", true)],
        limits(),
    )
    .await;
    let plan =
        empack_core::addition::AdditionPlan::prepare(&prior, ready(&outcome).group()).unwrap();
    let source = DocumentCodec
        .decode_intent(
            &DocumentCodec.encode_intent(plan.intent()).unwrap(),
            "merged",
        )
        .unwrap();
    let merged = plan.resolve(source.semantic_revision()).unwrap();
    let key = DependencyKey::parse("library").unwrap();
    assert_eq!(merged.intent().roots[&key], prior.intent().roots[&key]);
    assert_eq!(
        merged.lock().dependencies[&key],
        prior.lock().dependencies[&key]
    );
    assert_eq!(merged.lock().dependencies.len(), 3);
    assert!(
        !merged.lock().required_edges[&DependencyKey::parse("chosen-alias").unwrap()]
            .contains(&key)
    );
}

#[tokio::test]
async fn provider_acquisition_materializes_a_complete_native_addition_or_returns_no_subset() {
    use crate::engine::{
        acquisition::{HttpAcquisition, TransferLimits},
        content::SourceEvidencePolicy,
    };
    for mode in ["complete", "digest", "budget", "missing", "extra"] {
        let mut server = Server::new_async().await;
        for (id, slug, pin, deps) in [
            (
                "project1",
                "renderer",
                "version1",
                json!([{"project_id":"project2","version_id":"version2","dependency_type":"required"}]),
            ),
            ("project2", "library", "version2", json!([])),
        ] {
            let mut value = version(id, pin, deps);
            value["files"][0]["url"] = json!(format!("{}/{id}", server.url()));
            records(&mut server, id, slug, "mod", &value).await;
        }
        let first = server
            .mock("GET", "/project1")
            .with_body("payload")
            .expect(if matches!(mode, "missing" | "extra") {
                0
            } else {
                1
            })
            .create_async()
            .await;
        let second = server
            .mock("GET", "/project2")
            .with_body(if mode == "digest" {
                "changed"
            } else {
                "payload"
            })
            .expect(if matches!(mode, "missing" | "extra") {
                0
            } else {
                1
            })
            .create_async()
            .await;
        let catalog = ProviderCatalog::for_loopback_tests(&server.url(), None);
        let governor = ResourceGovernor::new(ResourceRequest {
            jobs: 2,
            memory_bytes: 8 << 20,
            scratch_bytes: 1 << 20,
            open_files: 20,
        });
        let runtime = OperationRuntime::new(governor.clone(), 1);
        let mut handle = runtime
            .start(move |mut scope| async move {
                let result = async {
                    let ProviderAdditionOutcome::Ready(addition) = catalog
                        .resolve_addition(
                            &mut scope,
                            &current(),
                            NonEmpty::new(vec![input("renderer", true)])?,
                            ReleasePolicy::PreferStable,
                            limits(),
                        )
                        .await?
                    else {
                        anyhow::bail!("incomplete closure")
                    };
                    let mut choices: BTreeMap<_, _> = references(addition.project())
                        .into_keys()
                        .map(|key| (key, ProviderContentChoice::Acquire))
                        .collect();
                    if mode == "missing" {
                        choices.pop_last();
                    }
                    if mode == "extra" {
                        choices.insert(
                            crate::engine::mrpack::LockedFileKey {
                                dependency: DependencyKey::parse("extra")?,
                                slot: FileSlot::parse("extra")?,
                            },
                            ProviderContentChoice::Acquire,
                        );
                    }
                    let content = addition
                        .acquire_content(
                            &mut scope,
                            &HttpAcquisition::for_loopback_tests(),
                            choices,
                            SourceEvidencePolicy::Compatibility,
                            TransferLimits {
                                file_bytes: 32,
                                transfer_bytes: if mode == "budget" { 10 } else { 32 },
                                deadline: Duration::from_secs(3),
                                redirects: 2,
                            },
                        )
                        .await?;
                    Ok::<_, anyhow::Error>((addition, content))
                }
                .await;
                Ok(result)
            })
            .unwrap();
        let outcome = handle.wait().await;
        runtime.release_completed(handle.id());
        runtime.shutdown().await;
        drop(handle);
        if mode == "complete" {
            let OperationOutcome::Completed(Ok((addition, content))) = &*outcome else {
                panic!("acquisition failed")
            };
            assert!(content.complete());
            assert_eq!(content.content().len(), 2);
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
                content.content().clone(),
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
            for id in ["project1", "project2"] {
                assert_eq!(
                    fs::read(root.path().join(format!("pack/mods/{id}.jar"))).unwrap(),
                    b"payload"
                );
            }
            for _ in 0..2 {
                let snapshot = reader
                    .capture_synchronization(root.path(), SnapshotLimits::default(), &cancel)
                    .unwrap();
                let sync = crate::engine::synchronization::plan_synchronization_with_resolution(
                    snapshot,
                    content.content().clone(),
                    None,
                    &cancel,
                )
                .unwrap()
                .stage(&cancel)
                .unwrap();
                assert!(sync.files().changes().is_empty());
            }
        } else {
            assert!(
                matches!(&*outcome, OperationOutcome::Completed(Err(_))),
                "{mode}"
            );
        }
        first.assert_async().await;
        second.assert_async().await;
        drop(outcome);
        assert_eq!(governor.status().reserved, ResourceRequest::default());
    }
}

#[tokio::test]
async fn restricted_provider_content_requires_explicit_verified_association_or_reference() {
    use crate::engine::{
        acquisition::{HttpAcquisition, TransferLimits},
        content::{AcquiredContent, InitialObservation, SourceEvidencePolicy, verify_stream},
        mrpack::AcquiredBuildFile,
    };
    use empack_core::{
        digest::DigestAlgorithm, files::FilePermissions, identity::CurseForgeFileId,
    };
    let mut server = Server::new_async().await;
    server.mock("GET", "/mods/123").with_body(json!({"data":{"id":123,"gameId":432,"classId":6,"slug":"restricted","name":"Restricted"}}).to_string()).create_async().await;
    server.mock("GET", "/mods/123/files/456").with_body(json!({"data":{"id":456,"gameId":432,"modId":123,"fileName":"restricted.jar","fileLength":7,"downloadUrl":null,"hashes":[{"algo":2,"value":"321c3cf486ed509164edec1e1981fec8"}],"gameVersions":["1.20.1","Fabric"],"dependencies":[],"isAvailable":true,"releaseType":1,"fileDate":"2026-01-01T00:00:00Z"}}).to_string()).create_async().await;
    let catalog = ProviderCatalog::for_loopback_tests(&server.url(), Some("fixture-key".into()));
    let governor = ResourceGovernor::new(ResourceRequest {
        jobs: 2,
        memory_bytes: 8 << 20,
        scratch_bytes: 1 << 20,
        open_files: 20,
    });
    let runtime = OperationRuntime::new(governor.clone(), 1);
    let mut handle = runtime
        .start(move |mut scope| async move {
            let result = async {
                let mut selected = input("renderer", false);
                selected.selector = ProjectSelector::parse(ProviderKind::CurseForge, "123")?;
                selected.pin = Some(PinSelector::CurseForgeFile(CurseForgeFileId::parse("456")?));
                let ProviderAdditionOutcome::Ready(addition) = catalog
                    .resolve_addition(
                        &mut scope,
                        &current(),
                        NonEmpty::new(vec![selected])?,
                        ReleasePolicy::PreferStable,
                        limits(),
                    )
                    .await?
                else {
                    anyhow::bail!("incomplete")
                };
                let key = references(addition.project()).into_keys().next().unwrap();
                let limits = TransferLimits {
                    file_bytes: 32,
                    transfer_bytes: 32,
                    deadline: Duration::from_secs(3),
                    redirects: 2,
                };
                let transport =
                    catalog.configure_acquisition(HttpAcquisition::for_loopback_tests());
                let pending = addition
                    .acquire_content(
                        &mut scope,
                        &transport,
                        BTreeMap::from([(key.clone(), ProviderContentChoice::Acquire)]),
                        SourceEvidencePolicy::Compatibility,
                        limits,
                    )
                    .await?;
                assert!(!pending.complete());
                assert!(pending.content().is_empty());
                assert_eq!(
                    pending.pending()[&key].reason,
                    ProviderInputReason::RestrictedDownload
                );
                assert_eq!(
                    pending.pending()[&key]
                        .expected
                        .digests
                        .as_ref()
                        .unwrap()
                        .strongest(),
                    DigestAlgorithm::Md5
                );
                drop(pending);
                let reference = addition
                    .acquire_content(
                        &mut scope,
                        &transport,
                        BTreeMap::from([(key.clone(), ProviderContentChoice::Reference)]),
                        SourceEvidencePolicy::Compatibility,
                        limits,
                    )
                    .await?;
                assert!(reference.complete());
                assert!(matches!(
                    reference.content()[&key],
                    DependencyContent::Reference
                ));
                drop(reference);
                for payload in [b"changed", b"payload"] {
                    let work = scope.spawn_blocking(
                        ResourceRequest {
                            jobs: 1,
                            memory_bytes: 1 << 16,
                            scratch_bytes: 32,
                            open_files: 3,
                        },
                        ResourceRequest {
                            scratch_bytes: 32,
                            open_files: 3,
                            ..Default::default()
                        },
                        move |cancel| {
                            verify_stream(
                                &mut &payload[..],
                                &ExpectedContent {
                                    digests: None,
                                    size: Some(7),
                                    accepted_observation: None,
                                },
                                32,
                                SourceEvidencePolicy::Compatibility,
                                InitialObservation::Accepted,
                                &cancel,
                            )
                        },
                    )?;
                    let supplied = AcquiredContent::retain_resources(
                        scope.accept(work.wait().await?)?.transpose()?,
                    )?;
                    let result = addition
                        .acquire_content(
                            &mut scope,
                            &transport,
                            BTreeMap::from([(
                                key.clone(),
                                ProviderContentChoice::Supplied(AcquiredBuildFile {
                                    content: supplied,
                                    permissions: FilePermissions {
                                        readonly: true,
                                        executable: false,
                                    },
                                }),
                            )]),
                            SourceEvidencePolicy::Compatibility,
                            limits,
                        )
                        .await;
                    if payload == b"changed" {
                        assert!(result.is_err());
                    } else {
                        let result = result?;
                        assert!(result.complete());
                        assert!(
                            result.content()[&key]
                                .materialized()
                                .unwrap()
                                .permissions
                                .readonly
                        );
                    }
                }
                Ok::<_, anyhow::Error>(())
            }
            .await;
            Ok(result)
        })
        .unwrap();
    let outcome = handle.wait().await;
    let OperationOutcome::Completed(Ok(())) = &*outcome else {
        panic!("restricted acquisition failed")
    };
    runtime.release_completed(handle.id());
    runtime.shutdown().await;
    drop(handle);
    drop(outcome);
    assert_eq!(governor.status().reserved, ResourceRequest::default());
}

#[tokio::test]
async fn retained_companion_participation_is_preserved_without_satisfying_the_main_content() {
    for main_on_server in [true, false] {
        let mut server = Server::new_async().await;
        let mut library_version = version("project2", "version2", json!([]));
        let mut companion = library_version["files"][0].clone();
        companion["filename"] = json!("companion.zip");
        companion["primary"] = json!(false);
        companion["file_type"] = json!("optional-resource-pack");
        library_version["files"]
            .as_array_mut()
            .unwrap()
            .push(companion);
        records(&mut server, "project2", "library", "mod", &library_version).await;
        let mut library = input("library", false);
        library.key = Some(DependencyKey::parse("library").unwrap());
        library.pin = Some(PinSelector::ModrinthVersion(
            ModrinthVersionId::parse("version2").unwrap(),
        ));
        library.requirements.server = if main_on_server {
            Requirement::Required
        } else {
            Requirement::Unsupported
        };
        library.files = ProviderFiles::Placed(BTreeMap::from([
            (
                "project2.jar".into(),
                NonEmpty::new(vec![Placement {
                    destination: InstallDestination::parse("mods/library.jar").unwrap(),
                    layer: ContentLayer::Common,
                    requirements: library.requirements.clone(),
                }])
                .unwrap(),
            ),
            (
                "companion.zip".into(),
                NonEmpty::new(vec![Placement {
                    destination: InstallDestination::parse("resourcepacks/companion.zip").unwrap(),
                    layer: ContentLayer::Common,
                    requirements: Requirements {
                        client: Requirement::Required,
                        server: if main_on_server {
                            Requirement::Unsupported
                        } else {
                            Requirement::Required
                        },
                    },
                }])
                .unwrap(),
            ),
        ]));
        let (prior, _) = resolve(&server, vec![library], limits()).await;
        let prior = ready(&prior).project().clone();
        records(
            &mut server,
            "project1",
            "renderer",
            "mod",
            &version(
                "project1",
                "version1",
                json!([{"project_id":"project2","dependency_type":"required"}]),
            ),
        )
        .await;
        let mut root = input("renderer", true);
        root.requirements.server = Requirement::Required;
        let (outcome, _) = resolve_with_current(&server, prior.clone(), vec![root], limits()).await;
        if !main_on_server {
            let OperationOutcome::Completed(Err(error)) = &*outcome else {
                panic!("companion incorrectly satisfied the mod requirement")
            };
            assert!(error.to_string().contains("participation needs"));
            continue;
        }
        let plan =
            empack_core::addition::AdditionPlan::prepare(&prior, ready(&outcome).group()).unwrap();
        let source = DocumentCodec
            .decode_intent(
                &DocumentCodec.encode_intent(plan.intent()).unwrap(),
                "merged",
            )
            .unwrap();
        let merged = plan.resolve(source.semantic_revision()).unwrap();
        let key = DependencyKey::parse("library").unwrap();
        assert_eq!(
            merged.lock().dependencies[&key],
            prior.lock().dependencies[&key]
        );
    }
}
