use super::*;
use crate::engine::{api::DependencyContent, mrpack::LockedFileKey};
use crate::test_support::file_bytes as snapshot;
use crate::{
    application::{
        BuildArgs, InitArgs,
        session_mocks::{
            MockCommandSession, MockConfigProvider, MockInteractiveProvider, MockInvocationProvider,
        },
    },
    engine::{
        api::RemovalSelector,
        build::BuildAcquisitions,
        documents::DocumentCodec,
        providers::{ProjectSelector, ProviderFiles},
    },
};
use empack_core::{
    identity::{ModrinthVersionId, PinSelector},
    model::{DependencyKey, DependencySelection, ProviderKind, ResolvedProject},
    removal::{RemovalEvidencePolicy, RemovalMode},
    requirements::{Requirement, Requirements},
};
use mockito::{Matcher, Server};
use serde_json::{Value, json};
use sha2::{Digest, Sha512};
use std::collections::BTreeMap;
use std::{fs, io::Read};

mod inspection;

fn session(root: &Path, yes: bool, dry: bool) -> MockCommandSession {
    MockCommandSession::new()
        .with_invocation(MockInvocationProvider::new().with_current_dir(root.to_path_buf()))
        .with_config(MockConfigProvider::new(AppConfig {
            workdir: Some("project".into()),
            state_dir: Some("state".into()),
            cache_dir: Some("cache".into()),
            yes,
            dry_run: dry,
            curseforge_api_client_key: None,
            ..Default::default()
        }))
}
async fn fixture(root: &Path) {
    fs::create_dir(root.join("project")).unwrap();
    initialize(
        &session(root, true, false),
        &InitArgs {
            mc_version: Some("1.21.1".into()),
            modloader: Some("fabric".into()),
            loader_version: Some("0.16.14".into()),
            pack_name: Some("Dependency Pack".into()),
            pack_version: Some("1.0".into()),
            author: Some("Tester".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
}
fn read(root: &Path) -> ResolvedProject {
    let intent = crate::testing::read_authoring(&root.join("empack.yml")).unwrap();
    DocumentCodec
        .decode_lock(
            &fs::read(root.join("empack.lock")).unwrap(),
            &intent,
            "test",
        )
        .unwrap()
}
fn input(selector: &str, key: Option<&str>) -> ProviderAddInput {
    ProviderAddInput {
        source_selector: None,
        releases: None,
        selector: ProjectSelector::parse(ProviderKind::Modrinth, selector).unwrap(),
        key: key.map(|value| DependencyKey::parse(value).unwrap()),
        kind: None,
        pin: Some(PinSelector::ModrinthVersion(
            ModrinthVersionId::parse("RootVer1").unwrap(),
        )),
        requirements: Requirements {
            client: Requirement::Required,
            server: Requirement::Required,
        },
        folder: None,
        files: ProviderFiles::Primary,
    }
}
fn version(project: &str, id: &str, dependencies: Value) -> Value {
    json!({"id":id,"project_id":project,"game_versions":["1.21.1"],"loaders":["fabric"],
        "files":[{"filename":format!("{project}.jar"),"primary":true,"size":7,
            "hashes":{"sha1":"f07e5a815613c5abeddc4b682247a4c42d8a95df","sha512":empack_core::digest::ExpectedDigest::Sha512(Sha512::digest(b"payload").into()).hex()},
            "url":format!("https://example.invalid/{project}.jar")}],
        "dependencies":dependencies,"date_published":"2026-01-01T00:00:00Z","status":"listed","version_type":"release"})
}
async fn records(server: &mut Server) {
    records_with_dependencies(
        server,
        json!([{"project_id":"Need0001","version_id":"NeedVer1","dependency_type":"required"}]),
    )
    .await;
}
async fn records_with_dependencies(server: &mut Server, dependencies: Value) {
    records_for_version(server, "RootVer1", dependencies).await;
}
async fn records_for_version(server: &mut Server, root_version: &str, dependencies: Value) {
    records_for_selections(server, root_version, "NeedVer1", dependencies).await;
}
async fn records_for_selections(
    server: &mut Server,
    root_version: &str,
    needed_version: &str,
    dependencies: Value,
) {
    for (id, slug, version) in [
        (
            "Root0001",
            "renderer",
            version("Root0001", root_version, dependencies),
        ),
        (
            "Need0001",
            "required-library",
            version("Need0001", needed_version, json!([])),
        ),
    ] {
        for selector in [id, slug] {
            server.mock("GET", format!("/project/{selector}").as_str()).with_body(json!({"id":id,"slug":slug,"title":slug,"project_type":"mod","loaders":["fabric"]}).to_string()).create_async().await;
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
            .mock("GET", format!("/project/{id}/version").as_str())
            .match_query(Matcher::Any)
            .with_body(json!([version]).to_string())
            .create_async()
            .await;
    }
}
async fn add(
    root: &Path,
    server: &Server,
    inputs: Vec<ProviderAddInput>,
    yes: bool,
    dry: bool,
) -> Result<()> {
    add_with_catalog(
        &session(root, yes, dry),
        NonEmpty::new(inputs).unwrap(),
        ReleasePolicy::PreferStable,
        ExistingDependencyPolicy::UpdateSameIdentity,
        ProviderCatalog::for_loopback_tests(&server.url(), None),
    )
    .await
}
fn sync_request(project: &ResolvedProject) -> SyncRequest {
    let mut content = BTreeMap::new();
    for (key, dependency) in &project.lock().dependencies {
        for file in dependency.files.as_slice() {
            content.insert(
                LockedFileKey {
                    dependency: key.clone(),
                    slot: file.slot.clone(),
                },
                DependencyContent::Reference,
            );
        }
    }
    SyncRequest::Supplied {
        resolution: None,
        content,
    }
}
fn removal(query: &str) -> RemoveRequest {
    RemoveRequest {
        selections: NonEmpty::new(vec![RemovalSelector::Query(query.into())]).unwrap(),
        mode: RemovalMode::RemoveContent,
        evidence: RemovalEvidencePolicy::RequireComplete,
    }
}
#[tokio::test]
async fn native_provider_host_preserves_alias_pin_required_content_and_cross_command_identity() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path()).await;
    let project = root.path().join("project");
    let mut server = Server::new_async().await;
    records(&mut server).await;
    let before = snapshot(root.path());
    for (yes, dry) in [(true, true), (false, false)] {
        add(
            root.path(),
            &server,
            vec![input("renderer", Some("my-alias"))],
            yes,
            dry,
        )
        .await
        .unwrap();
        assert_eq!(snapshot(root.path()), before);
    }
    add(
        root.path(),
        &server,
        vec![input("renderer", Some("my-alias"))],
        true,
        false,
    )
    .await
    .unwrap();
    let installed = read(&project);
    let key = DependencyKey::parse("my-alias").unwrap();
    assert_eq!(installed.intent().roots.len(), 1);
    assert_eq!(installed.lock().dependencies.len(), 2);
    assert!(matches!(
        installed.intent().roots[&key].selection,
        DependencySelection::ProviderPin(_)
    ));
    assert!(
        installed.lock().required_edges[&key]
            .contains(&DependencyKey::parse("required-library").unwrap())
    );
    let before = snapshot(&project);
    for selector in ["Root0001", "https://modrinth.com/mod/renderer"] {
        add(
            root.path(),
            &server,
            vec![input(selector, None)],
            true,
            false,
        )
        .await
        .unwrap();
        assert_eq!(
            snapshot(&project),
            before,
            "Equivalent selector must keep its alias and exact intent"
        );
    }
    for _ in 0..2 {
        synchronize(
            &session(root.path(), true, false),
            sync_request(&read(&project)),
        )
        .await
        .unwrap();
        assert_eq!(snapshot(&project), before);
    }
    // Reference export needs neither a provider requery nor payload acquisition when the
    // original catalog supplied durable origins and complete format evidence.
    super::super::build(
        &session(root.path(), true, false),
        &BuildArgs {
            targets: vec!["modrinth".into()],
            ..Default::default()
        },
        super::super::BuildDecisions::default(),
        crate::engine::build::BuildAcquisitions::default(),
    )
    .await
    .unwrap();
    let mut archive = zip::ZipArchive::new(
        fs::File::open(project.join("dist/Dependency Pack-1.0.mrpack")).unwrap(),
    )
    .unwrap();
    let index: Value =
        serde_json::from_reader(archive.by_name("modrinth.index.json").unwrap()).unwrap();
    assert_eq!(index["files"].as_array().unwrap().len(), 2);
    for id in ["Root0001", "Need0001"] {
        let file = index["files"]
            .as_array()
            .unwrap()
            .iter()
            .find(|file| file["path"] == format!("mods/{id}.jar"))
            .unwrap();
        assert_eq!(
            file["downloads"],
            json!([format!("https://example.invalid/{id}.jar")])
        );
        assert_eq!(file["fileSize"], 7);
        assert_eq!(
            file["hashes"]["sha1"],
            "f07e5a815613c5abeddc4b682247a4c42d8a95df"
        );
        assert!(
            archive
                .by_name(&format!("overrides/mods/{id}.jar"))
                .is_err()
        );
    }
    drop(archive);
    // Bundled Prism output needs actual payloads. Supply verified fixture bytes rather than
    // allowing synthetic provider IDs to reach the live service during this offline test.
    let mut supplied = BuildAcquisitions::default();
    for (key, dependency) in &read(&project).lock().dependencies {
        for file in dependency.files.as_slice() {
            let content = crate::engine::content::verify_stream(
                &mut b"payload".as_slice(),
                &file.expected,
                100,
                crate::engine::content::SourceEvidencePolicy::Compatibility,
                crate::engine::content::InitialObservation::RequireEvidence,
                &crate::application::process_runtime::Cancellation::default(),
            )
            .unwrap();
            supplied.locked.insert(
                LockedFileKey {
                    dependency: key.clone(),
                    slot: file.slot.clone(),
                },
                crate::engine::mrpack::AcquiredBuildFile {
                    content,
                    permissions: empack_core::files::FilePermissions {
                        readonly: false,
                        executable: false,
                    },
                },
            );
        }
    }
    super::super::build(
        &session(root.path(), true, false),
        &BuildArgs {
            targets: vec!["prism".into()],
            delivery: Some("bundled".into()),
            ..Default::default()
        },
        super::super::BuildDecisions::default(),
        supplied,
    )
    .await
    .unwrap();
    let mut archive = zip::ZipArchive::new(
        fs::File::open(project.join("dist/Dependency Pack-1.0-prism-bundled.zip")).unwrap(),
    )
    .unwrap();
    for file in ["Root0001.jar", "Need0001.jar"] {
        let mut bytes = Vec::new();
        archive
            .by_name(&format!(".minecraft/mods/{file}"))
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        assert_eq!(bytes, b"payload");
    }
    let before = snapshot(root.path());
    for (yes, dry) in [(true, true), (false, false)] {
        remove(&session(root.path(), yes, dry), removal("renderer"))
            .await
            .unwrap();
        assert_eq!(snapshot(root.path()), before);
    }
    remove(&session(root.path(), true, false), removal("renderer"))
        .await
        .unwrap();
    let retained = read(&project);
    assert!(retained.intent().roots.is_empty());
    assert_eq!(retained.lock().dependencies.len(), 1);
    assert!(
        retained
            .lock()
            .dependencies
            .contains_key(&DependencyKey::parse("required-library").unwrap())
    );
    let before = snapshot(&project);
    synchronize(&session(root.path(), true, false), sync_request(&retained))
        .await
        .unwrap();
    assert_eq!(snapshot(&project), before);
}
#[tokio::test]
async fn native_provider_host_publishes_nothing_when_any_requested_selection_fails() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path()).await;
    let mut server = Server::new_async().await;
    records(&mut server).await;
    server
        .mock("GET", "/project/missing")
        .with_status(404)
        .create_async()
        .await;
    let before = snapshot(root.path());
    let result = add(
        root.path(),
        &server,
        vec![input("renderer", Some("my-alias")), input("missing", None)],
        true,
        false,
    )
    .await;
    assert!(result.is_err());
    assert_eq!(snapshot(root.path()), before);
}
#[tokio::test]
async fn native_provider_resolution_rejects_a_concurrent_lock_edit_without_overwriting_it() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path()).await;
    let mut server = Server::new_async().await;
    let path = root.path().join("project/empack.lock");
    let mut changed = fs::read(&path).unwrap();
    changed.extend_from_slice(b"\n# An independent lock edit\n");
    let new_bytes = changed.clone();
    server.mock("GET", "/project/renderer").with_body_from_request(move |_| {
        fs::write(&path, &new_bytes).unwrap();
        json!({"id":"Root0001","slug":"renderer","title":"renderer","project_type":"mod","loaders":["fabric"]}).to_string().into_bytes()
    }).create_async().await;
    records(&mut server).await;
    let mut before = snapshot(root.path());
    before.insert(PathBuf::from("project/empack.lock"), changed);
    let error = add(
        root.path(),
        &server,
        vec![input("renderer", Some("my-alias"))],
        true,
        false,
    )
    .await
    .unwrap_err();
    assert!(
        format!("{error:#}").contains("changed project documents"),
        "{error:#}"
    );
    assert_eq!(snapshot(root.path()), before);
}

