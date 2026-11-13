use super::*;
use crate::application::process_runtime::Cancellation;
use crate::application::{
    InitArgs,
    session_mocks::{MockCommandSession, MockConfigProvider, MockInvocationProvider},
};
use crate::engine::documents::DocumentCodec;
use crate::test_support::file_bytes as snapshot;
use std::{collections::BTreeMap, fs, io::Read};

fn session(root: &Path, yes: bool, dry_run: bool) -> MockCommandSession {
    MockCommandSession::new()
        .with_invocation(MockInvocationProvider::new().with_current_dir(root.to_path_buf()))
        .with_config(MockConfigProvider::new(AppConfig {
            workdir: Some("project".into()),
            state_dir: Some("state".into()),
            cache_dir: Some("cache".into()),
            yes,
            dry_run,
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
            modloader: Some("none".into()),
            pack_name: Some("Native Pack".into()),
            pack_version: Some("1.0".into()),
            author: Some("Tester".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    fs::create_dir_all(root.join("project/pack/config")).unwrap();
    fs::write(root.join("project/pack/config/example.txt"), b"first").unwrap();
    fs::write(root.join("project/notes.txt"), b"outside managed changes").unwrap();
}
fn args() -> BuildArgs {
    BuildArgs {
        targets: vec!["modrinth".into(), "prism".into()],
        ..Default::default()
    }
}
async fn run(root: &Path, args: &BuildArgs, yes: bool, dry: bool) -> Result<()> {
    build(
        &session(root, yes, dry),
        args,
        BuildDecisions::default(),
        BuildAcquisitions::default(),
    )
    .await
}
fn zip_bytes(path: &Path, member: &str) -> Vec<u8> {
    let mut archive = zip::ZipArchive::new(fs::File::open(path).unwrap()).unwrap();
    let mut bytes = vec![];
    archive
        .by_name(member)
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    bytes
}
#[tokio::test]
async fn native_build_host_preserves_preview_decline_and_publishes_current_bytes() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path()).await;
    // This fixture deliberately retains an authored bundled profile across the default change.
    let project = root.path().join("project");
    let decoded = crate::testing::read_authoring(&project.join("empack.yml")).unwrap();
    let resolved = DocumentCodec
        .decode_lock(
            &fs::read(project.join("empack.lock")).unwrap(),
            &decoded,
            "test",
        )
        .unwrap();
    let mut intent = resolved.intent().clone();
    intent
        .distribution
        .profiles
        .get_mut("prism")
        .unwrap()
        .recipe = Recipe::PRISM_BUNDLED;
    crate::testing::write_authoring(&project.join("empack.yml"), &intent).unwrap();
    let revision = crate::testing::read_authoring(&project.join("empack.yml"))
        .unwrap()
        .semantic_revision();
    let mut lock = resolved.lock().clone();
    lock.intent_revision = revision;
    let resolved = empack_core::model::ResolvedProject::validate(intent, lock, revision).unwrap();
    fs::write(
        project.join("empack.lock"),
        DocumentCodec.encode_lock(&resolved).unwrap(),
    )
    .unwrap();
    let before = snapshot(root.path());
    for (yes, dry) in [(true, true), (false, false)] {
        let (host, messages) = session(root.path(), yes, dry).record_messages();
        build(
            &host,
            &args(),
            BuildDecisions::default(),
            BuildAcquisitions::default(),
        )
        .await
        .unwrap();
        let captured = messages.lock().unwrap().join("\n");
        assert!(captured.contains("Build plan: Minecraft 1.21.1 (Vanilla)"));
        assert!(captured.contains("dist/Native Pack-1.0.mrpack"));
        assert!(captured.contains("Prism (client), bundled files, fixed snapshot"));
        assert!(!captured.contains("Requested optional content:"));
        assert!(!captured.contains("Recipe {"));
        assert_eq!(snapshot(root.path()), before);
    }
    run(root.path(), &args(), true, false).await.unwrap();
    let project = root.path().join("project");
    let mrpack = project.join("dist/Native Pack-1.0.mrpack");
    let client = project.join("dist/Native Pack-1.0-prism-bundled.zip");
    assert_eq!(zip_bytes(&mrpack, "overrides/config/example.txt"), b"first");
    assert_eq!(
        zip_bytes(&client, ".minecraft/config/example.txt"),
        b"first"
    );
    fs::write(project.join("pack/config/example.txt"), b"current bytes").unwrap();
    fs::write(project.join("dist/obsolete.zip"), b"old artifact").unwrap();
    let mut options = args();
    options.clean = true;
    let before = snapshot(root.path());
    run(root.path(), &options, true, true).await.unwrap();
    assert_eq!(snapshot(root.path()), before);
    run(root.path(), &options, true, false).await.unwrap();
    assert!(!project.join("dist/obsolete.zip").exists());
    assert_eq!(
        zip_bytes(&mrpack, "overrides/config/example.txt"),
        b"current bytes"
    );
    assert_eq!(
        zip_bytes(&client, ".minecraft/config/example.txt"),
        b"current bytes"
    );
    assert_eq!(
        fs::read(project.join("notes.txt")).unwrap(),
        b"outside managed changes"
    );
    assert_eq!(
        fs::read(project.join("empack.yml")).unwrap(),
        before[Path::new("project/empack.yml")]
    );
    assert_eq!(
        fs::read(project.join("empack.lock")).unwrap(),
        before[Path::new("project/empack.lock")]
    );
}
#[tokio::test]
async fn native_clean_build_keeps_all_prior_artifacts_when_one_recipe_fails() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path()).await;
    run(root.path(), &args(), true, false).await.unwrap();
    let project = root.path().join("project");
    fs::write(project.join("dist/obsolete.zip"), b"retain me").unwrap();
    fs::write(
        project.join("templates/client/instance.cfg.template"),
        b"{{missing_required_value}}",
    )
    .unwrap();
    let before = snapshot(&project);
    let mut options = args();
    options.clean = true;
    assert!(run(root.path(), &options, true, false).await.is_err());
    assert_eq!(snapshot(&project), before);
}
#[tokio::test]
async fn native_build_rejects_unsupported_flags_and_invalid_targets_without_effects() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path()).await;
    let before = snapshot(root.path());
    for variant in ["continue", "associate", "unknown-after-all"] {
        let mut options = args();
        match variant {
            "continue" => options.continue_build = true,
            "associate" => options.associate_downloads.push("a=b".into()),
            _ => options.targets = vec!["all".into(), "typo".into()],
        }
        assert!(
            run(root.path(), &options, true, false).await.is_err(),
            "{variant}"
        );
        assert_eq!(snapshot(root.path()), before, "{variant}");
    }
}
#[tokio::test]
async fn native_build_target_mapping_retains_archives_defaults_and_explicit_decisions() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path()).await;
    let project = root.path().join("project");
    let captured = ProjectReader::new(RecoveryReader::new(root.path().join("state")))
        .capture(
            &project,
            &[],
            SnapshotLimits::default(),
            &crate::application::process_runtime::Cancellation::default(),
        )
        .unwrap();
    let intent = captured.require_resolved().unwrap();
    let choices = BuildDecisions {
        optional: OptionalPolicy::Resolve {
            choices: BTreeMap::from([("extra".into(), false)]),
            use_defaults: true,
        },
        mrpack_optional: OptionalConversion::AcknowledgedMetadataLoss,
        evidence: SourceEvidencePolicy::StrongSourceRequired,
        ..Default::default()
    };
    for (format, extension, archive) in [
        (CliArchiveFormat::Zip, "zip", DistributionArchive::Zip),
        (
            CliArchiveFormat::TarGz,
            "tar.gz",
            DistributionArchive::TarGz,
        ),
        (
            CliArchiveFormat::SevenZ,
            "7z",
            DistributionArchive::SevenZip,
        ),
    ] {
        let options = BuildArgs {
            format: Some(format),
            targets: vec!["prism".into(), "prism".into(), "modrinth".into()],
            ..Default::default()
        };
        let built = request(intent.intent(), &options, choices.clone()).unwrap();
        assert_eq!(built.outputs.as_slice().len(), 2);
        assert_eq!(
            built.outputs.as_slice()[0].artifact.as_str(),
            format!("Native Pack-1.0-prism-references.{extension}")
        );
        assert_eq!(
            built.outputs.as_slice()[1].artifact.as_str(),
            "Native Pack-1.0.mrpack"
        );
        assert_eq!(built.outputs.as_slice()[0].archive, archive);
        assert_eq!(
            built.outputs.as_slice()[1].archive,
            DistributionArchive::Zip
        );
        assert_eq!(built.optional, choices.optional);
        assert_eq!(built.mrpack_optional, choices.mrpack_optional);
        assert_eq!(built.evidence, choices.evidence);
    }
    assert_eq!(
        request(
            intent.intent(),
            &BuildArgs::default(),
            BuildDecisions::default()
        )
        .unwrap()
        .outputs
        .as_slice()
        .len(),
        3
    );
    let mut selected = intent.intent().clone();
    for profile in selected.distribution.profiles.values_mut() {
        profile.archive = DistributionArchive::TarGz;
    }
    let default = request(&selected, &args(), BuildDecisions::default()).unwrap();
    assert_eq!(
        default.outputs.as_slice()[1].archive,
        DistributionArchive::TarGz
    );
    assert!(
        default.outputs.as_slice()[1]
            .artifact
            .as_str()
            .ends_with(".tar.gz")
    );
    let explicit = BuildArgs {
        format: Some(CliArchiveFormat::Zip),
        ..args()
    };
    assert_eq!(
        request(&selected, &explicit, BuildDecisions::default())
            .unwrap()
            .outputs
            .as_slice()[1]
            .archive,
        DistributionArchive::Zip
    );
    let mut unsafe_intent = intent.intent().clone();
    for name in ["../escape", "a/b", "C:drive", "a\\b"] {
        unsafe_intent.metadata.name = name.into();
        assert!(request(&unsafe_intent, &args(), BuildDecisions::default()).is_err());
    }
}

