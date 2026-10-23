use super::*;
use crate::application::{
    cli::CliReleasePolicy,
    engine_host::cli::tests::{fixture, project, session},
};
use serde_json::json;
use std::fs;
fn args(plan: bool) -> MigrationArgs {
    MigrationArgs {
        minecraft: "1.21.2".into(),
        loader: "fabric".into(),
        loader_version: Some("0.16.14".into()),
        acceptable_versions: vec![],
        replacements: vec![],
        plan,
        json: false,
        materialize: false,
        release_policy: Some(CliReleasePolicy::PreferStable),
    }
}
fn services(server: &mockito::Server) -> dependencies::AdditionServices {
    dependencies::AdditionServices {
        catalog: ProviderCatalog::for_loopback_tests(&server.url(), None),
        transport: HttpAcquisition::for_loopback_tests(),
        files: DirectFileLimits::default(),
    }
}
async fn runtime(server: &mut mockito::Server) {
    server.mock("GET","/games").with_body(json!({"latest":{"release":"1.21.2"},"versions":[{"id":"1.21.2","type":"release"},{"id":"1.21.1","type":"release"}]}).to_string()).create_async().await;
    server.mock("GET","/fabric/1.21.2").with_body(json!([{"loader":{"version":"0.16.14","stable":true},"intermediary":{"version":"1.21.2"}}]).to_string()).create_async().await;
}
async fn execute(
    root: &Path,
    server: &mockito::Server,
    args: MigrationArgs,
    dry: bool,
) -> Result<()> {
    synchronization::synchronize_mode(
        &session(root, dry),
        false,
        services(server),
        RuntimeCatalog::for_loopback_tests(&server.url()),
        Some(MigrationOptions::parse(args)?),
    )
    .await
}
#[tokio::test]
async fn runtime_migration_plan_preview_and_application_share_candidate_without_early_edits() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path()).await;
    let mut server = mockito::Server::new_async().await;
    runtime(&mut server).await;
    let before = crate::application::engine_host::tests::snapshot(root.path());
    execute(root.path(), &server, args(true), false)
        .await
        .unwrap();
    assert_eq!(
        crate::application::engine_host::tests::snapshot(root.path()),
        before
    );
    execute(root.path(), &server, args(false), true)
        .await
        .unwrap();
    assert_eq!(
        crate::application::engine_host::tests::snapshot(root.path()),
        before
    );
    execute(root.path(), &server, args(false), false)
        .await
        .unwrap();
    let next = project(root.path());
    assert_eq!(next.intent().runtime.minecraft.as_str(), "1.21.2");
    assert_eq!(next.lock().runtime.minecraft.as_str(), "1.21.2");
    assert!(!root.path().join("project/empack.bindings.yml").exists());
    let completed = crate::application::engine_host::tests::snapshot(&root.path().join("project"));
    synchronization::synchronize(&session(root.path(), false), false)
        .await
        .unwrap();
    assert_eq!(
        crate::application::engine_host::tests::snapshot(&root.path().join("project")),
        completed
    );
}
async fn provider(server: &mut mockito::Server, id: &str, version: &str) {
    server.mock("GET",format!("/project/{id}").as_str()).with_body(json!({"id":id,"slug":"renderer","title":"Renderer","project_type":"mod","loaders":["fabric"]}).to_string()).create_async().await;
    let value = json!({"id":version,"project_id":id,"game_versions":["1.21.1","1.21.2"],"loaders":["fabric"],"files":[{"filename":"renderer.jar","primary":true,"size":7,"hashes":{"sha512":"11".repeat(64)},"url":"https://example.invalid/renderer.jar"}],"dependencies":[],"date_published":"2026-01-01T00:00:00Z","status":"listed","version_type":"release"});
    server
        .mock("GET", format!("/version/{version}").as_str())
        .with_body(value.to_string())
        .create_async()
        .await;
    server
        .mock("GET", format!("/project/{id}/version").as_str())
        .match_query(mockito::Matcher::Any)
        .with_body(json!([value]).to_string())
        .create_async()
        .await;
}
#[tokio::test]
async fn migration_retains_valid_follow_compatible_pin_and_requires_explicit_provider_replacement()
{
    let root = tempfile::tempdir().unwrap();
    fixture(root.path()).await;
    let mut server = mockito::Server::new_async().await;
    runtime(&mut server).await;
    provider(&mut server, "Root0001", "RootVer1").await;
    provider(&mut server, "Root0002", "RootVer2").await;
    let options = AddOptions {
        inputs: vec!["Root0001".into()],
        force: false,
        continue_independent: false,
        platform: Some(SearchPlatform::Modrinth),
        kind: None,
        version_id: Some("RootVer1".into()),
        file_id: None,
        file_plan: None,
        download_as_local: false,
    };
    super::super::add_with_catalog(
        &session(root.path(), false),
        options,
        ProviderCatalog::for_loopback_tests(&server.url(), None),
    )
    .await
    .unwrap();
    let old = project(root.path());
    let key = old.intent().roots.keys().next().unwrap().clone();
    let mut intent = old.intent().clone();
    intent.roots.get_mut(&key).unwrap().selection = DependencySelection::Automatic;
    crate::testing::write_authoring(&root.path().join("project/empack.yml"), &intent).unwrap();
    synchronization::synchronize(&session(root.path(), false), false)
        .await
        .unwrap();
    execute(root.path(), &server, args(false), false)
        .await
        .unwrap();
    let retained = project(root.path());
    assert_eq!(
        retained.lock().dependencies[&key].selected,
        old.lock().dependencies[&key].selected
    );
    assert_eq!(
        retained.lock().dependencies[&key].files,
        old.lock().dependencies[&key].files
    );
    let mut replace = args(false);
    replace.replacements = vec![format!("{}=modrinth:Root0002", key.as_str())];
    execute(root.path(), &server, replace, false).await.unwrap();
    let replaced = project(root.path());
    assert_eq!(
        replaced.lock().dependencies[&key]
            .selected
            .as_ref()
            .unwrap()
            .project
            .to_string(),
        "Root0002"
    );
    assert_eq!(
        replaced.intent().roots[&key].selection,
        DependencySelection::Automatic
    );
    let before = crate::application::engine_host::tests::snapshot(root.path());
    let mut invalid = args(false);
    invalid.replacements = vec!["missing=modrinth:Root0001".into()];
    assert!(
        execute(root.path(), &server, invalid, false)
            .await
            .unwrap_err()
            .to_string()
            .contains("not an explicit")
    );
    assert_eq!(
        crate::application::engine_host::tests::snapshot(root.path()),
        before
    );
}
#[tokio::test]
async fn migration_rejects_stale_lock_and_unknown_runtime_without_changing_documents() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path()).await;
    let mut server = mockito::Server::new_async().await;
    runtime(&mut server).await;
    let mut invalid = args(false);
    invalid.loader_version = Some("not-published".into());
    let before = crate::application::engine_host::tests::snapshot(root.path());
    assert!(
        execute(root.path(), &server, invalid, false)
            .await
            .unwrap_err()
            .to_string()
            .contains("absent")
    );
    assert_eq!(
        crate::application::engine_host::tests::snapshot(root.path()),
        before
    );
    let path = root.path().join("project/empack.yml");
    let mut intent = project(root.path()).intent().clone();
    intent.metadata.name = "Changed after lock".into();
    crate::testing::write_authoring(&path, &intent).unwrap();
    let before = fs::read(&path).unwrap();
    assert!(
        execute(root.path(), &server, args(false), false)
            .await
            .is_err()
    );
    assert_eq!(fs::read(path).unwrap(), before);
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LibraryCase {
    Required,
    DroppedCompatible,
    DroppedIncompatible,
    DroppedUnavailable,
    SyncDroppedIncompatible,
}
async fn library_migration(pinned_requirement: bool, case: LibraryCase) {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path()).await;
    let mut server = mockito::Server::new_async().await;
    runtime(&mut server).await;
    let dependency = |project: &str, version: Option<&str>| json!({"project_id":project,"version_id":version,"dependency_type":"required"});
    let versions = [
        (
            "Root0001",
            "RootOld1",
            vec!["1.21.1"],
            vec![dependency("Library1", None), dependency("Stable01", None)],
        ),
        (
            "Root0001",
            "RootNew1",
            vec!["1.21.2"],
            if case == LibraryCase::Required {
                vec![
                    dependency("Library1", pinned_requirement.then_some("LibNew01")),
                    dependency("Stable01", None),
                ]
            } else {
                vec![dependency("Stable01", None)]
            },
        ),
        (
            "Library1",
            "LibOld01",
            if case == LibraryCase::DroppedCompatible {
                vec!["1.21.1", "1.21.2"]
            } else {
                vec!["1.21.1"]
            },
            vec![],
        ),
        ("Library1", "LibNew01", vec!["1.21.2"], vec![]),
        ("Stable01", "StableV1", vec!["1.21.1", "1.21.2"], vec![]),
    ];
    let mut catalogs = std::collections::BTreeMap::<&str, Vec<serde_json::Value>>::new();
    let mut old_library_record = None;
    for (id, version, games, dependencies) in versions {
        let value = json!({"id":version,"project_id":id,"game_versions":games,"loaders":["fabric"],"files":[{"filename":format!("{id}.jar"),"primary":true,"size":7,"hashes":{"sha512":"11".repeat(64)},"url":format!("https://example.invalid/{id}.jar")}],"dependencies":dependencies,"date_published":"2026-01-01T00:00:00Z","status":"listed","version_type":"release"});
        let record = server
            .mock("GET", format!("/version/{version}").as_str())
            .with_body(value.to_string())
            .create_async()
            .await;
        if version == "LibOld01" {
            old_library_record = Some(record);
        }
        catalogs.entry(id).or_default().push(value);
    }
    for (id, versions) in catalogs {
        server.mock("GET", format!("/project/{id}").as_str()).with_body(json!({"id":id,"slug":id.to_lowercase(),"title":id,"project_type":"mod","loaders":["fabric"]}).to_string()).create_async().await;
        server
            .mock("GET", format!("/project/{id}/version").as_str())
            .match_query(mockito::Matcher::Any)
            .with_body(json!(versions).to_string())
            .create_async()
            .await;
    }
    super::super::add_with_catalog(
        &session(root.path(), false),
        AddOptions {
            inputs: vec!["Root0001".into()],
            force: false,
            continue_independent: false,
            platform: Some(SearchPlatform::Modrinth),
            kind: None,
            version_id: None,
            file_id: None,
            file_plan: None,
            download_as_local: false,
        },
        ProviderCatalog::for_loopback_tests(&server.url(), None),
    )
    .await
    .unwrap();
    let old = project(root.path());
    assert_eq!(old.intent().roots.len(), 1);
    let library = DependencyKey::parse("library1").unwrap();
    let stable = DependencyKey::parse("stable01").unwrap();
    assert_eq!(
        old.lock().dependencies[&library]
            .selected
            .as_ref()
            .unwrap()
            .selection,
        old.lock().dependencies[&library]
            .selected
            .as_ref()
            .unwrap()
            .project
            .parse_pin("LibOld01")
            .unwrap()
    );
    if case == LibraryCase::DroppedUnavailable {
        old_library_record.unwrap().remove_async().await;
        server
            .mock("GET", "/version/LibOld01")
            .with_status(404)
            .create_async()
            .await;
    }
    if case == LibraryCase::SyncDroppedIncompatible {
        let mut intent = old.intent().clone();
        intent.runtime = MigrationOptions::parse(args(false)).unwrap().runtime;
        crate::testing::write_authoring(&root.path().join("project/empack.yml"), &intent).unwrap();
    }
    let before = crate::application::engine_host::tests::snapshot(&root.path().join("project"));
    if matches!(
        case,
        LibraryCase::DroppedIncompatible
            | LibraryCase::DroppedUnavailable
            | LibraryCase::SyncDroppedIncompatible
    ) {
        let result = if case == LibraryCase::SyncDroppedIncompatible {
            synchronization::synchronize_with_services(
                &session(root.path(), false),
                false,
                services(&server),
                RuntimeCatalog::for_loopback_tests(&server.url()),
            )
            .await
        } else {
            execute(root.path(), &server, args(false), false).await
        };
        let error = result.unwrap_err().to_string();
        assert!(error.contains("library1"), "{error}");
        assert!(
            error.contains(if case == LibraryCase::DroppedUnavailable {
                "evidence is unavailable or invalid"
            } else {
                "incompatible"
            }),
            "{error}"
        );
        assert_eq!(
            crate::application::engine_host::tests::snapshot(&root.path().join("project")),
            before
        );
        return;
    }
    execute(root.path(), &server, args(true), false)
        .await
        .unwrap();
    assert_eq!(
        crate::application::engine_host::tests::snapshot(&root.path().join("project")),
        before
    );
    execute(root.path(), &server, args(false), false)
        .await
        .unwrap();
    let next = project(root.path());
    assert_eq!(next.intent().roots.len(), 1);
    assert_eq!(
        next.lock().dependencies[&library]
            .selected
            .as_ref()
            .unwrap()
            .selection,
        old.lock().dependencies[&library]
            .selected
            .as_ref()
            .unwrap()
            .project
            .parse_pin(if case == LibraryCase::Required {
                "LibNew01"
            } else {
                "LibOld01"
            })
            .unwrap()
    );
    assert_eq!(
        next.lock().dependencies[&stable],
        old.lock().dependencies[&stable]
    );
    assert_eq!(
        next.lock()
            .required_edges
            .values()
            .any(|edges| edges.contains(&library)),
        case == LibraryCase::Required
    );
    let report = delta::MigrationDelta::between(old.intent(), old.lock(), &next);
    let json = serde_json::to_value(&report).unwrap();
    assert_eq!(json["schema"], 2);
    let rows = json["dependencies"].as_array().unwrap();
    assert_eq!(rows.len(), 3);
    let library_row = rows.iter().find(|row| row["key"] == "library1").unwrap();
    assert_eq!(library_row["before"]["selection"]["selection"], "LibOld01");
    assert_eq!(
        library_row["after"]["selection"]["selection"],
        if case == LibraryCase::Required {
            "LibNew01"
        } else {
            "LibOld01"
        }
    );
    assert_eq!(library_row["before"]["role"], "transitive");
    assert_eq!(
        library_row["before"]["reason_chain"],
        json!(["root0001", "library1"])
    );
    assert!(
        !library_row["after"]["placements"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        library_row["change"],
        if case == LibraryCase::Required {
            "changed"
        } else {
            "retained"
        }
    );
    assert_eq!(
        library_row["requirement"],
        if case == LibraryCase::Required {
            "required"
        } else {
            "no-longer-required-retained"
        }
    );
    let text = report.text_lines().join("\n");
    assert!(text.contains("LibOld01"));
    assert!(text.contains("Library1.jar"));
    assert!(text.contains("root0001 -> library1"));
    if case == LibraryCase::Required {
        assert!(text.contains("LibNew01"));
    }
    // Union reporting preserves removed keys and identifies newly required entries.
    let mut synthetic_prior = old.lock().clone();
    let dependency = synthetic_prior.dependencies.remove(&library).unwrap();
    synthetic_prior
        .required_edges
        .values_mut()
        .for_each(|edges| {
            edges.remove(&library);
        });
    synthetic_prior.dependencies.insert(
        DependencyKey::parse("previous-library").unwrap(),
        dependency,
    );
    let union = serde_json::to_value(delta::MigrationDelta::between(
        old.intent(),
        &synthetic_prior,
        &next,
    ))
    .unwrap();
    let rows = union["dependencies"].as_array().unwrap();
    assert_eq!(rows.len(), 4);
    assert_eq!(
        rows.iter()
            .find(|row| row["key"] == "previous-library")
            .unwrap()["change"],
        "removed"
    );
    if case == LibraryCase::Required {
        let added = rows.iter().find(|row| row["key"] == "library1").unwrap();
        assert_eq!(added["change"], "added");
        assert_eq!(added["requirement"], "newly-required");
    }
    if case == LibraryCase::DroppedCompatible {
        let mut lock = next.lock().clone();
        let root_key = next.intent().roots.keys().next().unwrap();
        lock.coverage.insert(root_key.clone(), Coverage::Unknown);
        let unknown =
            ResolvedProject::validate(next.intent().clone(), lock, next.lock().intent_revision)
                .unwrap();
        let report = serde_json::to_value(delta::MigrationDelta::between(
            old.intent(),
            old.lock(),
            &unknown,
        ))
        .unwrap();
        let row = report["dependencies"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["key"] == "library1")
            .unwrap();
        assert_eq!(row["requirement"], "requirement-unknown-retained");
    }
    if case == LibraryCase::Required {
        let mut lock = next.lock().clone();
        let library = lock.dependencies.get_mut(&library).unwrap();
        let mut files = library.files.as_slice().to_vec();
        let mut placements = files[0].placements.as_slice().to_vec();
        placements[0].destination =
            empack_core::path::InstallDestination::parse("mods/relocated-library.jar").unwrap();
        files[0].placements = NonEmpty::new(placements).unwrap();
        library.files = NonEmpty::new(files).unwrap();
        let moved =
            ResolvedProject::validate(next.intent().clone(), lock, next.lock().intent_revision)
                .unwrap();
        let delta = delta::MigrationDelta::between(next.intent(), next.lock(), &moved);
        let report = serde_json::to_value(&delta).unwrap();
        let row = report["dependencies"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["key"] == "library1")
            .unwrap();
        assert_eq!(row["change"], "changed");
        assert_eq!(row["before"]["selection"], row["after"]["selection"]);
        assert_eq!(
            row["before"]["placements"][0]["destination"],
            "mods/Library1.jar"
        );
        assert_eq!(
            row["after"]["placements"][0]["destination"],
            "mods/relocated-library.jar"
        );
        assert!(
            delta
                .text_lines()
                .join("\n")
                .contains("mods/relocated-library.jar")
        );
    }
    if case == LibraryCase::Required {
        // Equal provider pins and placements must not hide changed bytes or acquisition.
        let mut lock = next.lock().clone();
        let dependency = lock
            .dependencies
            .get_mut(&DependencyKey::parse("library1").unwrap())
            .unwrap();
        let mut files = dependency.files.as_slice().to_vec();
        files[0].expected.size = Some(123456);
        files[0].expected.accepted_observation =
            Some(empack_core::digest::ContentId::from_sha256([42; 32]));
        let AcquisitionSpec::Provider { alternatives, .. } = &mut files[0].acquisition else {
            panic!()
        };
        *alternatives = vec!["https://example.org/revised-library.jar".into()];
        dependency.files = NonEmpty::new(files).unwrap();
        let changed =
            ResolvedProject::validate(next.intent().clone(), lock, next.lock().intent_revision)
                .unwrap();
        let delta = delta::MigrationDelta::between(next.intent(), next.lock(), &changed);
        let report = serde_json::to_value(&delta).unwrap();
        let row = report["dependencies"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["key"] == "library1")
            .unwrap();
        assert_eq!(row["change"], "changed");
        assert_eq!(row["before"]["selection"], row["after"]["selection"]);
        assert_eq!(row["before"]["placements"], row["after"]["placements"]);
        assert_ne!(row["before"]["files"], row["after"]["files"]);
        assert_eq!(row["after"]["files"][0]["expected"]["size"], 123456);
        let text = delta.text_lines().join("\n");
        assert!(text.contains("full assertions and sources available with --json"));
        assert!(!text.contains("revised-library.jar"));
        assert!(!text.contains(&"2a".repeat(32)));
    }
    let after = crate::application::engine_host::tests::snapshot(&root.path().join("project"));
    synchronization::synchronize(&session(root.path(), false), false)
        .await
        .unwrap();
    assert_eq!(
        crate::application::engine_host::tests::snapshot(&root.path().join("project")),
        after
    );
}
#[tokio::test]
async fn migration_reselects_incompatible_required_libraries() {
    library_migration(false, LibraryCase::Required).await;
}
#[tokio::test]
async fn migration_applies_new_exact_required_library_selection() {
    library_migration(true, LibraryCase::Required).await;
}

#[tokio::test]
async fn migration_refuses_incompatible_library_dropped_from_new_closure() {
    library_migration(false, LibraryCase::DroppedIncompatible).await;
}
#[tokio::test]
async fn synchronization_refuses_incompatible_library_dropped_from_new_closure() {
    library_migration(false, LibraryCase::SyncDroppedIncompatible).await;
}
#[tokio::test]
async fn migration_refuses_unknown_retained_library_evidence() {
    library_migration(false, LibraryCase::DroppedUnavailable).await;
}
#[tokio::test]
async fn migration_reports_compatible_library_dropped_from_new_closure_as_retained() {
    library_migration(false, LibraryCase::DroppedCompatible).await;
}

mod journey;