#[tokio::test]
async fn incomplete_required_evidence_is_a_decision_without_partial_publication() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path()).await;
    let mut server = Server::new_async().await;
    records_with_dependencies(&mut server, json!([{"dependency_type":"required"}])).await;
    let before = snapshot(root.path());
    let error = add(
        root.path(),
        &server,
        vec![input("renderer", Some("my-alias"))],
        true,
        false,
    )
    .await
    .unwrap_err();
    assert!(
        format!("{error:#}").contains("Dependency check could not finish"),
        "{error:#}"
    );
    assert_eq!(snapshot(root.path()), before);
}

fn local_batch_input(source: &str, key: &str, destination: &str) -> AddHostInput {
    use crate::engine::addition::{FileEvidence, FileKindPolicy};
    use empack_core::{
        model::{ContentKind, ContentLayer, Placement},
        path::InstallDestination,
    };
    let requirements = Requirements {
        client: Requirement::Required,
        server: Requirement::Unsupported,
    };
    AddHostInput::File(DirectFileInput {
        role: crate::engine::addition::FileInputRole::Primary,
        key: DependencyKey::parse(key).unwrap(),
        title: "Local settings".into(),
        source: DirectFileSource::Local(source.into()),
        evidence: FileEvidence::AcceptObserved,
        kind: ContentKind::Config,
        kind_policy: FileKindPolicy::RequireRecognized,
        requirements: requirements.clone(),
        placements: NonEmpty::new(vec![Placement {
            layer: ContentLayer::Client,
            destination: InstallDestination::parse(destination).unwrap(),
            requirements,
        }])
        .unwrap(),
    })
}
async fn mixed(
    root: &Path,
    server: &Server,
    inputs: Vec<AddHostInput>,
    yes: bool,
    dry: bool,
) -> Result<()> {
    add_with_services(
        &session(root, yes, dry),
        NonEmpty::new(inputs)?,
        ReleasePolicy::PreferStable,
        SourceEvidencePolicy::Compatibility,
        ExistingDependencyPolicy::UpdateSameIdentity,
        AdditionServices {
            catalog: ProviderCatalog::for_loopback_tests(&server.url(), None),
            transport: HttpAcquisition::for_loopback_tests(),
            files: DirectFileLimits::default(),
        },
    )
    .await
}
#[tokio::test]
async fn mixed_addition_uses_one_publication_and_converges_through_sync_and_export() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path()).await;
    fs::write(root.path().join("settings.toml"), b"enabled=true").unwrap();
    let mut server = Server::new_async().await;
    records(&mut server).await;
    let inputs = || {
        vec![
            AddHostInput::Provider(input("renderer", Some("my-alias"))),
            local_batch_input("settings.toml", "settings", "config/settings.toml"),
        ]
    };
    let before = snapshot(root.path());
    for (yes, dry) in [(true, true), (false, false)] {
        mixed(root.path(), &server, inputs(), yes, dry)
            .await
            .unwrap();
        assert_eq!(snapshot(root.path()), before);
    }
    mixed(root.path(), &server, inputs(), true, false)
        .await
        .unwrap();
    let project = root.path().join("project");
    let installed = read(&project);
    assert_eq!(installed.intent().roots.len(), 2);
    assert_eq!(installed.lock().dependencies.len(), 3);
    assert_eq!(
        installed.lock().required_edges[&DependencyKey::parse("my-alias").unwrap()].len(),
        1
    );
    assert_eq!(
        fs::read(project.join("overrides/client/config/settings.toml")).unwrap(),
        b"enabled=true"
    );
    let before = snapshot(&project);
    mixed(root.path(), &server, inputs(), true, false)
        .await
        .unwrap();
    assert_eq!(snapshot(&project), before);
    for _ in 0..2 {
        synchronize(
            &session(root.path(), true, false),
            SyncRequest::Recorded {
                resolution: None,
                evidence: SourceEvidencePolicy::Compatibility,
            },
        )
        .await
        .unwrap();
        assert_eq!(snapshot(&project), before);
    }
    build(
        &session(root.path(), true, false),
        &BuildArgs {
            targets: vec!["modrinth".into()],
            ..Default::default()
        },
        BuildDecisions::default(),
        BuildAcquisitions::default(),
    )
    .await
    .unwrap();
    let mut archive = zip::ZipArchive::new(
        fs::File::open(project.join("dist/Dependency Pack-1.0.mrpack")).unwrap(),
    )
    .unwrap();
    let index: Value =
        serde_json::from_reader(archive.by_name("modrinth.index.json").unwrap()).unwrap();
    assert_eq!(index["files"].as_array().unwrap().len(), 2);
    let mut bytes = Vec::new();
    archive
        .by_name("client-overrides/config/settings.toml")
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    assert_eq!(bytes, b"enabled=true");
}
#[tokio::test]
async fn mixed_addition_failures_never_publish_either_successful_subset() {
    for mode in [
        "provider",
        "file",
        "root-collision",
        "dependency-collision",
        "placement-collision",
    ] {
        let root = tempfile::tempdir().unwrap();
        fixture(root.path()).await;
        fs::write(root.path().join("settings.toml"), b"settings").unwrap();
        let mut server = Server::new_async().await;
        records(&mut server).await;
        let provider = if mode == "provider" {
            "absent"
        } else {
            "renderer"
        };
        let source = if mode == "file" {
            "missing.toml"
        } else {
            "settings.toml"
        };
        let key = match mode {
            "root-collision" => "my-alias",
            "dependency-collision" => "required-library",
            _ => "settings",
        };
        let mut file = local_batch_input(
            source,
            key,
            if mode == "placement-collision" {
                "mods/Root0001.jar"
            } else {
                "config/settings.toml"
            },
        );
        if mode == "placement-collision"
            && let AddHostInput::File(input) = &mut file
        {
            let mut placement = input.placements.as_slice()[0].clone();
            placement.layer = empack_core::model::ContentLayer::Common;
            input.placements = NonEmpty::new(vec![placement]).unwrap();
        }
        let before = snapshot(root.path());
        assert!(
            mixed(
                root.path(),
                &server,
                vec![
                    AddHostInput::Provider(input(provider, Some("my-alias"))),
                    file
                ],
                true,
                false
            )
            .await
            .is_err(),
            "{mode}"
        );
        assert_eq!(snapshot(root.path()), before, "{mode}");
    }
}

