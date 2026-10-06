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
        resources: BuildResources {
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
        plan: prepared.view().plan,
        network: NetworkPermission::Offline,
        run_installer: false,
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
        assert!(!view.needs_network);
        assert!(!view.runs_installer);
        assert!(view.content.is_empty());
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
        OperationOutcome::Completed(BuildOutcome::Completed(receipt)) => receipt,
        OperationOutcome::Completed(BuildOutcome::FailedBeforePublication(error)) => {
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
            OperationOutcome::Completed(BuildOutcome::Completed(_))
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
    assert!(prepared.view().needs_network);
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
    assert!(prepared.view().runs_installer);
    assert!(prepared.view().unresolved.is_empty());
    let permission = ExecutionGrant {
        plan: prepared.view().plan,
        network: NetworkPermission::Allow,
        run_installer: false,
    };
    assert!(prepared.authorize(permission).is_err());
    assert!(!host.path().join("state").exists());
    assert!(!root.path().join("dist").exists());
    assert_eq!(governor.status().reserved, ResourceRequest::default());
    engine.shutdown().await;
}