#[tokio::test]
async fn running_build_cancellation_retires_acquisition_without_releasing_other_preparations() {
    use crate::application::{process_runtime::Interrupted, session::LiveProcessProvider};
    use crate::engine::acquisition::HttpAcquisition;
    use crate::testing::http::StalledHttp;
    use empack_core::{
        digest::{DigestSet, ExpectedDigest},
        model::*,
        path::InstallDestination,
        requirements::{Requirement, Requirements},
    };

    let root = tempfile::tempdir().unwrap();
    fixture(root.path()).await;
    let project = root.path().join("project");
    let mut server =
        StalledHttp::new(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\npayl\r\n");
    let author = crate::testing::read_authoring(&project.join("empack.yml")).unwrap();
    let resolved = DocumentCodec
        .decode_lock(
            &fs::read(project.join("empack.lock")).unwrap(),
            &author,
            "budget fixture",
        )
        .unwrap();
    let mut intent = resolved.intent().clone();
    let mut lock = resolved.lock().clone();
    let key = DependencyKey::parse("stalled-assets").unwrap();
    let requirements = Requirements {
        client: Requirement::Required,
        server: Requirement::Unsupported,
    };
    let placement = Placement {
        destination: InstallDestination::parse("config/stalled-assets.txt").unwrap(),
        layer: ContentLayer::Common,
        requirements: requirements.clone(),
    };
    let downloads = NonEmpty::new(vec!["https://budget.test/file".into()]).unwrap();
    // Literal SHA-256 of b"payload"; this response never supplies complete bytes.
    let digests = DigestSet::new(vec![
        ExpectedDigest::parse(
            "sha256",
            "239f59ed55e737c77147cf55ad0c1b030b6d7ee748a7426952f9b852d5a935e5",
        )
        .unwrap(),
    ])
    .unwrap();
    intent.roots.insert(
        key.clone(),
        DependencyIntent {
            releases: None,
            source: SourceIntent::Url(downloads.clone()),
            kind: ContentKind::OtherFile,
            selection: DependencySelection::ContentAssertions(digests.clone()),
            placement: PlacementIntent::Explicit(NonEmpty::new(vec![placement.clone()]).unwrap()),
            requirements,
        },
    );
    lock.dependencies.insert(
        key.clone(),
        LockedDependency {
            incompatibilities: vec![],
            source_selector: None,
            title: "Stalled assets".into(),
            kind: ContentKind::OtherFile,
            identity: ResolvedIdentity::Url(key.clone()),
            selected: None,
            files: NonEmpty::new(vec![ResolvedFile {
                slot: FileSlot::parse("main").unwrap(),
                acquisition: AcquisitionSpec::Url(downloads),
                expected: ExpectedContent {
                    digests: Some(digests.clone()),
                    size: Some(7),
                    accepted_observation: None,
                },
                provenance: Provenance {
                    source: "budget fixture".into(),
                    location: None,
                    declared_digests: Some(digests),
                    conversions: vec![],
                },
                placements: NonEmpty::new(vec![placement]).unwrap(),
            }])
            .unwrap(),
        },
    );
    lock.coverage.insert(key, Coverage::Unknown);
    let author = DocumentCodec.encode_intent(&intent).unwrap();
    let revision = DocumentCodec
        .decode_intent(&author, "budget fixture")
        .unwrap()
        .semantic_revision();
    lock.intent_revision = revision;
    let resolved = ResolvedProject::validate(intent, lock, revision).unwrap();
    fs::write(project.join("empack.yml"), author).unwrap();
    fs::write(
        project.join("empack.lock"),
        DocumentCodec.encode_lock(&resolved).unwrap(),
    )
    .unwrap();
    fs::create_dir_all(project.join("dist")).unwrap();
    fs::write(project.join("dist/client.zip"), b"previous artifact").unwrap();
    fs::write(project.join("dist/neighbor.zip"), b"unrelated artifact").unwrap();
    fs::create_dir_all(project.join("worlds/playtest")).unwrap();
    fs::write(project.join("worlds/playtest/level.dat"), b"player bytes").unwrap();
    fs::create_dir(project.join("empty author directory")).unwrap();
    fs::write(root.path().join("neighbor.txt"), b"host neighbor").unwrap();
    let readonly_note = project.join("notes.txt");
    let original_note_permissions = fs::metadata(&readonly_note).unwrap().permissions();
    let mut note_permissions = original_note_permissions.clone();
    note_permissions.set_readonly(true);
    fs::set_permissions(&readonly_note, note_permissions).unwrap();
    let before = snapshot(root.path());
    let permissions: BTreeMap<_, _> = before
        .keys()
        .map(|path| {
            (
                path.clone(),
                fs::metadata(root.path().join(path))
                    .unwrap()
                    .permissions()
                    .readonly(),
            )
        })
        .collect();
    let cancellation = Cancellation::default();
    let mut host = session(root.path(), true, false);
    host.process_provider = LiveProcessProvider::new().with_cancellation(cancellation.clone());
    let governor = ResourceGovernor::new(ResourceRequest {
        jobs: 4,
        memory_bytes: 256 << 20,
        scratch_bytes: 1 << 30,
        open_files: 1024,
    });
    let engine = engine_with_governor(host.config().app_config(), root.path(), governor.clone())
        .unwrap()
        .with_test_transport(
            HttpAcquisition::for_loopback_tests()
                .with_test_origin("https://budget.test", &server.url),
        );
    let request = BuildRequest {
        clean: false,
        outputs: NonEmpty::new(vec![BuildOutput {
            archive: DistributionArchive::Zip,
            target: Recipe::PRISM_BUNDLED,
            artifact: PortableRelPath::parse("client.zip", PathSyntax::ArtifactName).unwrap(),
        }])
        .unwrap(),
        optional: OptionalPolicy::Preserve,
        mrpack_optional: OptionalConversion::RejectMetadataLoss,
        templates: TemplateOptions::default(),
        evidence: SourceEvidencePolicy::StrongSourceRequired,
    };
    let Preparation::Ready(other) = engine
        .prepare(project.clone(), request.clone())
        .await
        .unwrap()
    else {
        panic!("declared URL content must be available for approved acquisition");
    };
    let retained = governor.status().reserved;
    assert_eq!(retained.jobs, 0);
    assert!(retained.memory_bytes > 0);
    assert!(retained.open_files > 0);
    let Preparation::Ready(prepared) = engine
        .prepare(project.clone(), request.clone())
        .await
        .unwrap()
    else {
        panic!("second preparation must share the same budget");
    };
    assert!(prepared.view().needs_network());
    let observe = async {
        server.started().await;
        let active = governor.status().reserved;
        assert!(active.jobs > 0, "the acquisition must own running work");
        assert!(active.memory_bytes > retained.memory_bytes);
        cancellation.cancel();
    };
    let (result, ()) = tokio::time::timeout(Duration::from_secs(4), async {
        tokio::join!(
            execute_approved(&host, &engine, prepared, "Build", |_| {
                panic!("partial upstream content cannot produce a receipt")
            }),
            observe
        )
    })
    .await
    .expect("the host must await cancellation and retirement of active acquisition");
    let error = result.unwrap_err();
    assert!(error.is::<Interrupted>(), "{error:#}");
    assert_eq!(format!("{error:#}"), "Operation interrupted");
    assert_eq!(
        crate::application::classify_error(&error),
        crate::application::EmpackExitCode::Interrupted
    );
    server.assert_closed();
    assert_eq!(governor.status().reserved, retained);
    assert_eq!(snapshot(root.path()), before);
    for (path, readonly) in permissions {
        assert_eq!(
            fs::metadata(root.path().join(path))
                .unwrap()
                .permissions()
                .readonly(),
            readonly
        );
    }
    assert!(project.join("empty author directory").is_dir());
    // Keep the returned error alive while the unrelated preparation and registry
    // are released. Cancellation must not close the shared engine to later work.
    drop(other);
    assert_eq!(governor.status().reserved, ResourceRequest::default());
    let Preparation::Ready(followup) = engine.prepare(project, request).await.unwrap() else {
        panic!("released capacity must permit a new preparation");
    };
    drop(followup);
    assert_eq!(governor.status().reserved, ResourceRequest::default());
    engine.shutdown().await;
    fs::set_permissions(readonly_note, original_note_permissions).unwrap();
}

#[tokio::test]
async fn native_build_missing_content_is_read_only_and_exact_supplied_bytes_complete_it() {
    use crate::application::process_runtime::Cancellation;
    use crate::engine::{
        content::{InitialObservation, verify_stream},
        documents::DocumentCodec,
        mrpack::{AcquiredBuildFile, LockedFileKey},
    };
    use empack_core::{
        files::FilePermissions,
        model::*,
        path::InstallDestination,
        requirements::{Requirement, Requirements},
    };
    let root = tempfile::tempdir().unwrap();
    fixture(root.path()).await;
    let project = root.path().join("project");
    let decoded = crate::testing::read_authoring(&project.join("empack.yml")).unwrap();
    let resolved = DocumentCodec
        .decode_lock(
            &fs::read(project.join("empack.lock")).unwrap(),
            &decoded,
            "test",
        )
        .unwrap();
    let mut intent = resolved.intent().clone();
    let mut lock = resolved.lock().clone();
    let key = DependencyKey::parse("manual-assets").unwrap();
    let identity = empack_core::identity::ProviderProjectId::Modrinth(
        empack_core::identity::ModrinthProjectId::parse("Assets01").unwrap(),
    );
    let pin = ResolvedPin {
        project: identity.clone(),
        selection: identity.parse_pin("Version1").unwrap(),
    };
    let slot = FileSlot::parse("main").unwrap();
    let requirements = Requirements {
        client: Requirement::Required,
        server: Requirement::Unsupported,
    };
    let placement = Placement {
        destination: InstallDestination::parse("resourcepacks/custom.zip").unwrap(),
        layer: ContentLayer::Common,
        requirements: requirements.clone(),
    };
    let content = verify_stream(
        &mut &b"verified manual bytes"[..],
        &ExpectedContent {
            digests: None,
            size: None,
            accepted_observation: None,
        },
        100,
        SourceEvidencePolicy::Compatibility,
        InitialObservation::Accepted,
        &Cancellation::default(),
    )
    .unwrap();
    let expected = ExpectedContent {
        digests: Some(content.observed_digests().clone()),
        size: Some(21),
        accepted_observation: None,
    };
    intent.roots.insert(
        key.clone(),
        DependencyIntent {
            releases: None,
            source: SourceIntent::Provider(identity.clone()),
            kind: ContentKind::ResourcePack,
            selection: DependencySelection::Automatic,
            placement: PlacementIntent::Explicit(NonEmpty::new(vec![placement.clone()]).unwrap()),
            requirements,
        },
    );
    lock.dependencies.insert(
        key.clone(),
        LockedDependency {
            incompatibilities: Vec::new(),
            source_selector: None,
            title: "Manual assets".into(),
            kind: ContentKind::ResourcePack,
            identity: ResolvedIdentity::Provider(identity),
            selected: Some(pin.clone()),
            files: NonEmpty::new(vec![ResolvedFile {
                slot: slot.clone(),
                acquisition: AcquisitionSpec::Manual {
                    pin: Some(pin),
                    instructions: "Supply exact bytes".into(),
                },
                expected: expected.clone(),
                provenance: Provenance {
                    source: "test".into(),
                    location: None,
                    declared_digests: expected.digests,
                    conversions: vec![],
                },
                placements: NonEmpty::new(vec![placement]).unwrap(),
            }])
            .unwrap(),
        },
    );
    lock.coverage
        .insert(key.clone(), Coverage::CompleteForSelection);
    let raw = DocumentCodec.encode_intent(&intent).unwrap();
    let revision = DocumentCodec
        .decode_intent(&raw, "test")
        .unwrap()
        .semantic_revision();
    lock.intent_revision = revision;
    let resolved = ResolvedProject::validate(intent, lock, revision).unwrap();
    fs::write(project.join("empack.yml"), raw).unwrap();
    fs::write(
        project.join("empack.lock"),
        DocumentCodec.encode_lock(&resolved).unwrap(),
    )
    .unwrap();
    let options = BuildArgs {
        targets: vec!["prism".into()],
        delivery: Some("bundled".into()),
        ..Default::default()
    };
    let before = snapshot(root.path());
    let project_before = snapshot(&project);
    run(root.path(), &options, true, true).await.unwrap();
    assert_eq!(snapshot(root.path()), before);
    let error = run(root.path(), &options, true, false).await.unwrap_err();
    assert!(
        error
            .to_string()
            .contains("supply the displayed missing content"),
        "{error:#}"
    );
    assert_eq!(snapshot(&project), project_before);
    assert!(root.path().join("state/pending-builds").is_dir());
    let original_intent = fs::read(project.join("empack.yml")).unwrap();
    fs::write(project.join("empack.yml"), b"invalid changed document: [").unwrap();
    let stale = snapshot(root.path());
    for dry in [true, false] {
        let error = continue_build(
            &session(root.path(), true, dry),
            &BuildArgs {
                continue_build: true,
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("inputs changed"));
        assert_eq!(snapshot(root.path()), stale);
    }
    fs::write(project.join("empack.yml"), original_intent).unwrap();
    fs::create_dir(root.path().join("downloads")).unwrap();
    fs::write(
        root.path().join("downloads/renamed.bin"),
        b"verified manual bytes",
    )
    .unwrap();
    fs::write(
        root.path().join("downloads/custom.zip"),
        b"unrelated download",
    )
    .unwrap();
    let discovered = BuildArgs {
        downloads_dir: Some("downloads".into()),
        ..options.clone()
    };
    let before_discovery = snapshot(root.path());
    for (yes, dry) in [(true, true), (false, false)] {
        run(root.path(), &discovered, yes, dry).await.unwrap();
        assert_eq!(snapshot(root.path()), before_discovery);
    }
    let continuing = BuildArgs {
        continue_build: true,
        downloads_dir: Some("downloads".into()),
        ..Default::default()
    };
    for (yes, dry) in [(true, true), (false, false)] {
        continue_build(&session(root.path(), yes, dry), &continuing)
            .await
            .unwrap();
        assert_eq!(snapshot(root.path()), before_discovery);
    }
    for values in [
        vec!["custom.zip=downloads/custom.zip".into()],
        vec!["unknown=downloads/renamed.bin".into()],
        vec![
            "custom.zip=downloads/renamed.bin".into(),
            "custom.zip=downloads/renamed.bin".into(),
        ],
    ] {
        let invalid = BuildArgs {
            associate_downloads: values,
            ..continuing.clone()
        };
        assert!(
            continue_build(&session(root.path(), true, false), &invalid)
                .await
                .is_err()
        );
        assert_eq!(snapshot(root.path()), before_discovery);
    }
    let selected_key = AcquisitionKey::Locked(LockedFileKey {
        dependency: key.clone(),
        slot: slot.clone(),
    });
    let explicit = BuildArgs {
        associate_downloads: vec![format!(
            "{}=downloads/renamed.bin",
            input_selector(&selected_key)
        )],
        downloads_dir: None,
        ..continuing.clone()
    };
    let shared =
        crate::engine::pending::inspect(&root.path().join("state"), &project, &Default::default())
            .unwrap();
    let operation = &shared.operations[0];
    let saved_tree = snapshot(root.path());
    let wrong_kind = crate::application::engine_host::pending::dispatch(
        &session(root.path(), true, false),
        crate::application::cli::PendingCommand::Resume {
            id: operation.id.clone(),
            files: vec![],
            downloads_dir: Some("nonexistent-downloads".into()),
            wait_downloads: None,
            open_downloads: true,
            import_options: Box::new(crate::application::cli::PendingImportOptions {
                java: Some(21),
                ..Default::default()
            }),
        },
    )
    .await
    .unwrap_err();
    assert_eq!(
        crate::application::classify_error(&wrong_kind),
        crate::application::EmpackExitCode::Usage
    );
    assert_eq!(snapshot(root.path()), saved_tree);
    crate::application::engine_host::pending::dispatch(
        &session(root.path(), true, true),
        crate::application::cli::PendingCommand::Resume {
            id: operation.id.clone(),
            files: vec![],
            downloads_dir: Some("nonexistent-downloads".into()),
            wait_downloads: Some(60),
            open_downloads: true,
            import_options: Default::default(),
        },
    )
    .await
    .unwrap();
    assert_eq!(snapshot(root.path()), saved_tree);
    assert!(
        continue_build_selected(
            &session(root.path(), true, false),
            &continuing,
            Some(&"0".repeat(64))
        )
        .await
        .unwrap_err()
        .to_string()
        .contains("changed")
    );
    assert_eq!(snapshot(root.path()), saved_tree);

    assert!(
        operation
            .inputs
            .iter()
            .any(|input| input.key == input_selector(&selected_key))
    );
    crate::application::engine_host::pending::dispatch(
        &session(root.path(), true, false),
        crate::application::cli::PendingCommand::Resume {
            import_options: Default::default(),
            id: operation.id[..12].into(),
            files: explicit.associate_downloads.clone(),
            downloads_dir: None,
            wait_downloads: None,
            open_downloads: false,
        },
    )
    .await
    .unwrap();
    assert!(
        !fs::read_dir(root.path().join("state/pending-builds"))
            .unwrap()
            .any(|entry| entry
                .unwrap()
                .path()
                .extension()
                .is_some_and(|extension| extension == "json"))
    );
    let completed = snapshot(root.path());
    assert!(
        continue_build(&session(root.path(), true, false), &continuing)
            .await
            .is_err()
    );
    assert_eq!(snapshot(root.path()), completed);
    run(root.path(), &discovered, true, false).await.unwrap();
    assert_eq!(
        zip_bytes(
            &project.join("dist/Native Pack-1.0-prism-bundled.zip"),
            ".minecraft/resourcepacks/custom.zip"
        ),
        b"verified manual bytes"
    );
    // Restore an unresolved-content case after the approved build populated its cache.
    fs::remove_dir_all(root.path().join("cache")).unwrap();
    let after_discovery = snapshot(root.path());
    let missing_root = BuildArgs {
        downloads_dir: Some("missing".into()),
        ..options.clone()
    };
    assert!(run(root.path(), &missing_root, true, false).await.is_err());
    assert_eq!(snapshot(root.path()), after_discovery);
    fs::write(root.path().join("manual.bin"), b"verified manual bytes").unwrap();
    let files = || {
        BTreeMap::from([(
            AcquisitionKey::Locked(LockedFileKey {
                dependency: key.clone(),
                slot: slot.clone(),
            }),
            PathBuf::from("manual.bin"),
        )])
    };
    let before = snapshot(root.path());
    for (yes, dry) in [(true, true), (false, false)] {
        build_with_local_files(
            &session(root.path(), yes, dry),
            &options,
            BuildDecisions::default(),
            files(),
        )
        .await
        .unwrap();
        assert_eq!(snapshot(root.path()), before);
    }
    build_with_local_files(
        &session(root.path(), true, false),
        &options,
        BuildDecisions::default(),
        files(),
    )
    .await
    .unwrap();
    assert_eq!(
        zip_bytes(
            &project.join("dist/Native Pack-1.0-prism-bundled.zip"),
            ".minecraft/resourcepacks/custom.zip"
        ),
        b"verified manual bytes"
    );
    build(
        &session(root.path(), true, false),
        &options,
        BuildDecisions::default(),
        BuildAcquisitions {
            locked: BTreeMap::from([(
                LockedFileKey {
                    dependency: key,
                    slot,
                },
                AcquiredBuildFile {
                    content,
                    permissions: FilePermissions {
                        readonly: false,
                        executable: false,
                    },
                },
            )]),
        },
    )
    .await
    .unwrap();
    assert_eq!(
        zip_bytes(
            &project.join("dist/Native Pack-1.0-prism-bundled.zip"),
            ".minecraft/resourcepacks/custom.zip"
        ),
        b"verified manual bytes"
    );
}

#[tokio::test]
async fn derived_build_choices_reject_changed_intent_before_preparation() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path()).await;
    let project = root.path().join("project");
    let host = session(root.path(), true, false);
    let original = crate::testing::read_authoring(&project.join("empack.yml")).unwrap();
    let current = DocumentCodec
        .decode_lock(
            &fs::read(project.join("empack.lock")).unwrap(),
            &original,
            "original",
        )
        .unwrap();
    let old_request = request(current.intent(), &args(), BuildDecisions::default())
        .unwrap()
        .with_content(BuildAcquisitions::default())
        .require_intent(original.semantic_revision());
    let mut changed = current.intent().clone();
    changed.metadata.name = "Changed Pack".into();
    for profile in changed.distribution.profiles.values_mut() {
        profile.archive = DistributionArchive::TarGz;
    }
    let raw = DocumentCodec.encode_intent(&changed).unwrap();
    let revision = DocumentCodec
        .decode_intent(&raw, "changed")
        .unwrap()
        .semantic_revision();
    let mut lock = current.lock().clone();
    lock.intent_revision = revision;
    let updated = empack_core::model::ResolvedProject::validate(changed, lock, revision).unwrap();
    fs::write(project.join("empack.yml"), raw).unwrap();
    fs::write(
        project.join("empack.lock"),
        DocumentCodec.encode_lock(&updated).unwrap(),
    )
    .unwrap();
    let before = snapshot(root.path());
    let owner = engine(&host.config_provider.app_config, root.path()).unwrap();
    let result = build_with_engine(&host, &owner, project, old_request, None, None, false).await;
    owner.shutdown().await;
    assert!(
        result.is_err(),
        "Old host choices must not build the new project"
    );
    assert_eq!(snapshot(root.path()), before);
}