async fn update_mixed(
    root: &Path,
    server: &Server,
    inputs: Vec<AddHostInput>,
    yes: bool,
    dry: bool,
) -> Result<()> {
    change_with_services(
        &session(root, yes, dry),
        NonEmpty::new(inputs)?,
        ReleasePolicy::PreferStable,
        SourceEvidencePolicy::Compatibility,
        Change::Update {
            selection: crate::application::engine_host::dependencies::selection::UpdateSelection::Automatic,
            pins: Default::default(),
            mode: ApplicationMode::Apply,
            output: Default::default(),
        },
        AdditionServices {
            catalog: ProviderCatalog::for_loopback_tests(&server.url(), None),
            transport: HttpAcquisition::for_loopback_tests(),
            files: DirectFileLimits::default(),
        },
        BatchPolicy::AllRequested,
    )
    .await
}
async fn next_records(server: &mut Server) {
    records_for_version(
        server,
        "RootVer2",
        json!([{"project_id":"Need0001","version_id":"NeedVer1","dependency_type":"required"}]),
    )
    .await;
}

fn update_inputs(source: &str, pinned: bool) -> Vec<AddHostInput> {
    let mut provider = input("renderer", Some("alias"));
    if !pinned {
        provider.pin = None;
    }
    vec![
        AddHostInput::Provider(provider),
        local_batch_input(source, "settings", "config/settings.cfg"),
    ]
}
#[tokio::test]
async fn native_update_preserves_authored_intent_and_converges_across_mixed_content() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path()).await;
    let project = root.path().join("project");
    fs::write(root.path().join("settings.cfg"), b"enabled=false").unwrap();
    let mut old = Server::new_async().await;
    records(&mut old).await;
    mixed(
        root.path(),
        &old,
        update_inputs("settings.cfg", false),
        true,
        false,
    )
    .await
    .unwrap();
    let previous = read(&project);
    let key = DependencyKey::parse("alias").unwrap();
    let required = DependencyKey::parse("required-library").unwrap();
    let mut authored = b"# keep my release notes\n".to_vec();
    authored.extend(fs::read(project.join("empack.yml")).unwrap());
    fs::write(project.join("empack.yml"), &authored).unwrap();
    fs::write(root.path().join("settings.cfg"), b"enabled=true").unwrap();
    let mut next = Server::new_async().await;
    next_records(&mut next).await;
    let before = snapshot(&project);
    for (yes, dry) in [(true, true), (false, false)] {
        update_mixed(
            root.path(),
            &next,
            update_inputs("settings.cfg", false),
            yes,
            dry,
        )
        .await
        .unwrap();
        assert_eq!(snapshot(&project), before);
    }
    update_mixed(
        root.path(),
        &next,
        update_inputs("settings.cfg", false),
        true,
        false,
    )
    .await
    .unwrap();
    let updated = read(&project);
    assert_eq!(updated.intent(), previous.intent());
    assert_eq!(
        updated.lock().dependencies[&key]
            .selected
            .as_ref()
            .unwrap()
            .selection,
        PinSelector::ModrinthVersion(ModrinthVersionId::parse("RootVer2").unwrap())
    );
    assert_eq!(
        updated.lock().dependencies[&required],
        previous.lock().dependencies[&required]
    );
    assert_eq!(
        updated.lock().required_edges,
        previous.lock().required_edges
    );
    assert_eq!(fs::read(project.join("empack.yml")).unwrap(), authored);
    assert_eq!(
        fs::read(project.join("overrides/client/config/settings.cfg")).unwrap(),
        b"enabled=true"
    );
    let before = snapshot(&project);
    update_mixed(
        root.path(),
        &next,
        update_inputs("settings.cfg", false),
        true,
        false,
    )
    .await
    .unwrap();
    assert_eq!(snapshot(&project), before);
    // The public direct-only entry shares the same exact update lifecycle.
    super::update(
        &session(root.path(), true, false),
        NonEmpty::new(vec![local_batch_input(
            "settings.cfg",
            "settings",
            "config/settings.cfg",
        )])
        .unwrap(),
        ReleasePolicy::PreferStable,
        SourceEvidencePolicy::Compatibility,
    )
    .await
    .unwrap();
    assert_eq!(snapshot(&project), before);
    for _ in 0..2 {
        synchronize(
            &session(root.path(), true, false),
            SyncRequest::Recorded {
                resolution: None,
                evidence: SourceEvidencePolicy::Compatibility,
            },
        )
        .await
        .unwrap();
        assert_eq!(snapshot(&project), before);
    }
    build(
        &session(root.path(), true, false),
        &BuildArgs {
            targets: vec!["modrinth".into()],
            ..Default::default()
        },
        BuildDecisions::default(),
        BuildAcquisitions::default(),
    )
    .await
    .unwrap();
    let mut archive = zip::ZipArchive::new(
        fs::File::open(project.join("dist/Dependency Pack-1.0.mrpack")).unwrap(),
    )
    .unwrap();
    let mut bytes = Vec::new();
    archive
        .by_name("client-overrides/config/settings.cfg")
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    assert_eq!(bytes, b"enabled=true");
}
#[tokio::test]
async fn native_update_rejects_pin_changes_new_identities_and_failed_sources_as_one_batch() {
    for case in ["pin", "new-identity", "missing-file"] {
        let root = tempfile::tempdir().unwrap();
        fixture(root.path()).await;
        fs::write(root.path().join("settings.cfg"), b"enabled=false").unwrap();
        let mut old = Server::new_async().await;
        records(&mut old).await;
        mixed(
            root.path(),
            &old,
            update_inputs("settings.cfg", case == "pin"),
            true,
            false,
        )
        .await
        .unwrap();
        fs::write(root.path().join("settings.cfg"), b"enabled=true").unwrap();
        let mut next = Server::new_async().await;
        next_records(&mut next).await;
        let mut inputs = update_inputs(
            if case == "missing-file" {
                "missing.cfg"
            } else {
                "settings.cfg"
            },
            false,
        );
        if case == "new-identity" {
            inputs.push(local_batch_input("settings.cfg", "new", "config/new.cfg"));
        }
        let before = snapshot(&root.path().join("project"));
        assert!(
            update_mixed(root.path(), &next, inputs, true, false)
                .await
                .is_err(),
            "{case}"
        );
        assert_eq!(snapshot(&root.path().join("project")), before, "{case}");
    }
}
#[tokio::test]
async fn native_update_binds_resolution_to_the_captured_raw_lock() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path()).await;
    fs::write(root.path().join("settings.cfg"), b"enabled=false").unwrap();
    let mut old = Server::new_async().await;
    records(&mut old).await;
    mixed(
        root.path(),
        &old,
        update_inputs("settings.cfg", false),
        true,
        false,
    )
    .await
    .unwrap();
    let project = root.path().join("project");
    let path = project.join("empack.lock");
    let mut changed = fs::read(&path).unwrap();
    changed.extend_from_slice(b"\n# independent edit during resolution\n");
    let bytes = changed.clone();
    let mut next = Server::new_async().await;
    next.mock("GET", "/project/Root0001").with_body_from_request(move |_| {
        fs::write(&path, &bytes).unwrap();
        json!({"id":"Root0001","slug":"renderer","title":"renderer","project_type":"mod","loaders":["fabric"]}).to_string().into_bytes()
    }).create_async().await;
    next_records(&mut next).await;
    let mut expected = snapshot(&project);
    expected.insert(PathBuf::from("empack.lock"), changed);
    let error = update_mixed(
        root.path(),
        &next,
        update_inputs("settings.cfg", false),
        true,
        false,
    )
    .await
    .unwrap_err();
    assert!(
        format!("{error:#}").contains("changed project documents"),
        "{error:#}"
    );
    assert_eq!(snapshot(&project), expected);
}

