use super::*;
use crate::engine::{
    addition::tests::fixture,
    documents::DocumentCodec,
    resources::ResourceGovernor,
    runtime::{OperationOutcome, OperationRuntime},
};
use empack_core::model::DependencyKey;
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
    assess_request(server, project, AssessmentRequest::default(), limits)
        .await
        .0
        .unwrap()
}
async fn assess_request(
    server: &Server,
    project: ResolvedProject,
    request: AssessmentRequest,
    limits: ClosureLimits,
) -> (Result<UpdateAssessment>, Vec<AssessmentProgress>) {
    let progress = Arc::new(std::sync::Mutex::new(Vec::new()));
    let observed = progress.clone();
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
                .assess_updates(&mut scope, &project, request, limits, |event| {
                    observed.lock().unwrap().push(event)
                })
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
    let progress = Arc::try_unwrap(progress)
        .ok()
        .unwrap()
        .into_inner()
        .unwrap();
    (report, progress)
}
// Independent wire expectations: neither a production report writer nor a
// candidate constructor supplies these fields.
fn assert_selection_report(report: &UpdateAssessment, pinned: bool) {
    let mut actual = serde_json::to_value(report).unwrap();
    assert!(!actual["observed_at"].as_str().unwrap().is_empty());
    actual.as_object_mut().unwrap().remove("observed_at");
    let retained = json!({"provider":"modrinth", "project":"Project1", "selection":"Version1",
        "version":null, "published_at":null, "channel":null});
    let observed = json!({"provider":"modrinth", "project":"Project1", "selection":"Version2",
        "version":"Version2", "published_at":"2026-01-01T00:00:00Z", "channel":"release"});
    let selection = if pinned { "Version1" } else { "Version2" };
    assert_eq!(
        actual,
        json!({
            "schema":2, "catalog_validation":"runtime-compatible-payloads", "release_policy":"prefer-stable", "complete":true,
            "scope":{"kind":"all-author-roots","roots":["root"],"total_roots":1,"retained_inventory":0},
            "summary":{"change_available":usize::from(!pinned),"held":usize::from(pinned),"unknown":0,"manual":0,"unavailable":0,"conflicted":0},
            "dependencies":[{"release_policy":"prefer-stable", "release_policy_source":"project-default", "compatible_matches_release_policy":true, "key":"root", "name":"Title for Project1",
                "current":retained, "permitted":if pinned {retained.clone()} else {observed.clone()},
                "compatible":observed, "upstream":observed, "state":if pinned {"pinned"} else {"change-available"},
                "catalog_complete":true, "observed_records":1, "diagnostic":null,
                "reasons":if pinned {vec!["pinned"]} else {vec![]}}],
            "joint":{"scope":"provider-dependency-closure", "candidate_compatibility":"not-assessed",
                "declared_incompatibilities":[], "declarations_complete":true, "state":"validated",
                "selections":[{"provider":"modrinth", "project":"Project1", "selection":selection,
                    "version":null, "published_at":null, "channel":null}], "reasons":[], "blocking_roots":[]}
        })
    );
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
    assert_selection_report(&report, false);
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
    assert_selection_report(&report, true);
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
        assert_eq!(
            row.release_policy_source,
            if preference.is_some() {
                "dependency"
            } else {
                "project-default"
            }
        );
        assert_eq!(
            row.compatible_matches_release_policy,
            Some(preference.is_some())
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

#[tokio::test]
async fn empty_modrinth_files_are_local_evidence_failures_with_shared_limits() {
    for exhausted in [false, true] {
        let mut server = Server::new_async().await;
        let first = json!({"id":"Project1","slug":"Project1","title":"Project1","project_type":"resourcepack"}).to_string();
        server
            .mock("GET", "/project/Project1")
            .with_body(first.clone())
            .create_async()
            .await;
        let mut malformed = version("Project1", "Version1", "1.20.1", json!([]));
        malformed["files"] = json!([]);
        let page = json!([malformed]).to_string();
        server
            .mock("GET", "/project/Project1/version")
            .match_query(Matcher::Any)
            .with_body(page.clone())
            .create_async()
            .await;
        records(&mut server, "Project2", "Version2", json!([])).await;
        let mut limits = ClosureLimits::default();
        if exhausted {
            let second = json!({"id":"Project2","slug":"Project2","title":"Project2","project_type":"resourcepack"}).to_string();
            limits.selection.catalog.transfer_bytes =
                (first.len() + page.len() + second.len() - 1) as u64;
        }
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
            [UpdateReason::InvalidEvidence]
        );
        if exhausted {
            assert_eq!(report.dependencies[1].reasons, [UpdateReason::CatalogLimit]);
        } else {
            assert!(report.dependencies[1].catalog_complete);
            assert_eq!(report.dependencies[1].state, "unchanged");
        }
        assert!(!report.is_complete());
        assert_eq!(report.joint.state, "not-assessed");
    }
}

#[tokio::test]
async fn provider_access_and_service_failures_still_stop_later_roots() {
    for (status, reason) in [
        (401, UpdateReason::Unauthorized),
        (500, UpdateReason::ProviderUnavailable),
    ] {
        let mut server = Server::new_async().await;
        server
            .mock("GET", "/project/Project1")
            .with_status(status)
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
        assert_eq!(report.dependencies[0].reasons, [reason]);
        assert_eq!(
            report.dependencies[1].reasons,
            [UpdateReason::NotAssessedAfterFailure]
        );
        assert!(!report.is_complete());
        skipped.assert_async().await;
    }
}

#[tokio::test]
async fn selected_assessment_retains_context_and_limits_catalog_work() {
    let mut server = Server::new_async().await;
    records(&mut server, "Project1", "Version3", json!([])).await;
    let untouched = server
        .mock("GET", "/project/Project2")
        .expect(0)
        .create_async()
        .await;
    let project = fixture(
        &[("a", "Project1", "Version1"), ("b", "Project2", "Version2")],
        &["a", "b"],
        &[],
        true,
    );
    let (report, progress) = assess_request(
        &server,
        project.clone(),
        AssessmentRequest {
            keys: vec![DependencyKey::parse("a").unwrap()],
            releases: None,
            exact: Default::default(),
        },
        ClosureLimits::default(),
    )
    .await;
    let report = report.unwrap();
    assert_eq!(report.scope.kind, "selected-author-roots");
    assert_eq!(report.scope.roots, ["a"]);
    assert_eq!(report.scope.total_roots, 2);
    assert_eq!(report.scope.retained_inventory, 1);
    assert_eq!(report.dependencies.len(), 1);
    assert_eq!(report.summary().change_available, 1);
    assert!(report.is_complete());
    assert!(progress.iter().any(|event| matches!(event, AssessmentProgress::Catalog { key, total: 1, records: 1, .. } if key == "a")));
    let closure_start = progress
        .iter()
        .position(|event| matches!(event, AssessmentProgress::RequiredClosure { roots: 1 }))
        .unwrap();
    assert!(progress[closure_start + 1..].iter().any(|event| matches!(event, AssessmentProgress::RequiredDependency { project } if project == "a")));
    assert!(matches!(
        progress.last(),
        Some(AssessmentProgress::DeclaredConflicts)
    ));
    untouched.assert_async().await;
    for (keys, missing) in [(vec!["missing"], true), (vec!["a", "a"], false)] {
        let (report, progress) = assess_request(
            &server,
            project.clone(),
            AssessmentRequest {
                keys: keys
                    .iter()
                    .map(|key| DependencyKey::parse(key).unwrap())
                    .collect(),
                releases: None,
                exact: Default::default(),
            },
            ClosureLimits::default(),
        )
        .await;
        let error = report.unwrap_err();
        assert!(
            progress.is_empty(),
            "invalid selection started catalog work"
        );
        match error.downcast_ref::<AssessmentInputError>().unwrap() {
            AssessmentInputError::Missing(key) => {
                assert!(missing);
                assert_eq!(key.as_str(), "missing");
            }
            AssessmentInputError::Duplicate(key) => {
                assert!(!missing);
                assert_eq!(key.as_str(), "a");
            }
        }
    }
}

#[tokio::test]
async fn noncandidate_payload_limits_do_not_poison_current_catalog_metadata() {
    for problem in ["empty", "duplicate", "portable"] {
        let mut server = Server::new_async().await;
        let current = version("Project1", "Version2", "1.20.1", json!([]));
        let mut old = version("Project1", "OldVer01", "1.16.5", json!([]));
        old["date_published"] = json!("2024-01-01T00:00:00Z");
        match problem {
            "empty" => old["files"] = json!([]),
            "duplicate" => {
                let mut duplicate = old["files"][0].clone();
                duplicate["primary"] = json!(false);
                old["files"].as_array_mut().unwrap().push(duplicate);
            }
            "portable" => old["files"][0]["filename"] = json!("legacy|assets.zip"),
            _ => unreachable!(),
        }
        server
            .mock("GET", "/project/Project1")
            .with_body(
                json!({"id":"Project1","slug":"one","title":"One","project_type":"resourcepack"})
                    .to_string(),
            )
            .create_async()
            .await;
        let page = server
            .mock("GET", "/project/Project1/version")
            .match_query(Matcher::Any)
            .with_body(json!([old.clone(), current.clone()]).to_string())
            .create_async()
            .await;
        server
            .mock("GET", "/version/Version2")
            .with_body(current.to_string())
            .create_async()
            .await;
        let project = fixture(&[("a", "Project1", "Version1")], &["a"], &[], true);
        let report = assess(&server, project.clone()).await;
        assert!(
            report.is_complete(),
            "{problem}: {:?}",
            report.dependencies[0].diagnostic
        );
        assert_eq!(
            report.dependencies[0]
                .compatible
                .as_ref()
                .unwrap()
                .selection,
            "Version2"
        );
        assert_eq!(report.dependencies[0].observed_records, Some(2));
        // The very same historical payload remains ineligible for exact acquisition.
        let pin = ResolvedPin {
            project: project.lock().dependencies[&DependencyKey::parse("a").unwrap()]
                .selected
                .as_ref()
                .unwrap()
                .project
                .clone(),
            selection: empack_core::identity::PinSelector::ModrinthVersion(
                empack_core::identity::ModrinthVersionId::parse("OldVer01").unwrap(),
            ),
        };
        let canonical = modrinth::project(
            &serde_json::to_vec(
                &json!({"id":"Project1","slug":"one","title":"One","project_type":"resourcepack"}),
            )
            .unwrap(),
        )
        .unwrap();
        let error = modrinth::selection(canonical, &pin, &serde_json::to_vec(&old).unwrap())
            .err()
            .unwrap();
        assert!(error.is::<ProviderRecordProblem>());
        // If its runtime metadata becomes eligible, its invalid payload must stop qualification.
        old["game_versions"] = json!(["1.20.1"]);
        page.remove_async().await;
        server
            .mock("GET", "/project/Project1/version")
            .match_query(Matcher::Any)
            .with_body(json!([old, current]).to_string())
            .create_async()
            .await;
        let report = assess(&server, project).await;
        assert!(!report.is_complete());
        assert_eq!(
            report.dependencies[0].reasons,
            [UpdateReason::InvalidEvidence]
        );
        assert!(
            report.dependencies[0]
                .diagnostic
                .as_ref()
                .unwrap()
                .contains("files")
        );
    }
}

async fn analyzed_update(
    server: &Server,
    project: ResolvedProject,
    keys: &[&str],
) -> ProviderUpdateAnalysis {
    let inputs = NonEmpty::new(
        keys.iter()
            .map(|key| {
                ProviderAddInput::for_update(&project, &DependencyKey::parse(key).unwrap()).unwrap()
            })
            .collect(),
    )
    .unwrap();
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
                .analyze_provider_update(
                    &mut scope,
                    &project,
                    inputs,
                    None,
                    ClosureLimits::default(),
                    |_| {},
                )
                .await)
        })
        .unwrap();
    let outcome = handle.wait().await;
    runtime.shutdown().await;
    runtime.release_completed(handle.id());
    drop(handle);
    let OperationOutcome::Completed(result) =
        Arc::try_unwrap(outcome).ok().expect("retired analysis")
    else {
        panic!("analysis worker failed")
    };
    result.unwrap()
}

