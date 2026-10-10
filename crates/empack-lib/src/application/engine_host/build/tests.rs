use super::*;
use crate::application::{
    InitArgs,
    session_mocks::{MockCommandSession, MockConfigProvider, MockInvocationProvider},
};
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
fn snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    super::super::tests::snapshot(root)
}
fn args() -> BuildArgs {
    BuildArgs {
        targets: vec!["mrpack".into(), "client-full".into()],
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
    let before = snapshot(root.path());
    for (yes, dry) in [(true, true), (false, false)] {
        run(root.path(), &args(), yes, dry).await.unwrap();
        assert_eq!(snapshot(root.path()), before);
    }
    run(root.path(), &args(), true, false).await.unwrap();
    let project = root.path().join("project");
    let mrpack = project.join("dist/Native Pack-1.0.mrpack");
    let client = project.join("dist/Native Pack-1.0-client-full.zip");
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
        interaction: InstallerInteraction::Interactive,
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
            targets: vec!["client-full".into(), "client-full".into(), "mrpack".into()],
            ..Default::default()
        };
        let built = request(intent.intent(), &options, choices.clone()).unwrap();
        assert_eq!(built.outputs.as_slice().len(), 2);
        assert_eq!(
            built.outputs.as_slice()[0].artifact.as_str(),
            format!("Native Pack-1.0-client-full.{extension}")
        );
        assert_eq!(
            built.outputs.as_slice()[1].artifact.as_str(),
            "Native Pack-1.0.mrpack"
        );
        assert_eq!(built.archive, archive);
        assert_eq!(built.optional, choices.optional);
        assert_eq!(built.mrpack_optional, choices.mrpack_optional);
        assert_eq!(built.evidence, choices.evidence);
        assert_eq!(built.interaction, choices.interaction);
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
        5
    );
    let mut selected = intent.intent().clone();
    selected.distribution.archive = DistributionArchive::TarGz;
    let default = request(&selected, &args(), BuildDecisions::default()).unwrap();
    assert_eq!(default.archive, DistributionArchive::TarGz);
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
    let decoded = DocumentCodec
        .decode_intent(&fs::read(project.join("empack.yml")).unwrap(), "test")
        .unwrap();
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
            source: SourceIntent::Provider(identity.clone()),
            kind: ContentKind::ResourcePack,
            version: VersionIntent::FollowCompatible,
            placement: PlacementIntent::Explicit(NonEmpty::new(vec![placement.clone()]).unwrap()),
            requirements,
        },
    );
    lock.dependencies.insert(
        key.clone(),
        LockedDependency {
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
        targets: vec!["client-full".into()],
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
    continue_build(&session(root.path(), true, false), &explicit)
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
            &project.join("dist/Native Pack-1.0-client-full.zip"),
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
            &project.join("dist/Native Pack-1.0-client-full.zip"),
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
            &project.join("dist/Native Pack-1.0-client-full.zip"),
            ".minecraft/resourcepacks/custom.zip"
        ),
        b"verified manual bytes"
    );
}

#[tokio::test]
async fn derived_build_choices_reject_changed_intent_before_preparation() {
    use crate::engine::documents::DocumentCodec;
    let root = tempfile::tempdir().unwrap();
    fixture(root.path()).await;
    let project = root.path().join("project");
    let host = session(root.path(), true, false);
    let original = DocumentCodec
        .decode_intent(&fs::read(project.join("empack.yml")).unwrap(), "original")
        .unwrap();
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
    changed.distribution.archive = DistributionArchive::TarGz;
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
    use crate::engine::documents::DocumentCodec;
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
    run(
        root.path(),
        &BuildArgs {
            targets: vec!["all".into()],
            ..Default::default()
        },
        true,
        false,
    )
    .await
    .unwrap();
    let dist = root.path().join("project/dist");
    for suffix in [
        ".mrpack",
        "-client.zip",
        "-server.zip",
        "-client-full.zip",
        "-server-full.zip",
    ] {
        let path = dist.join(format!("Budget Probe-1.0{suffix}"));
        let mut archive = zip::ZipArchive::new(fs::File::open(&path).unwrap()).unwrap();
        assert!(!archive.is_empty(), "empty artifact: {}", path.display());
        if suffix == "-server-full.zip" {
            assert!(archive.by_name("start.sh").is_ok());
        }
    }
}

#[tokio::test]
async fn curseforge_cli_recipe_uses_zip_and_only_publishes_requested_output() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path()).await;
    let args = BuildArgs {
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