#[tokio::test]
async fn native_update_checks_retained_dependents_without_promoting_selected_transitives() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path()).await;
    let mut old = Server::new_async().await;
    records(&mut old).await;
    let mut root_input = input("renderer", Some("alias"));
    root_input.pin = None;
    add(root.path(), &old, vec![root_input.clone()], true, false)
        .await
        .unwrap();
    let project = root.path().join("project");
    let authored = fs::read(project.join("empack.yml")).unwrap();
    let mut next = Server::new_async().await;
    records_for_selections(
        &mut next,
        "RootVer2",
        "NeedVer2",
        json!([{"project_id":"Need0001","version_id":"NeedVer2","dependency_type":"required"}]),
    )
    .await;
    let mut required = input("Need0001", Some("required-library"));
    required.pin = Some(PinSelector::ModrinthVersion(
        ModrinthVersionId::parse("NeedVer2").unwrap(),
    ));
    let before = snapshot(&project);
    let error = update_mixed(
        root.path(),
        &next,
        vec![AddHostInput::Provider(required.clone())],
        true,
        false,
    )
    .await
    .unwrap_err();
    assert!(
        format!("{error:#}").contains("retained required dependents"),
        "{error:#}"
    );
    assert_eq!(snapshot(&project), before);
    update_mixed(
        root.path(),
        &next,
        vec![
            AddHostInput::Provider(root_input),
            AddHostInput::Provider(required),
        ],
        true,
        false,
    )
    .await
    .unwrap();
    let updated = read(&project);
    assert_eq!(fs::read(project.join("empack.yml")).unwrap(), authored);
    assert_eq!(updated.intent().roots.len(), 1);
    assert!(
        !updated
            .intent()
            .roots
            .contains_key(&DependencyKey::parse("required-library").unwrap())
    );
    assert_eq!(
        updated.lock().dependencies[&DependencyKey::parse("required-library").unwrap()]
            .selected
            .as_ref()
            .unwrap()
            .selection,
        PinSelector::ModrinthVersion(ModrinthVersionId::parse("NeedVer2").unwrap())
    );
    let before = snapshot(&project);
    synchronize(
        &session(root.path(), true, false),
        SyncRequest::Recorded {
            resolution: None,
            evidence: SourceEvidencePolicy::Compatibility,
        },
    )
    .await
    .unwrap();
    assert_eq!(snapshot(&project), before);
}

