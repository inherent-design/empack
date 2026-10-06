use super::*;
use crate::engine::{documents::DocumentCodec, mrpack::tests::project};
use std::{fs, path::Path, time::Duration};
fn path(value: &str) -> PortableRelPath {
    PortableRelPath::parse(value, PathSyntax::ProjectContent).unwrap()
}
fn engine(state: PathBuf) -> (Engine, ResourceGovernor) {
    let work = ResourceRequest {
        jobs: 1,
        memory_bytes: 16 << 20,
        scratch_bytes: 8 << 20,
        open_files: 32,
    };
    let governor = ResourceGovernor::new(ResourceRequest {
        jobs: 2,
        memory_bytes: 128 << 20,
        scratch_bytes: 64 << 20,
        open_files: 128,
    });
    let config = EngineConfig {
        state_root: state,
        retained_operations: 4,
        resources: OperationResources {
            capture: work,
            prepared: ResourceRequest {
                jobs: 0,
                memory_bytes: 1 << 20,
                scratch_bytes: 0,
                open_files: 1,
            },
            local_acquisition: work,
            acquired: ResourceRequest {
                jobs: 0,
                memory_bytes: 1 << 20,
                scratch_bytes: 4 << 20,
                open_files: 8,
            },
            assembly: work,
            receipt: ResourceRequest {
                jobs: 0,
                memory_bytes: 1 << 20,
                scratch_bytes: 0,
                open_files: 0,
            },
        },
        snapshot: SnapshotLimits {
            total_bytes: 2 << 20,
            file_bytes: 1 << 20,
            entries: 1000,
            depth: 32,
        },
        archive: ArchiveLimits {
            compressed_bytes: 2 << 20,
            total_bytes: 4 << 20,
            file_bytes: 1 << 20,
            entries: 1000,
            depth: 32,
        },
        transfer: TransferLimits {
            file_bytes: 1 << 20,
            transfer_bytes: 1 << 20,
            deadline: Duration::from_secs(10),
            redirects: 2,
        },
        installer: InstallerExecution {
            java: "must-not-be-executed".into(),
            deadline: Duration::from_secs(10),
            heap_megabytes: 64,
            output: SnapshotLimits::default(),
        },
    };
    (Engine::new(config, governor.clone()).unwrap(), governor)
}
fn put(root: &Path, name: &str, bytes: &[u8]) {
    let destination = root.join(name);
    fs::create_dir_all(destination.parent().unwrap()).unwrap();
    fs::write(destination, bytes).unwrap();
}
fn fixture(root: &Path) {
    let resolved = project(false, false);
    put(
        root,
        "empack.yml",
        &DocumentCodec.encode_intent(resolved.intent()).unwrap(),
    );
    put(
        root,
        "empack.lock",
        &DocumentCodec.encode_lock(&resolved).unwrap(),
    );
    for file in ["a.zip", "copy.zip", "b.zip"] {
        put(root, &format!("pack/resourcepacks/{file}"), b"payload");
    }
    put(root, "pack/config/value", b"current config");
    put(root, "dist/result.mrpack", b"previous mrpack");
    put(root, "dist/client.zip", b"previous client");
}
fn request() -> BuildRequest {
    BuildRequest {
        outputs: NonEmpty::new(vec![
            BuildOutput {
                target: BuildTarget::Mrpack,
                artifact: path("result.mrpack"),
            },
            BuildOutput {
                target: BuildTarget::ClientFull,
                artifact: path("client.zip"),
            },
        ])
        .unwrap(),
        archive: DistributionArchive::Zip,
        optional: OptionalPolicy::Preserve,
        mrpack_optional: OptionalConversion::RejectMetadataLoss,
        templates: TemplateOptions::default(),
        evidence: SourceEvidencePolicy::Compatibility,
        interaction: InstallerInteraction::Headless,
    }
}
async fn ready(engine: &Engine, root: &Path, request: BuildRequest) -> PreparedOperation {
    match engine.prepare(root.to_path_buf(), request).await.unwrap() {
        Preparation::Ready(value) => value,
        Preparation::NeedsInput(_) => panic!("unexpected missing input"),
    }
}
fn grant(prepared: &PreparedOperation) -> ExecutionGrant {
    ExecutionGrant {
        plan: prepared.view().plan(),
        network: NetworkPermission::Offline,
        run_installer: false,
        replacement: None,
    }
}
fn inventory(root: &Path) -> std::collections::BTreeMap<PathBuf, Vec<u8>> {
    fn walk(root: &Path, path: &Path, map: &mut std::collections::BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                walk(root, &entry.path(), map);
            } else {
                map.insert(
                    entry.path().strip_prefix(root).unwrap().to_owned(),
                    fs::read(entry.path()).unwrap(),
                );
            }
        }
    }
    let mut map = Default::default();
    walk(root, root, &mut map);
    map
}
#[tokio::test]
async fn preview_and_rejected_authorization_preserve_project_and_host_state() {
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    fixture(root.path());
    let (engine, governor) = engine(host.path().join("state"));
    let before = inventory(root.path());
    for _ in 0..8 {
        let view = engine
            .preview(root.path().to_owned(), request())
            .await
            .unwrap();
        assert!(!view.build().unwrap().needs_network);
        assert!(!view.build().unwrap().runs_installer);
        assert!(view.build().unwrap().content.is_empty());
    }
    assert_eq!(governor.status().reserved, ResourceRequest::default());
    let first = ready(&engine, root.path(), request()).await;
    let second = ready(&engine, root.path(), request()).await;
    assert!(first.authorize(grant(&second)).is_err());
    drop(second);
    assert_eq!(before, inventory(root.path()));
    assert!(!host.path().join("state").exists());
    assert_eq!(governor.status().reserved, ResourceRequest::default());
    engine.shutdown().await;
}
#[tokio::test]
async fn owned_build_publishes_all_outputs_and_retains_receipt_resources() {
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    fixture(root.path());
    let (engine, governor) = engine(host.path().join("state"));
    let prepared = ready(&engine, root.path(), request()).await;
    assert_eq!(governor.status().reserved, engine.config.resources.prepared);
    let permission = grant(&prepared);
    let approved = prepared.authorize(permission).unwrap();
    let mut handle = engine.start(approved).unwrap();
    let id = handle.id();
    let outcome = handle.wait().await;
    let receipt = match &*outcome {
        OperationOutcome::Completed(ExecutionOutcome::Completed(ExecutionReceipt::Build(
            receipt,
        ))) => receipt,
        OperationOutcome::Completed(ExecutionOutcome::FailedBeforePublication(error)) => {
            panic!("{error:#}")
        }
        _ => panic!("build did not complete"),
    };
    assert_eq!(receipt.artifacts.len(), 2);
    assert_eq!(receipt.publication.changed_files, 2);
    assert_eq!(governor.status().reserved, engine.config.resources.receipt);
    let mut archive =
        zip::ZipArchive::new(fs::File::open(root.path().join("dist/client.zip")).unwrap()).unwrap();
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(
        &mut archive.by_name(".minecraft/config/value").unwrap(),
        &mut bytes,
    )
    .unwrap();
    assert_eq!(bytes, b"current config");
    assert!(engine.release_completed(id));
    drop(outcome);
    drop(handle);
    assert_eq!(governor.status().reserved, ResourceRequest::default());
    engine.shutdown().await;
}
#[tokio::test]
async fn source_conflicts_cancellation_and_late_failure_preserve_every_artifact() {
    for scenario in ["conflict", "cancel", "template"] {
        let root = tempfile::tempdir().unwrap();
        let host = tempfile::tempdir().unwrap();
        fixture(root.path());
        if scenario == "template" {
            put(
                root.path(),
                "templates/client/broken.template",
                b"{{missing_contract_variable}}",
            );
        }
        let (engine, _) = engine(host.path().join("state"));
        let prepared = ready(&engine, root.path(), request()).await;
        if scenario == "conflict" {
            put(root.path(), "pack/config/value", b"edited");
        }
        let permission = grant(&prepared);
        let mut handle = engine
            .start(prepared.authorize(permission).unwrap())
            .unwrap();
        if scenario == "cancel" {
            handle.cancel();
        }
        let outcome = handle.wait().await;
        assert!(!matches!(
            &*outcome,
            OperationOutcome::Completed(ExecutionOutcome::Completed(ExecutionReceipt::Build(_)))
        ));
        assert_eq!(
            fs::read(root.path().join("dist/result.mrpack")).unwrap(),
            b"previous mrpack"
        );
        assert_eq!(
            fs::read(root.path().join("dist/client.zip")).unwrap(),
            b"previous client"
        );
        assert!(!host.path().join("state").exists());
        engine.shutdown().await;
    }
}
#[tokio::test]
async fn cross_engine_and_missing_network_grants_are_rejected_before_effects() {
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    fixture(root.path());
    let (first, _) = engine(host.path().join("first"));
    let (second, _) = engine(host.path().join("second"));
    let prepared = ready(&first, root.path(), request()).await;
    let permission = grant(&prepared);
    assert!(
        second
            .start(prepared.authorize(permission).unwrap())
            .is_err()
    );
    let mut server = request();
    server.outputs = NonEmpty::new(vec![BuildOutput {
        target: BuildTarget::ServerFull,
        artifact: path("server.zip"),
    }])
    .unwrap();
    let prepared = ready(&first, root.path(), server).await;
    assert!(prepared.view().build().unwrap().needs_network);
    let permission = grant(&prepared);
    assert!(prepared.authorize(permission).is_err());
    assert!(!host.path().join("first").exists());
    assert!(!host.path().join("second").exists());
    first.shutdown().await;
    second.shutdown().await;
}

