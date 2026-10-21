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
    assess_with_limits(server, project, ClosureLimits::default()).await
}
async fn assess_with_limits(
    server: &Server,
    project: ResolvedProject,
    limits: ClosureLimits,
) -> UpdateAssessment {
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
                .assess_updates(&mut scope, &project, None, limits)
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
    assert!(report.joint.declarations_complete);
    let mut intent = project.intent().clone();
    intent.roots.values_mut().next().unwrap().selection = DependencySelection::ProviderPin(
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
async fn declared_incompatibilities_do_not_claim_placement_aware_validation() {
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
    assert_eq!(report.joint.state, "validated");
    assert_eq!(report.joint.candidate_compatibility, "not-assessed");
    assert_eq!(report.joint.declared_incompatibilities.len(), 1);
    assert_eq!(
        report.joint.declared_incompatibilities[0].from.selection,
        "Version3"
    );
    assert_eq!(
        report.joint.declared_incompatibilities[0].target.selection,
        "Version4"
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

#[tokio::test]
async fn assessment_uses_authored_preferences_and_reports_each_effective_policy() {
    let mut server = Server::new_async().await;
    server
        .mock("GET", "/project/Project1")
        .with_body(
            json!({"id":"Project1","slug":"assets","title":"Assets","project_type":"resourcepack"})
                .to_string(),
        )
        .create_async()
        .await;
    let mut candidate = version("Project1", "Version2", "1.20.1", json!([]));
    candidate["version_type"] = json!("beta");
    server
        .mock("GET", "/project/Project1/version")
        .match_query(Matcher::Any)
        .with_body(json!([candidate.clone()]).to_string())
        .create_async()
        .await;
    server
        .mock("GET", "/version/Version2")
        .with_body(candidate.to_string())
        .create_async()
        .await;
    server
        .mock("GET", "/version/Version1")
        .with_body(version("Project1", "Version1", "1.20.1", json!([])).to_string())
        .create_async()
        .await;
    let base = fixture(&[("root", "Project1", "Version1")], &["root"], &[], true);
    for preference in [Some(ReleasePolicy::Any), None] {
        let mut intent = base.intent().clone();
        intent.dependency_policy = ReleasePolicy::StableOnly;
        intent.roots.values_mut().next().unwrap().releases = preference;
        let revision = DocumentCodec
            .decode_intent(&DocumentCodec.encode_intent(&intent).unwrap(), "policy")
            .unwrap()
            .semantic_revision();
        let mut lock = base.lock().clone();
        lock.intent_revision = revision;
        let report = assess(
            &server,
            ResolvedProject::validate(intent, lock, revision).unwrap(),
        )
        .await;
        assert_eq!(report.release_policy, "stable-only");
        let row = &report.dependencies[0];
        assert_eq!(
            row.release_policy,
            preference.unwrap_or(ReleasePolicy::StableOnly).as_str()
        );
        if preference.is_some() {
            assert_eq!(row.permitted.as_ref().unwrap().selection, "Version2");
        } else {
            assert!(row.permitted.is_none());
            assert!(row.reasons.contains(&UpdateReason::ReleasePolicy));
        }
    }
}

#[tokio::test]
async fn assessment_exposes_retained_conflicts_that_update_preparation_rejects() {
    use empack_core::{
        addition::{AdditionGroup, AdditionPlan},
        model::DependencyKey,
    };
    for reverse in [false, true] {
        let mut server = Server::new_async().await;
        records(
            &mut server,
            "Project1",
            "Version3",
            if reverse {
                json!([])
            } else {
                json!([{"project_id":"Project2", "dependency_type":"incompatible"}])
            },
        )
        .await;
        let base = fixture(
            &[("a", "Project1", "Version1"), ("b", "Project2", "Version2")],
            &["a"],
            &[],
            true,
        );
        let a = DependencyKey::parse("a").unwrap();
        let b = DependencyKey::parse("b").unwrap();
        let selected = fixture(&[("a", "Project1", "Version3")], &["a"], &[], true);
        let mut prior = base.lock().clone();
        let mut next = selected.lock().clone();
        let assertion = if reverse {
            let pin = next.dependencies[&a].selected.as_ref().unwrap();
            ProviderIncompatibility {
                project: Some(pin.project.clone()),
                selection: Some(pin.selection.clone()),
            }
        } else {
            ProviderIncompatibility {
                project: Some(
                    prior.dependencies[&b]
                        .selected
                        .as_ref()
                        .unwrap()
                        .project
                        .clone(),
                ),
                selection: None,
            }
        };
        if reverse {
            prior
                .dependencies
                .get_mut(&b)
                .unwrap()
                .incompatibilities
                .push(assertion);
        } else {
            next.dependencies
                .get_mut(&a)
                .unwrap()
                .incompatibilities
                .push(assertion);
        }
        let current =
            ResolvedProject::validate(base.intent().clone(), prior, base.lock().intent_revision)
                .unwrap();
        let proposed = ResolvedProject::validate(
            selected.intent().clone(),
            next,
            selected.lock().intent_revision,
        )
        .unwrap();
        let report = assess(&server, current.clone()).await;
        assert_eq!(report.joint.state, "validated");
        assert_eq!(report.joint.candidate_compatibility, "not-assessed");
        assert_eq!(report.joint.declared_incompatibilities.len(), 1);
        let pair = &report.joint.declared_incompatibilities[0];
        assert_eq!(
            pair.from.project,
            if reverse { "Project2" } else { "Project1" }
        );
        assert_eq!(
            pair.target.project,
            if reverse { "Project1" } else { "Project2" }
        );
        let json = serde_json::to_value(&report).unwrap();
        assert_eq!(json["joint"]["candidate_compatibility"], "not-assessed");
        let group = AdditionGroup::from_resolved(&proposed).unwrap();
        let error = AdditionPlan::prepare_update(&current, &group).unwrap_err();
        assert!(error.to_string().contains("incompatib"), "{error}");
    }
}

fn project_with_retained_declarations() -> ResolvedProject {
    use empack_core::{
        model::{ContentLayer, DependencyKey},
        requirements::{Requirement, Requirements},
    };
    let base = fixture(
        &[
            ("a", "Project1", "Version1"),
            ("b", "Project2", "Version2"),
            ("c", "Project3", "Version3"),
        ],
        &["a"],
        &[],
        true,
    );
    let mut lock = base.lock().clone();
    let key = |value| DependencyKey::parse(value).unwrap();
    for (from, target, client) in [("b", "c", true), ("c", "b", false)] {
        let target = lock.dependencies[&key(target)]
            .selected
            .as_ref()
            .unwrap()
            .project
            .clone();
        let dependency = lock.dependencies.get_mut(&key(from)).unwrap();
        dependency.incompatibilities.push(ProviderIncompatibility {
            project: Some(target),
            selection: None,
        });
        let mut files = dependency.files.as_slice().to_vec();
        for file in &mut files {
            let mut placements = file.placements.as_slice().to_vec();
            for placement in &mut placements {
                placement.layer = ContentLayer::Common;
                placement.requirements = Requirements {
                    client: if client {
                        Requirement::Required
                    } else {
                        Requirement::Unsupported
                    },
                    server: if client {
                        Requirement::Unsupported
                    } else {
                        Requirement::Required
                    },
                };
            }
            file.placements = NonEmpty::new(placements).unwrap();
        }
        dependency.files = NonEmpty::new(files).unwrap();
    }
    ResolvedProject::validate(base.intent().clone(), lock, base.lock().intent_revision).unwrap()
}

#[tokio::test]
async fn declaration_limit_keeps_completed_rows_and_marks_partial_evidence() {
    let mut server = Server::new_async().await;
    records(&mut server, "Project1", "Version4", json!([])).await;
    let project = project_with_retained_declarations();
    for maximum in [1, 2] {
        let report = assess_with_limits(
            &server,
            project.clone(),
            ClosureLimits {
                edges: maximum,
                ..Default::default()
            },
        )
        .await;
        assert_eq!(report.dependencies.len(), 1);
        assert_eq!(report.dependencies[0].state, "change-available");
        assert_eq!(report.joint.state, "validated");
        assert_eq!(report.joint.declared_incompatibilities.len(), maximum);
        assert_eq!(report.joint.declarations_complete, maximum == 2);
        assert_eq!(
            report.joint.reasons.contains(&UpdateReason::CatalogLimit),
            maximum == 1
        );
        assert_eq!(
            serde_json::to_value(&report).unwrap()["joint"]["declarations_complete"],
            maximum == 2
        );
    }
}

#[tokio::test]
async fn declaration_scan_honors_original_deadline_and_keeps_partial_evidence() {
    let mut server = Server::new_async().await;
    records(&mut server, "Project1", "Version4", json!([])).await;
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
            let project = project_with_retained_declarations();
            let mut pin = project
                .lock()
                .dependencies
                .values()
                .next()
                .unwrap()
                .selected
                .clone()
                .unwrap();
            pin.selection = pin.project.parse_pin("Version4").unwrap();
            let limits = ClosureLimits::default();
            let budget = transport::RequestBudget::new(limits.selection.catalog).unwrap();
            let deadline = budget.deadline();
            let closure = catalog
                .resolve_required_closure_budget(
                    &mut scope,
                    ClosureRequest {
                        roots: NonEmpty::new(vec![ClosureRoot {
                            pin,
                            kind: ContentKind::ResourcePack,
                        }])
                        .unwrap(),
                        game_versions: NonEmpty::new(vec![
                            project.intent().runtime.minecraft.clone(),
                        ])
                        .unwrap(),
                        loader: project.intent().runtime.loader,
                        releases: ReleasePolicy::PreferStable,
                    },
                    limits,
                    budget,
                    None,
                    false,
                )
                .await
                .unwrap();
            let mut saw_partial = false;
            let mut saw_complete = false;
            // Sweep expiry across setup and comparisons without depending on host scheduling.
            for expires_after in 0..32 {
                let mut calls = 0;
                let (declarations, incomplete) =
                    declared_incompatibilities(&scope, &project, &closure, 10, deadline, || {
                        calls += 1;
                        if calls > expires_after {
                            deadline
                        } else {
                            deadline - Duration::from_secs(1)
                        }
                    })
                    .unwrap();
                if let Some(reason) = incomplete {
                    assert_eq!(reason, UpdateReason::Deadline);
                    saw_partial |= declarations.len() == 1;
                    assert!(declarations.len() <= 2);
                } else {
                    saw_complete = true;
                    assert_eq!(declarations.len(), 2);
                }
            }
            assert!(saw_partial && saw_complete);
            Ok(())
        })
        .unwrap();
    let outcome = handle.wait().await;
    runtime.shutdown().await;
    if let OperationOutcome::Failed(error) = &*outcome {
        panic!("declaration scan failed: {error}");
    }
}

#[tokio::test]
async fn malformed_root_does_not_hide_independent_catalog_evidence() {
    let mut server = Server::new_async().await;
    server
        .mock("GET", "/project/Project1")
        .with_body("{}")
        .create_async()
        .await;
    records(&mut server, "Project2", "Version2", json!([])).await;
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
    assert_eq!(
        report.dependencies[0].reasons,
        vec![UpdateReason::InvalidEvidence]
    );
    assert!(report.dependencies[1].catalog_complete);
    assert_eq!(report.dependencies[1].state, "unchanged");
    assert_eq!(report.joint.state, "not-assessed");
}

#[tokio::test]
async fn malformed_root_still_consumes_the_shared_transfer_allowance() {
    let mut server = Server::new_async().await;
    server
        .mock("GET", "/project/Project1")
        .with_body("{}")
        .create_async()
        .await;
    // Exactly the whole allowance: it would parse if the first request's two bytes were lost.
    let second =
        json!({"id":"Project2","slug":"Project2","title":"Project2","project_type":"resourcepack"})
            .to_string();
    server
        .mock("GET", "/project/Project2")
        .with_body(second.clone())
        .create_async()
        .await;
    let skipped = server
        .mock("GET", "/project/Project2/version")
        .expect(0)
        .create_async()
        .await;
    let mut limits = ClosureLimits::default();
    limits.selection.catalog.transfer_bytes = second.len() as u64;
    let report = assess_with_limits(
        &server,
        fixture(
            &[("a", "Project1", "Version1"), ("b", "Project2", "Version2")],
            &["a", "b"],
            &[],
            true,
        ),
        limits,
    )
    .await;
    assert_eq!(
        report.dependencies[0].reasons,
        vec![UpdateReason::InvalidEvidence]
    );
    assert_eq!(
        report.dependencies[1].reasons,
        vec![UpdateReason::CatalogLimit]
    );
    assert!(!report.is_complete());
    assert!(
        report.dependencies[0]
            .diagnostic
            .as_deref()
            .unwrap()
            .contains("Dependency a (Project1)")
    );
    skipped.assert_async().await;
}

#[test]
fn surviving_catalog_budget_keeps_its_original_deadline() {
    let budget = transport::RequestBudget::new(CatalogLimits::default()).unwrap();
    assert_eq!(budget.deadline(), budget.clone().deadline());
}

#[tokio::test]
async fn invalid_provider_identity_and_kind_do_not_stop_independent_roots() {
    for (id, kind) in [("invalid", "resourcepack"), ("Project1", "unsupported")] {
        let mut server = Server::new_async().await;
        server
            .mock("GET", "/project/Project1")
            .with_body(
                json!({"id":id,"slug":"first","title":"First","project_type":kind}).to_string(),
            )
            .create_async()
            .await;
        records(&mut server, "Project2", "Version2", json!([])).await;
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
        assert_eq!(
            report.dependencies[0].reasons,
            [UpdateReason::InvalidEvidence],
            "{id}/{kind}"
        );
        assert!(report.dependencies[1].catalog_complete, "{id}/{kind}");
        assert_eq!(report.dependencies[1].state, "unchanged");
        assert_eq!(report.joint.state, "not-assessed");
        assert!(!report.is_complete());
    }
}