async fn observed_fixture(root: &Path) -> ResolvedProject {
    fixture(root).await;
    let mut server = Server::new_async().await;
    records(&mut server).await;
    add(
        root,
        &server,
        vec![input("renderer", Some("my-alias"))],
        true,
        false,
    )
    .await
    .unwrap();
    let project = root.join("project");
    let proposed = read(&project);
    fs::create_dir_all(project.join("pack/mods")).unwrap();
    for id in ["Root0001", "Need0001"] {
        fs::write(project.join(format!("pack/mods/{id}.jar")), b"payload").unwrap();
    }
    fs::remove_file(project.join("empack.lock")).unwrap();
    proposed
}
fn adoption(project: &ResolvedProject) -> AdoptObservedRequest {
    AdoptObservedRequest {
        group: empack_core::addition::AdditionGroup::from_resolved(project).unwrap(),
    }
}
#[tokio::test]
async fn native_adoption_creates_a_lock_without_rewriting_payloads_then_sync_export_remove_converge()
 {
    let root = tempfile::tempdir().unwrap();
    let proposed = observed_fixture(root.path()).await;
    let project = root.path().join("project");
    let payload = project.join("pack/mods/Root0001.jar");
    let modified = fs::metadata(&payload).unwrap().modified().unwrap();
    let authored = fs::read(project.join("empack.yml")).unwrap();
    let before = snapshot(root.path());
    for (yes, dry) in [(true, true), (false, false)] {
        adopt_observed(&session(root.path(), yes, dry), adoption(&proposed))
            .await
            .unwrap();
        assert_eq!(snapshot(root.path()), before);
        assert_eq!(
            fs::metadata(&payload).unwrap().modified().unwrap(),
            modified
        );
    }
    adopt_observed(&session(root.path(), true, false), adoption(&proposed))
        .await
        .unwrap();
    assert_eq!(fs::read(project.join("empack.yml")).unwrap(), authored);
    assert_eq!(
        fs::metadata(&payload).unwrap().modified().unwrap(),
        modified
    );
    let adopted = read(&project);
    assert_eq!(adopted.intent(), proposed.intent());
    assert_eq!(adopted.lock().dependencies, proposed.lock().dependencies);
    assert_eq!(
        adopted.lock().required_edges,
        proposed.lock().required_edges
    );
    let before = snapshot(&project);
    adopt_observed(&session(root.path(), true, false), adoption(&proposed))
        .await
        .unwrap();
    assert_eq!(snapshot(&project), before);
    for _ in 0..2 {
        synchronize(
            &session(root.path(), true, false),
            SyncRequest::Recorded {
                resolution: None,
                evidence: SourceEvidencePolicy::Compatibility,
            },
        )
        .await
        .unwrap();
        assert_eq!(snapshot(&project), before);
    }
    super::super::build(
        &session(root.path(), true, false),
        &BuildArgs {
            targets: vec!["modrinth".into()],
            ..Default::default()
        },
        super::super::BuildDecisions::default(),
        BuildAcquisitions::default(),
    )
    .await
    .unwrap();
    let mut archive = zip::ZipArchive::new(
        fs::File::open(project.join("dist/Dependency Pack-1.0.mrpack")).unwrap(),
    )
    .unwrap();
    let index: Value =
        serde_json::from_reader(archive.by_name("modrinth.index.json").unwrap()).unwrap();
    assert_eq!(index["files"].as_array().unwrap().len(), 2);
    remove(&session(root.path(), true, false), removal("my-alias"))
        .await
        .unwrap();
    assert!(!payload.exists());
    assert_eq!(
        fs::read(project.join("pack/mods/Need0001.jar")).unwrap(),
        b"payload"
    );
    let before = snapshot(&project);
    synchronize(
        &session(root.path(), true, false),
        SyncRequest::Recorded {
            resolution: None,
            evidence: SourceEvidencePolicy::Compatibility,
        },
    )
    .await
    .unwrap();
    assert_eq!(snapshot(&project), before);
    assert!(read(&project).intent().roots.is_empty());
}

#[tokio::test]
async fn native_adoption_rejects_incomplete_or_incoherent_observations_without_publishing() {
    let root = tempfile::tempdir().unwrap();
    let proposed = observed_fixture(root.path()).await;
    let project = root.path().join("project");
    let payload = project.join("pack/mods/Root0001.jar");
    for mode in ["missing", "wrong-bytes", "directory", "invalid-lock"] {
        match mode {
            "missing" => fs::remove_file(&payload).unwrap(),
            "wrong-bytes" => fs::write(&payload, b"changed").unwrap(),
            "directory" => {
                fs::remove_file(&payload).unwrap();
                fs::create_dir(&payload).unwrap();
            }
            "invalid-lock" => fs::write(project.join("empack.lock"), b"invalid lock").unwrap(),
            _ => unreachable!(),
        }
        let before = snapshot(root.path());
        assert!(
            adopt_observed(&session(root.path(), true, false), adoption(&proposed))
                .await
                .is_err(),
            "{mode}"
        );
        assert_eq!(snapshot(root.path()), before, "{mode}");
        if mode == "directory" {
            fs::remove_dir(&payload).unwrap();
        }
        fs::write(&payload, b"payload").unwrap();
        if mode == "invalid-lock" {
            fs::remove_file(project.join("empack.lock")).unwrap();
        }
    }
}