#[tokio::test]
async fn pending_provider_content_and_installer_effects_are_explicit() {
    use empack_core::{
        identity::{ModrinthProjectId, ProviderProjectId},
        model::{
            AcquisitionSpec, LoaderVersion, ResolvedIdentity, ResolvedPin, ResolvedProject,
            SourceIntent,
        },
    };
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    let base = project(false, false);
    let identity = ProviderProjectId::Modrinth(ModrinthProjectId::parse("AANobbMI").unwrap());
    let pin = ResolvedPin {
        project: identity.clone(),
        selection: identity.parse_pin("Version1").unwrap(),
    };
    let mut intent = base.intent().clone();
    intent.roots.values_mut().next().unwrap().source = SourceIntent::Provider(identity.clone());
    intent.runtime.loader = LoaderKind::Forge;
    intent.runtime.loader_version = Some(LoaderVersion::parse("47.4.0").unwrap());
    let revision = DocumentCodec
        .decode_intent(&DocumentCodec.encode_intent(&intent).unwrap(), "fixture")
        .unwrap()
        .semantic_revision();
    let mut lock = base.lock().clone();
    lock.intent_revision = revision;
    lock.runtime.loader = intent.runtime.loader;
    lock.runtime.loader_version = intent.runtime.loader_version.clone();
    let dependency = lock.dependencies.values_mut().next().unwrap();
    dependency.identity = ResolvedIdentity::Provider(identity);
    dependency.selected = Some(pin.clone());
    let mut files = dependency.files.as_slice().to_vec();
    for file in &mut files {
        file.acquisition = AcquisitionSpec::Manual {
            pin: Some(pin.clone()),
            instructions: "Select the declared file".into(),
        };
    }
    dependency.files = NonEmpty::new(files).unwrap();
    let resolved = ResolvedProject::validate(intent, lock, revision).unwrap();
    put(
        root.path(),
        "empack.yml",
        &DocumentCodec.encode_intent(resolved.intent()).unwrap(),
    );
    put(
        root.path(),
        "empack.lock",
        &DocumentCodec.encode_lock(&resolved).unwrap(),
    );
    let (engine, governor) = engine(host.path().join("state"));
    match engine
        .prepare(root.path().to_owned(), request())
        .await
        .unwrap()
    {
        Preparation::NeedsInput(report) => {
            let report = report.build().unwrap();
            assert_eq!(report.unresolved.len(), 2);
            assert!(
                report
                    .content
                    .iter()
                    .all(|need| need.kind == ContentRequirementKind::Manual)
            );
        }
        Preparation::Ready(_) => panic!("manual content cannot disappear from preparation"),
    }
    let mut server = request();
    server.outputs = NonEmpty::new(vec![BuildOutput {
        target: BuildTarget::ServerFull,
        artifact: path("server.zip"),
    }])
    .unwrap();
    let prepared = ready(&engine, root.path(), server).await;
    assert!(prepared.view().build().unwrap().runs_installer);
    assert!(prepared.view().build().unwrap().unresolved.is_empty());
    let permission = ExecutionGrant {
        plan: prepared.view().plan(),
        network: NetworkPermission::Allow,
        run_installer: false,
        replacement: None,
    };
    assert!(prepared.authorize(permission).is_err());
    assert!(!host.path().join("state").exists());
    assert!(!root.path().join("dist").exists());
    assert_eq!(governor.status().reserved, ResourceRequest::default());
    engine.shutdown().await;
}