#[tokio::test]
async fn analyzed_update_reuses_observed_closure_for_retained_candidate_validation() {
    use empack_core::{addition::AdditionPlan, model::PlacementIntent};
    let mut server = Server::new_async().await;
    let project_record =
        json!({"id":"Project1","slug":"example","title":"Example","project_type":"resourcepack"});
    let project_requests = server
        .mock("GET", "/project/Project1")
        .with_body(project_record.to_string())
        .expect(2)
        .create_async()
        .await;
    let record = version("Project1", "Version2", "1.20.1", json!([]));
    let listed = server
        .mock("GET", "/project/Project1/version")
        .match_query(Matcher::Any)
        .with_body(json!([record]).to_string())
        .expect(1)
        .create_async()
        .await;
    let exact = server
        .mock("GET", "/version/Version2")
        .with_body(record.to_string())
        .expect(1)
        .create_async()
        .await;
    let retained_requests = server
        .mock("GET", "/project/Project2")
        .expect(0)
        .create_async()
        .await;
    let base = fixture(
        &[
            ("root", "Project1", "Version1"),
            ("retained", "Project2", "Version1"),
        ],
        &["root"],
        &[],
        true,
    );
    let root = DependencyKey::parse("root").unwrap();
    let retained = DependencyKey::parse("retained").unwrap();
    let mut intent = base.intent().clone();
    intent.roots.get_mut(&root).unwrap().placement = PlacementIntent::Explicit(
        base.lock().dependencies[&root].files.as_slice()[0]
            .placements
            .clone(),
    );
    let revision = DocumentCodec
        .decode_intent(&DocumentCodec.encode_intent(&intent).unwrap(), "author")
        .unwrap()
        .semantic_revision();
    let mut lock = base.lock().clone();
    lock.intent_revision = revision;
    let selected = lock.dependencies.get_mut(&root).unwrap();
    selected.files = NonEmpty::new(vec![selected.files.as_slice()[0].clone()]).unwrap();
    lock.dependencies
        .get_mut(&retained)
        .unwrap()
        .incompatibilities
        .push(ProviderIncompatibility {
            project: Some(
                base.lock().dependencies[&root]
                    .selected
                    .as_ref()
                    .unwrap()
                    .project
                    .clone(),
            ),
            selection: Some(
                base.lock().dependencies[&root]
                    .selected
                    .as_ref()
                    .unwrap()
                    .project
                    .parse_pin("Version2")
                    .unwrap(),
            ),
        });
    let current = ResolvedProject::validate(intent, lock, revision).unwrap();
    let before = DocumentCodec.encode_lock(&current).unwrap();
    let analysis = analyzed_update(&server, current.clone(), &["root"]).await;
    assert!(analysis.report.is_complete());
    assert_eq!(analysis.report.summary().change_available, 1);
    assert_eq!(analysis.report.joint.declared_incompatibilities.len(), 1);
    let ProviderAdditionOutcome::Ready(addition) = analysis.proposal.unwrap() else {
        panic!("exact provider group should normalize")
    };
    assert_eq!(addition.evidence().selections.len(), 1);
    assert_eq!(
        ObservedVersion::from_pin(
            addition.project().lock().dependencies[&root]
                .selected
                .as_ref()
                .unwrap()
        )
        .selection,
        "Version2"
    );
    let error = AdditionPlan::prepare_update(&current, addition.group()).unwrap_err();
    assert!(error.to_string().contains("incompatib"), "{error}");
    assert_eq!(DocumentCodec.encode_lock(&current).unwrap(), before);
    project_requests.assert_async().await;
    listed.assert_async().await;
    exact.assert_async().await;
    retained_requests.assert_async().await;
}