#[tokio::test]
async fn selected_provider_changes_check_retained_inventory_before_publication() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path()).await;
    let mut initial = Server::new_async().await;
    records_with_dependencies(&mut initial, json!([])).await;
    let mut a = input("renderer", Some("a"));
    a.pin = None;
    let mut b = input("required-library", Some("b"));
    b.pin = None;
    add(
        temp.path(),
        &initial,
        vec![a.clone(), b.clone()],
        true,
        false,
    )
    .await
    .unwrap();
    let before = snapshot(&temp.path().join("project"));
    let mut next = Server::new_async().await;
    records_for_version(
        &mut next,
        "RootVer2",
        json!([{
            "project_id":"Need0001", "version_id":"NeedVer1", "dependency_type":"incompatible"
        }]),
    )
    .await;
    for updating in [true, false] {
        let error = if updating {
            update_mixed(
                temp.path(),
                &next,
                vec![AddHostInput::Provider(a.clone())],
                true,
                false,
            )
            .await
        } else {
            add(temp.path(), &next, vec![a.clone()], true, false).await
        }
        .unwrap_err();
        assert!(format!("{error:#}").contains("incompatib"), "{error:#}");
        assert_eq!(snapshot(&temp.path().join("project")), before);
    }
    // Replacing B1 with B2 in the same candidate satisfies the version-specific assertion.
    let mut jointly = Server::new_async().await;
    records_for_selections(
        &mut jointly,
        "RootVer2",
        "NeedVer2",
        json!([{
            "project_id":"Need0001", "version_id":"NeedVer1", "dependency_type":"incompatible"
        }]),
    )
    .await;
    update_mixed(
        temp.path(),
        &jointly,
        vec![
            AddHostInput::Provider(a.clone()),
            AddHostInput::Provider(b.clone()),
        ],
        true,
        false,
    )
    .await
    .unwrap();
    let project = read(&temp.path().join("project"));
    assert_eq!(
        project.lock().dependencies[&DependencyKey::parse("a").unwrap()]
            .incompatibilities
            .len(),
        1
    );
    let after = snapshot(&temp.path().join("project"));
    // The retained A2 assertion still rejects reverting only B to its prohibited B1.
    let error = update_mixed(
        temp.path(),
        &initial,
        vec![AddHostInput::Provider(b)],
        true,
        false,
    )
    .await
    .unwrap_err();
    assert!(format!("{error:#}").contains("incompatib"), "{error:#}");
    assert_eq!(snapshot(&temp.path().join("project")), after);
}

#[tokio::test]
async fn separated_conflicts_preserve_evidence_and_project_distinct_build_inventories() {
    use crate::application::process_runtime::Cancellation;
    use crate::engine::{
        build::acquisition::plan_target_build_acquisitions, content::SourceEvidencePolicy,
        project::ProjectReader, publication::RecoveryReader, snapshot::SnapshotLimits,
    };
    use empack_core::{distribution::Recipe, inventory::OptionalPolicy};
    for mode in ["selected", "batch"] {
        let temp = tempfile::tempdir().unwrap();
        fixture(temp.path()).await;
        let mut a = input("renderer", Some("a"));
        a.pin = None;
        a.requirements.server = Requirement::Unsupported;
        let mut b = input("required-library", Some("b"));
        b.pin = None;
        b.requirements.client = Requirement::Unsupported;
        if mode == "selected" {
            let mut initial = Server::new_async().await;
            records_with_dependencies(&mut initial, json!([])).await;
            add(
                temp.path(),
                &initial,
                vec![a.clone(), b.clone()],
                true,
                false,
            )
            .await
            .unwrap();
        }
        let mut next = Server::new_async().await;
        records_for_version(
            &mut next,
            "RootVer2",
            json!([{
                "project_id":"Need0001", "dependency_type":"incompatible"
            }]),
        )
        .await;
        if mode == "selected" {
            update_mixed(
                temp.path(),
                &next,
                vec![AddHostInput::Provider(a.clone())],
                true,
                false,
            )
            .await
            .unwrap();
        } else {
            add(temp.path(), &next, vec![a, b.clone()], true, false)
                .await
                .unwrap();
        }
        let root = temp.path().join("project");
        let project = read(&root);
        assert_eq!(
            project.lock().dependencies[&DependencyKey::parse("a").unwrap()]
                .incompatibilities
                .len(),
            1
        );
        let cancel = Cancellation::default();
        let workspace = ProjectReader::new(RecoveryReader::new(temp.path().join("state")))
            .capture_build(&root, &[], SnapshotLimits::default(), &cancel)
            .unwrap();
        for (recipe, expected) in [(Recipe::PRISM_BUNDLED, "a"), (Recipe::SERVER_BUNDLED, "b")] {
            let plan = plan_target_build_acquisitions(
                &workspace,
                &BuildAcquisitions::default(),
                recipe,
                &OptionalPolicy::Resolve {
                    choices: BTreeMap::new(),
                    use_defaults: true,
                },
                SourceEvidencePolicy::Compatibility,
                &cancel,
            )
            .unwrap();
            assert_eq!(plan.needs().len(), 1, "{mode}: {recipe:?}");
            let crate::engine::build::acquisition::AcquisitionKey::Locked(key) =
                &plan.needs()[0].key;
            assert_eq!(key.dependency.as_str(), expected);
        }
        // Retained A's assertion must reject making B participate on the client.
        let before = snapshot(&root);
        b.requirements.client = Requirement::Required;
        let error = update_mixed(
            temp.path(),
            &next,
            vec![AddHostInput::Provider(b)],
            true,
            false,
        )
        .await
        .unwrap_err();
        assert!(format!("{error:#}").contains("incompatib"), "{error:#}");
        assert_eq!(snapshot(&root), before);
    }
}

#[test]
fn provider_failure_advice_uses_typed_deadlines_and_keeps_original_causes() {
    use crate::engine::providers::CatalogError;
    for deadline in [true, false] {
        let cause = if deadline {
            CatalogError::Deadline
        } else {
            CatalogError::Unauthorized
        };
        let error = super::provider_failure(cause.into(), Duration::from_secs(17), false);
        let text = format!("{error:#}");
        assert_eq!(text.contains("--net-timeout"), deadline, "{text}");
        assert!(!text.contains("--continue-independent"));
        assert!(error.downcast_ref::<CatalogError>().is_some());
    }
    let error =
        super::provider_failure(CatalogError::NotFound.into(), Duration::from_secs(17), true);
    assert!(format!("{error:#}").contains("--continue-independent cannot skip a failed lookup"));
    assert!(matches!(
        error.downcast_ref::<CatalogError>(),
        Some(CatalogError::NotFound)
    ));
}

