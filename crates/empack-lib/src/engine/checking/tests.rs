use super::*;
use crate::{
    engine::{
        documents::DocumentCodec,
        resources::ResourceGovernor,
        runtime::{OperationOutcome, OperationRuntime},
    },
    test_support::file_bytes,
};
use empack_core::{
    digest::ExpectedDigest,
    identity::{ModrinthProjectId, ModrinthVersionId, PinSelector, ProviderProjectId},
    path::InstallDestination,
    requirements::{Requirement, Requirements},
};
use mockito::{Mock, Server};
use serde_json::{Value, json};
use sha2::{Digest, Sha512};
use std::{fs, path::Path, time::Duration};
fn path(value: &str) -> PortableRelPath {
    PortableRelPath::parse(value, PathSyntax::ProjectContent).unwrap()
}
fn key(value: &str) -> DependencyKey {
    DependencyKey::parse(value).unwrap()
}
fn pin(project: &str, version: &str) -> ResolvedPin {
    ResolvedPin {
        project: ProviderProjectId::Modrinth(ModrinthProjectId::parse(project).unwrap()),
        selection: PinSelector::ModrinthVersion(ModrinthVersionId::parse(version).unwrap()),
    }
}
fn expected(bytes: &[u8]) -> ExpectedContent {
    ExpectedContent {
        size: Some(bytes.len() as u64),
        digests: Some(
            DigestSet::new(vec![ExpectedDigest::Sha512(Sha512::digest(bytes).into())]).unwrap(),
        ),
        accepted_observation: None,
    }
}
fn base() -> ResolvedProject {
    let project = crate::engine::mrpack::tests::project(false, false);
    let mut intent = project.intent().clone();
    intent.roots.clear();
    intent.runtime.minecraft = GameVersion::parse("1.21.1").unwrap();
    intent.runtime.java_major = 21;
    let revision = DocumentCodec
        .decode_intent(&DocumentCodec.encode_intent(&intent).unwrap(), "test")
        .unwrap()
        .semantic_revision();
    let mut lock = project.lock().clone();
    lock.dependencies.clear();
    lock.required_edges.clear();
    lock.coverage.clear();
    lock.intent_revision = revision;
    lock.runtime.minecraft = intent.runtime.minecraft.clone();
    ResolvedProject::validate(intent, lock, revision).unwrap()
}
fn write(root: &Path, project: &ResolvedProject) {
    fs::write(
        root.join("empack.yml"),
        DocumentCodec.encode_intent(project.intent()).unwrap(),
    )
    .unwrap();
    fs::write(
        root.join("empack.lock"),
        DocumentCodec.encode_lock(project).unwrap(),
    )
    .unwrap();
    fs::write(root.join("prior.zip"), b"preserve prior artifact").unwrap();
}
fn request(root: &Path, scope: CheckScope) -> CheckRequest {
    CheckRequest {
        project: root.to_path_buf(),
        state: root.join("state"),
        scope,
        files: SnapshotLimits {
            entries: 200,
            depth: 16,
            file_bytes: 1 << 20,
            total_bytes: 8 << 20,
        },
        providers: ClosureLimits {
            selection: crate::engine::providers::SelectionLimits {
                catalog: crate::engine::providers::CatalogLimits {
                    response_bytes: 16384,
                    transfer_bytes: 131072,
                    deadline: Duration::from_secs(5),
                },
                ..Default::default()
            },
            projects: 16,
            edges: 32,
        },
    }
}
async fn run(root: &Path, mode: CheckScope, origin: &str) -> ProjectCheck {
    run_request(request(root, mode), origin).await
}
async fn run_request(request: CheckRequest, origin: &str) -> ProjectCheck {
    let catalog = ProviderCatalog::for_loopback_tests(origin, None);
    let runtime = OperationRuntime::new(
        ResourceGovernor::new(ResourceRequest {
            jobs: 1,
            memory_bytes: 256 << 20,
            open_files: 32,
            ..Default::default()
        }),
        1,
    );
    let mut handle = runtime
        .start(
            move |mut scope| async move { Ok(check_project(&mut scope, request, &catalog).await) },
        )
        .unwrap();
    let outcome = handle.wait().await;
    runtime.shutdown().await;
    // ProjectCheck's typed cause is not cloneable; unwrap the uniquely held completed result.
    drop(handle);
    drop(runtime);
    match std::sync::Arc::try_unwrap(outcome)
        .ok()
        .expect("terminal outcome only")
    {
        OperationOutcome::Completed(result) => result.unwrap(),
        _ => panic!("check runtime did not complete"),
    }
}
fn provider_project(coverage: Coverage) -> ResolvedProject {
    let current = base();
    let mut intent = current.intent().clone();
    let mut lock = current.lock().clone();
    for (name, project, version) in [
        ("menu", "Root0001", "RootVer1"),
        ("library", "Lib00001", "LibVer01"),
    ] {
        let selected = pin(project, version);
        let id = selected.project.clone();
        let placement = Placement {
            layer: ContentLayer::Common,
            destination: InstallDestination::parse(&format!("mods/{project}.jar")).unwrap(),
            requirements: Requirements {
                client: Requirement::Required,
                server: Requirement::Required,
            },
        };
        if name == "menu" {
            intent.roots.insert(
                key(name),
                DependencyIntent {
                    source: SourceIntent::Provider(id.clone()),
                    selection: DependencySelection::Automatic,
                    kind: ContentKind::Mod,
                    requirements: placement.requirements.clone(),
                    placement: PlacementIntent::Automatic,
                    releases: None,
                },
            );
        }
        lock.dependencies.insert(
            key(name),
            LockedDependency {
                source_selector: None,
                title: name.into(),
                identity: ResolvedIdentity::Provider(id),
                selected: Some(selected.clone()),
                kind: ContentKind::Mod,
                incompatibilities: vec![],
                files: NonEmpty::new(vec![ResolvedFile {
                    slot: FileSlot::parse("primary").unwrap(),
                    acquisition: AcquisitionSpec::Provider {
                        pin: selected,
                        slot: FileSlot::parse(&format!("{project}.jar")).unwrap(),
                        alternatives: vec![],
                    },
                    expected: expected(b"x"),
                    provenance: Provenance {
                        source: "fixture".into(),
                        location: None,
                        declared_digests: None,
                        conversions: vec![],
                    },
                    placements: NonEmpty::new(vec![placement]).unwrap(),
                }])
                .unwrap(),
            },
        );
        lock.coverage.insert(key(name), coverage);
    }
    lock.required_edges.insert(
        key("menu"),
        std::collections::BTreeSet::from([key("library")]),
    );
    crate::engine::mrpack::tests::explicitly_placed(intent, lock)
}
async fn records(server: &mut Server, root_deps: Value, bytes: &[u8]) -> Vec<Mock> {
    let mut result = vec![];
    for (project, version, deps) in [
        ("Root0001", "RootVer1", root_deps),
        ("Lib00001", "LibVer01", json!([])),
    ] {
        result.push(server.mock("GET",format!("/project/{project}").as_str()).with_body(json!({"id":project,"slug":project,"title":project,"project_type":"mod","loaders":["fabric"]}).to_string()).expect(1).create_async().await);
        result.push(server.mock("GET",format!("/version/{version}").as_str()).with_body(json!({"id":version,"project_id":project,"files":[{"filename":format!("{project}.jar"),"primary":true,"size":bytes.len(),"hashes":{"sha512":ExpectedDigest::Sha512(Sha512::digest(bytes).into()).hex()},"url":format!("https://example.invalid/{project}.jar")}],"dependencies":deps,"game_versions":["1.21.1"],"loaders":["fabric"],"status":"listed","version_type":"release","date_published":"2026-01-01T00:00:00Z"}).to_string()).expect(1).create_async().await);
        result.push(
            server
                .mock("GET", format!("/project/{project}/version").as_str())
                .match_query(mockito::Matcher::Any)
                .expect(0)
                .create_async()
                .await,
        );
    }
    result
}
#[tokio::test]
async fn documents_are_diagnosed_without_content_or_mutation_authority() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), &base());
    fs::remove_file(root.path().join("empack.lock")).unwrap();
    let before = file_bytes(root.path());
    let report = run(root.path(), CheckScope::Documents, "http://127.0.0.1:9").await;
    assert!(report.result.is_ok(), "{:?}", report.result);
    assert_eq!(report.report.lock, "missing");
    assert!(report.report.complete);
    let report = run(root.path(), CheckScope::Recorded, "http://127.0.0.1:9").await;
    assert!(format!("{:#}", report.result.unwrap_err()).contains("empack sync"));
    assert_eq!(file_bytes(root.path()), before);
    fs::write(root.path().join("empack.yml"), b"schema: [broken").unwrap();
    fs::write(root.path().join("empack.lock"), b"invalid lock").unwrap();
    let before = file_bytes(root.path());
    let report = run(root.path(), CheckScope::Documents, "http://127.0.0.1:9").await;
    assert!(!report.report.complete);
    assert_eq!(report.report.lock, "invalid");
    assert_eq!(
        report
            .report
            .findings
            .iter()
            .filter(|f| f.state == CheckState::Failed)
            .count(),
        2
    );
    assert_eq!(file_bytes(root.path()), before);
}
#[tokio::test]
async fn stale_lock_is_a_failed_check_without_rewriting_it() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), &base());
    let bytes = fs::read(root.path().join("empack.yml")).unwrap();
    let mut intent = DocumentCodec
        .decode_intent(&bytes, "test")
        .unwrap()
        .intent()
        .clone();
    intent.runtime.minecraft = GameVersion::parse("1.21.3").unwrap();
    fs::write(
        root.path().join("empack.yml"),
        DocumentCodec.encode_intent(&intent).unwrap(),
    )
    .unwrap();
    let before = file_bytes(root.path());
    let report = run(root.path(), CheckScope::Documents, "http://127.0.0.1:9").await;
    assert!(report.result.is_err());
    assert_eq!(report.report.lock, "stale");
    assert_eq!(file_bytes(root.path()), before);
}
#[tokio::test]
async fn online_checks_all_exact_selections_without_catalog_discovery_or_materialization() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), &provider_project(Coverage::Partial));
    let before = file_bytes(root.path());
    assert!(
        run(root.path(), CheckScope::Recorded, "http://127.0.0.1:9")
            .await
            .result
            .is_err()
    );
    let mut server = Server::new_async().await;
    let mocks = records(
        &mut server,
        json!([{"project_id":"Lib00001","dependency_type":"required"}]),
        b"x",
    )
    .await;
    let checked = run(root.path(), CheckScope::Online, &server.url()).await;
    assert!(checked.result.is_ok(), "{:?}", checked.result);
    assert!(checked.report.complete);
    assert_eq!(checked.report.references, 2);
    for mock in mocks {
        mock.assert_async().await;
    }
    assert_eq!(file_bytes(root.path()), before);
}
#[tokio::test]
async fn fresh_missing_or_conflicting_requirements_fail_without_selecting_replacements() {
    for deps in [
        json!([{"project_id":"Absent01","dependency_type":"required"}]),
        json!([{"project_id":"Lib00001","version_id":"WrongVer","dependency_type":"required"}]),
        json!([{"project_id":"Lib00001","dependency_type":"incompatible"}]),
    ] {
        let root = tempfile::tempdir().unwrap();
        write(
            root.path(),
            &provider_project(Coverage::CompleteForSelection),
        );
        let before = file_bytes(root.path());
        let mut server = Server::new_async().await;
        let mocks = records(&mut server, deps, b"x").await;
        let no_replacement = server
            .mock("GET", "/version/WrongVer")
            .expect(0)
            .create_async()
            .await;
        let missing = server
            .mock("GET", "/project/Absent01")
            .expect(0)
            .create_async()
            .await;
        let checked = run(root.path(), CheckScope::Online, &server.url()).await;
        assert!(checked.result.is_err());
        assert!(!checked.report.complete);
        assert_eq!(file_bytes(root.path()), before);
        missing.assert_async().await;
        no_replacement.assert_async().await;
        for mock in mocks {
            mock.assert_async().await;
        }
    }
}
#[tokio::test]
async fn fresh_file_changes_and_transport_failure_retain_document_findings() {
    let root = tempfile::tempdir().unwrap();
    write(
        root.path(),
        &provider_project(Coverage::CompleteForSelection),
    );
    let before = file_bytes(root.path());
    let mut server = Server::new_async().await;
    let mocks = records(&mut server, json!([]), b"changed").await;
    let checked = run(root.path(), CheckScope::Online, &server.url()).await;
    assert!(checked.result.is_err());
    assert!(
        checked
            .report
            .findings
            .iter()
            .any(|f| f.state == CheckState::Failed && f.subject.contains("menu"))
    );
    for mock in mocks {
        mock.assert_async().await;
    }
    let checked = run(root.path(), CheckScope::Online, "http://127.0.0.1:9").await;
    assert!(checked.result.is_err());
    assert!(
        checked
            .report
            .findings
            .iter()
            .any(|f| f.subject == "Authoring" && f.state == CheckState::Passed)
    );
    assert_eq!(file_bytes(root.path()), before);
}
#[tokio::test]
async fn changed_documents_after_provider_work_are_not_a_successful_check() {
    let root = tempfile::tempdir().unwrap();
    write(
        root.path(),
        &provider_project(Coverage::CompleteForSelection),
    );
    let mut server = Server::new_async().await;
    let changed = root.path().join("empack.yml");
    let original = fs::read(&changed).unwrap();
    // Changing a comment preserves semantics but must invalidate this check's captured input.
    let mut changed_bytes = b"# external edit\n".to_vec();
    changed_bytes.extend(original);
    let replacement=server.mock("GET","/project/Root0001").with_body_from_request(move |_| {
        fs::write(&changed,&changed_bytes).unwrap();json!({"id":"Root0001","slug":"Root0001","title":"Root","project_type":"mod","loaders":["fabric"]}).to_string().into_bytes()
    }).expect(1).create_async().await;
    // Register the other records; root project has already been supplied by the edit hook.
    let mut mocks = records(&mut server, json!([]), b"x").await;
    mocks.remove(0).remove_async().await;
    let checked = run(root.path(), CheckScope::Online, &server.url()).await;
    assert!(checked.result.is_err());
    assert!(
        checked
            .report
            .findings
            .iter()
            .any(|f| f.subject == "Final input check")
    );
    replacement.assert_async().await;
}
#[tokio::test]
async fn embedded_and_local_assertions_are_checked_without_copying_or_repairing_sources() {
    use std::io::{Cursor, Write};
    let root = tempfile::tempdir().unwrap();
    let base = base();
    let mut intent = base.intent().clone();
    let mut lock = base.lock().clone();
    fs::create_dir(root.path().join("vendor")).unwrap();
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file("config/rules.txt", zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.write_all(b"rules").unwrap();
    fs::write(
        root.path().join("vendor/source.zip"),
        zip.finish().unwrap().into_inner(),
    )
    .unwrap();
    fs::write(root.path().join("vendor/settings.txt"), b"settings").unwrap();
    for (name, source, bytes) in [
        (
            "rules",
            AcquisitionSpec::Embedded {
                archive: path("vendor/source.zip"),
                member: path("config/rules.txt"),
            },
            &b"rules"[..],
        ),
        (
            "settings",
            AcquisitionSpec::Local(path("vendor/settings.txt")),
            &b"settings"[..],
        ),
    ] {
        let source_intent = match &source {
            AcquisitionSpec::Local(p) => Some(SourceIntent::Local(p.clone())),
            AcquisitionSpec::Embedded { .. } => None,
            _ => unreachable!(),
        };
        let placements = NonEmpty::new(vec![Placement {
            layer: ContentLayer::Common,
            destination: InstallDestination::parse(&format!("config/{name}.txt")).unwrap(),
            requirements: Requirements {
                client: Requirement::Required,
                server: Requirement::Required,
            },
        }])
        .unwrap();
        if let Some(source_intent) = source_intent {
            intent.roots.insert(
                key(name),
                DependencyIntent {
                    source: source_intent,
                    selection: DependencySelection::Automatic,
                    kind: ContentKind::Config,
                    requirements: placements.as_slice()[0].requirements.clone(),
                    placement: PlacementIntent::Automatic,
                    releases: None,
                },
            );
        }
        lock.dependencies.insert(
            key(name),
            LockedDependency {
                source_selector: None,
                title: name.into(),
                identity: ResolvedIdentity::Local(key(name)),
                selected: None,
                kind: ContentKind::Config,
                incompatibilities: vec![],
                files: NonEmpty::new(vec![ResolvedFile {
                    slot: FileSlot::parse("primary").unwrap(),
                    acquisition: source,
                    expected: expected(bytes),
                    placements,
                    provenance: Provenance {
                        source: "test".into(),
                        location: None,
                        declared_digests: None,
                        conversions: vec![],
                    },
                }])
                .unwrap(),
            },
        );
        lock.coverage.insert(key(name), Coverage::Unknown);
    }
    let project = crate::engine::mrpack::tests::explicitly_placed(intent, lock);
    write(root.path(), &project);
    let before = file_bytes(root.path());
    let checked = run(root.path(), CheckScope::Recorded, "http://127.0.0.1:9").await;
    assert!(checked.result.is_ok(), "{:?}", checked.result);
    assert_eq!(checked.report.verified_files, 2);
    assert_eq!(checked.report.references, 2);
    assert_eq!(file_bytes(root.path()), before);
    fs::write(root.path().join("vendor/settings.txt"), b"bad bytes").unwrap();
    let before = file_bytes(root.path());
    let checked = run(root.path(), CheckScope::Recorded, "http://127.0.0.1:9").await;
    assert!(checked.result.is_err());
    assert_eq!(checked.report.verified_files, 1);
    assert_eq!(file_bytes(root.path()), before);
}

#[test]
fn fresh_digest_comparison_requires_shared_evidence_or_verified_local_bytes() {
    let locked = ExpectedContent {
        digests: Some(DigestSet::new(vec![ExpectedDigest::Sha1([1; 20])]).unwrap()),
        size: Some(1),
        accepted_observation: None,
    };
    let fresh = ExpectedContent {
        digests: Some(DigestSet::new(vec![ExpectedDigest::Sha512([2; 64])]).unwrap()),
        size: Some(1),
        accepted_observation: None,
    };
    assert!(online::compare_assertions(&locked, &fresh, None).is_err());
    assert!(
        online::compare_assertions(
            &locked,
            &fresh,
            Some(&CheckedContent {
                digests: fresh.digests.clone().unwrap(),
                bytes: 1
            })
        )
        .is_ok()
    );
    let conflicting = ExpectedContent {
        digests: Some(
            DigestSet::new(vec![
                ExpectedDigest::Sha1([9; 20]),
                ExpectedDigest::Sha512([2; 64]),
            ])
            .unwrap(),
        ),
        ..fresh.clone()
    };
    assert!(online::compare_assertions(&locked, &conflicting, None).is_err());
}

#[tokio::test]
async fn exact_checks_share_one_transfer_allowance_across_retained_selections() {
    let root = tempfile::tempdir().unwrap();
    write(
        root.path(),
        &provider_project(Coverage::CompleteForSelection),
    );
    let before = file_bytes(root.path());
    let mut server = Server::new_async().await;
    let mocks = records(&mut server, json!([]), b"x").await;
    let mut request = request(root.path(), CheckScope::Online);
    request.providers.selection.catalog.transfer_bytes = 900;
    let checked = run_request(request, &server.url()).await;
    let error = checked.result.unwrap_err();
    assert!(
        matches!(
            error.downcast_ref::<crate::engine::providers::CatalogError>(),
            Some(crate::engine::providers::CatalogError::Limit)
        ),
        "{error:#}"
    );
    assert!(mocks[0].matched_async().await || mocks[3].matched_async().await);
    assert!(!checked.report.complete);
    assert_eq!(file_bytes(root.path()), before);
}