#[tokio::test]
async fn derived_build_choices_allow_comment_edits_without_changing_the_output_plan() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path()).await;
    let project = root.path().join("project");
    let host = session(root.path(), true, false);
    let mut raw = fs::read(project.join("empack.yml")).unwrap();
    let original = DocumentCodec.decode_intent(&raw, "original").unwrap();
    let selected = request(original.intent(), &args(), BuildDecisions::default())
        .unwrap()
        .with_content(BuildAcquisitions::default())
        .require_intent(original.semantic_revision());
    raw.extend_from_slice(b"\n# A comment changes bytes but not build choices\n");
    fs::write(project.join("empack.yml"), &raw).unwrap();
    let owner = engine(&host.config_provider.app_config, root.path()).unwrap();
    build_with_engine(&host, &owner, project.clone(), selected, None, None, false)
        .await
        .unwrap();
    owner.shutdown().await;
    assert_eq!(fs::read(project.join("empack.yml")).unwrap(), raw);
    assert_eq!(
        zip_bytes(
            &project.join("dist/Native Pack-1.0.mrpack"),
            "overrides/config/example.txt"
        ),
        b"first"
    );
}

/// Exercise production CLI allowances; engine-only smoke tests supply their own budgets.
#[tokio::test]
#[ignore = "live official NeoForge installer and Java 21"]
async fn cli_neoforge_build_all_targets_with_default_resource_budget() {
    let root = tempfile::tempdir().unwrap();
    initialize(
        &session(root.path(), true, false),
        &InitArgs {
            mc_version: Some("1.21.1".into()),
            modloader: Some("neoforge".into()),
            loader_version: Some("21.1.209".into()),
            pack_name: Some("Budget Probe".into()),
            pack_version: Some("1.0".into()),
            author: Some("Tester".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let project = root.path().join("project");
    let codec = crate::engine::documents::DocumentCodec;
    let old = codec
        .decode_intent(&fs::read(project.join("empack.yml")).unwrap(), "test")
        .unwrap();
    let mut lock = codec
        .decode_lock(
            &fs::read(project.join("empack.lock")).unwrap(),
            &old,
            "test",
        )
        .unwrap()
        .lock()
        .clone();
    let mut intent = old.intent().clone();
    intent.metadata.id = "budget.probe".into();
    intent.runtime.java_major =
        empack_core::java::JavaPolicy::for_game(intent.runtime.minecraft.as_str())
            .select(None)
            .unwrap();
    intent.installation_policies = BTreeMap::new();
    intent.distribution = empack_core::model::DistributionIntent::from_recipes(
        NonEmpty::new(vec![
            Recipe::MODRINTH,
            Recipe::PRISM_REFERENCES,
            Recipe::SERVER_REFERENCES,
            Recipe::PRISM_BUNDLED,
            Recipe::SERVER_BUNDLED,
        ])
        .unwrap(),
        DistributionArchive::Zip,
    );
    let bytes = codec.encode_intent(&intent).unwrap();
    lock.intent_revision = codec
        .decode_intent(&bytes, "test")
        .unwrap()
        .semantic_revision();
    let revision = lock.intent_revision;
    fs::write(project.join("empack.yml"), bytes).unwrap();
    fs::write(
        project.join("empack.lock"),
        codec
            .encode_lock(
                &empack_core::model::ResolvedProject::validate(intent, lock, revision).unwrap(),
            )
            .unwrap(),
    )
    .unwrap();
    run(root.path(), &BuildArgs::default(), true, false)
        .await
        .unwrap();
    let dist = root.path().join("project/dist");
    for suffix in [
        ".mrpack",
        "-prism-references.zip",
        "-server-references.zip",
        "-prism-bundled.zip",
        "-server-bundled.zip",
    ] {
        let path = dist.join(format!("Budget Probe-1.0{suffix}"));
        let mut archive = zip::ZipArchive::new(fs::File::open(&path).unwrap()).unwrap();
        assert!(!archive.is_empty(), "empty artifact: {}", path.display());
        if suffix == "-server-bundled.zip" {
            assert!(archive.by_name("start.sh").is_ok());
        }
    }
}

#[tokio::test]
async fn curseforge_cli_recipe_uses_zip_and_only_publishes_requested_output() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path()).await;
    let args = BuildArgs {
        ad_hoc: true,
        targets: vec!["curseforge".into()],
        format: Some(CliArchiveFormat::TarGz),
        ..Default::default()
    };
    let before = snapshot(root.path());
    run(root.path(), &args, true, true).await.unwrap();
    assert_eq!(snapshot(root.path()), before);
    run(root.path(), &args, true, false).await.unwrap();
    let artifacts: Vec<_> = fs::read_dir(root.path().join("project/dist"))
        .unwrap()
        .map(|file| file.unwrap().file_name())
        .collect();
    assert_eq!(
        artifacts,
        vec![std::ffi::OsString::from("Native Pack-1.0-curseforge.zip")]
    );
    let mut zip = zip::ZipArchive::new(
        fs::File::open(
            root.path()
                .join("project/dist/Native Pack-1.0-curseforge.zip"),
        )
        .unwrap(),
    )
    .unwrap();
    let manifest: serde_json::Value =
        serde_json::from_reader(zip.by_name("manifest.json").unwrap()).unwrap();
    assert_eq!(manifest["name"], "Native Pack");
    assert_eq!(manifest["minecraft"]["version"], "1.21.1");
    assert_eq!(manifest["files"], serde_json::json!([]));
    let mut bytes = String::new();
    zip.by_name("overrides/config/example.txt")
        .unwrap()
        .read_to_string(&mut bytes)
        .unwrap();
    assert_eq!(bytes, "first");
}

#[tokio::test]
async fn consumer_overrides_preserve_delivery_and_reject_lossy_policies() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path()).await;
    let parsed = crate::testing::read_authoring(&root.path().join("project/empack.yml")).unwrap();
    for (consumer, bundled, references) in [
        ("prism", Recipe::PRISM_BUNDLED, Recipe::PRISM_REFERENCES),
        ("server", Recipe::SERVER_BUNDLED, Recipe::SERVER_REFERENCES),
    ] {
        let mut options = BuildArgs {
            ad_hoc: true,
            targets: vec![consumer.into()],
            ..Default::default()
        };
        let snapshot = request(parsed.intent(), &options, BuildDecisions::default()).unwrap();
        assert_eq!(snapshot.outputs.as_slice()[0].target, references);
        options.delivery = Some("bundled".into());
        let explicit = request(parsed.intent(), &options, BuildDecisions::default()).unwrap();
        assert_eq!(explicit.outputs.as_slice()[0].target, bundled);
        assert_ne!(
            snapshot.outputs.as_slice()[0].artifact,
            explicit.outputs.as_slice()[0].artifact
        );
        options.environment = Some("both".into());
        assert!(request(parsed.intent(), &options, BuildDecisions::default()).is_err());
    }
    let invalid = BuildArgs {
        targets: vec!["modrinth".into()],
        delivery: Some("bundled".into()),
        ..Default::default()
    };
    let before = snapshot(root.path());
    assert!(run(root.path(), &invalid, true, false).await.is_err());
    assert_eq!(snapshot(root.path()), before);
    for obsolete in ["mrpack", "client", "client-full", "server-full", "all"] {
        assert!(
            request(
                parsed.intent(),
                &BuildArgs {
                    targets: vec![obsolete.into()],
                    ..Default::default()
                },
                BuildDecisions::default()
            )
            .is_err()
        );
    }
}