#[tokio::test]
async fn update_json_preserves_provider_observations_and_prepared_proposal_without_effects() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path()).await;
    let mut old = Server::new_async().await;
    records(&mut old).await;
    let mut selected = input("renderer", Some("alias"));
    selected.pin = None;
    add(root.path(), &old, vec![selected.clone()], true, false)
        .await
        .unwrap();
    let mut next = Server::new_async().await;
    next_records(&mut next).await;
    let before = snapshot(root.path());
    let (host, messages) = session(root.path(), true, false).record_status_messages();
    change_with_services(
        &host,
        NonEmpty::new(vec![AddHostInput::Provider(selected)]).unwrap(),
        ReleasePolicy::PreferStable,
        SourceEvidencePolicy::Compatibility,
        Change::Update {
            selection: crate::application::engine_host::dependencies::selection::UpdateSelection::Automatic,
            pins: Default::default(),
            mode: ApplicationMode::Preview,
            output: UpdateOutput {
                json: true,
                quiet: true,
                details: false,
            },
        },
        AdditionServices {
            catalog: ProviderCatalog::for_loopback_tests(&next.url(), None),
            transport: HttpAcquisition::for_loopback_tests(),
            files: DirectFileLimits::default(),
        },
        BatchPolicy::AllRequested,
    )
    .await
    .unwrap();
    let messages = messages.lock().unwrap();
    assert_eq!(
        messages.len(),
        1,
        "JSON must not be mixed with a human plan: {messages:?}"
    );
    let report: Value = serde_json::from_str(&messages[0]).unwrap();
    assert_eq!(report["proposal"], "ready");
    assert_eq!(report["requested"], json!(["alias"]));
    assert_eq!(
        report["discovery"]["dependencies"][0]["current"]["selection"],
        "RootVer1"
    );
    assert_eq!(
        report["discovery"]["dependencies"][0]["permitted"]["selection"],
        "RootVer2"
    );
    assert_eq!(report["discovery"]["required_dependencies"], "validated");
    assert_eq!(report["discovery"]["scope"]["roots"], json!(["alias"]));
    assert_eq!(report["discovery"]["scope"]["total_roots"], 1);
    assert_eq!(report["discovery"]["complete"], true);
    assert_eq!(report["discovery"]["summary"]["change_available"], 1);
    assert!(report["discovery"].get("candidate_compatibility").is_none());
    assert_eq!(report["changed_selections"], json!(["alias"]));
    assert!(!report["file_changes"].as_array().unwrap().is_empty());
    assert_eq!(
        snapshot(root.path()),
        before,
        "Even --yes must leave project and operation state unchanged"
    );
}

#[tokio::test]
async fn failed_update_json_retains_provider_field_evidence_before_failure() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path()).await;
    let mut old = Server::new_async().await;
    records(&mut old).await;
    let mut selected = input("renderer", Some("alias"));
    selected.pin = None;
    add(root.path(), &old, vec![selected.clone()], true, false)
        .await
        .unwrap();
    let mut failed = Server::new_async().await;
    failed.mock("GET", "/project/Root0001").with_body(json!({"id":"Root0001","slug":"renderer","title":"Renderer","project_type":"mod","loaders":["fabric"]}).to_string()).create_async().await;
    let mut malformed = version("Root0001", "RootVer2", json!([]));
    malformed["files"] = json!([]);
    failed
        .mock("GET", "/project/Root0001/version")
        .match_query(Matcher::Any)
        .with_body(json!([malformed]).to_string())
        .create_async()
        .await;
    let before = snapshot(root.path());
    let (host, messages) = session(root.path(), true, false).record_status_messages();
    let error = change_with_services(
        &host,
        NonEmpty::new(vec![AddHostInput::Provider(selected)]).unwrap(),
        ReleasePolicy::PreferStable,
        SourceEvidencePolicy::Compatibility,
        Change::Update {
            selection: crate::application::engine_host::dependencies::selection::UpdateSelection::Automatic,
            pins: Default::default(),
            mode: ApplicationMode::Preview,
            output: UpdateOutput {
                json: true,
                quiet: true,
                details: false,
            },
        },
        AdditionServices {
            catalog: ProviderCatalog::for_loopback_tests(&failed.url(), None),
            transport: HttpAcquisition::for_loopback_tests(),
            files: DirectFileLimits::default(),
        },
        BatchPolicy::AllRequested,
    )
    .await
    .unwrap_err();
    assert!(
        matches!(
            error.downcast_ref::<crate::engine::providers::CatalogError>(),
            Some(crate::engine::providers::CatalogError::InvalidRecord)
        ),
        "{error:#}"
    );
    let messages = messages.lock().unwrap();
    assert_eq!(messages.len(), 1);
    let report: Value = serde_json::from_str(&messages[0]).unwrap();
    assert_eq!(report["proposal"], "incomplete");
    assert_eq!(report["discovery"]["complete"], false);
    assert_eq!(report["discovery"]["summary"]["unknown"], 1);
    assert_eq!(report["discovery"]["scope"]["roots"], json!(["alias"]));
    assert_eq!(
        report["discovery"]["blocking_dependencies"],
        json!(["alias"])
    );
    assert_eq!(report["requested"], json!(["alias"]));
    let row = &report["discovery"]["dependencies"][0];
    assert_eq!(row["key"], "alias");
    assert_eq!(row["current"]["selection"], "RootVer1");
    assert_eq!(row["reasons"], json!(["invalid-evidence"]));
    assert_eq!(row["provider_problem"]["field"], "files");
    assert_eq!(row["provider_problem"]["invariant"], "non-empty");
    assert!(!row["catalog_complete"].as_bool().unwrap());
    assert_eq!(report["file_changes"], json!([]));
    assert_eq!(snapshot(root.path()), before);
}

#[tokio::test]
async fn update_json_distinguishes_known_required_conflict_from_missing_evidence() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path()).await;
    let mut old = Server::new_async().await;
    records(&mut old).await;
    let mut selected = input("renderer", Some("alias"));
    selected.pin = None;
    add(root.path(), &old, vec![selected.clone()], true, false)
        .await
        .unwrap();
    let mut next = Server::new_async().await;
    // Both requested versions exist, but the renderer requires a different exact
    // library version. This is a known unsatisfied relationship, not a failed lookup.
    records_for_selections(
        &mut next,
        "RootVer2",
        "NeedVer2",
        json!([{"project_id":"Need0001","version_id":"NeedVer1","dependency_type":"required"}]),
    )
    .await;
    let mut library = input("Need0001", Some("required-library"));
    library.pin = Some(PinSelector::ModrinthVersion(
        ModrinthVersionId::parse("NeedVer2").unwrap(),
    ));
    let before = snapshot(root.path());
    let (host, messages) = session(root.path(), true, false).record_status_messages();
    let result = change_with_services(
        &host,
        NonEmpty::new(vec![
            AddHostInput::Provider(selected),
            AddHostInput::Provider(library),
        ])
        .unwrap(),
        ReleasePolicy::PreferStable,
        SourceEvidencePolicy::Compatibility,
        Change::Update {
            selection: crate::application::engine_host::dependencies::selection::UpdateSelection::Automatic,
            pins: Default::default(),
            mode: ApplicationMode::Preview,
            output: UpdateOutput {
                json: true,
                quiet: true,
                details: false,
            },
        },
        AdditionServices {
            catalog: ProviderCatalog::for_loopback_tests(&next.url(), None),
            transport: HttpAcquisition::for_loopback_tests(),
            files: DirectFileLimits::default(),
        },
        BatchPolicy::AllRequested,
    )
    .await;
    let error = result.unwrap_err();
    let diagnostic = format!("{error:#}");
    assert!(diagnostic.contains("Selected dependency requirements cannot be satisfied"));
    assert!(diagnostic.contains("Root0001@RootVer2"));
    assert!(diagnostic.contains("Need0001@NeedVer2"));
    assert!(diagnostic.contains("Need0001@NeedVer1"));
    let messages = messages.lock().unwrap();
    assert_eq!(messages.len(), 1, "{messages:?}");
    let report: Value = serde_json::from_str(&messages[0]).unwrap();
    assert_eq!(
        report["discovery"]["reasons"],
        json!(["dependency-conflict"])
    );
    assert!(
        report["discovery"]["dependencies"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row["catalog_complete"] == true)
    );
    assert_eq!(report["proposal"], "blocked");
    assert_eq!(
        report["diagnostic"]["code"],
        "dependency-resolution-required"
    );
    assert_eq!(report["file_changes"], json!([]));
    assert_eq!(snapshot(root.path()), before);
}