fn provider_fixture(root: &Path, curseforge: bool) -> empack_core::model::ExpectedContent {
    use empack_core::{identity::ModrinthProjectId, model::*};
    fixture(root);
    let original = project(false, false);
    let mut intent = original.intent().clone();
    let mut lock = original.lock().clone();
    let id = if curseforge {
        empack_core::identity::ProviderProjectId::CurseForge(
            empack_core::identity::CurseForgeProjectId::parse("123").unwrap(),
        )
    } else {
        empack_core::identity::ProviderProjectId::Modrinth(
            ModrinthProjectId::parse("AANobbMI").unwrap(),
        )
    };
    let pin = ResolvedPin {
        project: id.clone(),
        selection: id
            .parse_pin(if curseforge { "456" } else { "abcdefgh" })
            .unwrap(),
    };
    let key = DependencyKey::parse("assets").unwrap();
    intent.roots.get_mut(&key).unwrap().source = SourceIntent::Provider(id.clone());
    let dependency = lock.dependencies.get_mut(&key).unwrap();
    dependency.identity = ResolvedIdentity::Provider(id);
    dependency.selected = Some(pin.clone());
    let mut file = dependency.files.as_slice()[0].clone();
    file.slot = FileSlot::parse("primary").unwrap();
    file.acquisition = AcquisitionSpec::Provider {
        pin,
        slot: file.slot.clone(),
        alternatives: vec![],
    };
    let expected = file.expected.clone();
    dependency.files = NonEmpty::new(vec![file]).unwrap();
    let raw = DocumentCodec.encode_intent(&intent).unwrap();
    lock.intent_revision = DocumentCodec
        .decode_intent(&raw, "fixture")
        .unwrap()
        .semantic_revision();
    let resolved = ResolvedProject::validate(intent, lock.clone(), lock.intent_revision).unwrap();
    put(root, "empack.yml", &raw);
    put(
        root,
        "empack.lock",
        &DocumentCodec.encode_lock(&resolved).unwrap(),
    );
    for file in ["a.zip", "copy.zip", "b.zip"] {
        fs::remove_file(root.join(format!("pack/resourcepacks/{file}"))).unwrap();
    }
    expected
}
#[tokio::test]
async fn provider_refresh_executes_only_after_grant_and_preserves_locked_intent() {
    provider_build_case(false, false).await;
    provider_build_case(true, false).await;
    provider_build_case(false, true).await;
}
async fn provider_build_case(changed_digest: bool, unavailable: bool) {
    use serde_json::json;
    use std::io::Read;
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    let expected = provider_fixture(root.path(), false);
    let before = inventory(root.path());
    let mut server = mockito::Server::new_async().await;
    let catalog = ProviderCatalog::for_loopback_tests(&server.url(), None);
    let (mut engine, governor) = engine(host.path().join("state"));
    engine.transport = HttpAcquisition::for_loopback_tests();
    let engine = engine.with_provider_catalog(
        catalog,
        CatalogLimits {
            response_bytes: 4096,
            transfer_bytes: 8192,
            deadline: Duration::from_secs(2),
        },
    );
    let mut request = request();
    request.outputs = NonEmpty::new(vec![BuildOutput {
        target: BuildTarget::ClientFull,
        artifact: path("client.zip"),
    }])
    .unwrap();
    let view = engine
        .preview(root.path().to_owned(), request.clone())
        .await
        .unwrap();
    assert!(view.build().unwrap().needs_network);
    assert!(view.build().unwrap().unresolved.is_empty());
    assert_eq!(before, inventory(root.path()));
    assert!(!host.path().join("state").exists());
    // Install mocks after preview: any accidental request during preview cannot succeed.
    let project = server
        .mock("GET", "/project/AANobbMI")
        .with_body(
            json!({"id":"AANobbMI","slug":"assets","title":"Assets","project_type":"resourcepack"})
                .to_string(),
        )
        .create_async()
        .await;
    let mut hashes: serde_json::Map<String, serde_json::Value> = expected
        .digests
        .as_ref()
        .unwrap()
        .values()
        .iter()
        .map(|digest| (digest.algorithm().name().to_owned(), json!(digest.hex())))
        .collect();
    if changed_digest {
        hashes.insert("sha512".into(), json!("00".repeat(64)));
    }
    // Modrinth requires a URL; use an explicit blocked locator to test acquisition failure.
    let download = if unavailable {
        "https://127.0.0.1:1/unavailable".to_owned()
    } else {
        format!("{}/payload", server.url())
    };
    let metadata = server.mock("GET", "/version/abcdefgh").with_body(json!({"id":"abcdefgh","project_id":"AANobbMI","files":[{"filename":"assets.zip","primary":true,"size":7,"hashes":hashes,"url":download}],"game_versions":["1.20.1"],"loaders":["minecraft"],"dependencies":[]}).to_string()).create_async().await;
    let content = server
        .mock("GET", "/payload")
        .with_body("payload")
        .expect(if changed_digest || unavailable { 0 } else { 1 })
        .create_async()
        .await;
    let prepared = ready(&engine, root.path(), request).await;
    let approval = ExecutionGrant {
        network: NetworkPermission::Allow,
        ..grant(&prepared)
    };
    let mut handle = engine.start(prepared.authorize(approval).unwrap()).unwrap();
    let outcome = handle.wait().await;
    match &*outcome {
        OperationOutcome::Completed(ExecutionOutcome::Completed(ExecutionReceipt::Build(_)))
            if !changed_digest && !unavailable =>
        {
            let mut archive =
                zip::ZipArchive::new(fs::File::open(root.path().join("dist/client.zip")).unwrap())
                    .unwrap();
            let mut bytes = Vec::new();
            archive
                .by_name(".minecraft/resourcepacks/a.zip")
                .unwrap()
                .read_to_end(&mut bytes)
                .unwrap();
            assert_eq!(bytes, b"payload");
            for name in ["empack.yml", "empack.lock"] {
                assert_eq!(
                    fs::read(root.path().join(name)).unwrap(),
                    before[&PathBuf::from(name)]
                );
            }
        }
        OperationOutcome::Completed(ExecutionOutcome::FailedBeforePublication(_))
            if changed_digest || unavailable =>
        {
            assert_eq!(before, inventory(root.path()));
            assert!(!host.path().join("state").exists());
        }
        OperationOutcome::Completed(ExecutionOutcome::FailedBeforePublication(error)) => {
            panic!("provider build failed: {error:#}")
        }
        _ => panic!("unexpected provider build outcome"),
    }
    project.assert_async().await;
    metadata.assert_async().await;
    content.assert_async().await;
    engine.release_completed(handle.id());
    drop(outcome);
    drop(handle);
    engine.shutdown().await;
    assert_eq!(governor.status().reserved, ResourceRequest::default());
}