#[tokio::test]
async fn subscribed_recipe_selection_retains_policy_and_distinct_output_names() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path()).await;
    let parsed = crate::testing::read_authoring(&root.path().join("project/empack.yml")).unwrap();
    for consumer in ["prism", "server"] {
        for delivery in ["references", "bundled"] {
            let selected = request(
                parsed.intent(),
                &BuildArgs {
                    ad_hoc: true,
                    targets: vec![consumer.into()],
                    delivery: Some(delivery.into()),
                    updates: Some("empack".into()),
                    ..Default::default()
                },
                BuildDecisions::default(),
            )
            .unwrap();
            let output = &selected.outputs.as_slice()[0];
            assert_eq!(output.target.update_authority(), UpdateAuthority::Empack);
            assert!(
                output
                    .artifact
                    .as_str()
                    .ends_with(&format!("-{consumer}-{delivery}-subscribed.zip"))
            );
        }
    }
}

#[tokio::test]
async fn native_and_platform_cli_policies_select_distinct_outputs() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path()).await;
    let parsed = crate::testing::read_authoring(&root.path().join("project/empack.yml")).unwrap();
    for (consumer, authority, suffix) in [
        ("empack", "empack", "-subscribed.empack"),
        ("modrinth", "platform", "-platform.mrpack"),
        ("curseforge", "platform", "-platform.zip"),
    ] {
        let selected = request(
            parsed.intent(),
            &BuildArgs {
                ad_hoc: true,
                targets: vec![consumer.into()],
                updates: Some(authority.into()),
                ..Default::default()
            },
            BuildDecisions::default(),
        )
        .unwrap();
        let output = &selected.outputs.as_slice()[0];
        assert!(output.artifact.as_str().ends_with(suffix));
        assert_ne!(output.target.update_authority(), UpdateAuthority::Snapshot);
    }
}