#[tokio::test]
async fn interactive_update_uses_observed_versions_and_requires_separate_application() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path()).await;
    let mut old = Server::new_async().await;
    records(&mut old).await;
    let selected = input("renderer", Some("alias"));
    add(root.path(), &old, vec![selected.clone()], true, false)
        .await
        .unwrap();
    let mut next = Server::new_async().await;
    next.mock("GET", "/project/Root0001")
        .with_body(json!({"id":"Root0001","slug":"renderer","title":"renderer","project_type":"mod","loaders":["fabric"]}).to_string())
        .create_async().await;
    let mut installed = version("Root0001", "RootVer1", json!([]));
    installed["version_number"] = json!("1.0 stable");
    let mut beta = version("Root0001", "RootVer2", json!([]));
    beta["version_number"] = json!("2.0 beta 日本\nnot an action\u{202e}");
    beta["version_type"] = json!("beta");
    let mut other_runtime = version("Root0001", "OtherVer", json!([]));
    other_runtime["game_versions"] = json!(["1.20.1"]);
    let catalog = next
        .mock("GET", "/project/Root0001/version")
        .match_query(Matcher::Any)
        .with_body(json!([other_runtime, beta.clone(), installed]).to_string())
        .expect(2)
        .create_async()
        .await;
    next.mock("GET", "/version/RootVer2")
        .with_body(beta.to_string())
        .expect(2)
        .create_async()
        .await;
    let before = snapshot(root.path());
    for mode in [ApplicationMode::Preview, ApplicationMode::Apply] {
        let (host, messages) = session(root.path(), false, false)
            .with_interactive(
                MockInteractiveProvider::new()
                    .with_fuzzy_select(1)
                    .with_confirm(true),
            )
            .record_status_messages();
        change_with_services(
            &host,
            NonEmpty::new(vec![AddHostInput::Provider(selected.clone())]).unwrap(),
            ReleasePolicy::StableOnly,
            SourceEvidencePolicy::Compatibility,
            Change::Update {
                selection: UpdateSelection::Interactive,
                pins: Default::default(),
                mode,
                output: UpdateOutput::default(),
            },
            AdditionServices {
                catalog: ProviderCatalog::for_loopback_tests(&next.url(), None),
                transport: HttpAcquisition::for_loopback_tests(),
                files: DirectFileLimits::default(),
            },
            BatchPolicy::AllRequested,
        )
        .await
        .unwrap();
        let choices = host
            .interactive_provider
            .fuzzy_select_options
            .lock()
            .unwrap();
        assert_eq!(choices.len(), 1);
        assert_eq!(choices[0].len(), 2);
        assert!(
            choices[0][0].contains("1.0 stable (release) [modrinth:Root0001@RootVer1] — installed")
        );
        assert!(choices[0][1].contains(
            "2.0 beta 日本\\nnot an action\\u{202e} (beta) [modrinth:Root0001@RootVer2]"
        ));
        assert!(!choices[0][1].contains('\n') && !choices[0][1].contains('\u{202e}'));
        let messages = messages.lock().unwrap();
        assert!(
            messages
                .iter()
                .any(|message| message.contains("Pin alias to RootVer2")),
            "{messages:?}"
        );
        assert!(
            messages
                .iter()
                .any(|message| message.starts_with("Repeat this selection")
                    && message.contains("update alias --pin 'alias=RootVer2'")),
            "{messages:?}"
        );
        if mode == ApplicationMode::Preview {
            assert_eq!(
                snapshot(root.path()),
                before,
                "A version choice is not approval to apply"
            );
        }
    }
    catalog.assert_async().await;
    let after = read(&root.path().join("project"));
    let key = DependencyKey::parse("alias").unwrap();
    let pin = PinSelector::ModrinthVersion(ModrinthVersionId::parse("RootVer2").unwrap());
    assert_eq!(
        after.intent().roots[&key].selection,
        DependencySelection::ProviderPin(pin.clone())
    );
    assert_eq!(
        after.lock().dependencies[&key]
            .selected
            .as_ref()
            .unwrap()
            .selection,
        pin
    );
    assert_eq!(after.lock().dependencies.len(), 2);
    assert_eq!(
        after.lock().dependencies[&DependencyKey::parse("required-library").unwrap()]
            .selected
            .as_ref()
            .unwrap()
            .selection,
        PinSelector::ModrinthVersion(ModrinthVersionId::parse("NeedVer1").unwrap())
    );
    assert_eq!(
        after.intent().roots.len(),
        1,
        "Retained libraries must not become authored roots"
    );
}

#[tokio::test]
async fn interactive_update_cancellation_preserves_author_and_host_state() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path()).await;
    let mut server = Server::new_async().await;
    records(&mut server).await;
    let selected = input("renderer", Some("alias"));
    add(root.path(), &server, vec![selected.clone()], true, false)
        .await
        .unwrap();
    let before = snapshot(root.path());
    let host = session(root.path(), false, false)
        .with_interactive(MockInteractiveProvider::new().queue_fuzzy_select(None));
    let error = change_with_services(
        &host,
        NonEmpty::new(vec![AddHostInput::Provider(selected)]).unwrap(),
        None,
        SourceEvidencePolicy::Compatibility,
        Change::Update {
            selection: UpdateSelection::Interactive,
            pins: Default::default(),
            mode: ApplicationMode::Apply,
            output: UpdateOutput::default(),
        },
        AdditionServices {
            catalog: ProviderCatalog::for_loopback_tests(&server.url(), None),
            transport: HttpAcquisition::for_loopback_tests(),
            files: DirectFileLimits::default(),
        },
        BatchPolicy::AllRequested,
    )
    .await
    .unwrap_err();
    assert_eq!(
        crate::application::exit::classify_error(&error),
        crate::application::exit::EmpackExitCode::Interrupted
    );
    assert_eq!(snapshot(root.path()), before);
}