#[tokio::test]
async fn missing_provider_credentials_and_restricted_files_remain_explicit_input() {
    use serde_json::json;
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    let expected = provider_fixture(root.path(), true);
    // A second file is downloadable in principle but deliberately unreachable here.
    // Its failure must not hide the provider file's manual-input requirement.
    {
        use empack_core::model::{AcquisitionSpec, DependencyKey, FileSlot, ResolvedProject};
        let intent = DocumentCodec
            .decode_intent(
                &fs::read(root.path().join("empack.yml")).unwrap(),
                "fixture",
            )
            .unwrap();
        let resolved = DocumentCodec
            .decode_lock(
                &fs::read(root.path().join("empack.lock")).unwrap(),
                &intent,
                "fixture",
            )
            .unwrap();
        let mut lock = resolved.lock().clone();
        let dependency = lock
            .dependencies
            .get_mut(&DependencyKey::parse("assets").unwrap())
            .unwrap();
        let primary = dependency.files.as_slice()[0].clone();
        let mut other = primary.clone();
        other.slot = FileSlot::parse("bystander").unwrap();
        other.acquisition = AcquisitionSpec::Provider {
            pin: dependency.selected.clone().unwrap(),
            slot: other.slot.clone(),
            alternatives: vec!["https://127.0.0.1:1/must-not-download".into()],
        };
        let mut placement = other.placements.as_slice()[0].clone();
        placement.destination =
            empack_core::path::InstallDestination::parse("resourcepacks/bystander.zip").unwrap();
        other.placements = NonEmpty::new(vec![placement]).unwrap();
        dependency.files = NonEmpty::new(vec![primary, other]).unwrap();
        let resolved =
            ResolvedProject::validate(resolved.intent().clone(), lock, intent.semantic_revision())
                .unwrap();
        put(
            root.path(),
            "empack.lock",
            &DocumentCodec.encode_lock(&resolved).unwrap(),
        );
    }
    let before = inventory(root.path());
    let mut server = mockito::Server::new_async().await;
    let (engine, _) = engine(host.path().join("state"));
    let engine = engine.with_provider_catalog(
        ProviderCatalog::for_loopback_tests(&server.url(), None),
        CatalogLimits::default(),
    );
    let mut request = request();
    request.outputs = NonEmpty::new(vec![BuildOutput {
        target: BuildTarget::ClientFull,
        artifact: path("client.zip"),
    }])
    .unwrap();
    let preparation = engine
        .prepare(root.path().to_owned(), request.clone())
        .await
        .unwrap();
    assert!(
        matches!(preparation, Preparation::NeedsInput(view) if view.build().unwrap().unresolved.len() == 1)
    );
    let engine = engine.with_provider_catalog(
        ProviderCatalog::for_loopback_tests(&server.url(), Some("fixture-key".into())),
        CatalogLimits::default(),
    );
    let project = server
        .mock("GET", "/mods/123")
        .with_body(
            json!({"data":{"id":123,"gameId":432,"slug":"assets","name":"Assets","classId":12}})
                .to_string(),
        )
        .create_async()
        .await;
    let hashes: Vec<_> = expected
        .digests
        .as_ref()
        .unwrap()
        .values()
        .iter()
        .filter_map(|digest| match digest.algorithm() {
            empack_core::digest::DigestAlgorithm::Md5 => {
                Some(json!({"algo":2,"value":digest.hex()}))
            }
            empack_core::digest::DigestAlgorithm::Sha1 => {
                Some(json!({"algo":1,"value":digest.hex()}))
            }
            _ => None,
        })
        .collect();
    let metadata = server.mock("GET", "/mods/123/files/456").with_body(json!({"data":{"id":456,"gameId":432,"modId":123,"fileName":"assets.zip","fileLength":7,"downloadUrl":null,"hashes":hashes,"gameVersions":["1.20.1"],"dependencies":[]}}).to_string()).create_async().await;
    let prepared = ready(&engine, root.path(), request).await;
    let approval = ExecutionGrant {
        network: NetworkPermission::Allow,
        ..grant(&prepared)
    };
    let mut handle = engine.start(prepared.authorize(approval).unwrap()).unwrap();
    let outcome = handle.wait().await;
    match &*outcome {
        OperationOutcome::Completed(ExecutionOutcome::NeedsInput(needs)) => {
            assert_eq!(needs.len(), 1);
            assert_eq!(needs[0].kind, ContentRequirementKind::Manual);
            assert_eq!(needs[0].expected, expected);
        }
        OperationOutcome::Completed(ExecutionOutcome::FailedBeforePublication(error)) => {
            panic!("restricted file became a failure: {error:#}")
        }
        _ => panic!("restricted file did not report missing input"),
    }
    assert_eq!(before, inventory(root.path()));
    assert!(!host.path().join("state").exists());
    project.assert_async().await;
    metadata.assert_async().await;
    engine.shutdown().await;
}