#[tokio::test]
async fn named_profiles_keep_policy_explain_without_effects_and_reject_ambiguity() {
    use empack_core::model::BuildProfile;
    let root = tempfile::tempdir().unwrap();
    fixture(root.path()).await;
    let project = root.path().join("project");
    let source = crate::testing::read_authoring(&project.join("empack.yml")).unwrap();
    let mut intent = source.intent().clone();
    intent.metadata.id = "profile.fixture".into();
    intent.runtime.java_major =
        empack_core::java::JavaPolicy::for_game(intent.runtime.minecraft.as_str())
            .select(None)
            .unwrap();
    intent.installation_policies = BTreeMap::new();
    intent.distribution.profiles.insert(
        "prism".into(),
        BuildProfile {
            recipe: Recipe::PRISM_BUNDLED
                .with_update_authority(UpdateAuthority::Empack)
                .unwrap(),
            archive: DistributionArchive::Zip,
        },
    );
    intent.distribution.defaults = vec!["prism".into()];
    let configured = request(
        &intent,
        &BuildArgs {
            targets: vec!["prism".into()],
            ..Default::default()
        },
        BuildDecisions::default(),
    )
    .unwrap();
    assert_eq!(
        configured.outputs.as_slice()[0].target.update_authority(),
        UpdateAuthority::Empack
    );
    assert!(
        configured.outputs.as_slice()[0]
            .artifact
            .as_str()
            .contains("subscribed")
    );
    let named = request(
        &intent,
        &BuildArgs {
            profiles: vec!["prism".into()],
            ..Default::default()
        },
        BuildDecisions::default(),
    )
    .unwrap();
    assert_eq!(named.outputs, configured.outputs);
    assert_eq!(
        request(&intent, &BuildArgs::default(), BuildDecisions::default())
            .unwrap()
            .outputs,
        named.outputs
    );
    let ad_hoc = request(
        &intent,
        &BuildArgs {
            ad_hoc: true,
            targets: vec!["prism".into()],
            ..Default::default()
        },
        BuildDecisions::default(),
    )
    .unwrap();
    assert_eq!(
        ad_hoc.outputs.as_slice()[0].target.update_authority(),
        UpdateAuthority::Snapshot
    );
    intent.distribution.profiles.insert(
        "other".into(),
        intent.distribution.profiles["prism"].clone(),
    );
    assert!(
        request(
            &intent,
            &BuildArgs {
                targets: vec!["prism".into()],
                ..Default::default()
            },
            BuildDecisions::default()
        )
        .err()
        .unwrap()
        .to_string()
        .contains("Multiple profiles")
    );
    assert!(
        request(
            &intent,
            &BuildArgs {
                profiles: vec!["missing".into()],
                ..Default::default()
            },
            BuildDecisions::default()
        )
        .is_err()
    );
    assert!(
        request(
            &intent,
            &BuildArgs {
                profiles: vec!["prism".into(), "other".into()],
                ..Default::default()
            },
            BuildDecisions::default()
        )
        .is_err(),
        "distinct profiles may not own the same artifact"
    );
    crate::testing::write_authoring(&project.join("empack.yml"), &intent).unwrap();
    fs::remove_file(project.join("empack.lock")).unwrap();
    let before = snapshot(root.path());
    let args = BuildArgs {
        profiles: vec!["prism".into()],
        explain: true,
        json: true,
        updates: Some("snapshot".into()),
        ..Default::default()
    };
    run(root.path(), &args, true, false).await.unwrap();
    assert_eq!(snapshot(root.path()), before);
    let workspace = ProjectReader::new(RecoveryReader::new(root.path().join("state")))
        .capture(
            &project,
            &[],
            SnapshotLimits::default(),
            &Cancellation::default(),
        )
        .unwrap();
    let selected = request(
        workspace.intent().intent(),
        &args,
        BuildDecisions::default(),
    )
    .unwrap();
    let view = profiles::explain(&workspace, &args, &selected).unwrap();
    assert_eq!(view["schema"], 2);
    assert_eq!(view["lock"], "missing");
    assert_eq!(view["profiles"][0]["recipe"]["updates"], "snapshot");
    assert_eq!(view["profiles"][0]["origins"]["updates"], "cli-override");
    assert_eq!(view["profiles"][0]["recipient_empack_required"], false);
    assert!(view["profiles"][0]["bootstrap"].is_null());
    assert_eq!(
        view["profiles"][0]["output"],
        format!("dist/{}", selected.outputs.as_slice()[0].artifact.as_str())
    );
    assert!(view["declared_manual_inputs"].is_null());
    assert!(
        profiles::text(&view)
            .unwrap()
            .contains("Command overrides: updates")
    );
    let mut reference_args = args;
    reference_args.delivery = Some("references".into());
    let selected = request(
        workspace.intent().intent(),
        &reference_args,
        BuildDecisions::default(),
    )
    .unwrap();
    let view = profiles::explain(&workspace, &reference_args, &selected).unwrap();
    assert_eq!(
        view["profiles"][0]["bootstrap"]["engine_version"],
        crate::engine::compatibility_version()
    );
    assert_eq!(
        view["profiles"][0]["bootstrap"]["sha256"],
        env!("EMPACK_BOOTSTRAP_SHA256")
    );
    assert!(
        profiles::text(&view)
            .unwrap()
            .contains("No separate empack installation is needed.")
    );
}