#[tokio::test]
async fn analyzed_update_keeps_independent_findings_and_original_provider_error() {
    let mut server = Server::new_async().await;
    let current = fixture(
        &[("a", "Project1", "Version1"), ("b", "Project2", "Version1")],
        &["a", "b"],
        &[],
        true,
    );
    for id in ["Project1", "Project2"] {
        server
            .mock("GET", format!("/project/{id}").as_str())
            .with_body(
                json!({"id":id,"slug":id,"title":id,"project_type":"resourcepack"}).to_string(),
            )
            .expect(1)
            .create_async()
            .await;
    }
    let mut invalid = version("Project1", "Version2", "1.20.1", json!([]));
    invalid["files"] = json!([]);
    server
        .mock("GET", "/project/Project1/version")
        .match_query(Matcher::Any)
        .with_body(json!([invalid]).to_string())
        .expect(1)
        .create_async()
        .await;
    let independent = server
        .mock("GET", "/project/Project2/version")
        .match_query(Matcher::Any)
        .with_body(json!([version("Project2", "Version3", "1.20.1", json!([]))]).to_string())
        .expect(1)
        .create_async()
        .await;
    let no_candidate_fetch = server
        .mock("GET", "/version/Version3")
        .expect(0)
        .create_async()
        .await;
    let analysis = analyzed_update(&server, current, &["a", "b"]).await;
    assert!(!analysis.report.is_complete());
    assert_eq!(
        analysis.report.dependencies[0].reasons,
        [UpdateReason::InvalidEvidence]
    );
    assert_eq!(analysis.report.dependencies[1].state, "change-available");
    assert_eq!(
        analysis.report.dependencies[1]
            .permitted
            .as_ref()
            .unwrap()
            .selection,
        "Version3"
    );
    let error = analysis
        .proposal
        .err()
        .expect("incomplete discovery cannot supply a complete group");
    assert!(
        matches!(
            error.downcast_ref::<CatalogError>(),
            Some(CatalogError::InvalidRecord)
        ),
        "{error:#}"
    );
    assert!(error.downcast_ref::<ProviderRecordProblem>().is_some());
    independent.assert_async().await;
    no_candidate_fetch.assert_async().await;
}