#[tokio::test]
async fn large_local_build_publishes_under_a_normal_descriptor_limit() {
    const CHILD: &str = "EMPACK_LARGE_BUILD_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let executable = std::env::current_exe().unwrap();
        let name =
            "engine::api::tests::large_local_build_publishes_under_a_normal_descriptor_limit";
        #[cfg(unix)]
        let mut command = {
            let mut command = std::process::Command::new("/bin/sh");
            command.args([
                "-c",
                "ulimit -n 256; exec \"$1\" --exact \"$2\" --nocapture",
                "empack-build-fixture",
            ]);
            command.arg(executable).arg(name);
            command
        };
        #[cfg(not(unix))]
        let mut command = {
            let mut command = std::process::Command::new(executable);
            command.args(["--exact", name, "--nocapture"]);
            command
        };
        let output = command.env(CHILD, "1").output().unwrap();
        assert!(
            output.status.success(),
            "large build failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    fixture(root.path());
    for index in 0..600 {
        put(
            root.path(),
            &format!("pack/config/{index}.txt"),
            format!("configuration {index}").as_bytes(),
        );
    }
    let (engine, governor) = engine(host.path().join("state"));
    let prepared = ready(&engine, root.path(), request()).await;
    let permission = grant(&prepared);
    let mut handle = engine
        .start(prepared.authorize(permission).unwrap())
        .unwrap();
    let outcome = handle.wait().await;
    match &*outcome {
        OperationOutcome::Completed(ExecutionOutcome::Completed(ExecutionReceipt::Build(
            receipt,
        ))) => {
            assert_eq!(receipt.artifacts.len(), 2)
        }
        OperationOutcome::Completed(ExecutionOutcome::FailedBeforePublication(error)) => {
            panic!("{error:#}")
        }
        _ => panic!("large build did not complete"),
    }
    for (artifact, prefix) in [("client.zip", ".minecraft"), ("result.mrpack", "overrides")] {
        let mut archive =
            zip::ZipArchive::new(fs::File::open(root.path().join("dist").join(artifact)).unwrap())
                .unwrap();
        for index in 0..600 {
            let mut bytes = Vec::new();
            std::io::Read::read_to_end(
                &mut archive
                    .by_name(&format!("{prefix}/config/{index}.txt"))
                    .unwrap(),
                &mut bytes,
            )
            .unwrap();
            assert_eq!(bytes, format!("configuration {index}").as_bytes());
        }
    }
    assert!(engine.release_completed(handle.id()));
    drop(outcome);
    drop(handle);
    engine.shutdown().await;
    assert_eq!(governor.status().reserved, ResourceRequest::default());
}

async fn imported(governor: ResourceGovernor) -> crate::engine::import::ImportCandidate {
    use crate::engine::{
        content::{InitialObservation, verify_stream},
        import::{
            ImportCandidateOptions, ImportContentKey, ImportContentLimits, ImportContentOutcome,
            ImportContentPlan, ImportFileDecision, ImportLimits, ImportPersistence, inspect_import,
        },
    };
    use empack_core::{
        model::*,
        requirements::{Requirement, Requirements},
    };
    use std::{
        collections::BTreeMap,
        io::{Cursor, Write},
    };
    let mut zip = zip::ZipWriter::new(Cursor::new(vec![]));
    zip.start_file(
        "modrinth.index.json",
        zip::write::SimpleFileOptions::default(),
    )
    .unwrap();
    zip.write_all(br#"{"formatVersion":1,"game":"minecraft","name":"Imported","versionId":"1","files":[],"dependencies":{"minecraft":"1.21.1"}}"#).unwrap();
    zip.start_file(
        "overrides/config/value",
        zip::write::SimpleFileOptions::default(),
    )
    .unwrap();
    zip.write_all(b"new config").unwrap();
    let bytes = zip.finish().unwrap().into_inner();
    let source = verify_stream(
        &mut bytes.as_slice(),
        &ExpectedContent {
            digests: None,
            size: Some(bytes.len() as u64),
            accepted_observation: None,
        },
        bytes.len() as u64,
        SourceEvidencePolicy::Compatibility,
        InitialObservation::Accepted,
        &crate::application::process_runtime::Cancellation::default(),
    )
    .unwrap();
    let runtime = OperationRuntime::new(governor, 1);
    let mut handle = runtime
        .start(move |mut scope| async move {
            let result = async {
                let input = inspect_import(
                    &mut scope,
                    source,
                    ImportLimits {
                        manifest_bytes: 4096,
                        records: 16,
                        archive: ArchiveLimits {
                            entries: 32,
                            compressed_bytes: 1 << 20,
                            total_bytes: 1 << 20,
                            file_bytes: 1 << 20,
                            depth: 16,
                        },
                    },
                )
                .await?;
                let catalog = ProviderCatalog::for_loopback_tests("http://127.0.0.1:1", None);
                let plan = ImportContentPlan::resolve(
                    &mut scope,
                    input,
                    &catalog,
                    ImportContentLimits {
                        catalog: crate::engine::providers::CatalogLimits {
                            response_bytes: 4096,
                            transfer_bytes: 8192,
                            ..Default::default()
                        },
                        archive: ArchiveLimits {
                            entries: 32,
                            compressed_bytes: 1 << 20,
                            total_bytes: 1 << 20,
                            file_bytes: 1 << 20,
                            depth: 16,
                        },
                        records: 16,
                        total_bytes: 1 << 20,
                        transfer: crate::engine::acquisition::TransferLimits {
                            file_bytes: 1 << 20,
                            transfer_bytes: 1 << 20,
                            ..Default::default()
                        },
                    },
                )
                .await?;
                let outcome = plan
                    .acquire(
                        &mut scope,
                        &HttpAcquisition::for_loopback_tests(),
                        BTreeMap::new(),
                        SourceEvidencePolicy::Compatibility,
                    )
                    .await?;
                let ImportContentOutcome::Ready(content) = outcome else {
                    anyhow::bail!("fixture needs input");
                };
                content.into_candidate(
                    &mut scope,
                    ImportCandidateOptions {
                        metadata: PackMetadata {
                            name: "Imported".into(),
                            version: "1".into(),
                            author: None,
                            description: None,
                        },
                        loader: None,
                        layout: BTreeMap::new(),
                        exclude_auxiliary_members: false,
                        distribution: DistributionIntent {
                            targets: NonEmpty::new(vec![BuildTarget::Mrpack])?,
                            archive: DistributionArchive::Zip,
                        },
                        files: BTreeMap::from([(
                            ImportContentKey::Override(0),
                            ImportFileDecision {
                                key: DependencyKey::parse("config")?,
                                kind: ContentKind::Config,
                                requirements: Requirements {
                                    client: Requirement::Required,
                                    server: Requirement::Required,
                                },
                                persistence: ImportPersistence::Local,
                                provider_destination: None,
                            },
                        )]),
                    },
                )
            }
            .await;
            Ok(result)
        })
        .unwrap();
    let outcome = handle.wait().await;
    runtime.shutdown().await;
    drop(handle);
    drop(runtime);
    match Arc::into_inner(outcome) {
        Some(OperationOutcome::Completed(Ok(candidate))) => candidate,
        Some(OperationOutcome::Completed(Err(error))) => panic!("{error:#}"),
        _ => panic!("import fixture failed"),
    }
}
async fn ready_import(
    engine: &Engine,
    governor: &ResourceGovernor,
    root: &Path,
) -> PreparedOperation {
    let request = ImportRequest {
        candidate: imported(governor.clone()).await,
        replacement: crate::engine::import::ImportReplacementPolicy::ReplaceManagedContent,
    };
    match engine.prepare(root.to_path_buf(), request).await.unwrap() {
        Preparation::Ready(prepared) => prepared,
        Preparation::NeedsInput(_) => panic!("fully acquired import cannot need content"),
    }
}
fn import_grant(prepared: &PreparedOperation) -> ExecutionGrant {
    ExecutionGrant {
        plan: prepared.view().plan(),
        network: NetworkPermission::Offline,
        run_installer: false,
        replacement: prepared.view().import().unwrap().replacement,
    }
}
#[tokio::test]
async fn import_preview_and_replacement_acknowledgement_preserve_unapproved_files() {
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    put(root.path(), "empack.yml", b"broken");
    put(root.path(), "pack/old.jar", b"old bytes");
    let before = inventory(root.path());
    let (engine, governor) = engine(host.path().join("state"));
    let view = engine
        .preview(
            root.path().to_path_buf(),
            ImportRequest {
                candidate: imported(governor.clone()).await,
                replacement: crate::engine::import::ImportReplacementPolicy::ReplaceManagedContent,
            },
        )
        .await
        .unwrap();
    let old_ack = view.import().unwrap().replacement.unwrap();
    assert!(!view.needs_network() && !view.runs_installer());
    assert_eq!(before, inventory(root.path()));
    assert_eq!(governor.status().reserved, ResourceRequest::default());
    let prepared = ready_import(&engine, &governor, root.path()).await;
    assert!(governor.status().reserved.scratch_bytes > 0);
    let mut permission = import_grant(&prepared);
    permission.replacement = None;
    assert!(prepared.authorize(permission).is_err());
    assert_eq!(before, inventory(root.path()));
    put(root.path(), "pack/old.jar", b"edited original");
    let prepared = ready_import(&engine, &governor, root.path()).await;
    assert_ne!(prepared.view().import().unwrap().replacement, Some(old_ack));
    let mut permission = import_grant(&prepared);
    permission.replacement = Some(old_ack);
    assert!(prepared.authorize(permission).is_err());
    assert_eq!(
        fs::read(root.path().join("pack/old.jar")).unwrap(),
        b"edited original"
    );
    assert!(!host.path().join("state").exists());
    assert_eq!(governor.status().reserved, ResourceRequest::default());
    engine.shutdown().await;
}
#[tokio::test]
async fn owned_import_publishes_complete_content_and_retains_a_typed_receipt() {
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    let (engine, governor) = engine(host.path().join("state"));
    put(root.path(), "README", b"unrelated");
    put(root.path(), "pack/backup.zip", b"private");
    put(root.path(), "pack/.packwizignore", b"private/\n");
    let prepared = ready_import(&engine, &governor, root.path()).await;
    assert!(prepared.view().import().unwrap().replacement.is_none());
    let expected_bytes: u64 = prepared
        .view()
        .import()
        .unwrap()
        .files
        .expected()
        .values()
        .map(|file| file.bytes)
        .sum();
    assert_eq!(governor.status().reserved.scratch_bytes, expected_bytes);
    let permission = import_grant(&prepared);
    let mut handle = engine
        .start(prepared.authorize(permission).unwrap())
        .unwrap();
    let outcome = handle.wait().await;
    match &*outcome {
        OperationOutcome::Completed(ExecutionOutcome::Completed(ExecutionReceipt::Import(
            receipt,
        ))) => {
            assert_eq!(receipt.plan, permission.plan);
            assert_eq!(receipt.project.intent().roots.len(), 1);
        }
        OperationOutcome::Completed(ExecutionOutcome::FailedBeforePublication(error)) => {
            panic!("{error:#}")
        }
        _ => panic!("import did not complete"),
    }
    assert_eq!(
        fs::read(root.path().join("overrides/common/config/value")).unwrap(),
        b"new config"
    );
    assert_eq!(
        fs::read(root.path().join("pack/backup.zip")).unwrap(),
        b"private"
    );
    assert_eq!(fs::read(root.path().join("README")).unwrap(), b"unrelated");
    assert_eq!(governor.status().reserved, engine.config.resources.receipt);
    assert!(engine.observe(handle.id()).is_some());
    assert!(engine.release_completed(handle.id()));
    drop(outcome);
    drop(handle);
    engine.shutdown().await;
    assert_eq!(governor.status().reserved, ResourceRequest::default());
}
#[tokio::test]
async fn import_rejects_cross_engine_grants_and_preserves_conflicting_or_cancelled_projects() {
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    let (engine, governor) = engine(host.path().join("state"));
    let (other, _) = self::engine(host.path().join("other"));
    let prepared = ready_import(&engine, &governor, root.path()).await;
    let permission = import_grant(&prepared);
    assert!(
        other
            .start(prepared.authorize(permission).unwrap())
            .is_err()
    );
    for cancel in [false, true] {
        let prepared = ready_import(&engine, &governor, root.path()).await;
        let permission = import_grant(&prepared);
        if !cancel {
            put(root.path(), "empack.yml", b"concurrent edit");
        }
        let before = inventory(root.path());
        let mut handle = engine
            .start(prepared.authorize(permission).unwrap())
            .unwrap();
        if cancel {
            handle.cancel();
        }
        let outcome = handle.wait().await;
        assert!(!matches!(
            &*outcome,
            OperationOutcome::Completed(ExecutionOutcome::Completed(_))
        ));
        assert_eq!(before, inventory(root.path()));
        assert!(engine.release_completed(handle.id()));
        drop(outcome);
        drop(handle);
    }
    engine.shutdown().await;
    other.shutdown().await;
    assert_eq!(governor.status().reserved, ResourceRequest::default());
}