#[tokio::test]
async fn distinct_profile_formats_are_built_and_published_together() {
    use empack_core::model::BuildProfile;
    let root = tempfile::tempdir().unwrap();
    fixture(root.path()).await;
    let project = root.path().join("project");
    let original = crate::testing::read_authoring(&project.join("empack.yml")).unwrap();
    let mut lock = DocumentCodec
        .decode_prior_lock(&fs::read(project.join("empack.lock")).unwrap(), "fixture")
        .unwrap()
        .lock()
        .clone();
    let mut intent = original.intent().clone();
    intent.distribution.profiles = BTreeMap::from([
        (
            "zip".into(),
            BuildProfile {
                recipe: Recipe::PRISM_BUNDLED,
                archive: DistributionArchive::Zip,
            },
        ),
        (
            "tar".into(),
            BuildProfile {
                recipe: Recipe::PRISM_BUNDLED,
                archive: DistributionArchive::TarGz,
            },
        ),
    ]);
    intent.distribution.defaults = vec!["zip".into(), "tar".into()];
    crate::testing::write_authoring(&project.join("empack.yml"), &intent).unwrap();
    let source = crate::testing::read_authoring(&project.join("empack.yml")).unwrap();
    lock.intent_revision = source.semantic_revision();
    let resolved =
        empack_core::model::ResolvedProject::validate(intent, lock, source.semantic_revision())
            .unwrap();
    fs::write(
        project.join("empack.lock"),
        DocumentCodec.encode_lock(&resolved).unwrap(),
    )
    .unwrap();
    let args = BuildArgs {
        optional_defaults: true,
        ..Default::default()
    };
    run(root.path(), &args, true, false).await.unwrap();
    assert_eq!(
        zip_bytes(
            &project.join("dist/Native Pack-1.0-prism-bundled.zip"),
            ".minecraft/config/example.txt"
        ),
        b"first"
    );
    let input = fs::File::open(project.join("dist/Native Pack-1.0-prism-bundled.tar.gz")).unwrap();
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(input));
    let mut found = false;
    for entry in archive.entries().unwrap() {
        let mut entry = entry.unwrap();
        if entry.path().unwrap().as_ref() == Path::new(".minecraft/config/example.txt") {
            let mut bytes = Vec::new();
            std::io::Read::read_to_end(&mut entry, &mut bytes).unwrap();
            assert_eq!(bytes, b"first");
            found = true;
        }
    }
    assert!(found);
}
