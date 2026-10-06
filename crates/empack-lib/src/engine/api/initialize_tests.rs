use super::tests::{engine, path, put};
use super::*;
use crate::engine::{
    documents::DocumentCodec,
    initialize::{InitializeCandidate, default_templates},
    project_change::ProjectReplacementPolicy,
};
use empack_core::{files::ManagedPath, model::*};
use std::{collections::BTreeMap, fs};

fn request(loader: LoaderKind, replace: bool) -> InitializeRequest {
    let runtime = RuntimeResolution {
        minecraft: GameVersion::parse("1.20.1").unwrap(),
        loader,
        loader_version: (loader != LoaderKind::Vanilla)
            .then(|| LoaderVersion::parse("exact-test-version").unwrap()),
    };
    let intent = ProjectIntent {
        metadata: PackMetadata {
            name: "New pack $(literal)".into(),
            version: "0.1.0".into(),
            author: Some("Author".into()),
            description: Some("Pack description".into()),
        },
        runtime: RuntimeIntent {
            minecraft: runtime.minecraft.clone(),
            acceptable_versions: vec![GameVersion::parse("1.20.2").unwrap()],
            loader,
            loader_version: runtime.loader_version.clone(),
        },
        roots: BTreeMap::new(),
        layout: BTreeMap::from([(ContentKind::DataPack, path("custom-data"))]),
        distribution: DistributionIntent {
            targets: NonEmpty::new(vec![BuildTarget::Mrpack, BuildTarget::ClientFull]).unwrap(),
            archive: DistributionArchive::SevenZip,
        },
        extensions: BTreeMap::from([(
            "example.fixture".into(),
            ExtensionValue::Text("preserved".into()),
        )]),
    };
    InitializeRequest {
        candidate: InitializeCandidate::new(intent, runtime, default_templates()).unwrap(),
        replacement: if replace {
            ProjectReplacementPolicy::ReplaceManagedContent
        } else {
            ProjectReplacementPolicy::RejectExisting
        },
    }
}
fn grant(prepared: &PreparedOperation) -> ExecutionGrant {
    ExecutionGrant {
        plan: prepared.view().plan(),
        network: NetworkPermission::Offline,
        run_installer: false,
        replacement: prepared.view().replacement(),
    }
}
async fn prepare(
    engine: &Engine,
    target: ProjectTarget,
    request: InitializeRequest,
) -> PreparedOperation {
    match engine.prepare(target, request).await.unwrap() {
        Preparation::Ready(value) => value,
        _ => panic!("exact initialization unexpectedly needs input"),
    }
}

#[tokio::test]
async fn initialize_all_loader_intents_without_preview_writes() {
    for loader in [
        LoaderKind::Vanilla,
        LoaderKind::Fabric,
        LoaderKind::Quilt,
        LoaderKind::Forge,
        LoaderKind::NeoForge,
    ] {
        let temp = tempfile::tempdir().unwrap();
        let selected = temp.path().join("new");
        let host = temp.path().join("state");
        let (engine, _) = engine(host.clone());
        let request = request(loader, false);
        let expected = request.candidate.project().clone();
        let prepared = prepare(&engine, ProjectTarget::New(selected.clone()), request).await;
        let view = prepared.view().initialize().unwrap();
        assert_eq!(view.runtime, expected.lock().runtime);
        assert!(view.replacement.is_none());
        assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 0);
        assert!(
            view.files
                .expected()
                .contains_key(&ManagedPath::UserTemplate(path(
                    "client/instance.cfg.template"
                )))
        );
        let permission = grant(&prepared);
        let mut operation = engine
            .start(prepared.authorize(permission).unwrap())
            .unwrap();
        let outcome = operation.wait().await;
        match &*outcome {
            OperationOutcome::Completed(ExecutionOutcome::Completed(
                ExecutionReceipt::Initialize(receipt),
            )) => {
                assert_eq!(receipt.project.intent(), expected.intent());
                assert_eq!(receipt.project.lock(), expected.lock());
                assert_eq!(receipt.publication.changed_files, 4);
            }
            _ => panic!("initialization failed"),
        }
        let workspace = ProjectReader::new(RecoveryReader::new(host))
            .capture_build(
                &selected,
                &[],
                SnapshotLimits::default(),
                &crate::application::process_runtime::Cancellation::default(),
            )
            .unwrap();
        let observed = workspace.require_resolved().unwrap();
        assert_eq!(observed.intent(), expected.intent());
        assert_eq!(observed.lock(), expected.lock());
        assert!(
            fs::read_to_string(selected.join("templates/client/instance.cfg.template"))
                .unwrap()
                .contains("{{ini_quote NAME}}")
        );
        engine.shutdown().await;
    }
}

#[tokio::test]
async fn forced_initialize_keeps_templates_and_requires_exact_replacement_approval() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let (engine, _) = engine(state.path().join("state"));
    put(root.path(), "empack.yml", b"broken: [");
    put(root.path(), "pack/mods/old.jar", b"old content");
    put(
        root.path(),
        "templates/client/instance.cfg.template",
        b"user template",
    );
    put(root.path(), "templates/server/custom.bin", b"user bytes");
    put(root.path(), "dist/old.zip", b"old artifact");
    let target = ProjectTarget::Existing(root.path().to_path_buf());
    assert!(
        engine
            .prepare(target.clone(), request(LoaderKind::Vanilla, false))
            .await
            .is_err()
    );
    let prepared = prepare(&engine, target.clone(), request(LoaderKind::Vanilla, true)).await;
    assert!(prepared.view().replacement().is_some());
    let mut missing = grant(&prepared);
    missing.replacement = None;
    assert!(prepared.authorize(missing).is_err());
    assert_eq!(
        fs::read(root.path().join("empack.yml")).unwrap(),
        b"broken: ["
    );
    let prepared = prepare(&engine, target, request(LoaderKind::Vanilla, true)).await;
    assert!(!prepared.view().initialize().unwrap().files.changes().iter().any(|change| matches!(change.target(), ManagedPath::UserTemplate(path) if path.as_str() == "client/instance.cfg.template")));
    let permission = grant(&prepared);
    let mut operation = engine
        .start(prepared.authorize(permission).unwrap())
        .unwrap();
    assert!(matches!(
        &*operation.wait().await,
        OperationOutcome::Completed(ExecutionOutcome::Completed(ExecutionReceipt::Initialize(_)))
    ));
    assert!(!root.path().join("pack/mods/old.jar").exists());
    for (file, bytes) in [
        (
            "templates/client/instance.cfg.template",
            b"user template".as_slice(),
        ),
        ("templates/server/custom.bin", b"user bytes"),
        ("dist/old.zip", b"old artifact"),
    ] {
        assert_eq!(fs::read(root.path().join(file)).unwrap(), bytes);
    }
    engine.shutdown().await;
}

#[tokio::test]
async fn initialize_rejects_changed_templates_before_any_publication() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let (engine, _) = engine(state.path().join("state"));
    put(
        root.path(),
        "templates/server/server.properties.template",
        b"original",
    );
    let prepared = prepare(
        &engine,
        ProjectTarget::Existing(root.path().to_path_buf()),
        request(LoaderKind::Vanilla, false),
    )
    .await;
    put(
        root.path(),
        "templates/server/server.properties.template",
        b"edited",
    );
    let permission = grant(&prepared);
    let mut operation = engine
        .start(prepared.authorize(permission).unwrap())
        .unwrap();
    assert!(matches!(
        &*operation.wait().await,
        OperationOutcome::Completed(ExecutionOutcome::FailedBeforePublication(_))
    ));
    assert!(!root.path().join("empack.yml").exists());
    assert_eq!(
        fs::read(
            root.path()
                .join("templates/server/server.properties.template")
        )
        .unwrap(),
        b"edited"
    );
    engine.shutdown().await;
}

#[test]
fn initialization_rejects_incoherent_runtime_and_unresolved_roots() {
    let project = request(LoaderKind::Fabric, false)
        .candidate
        .project()
        .clone();
    let mut runtime = project.lock().runtime.clone();
    runtime.loader_version = None;
    assert!(
        InitializeCandidate::new(project.intent().clone(), runtime, default_templates()).is_err()
    );
    let nonempty = crate::engine::mrpack::tests::project(false, false);
    assert!(
        InitializeCandidate::new(
            nonempty.intent().clone(),
            nonempty.lock().runtime.clone(),
            default_templates()
        )
        .is_err()
    );
    let mut templates = BTreeMap::new();
    templates.insert(path("unrecognized/file"), vec![]);
    assert!(
        InitializeCandidate::new(
            project.intent().clone(),
            project.lock().runtime.clone(),
            templates
        )
        .is_err()
    );
}

#[tokio::test]
async fn initialized_project_builds_an_empty_mrpack_with_preserved_runtime() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let (engine, _) = engine(state.path().join("state"));
    let target = root.path().join("new");
    let prepared = prepare(
        &engine,
        ProjectTarget::New(target.clone()),
        request(LoaderKind::Vanilla, false),
    )
    .await;
    let permission = grant(&prepared);
    let mut operation = engine
        .start(prepared.authorize(permission).unwrap())
        .unwrap();
    assert!(matches!(
        &*operation.wait().await,
        OperationOutcome::Completed(ExecutionOutcome::Completed(ExecutionReceipt::Initialize(_)))
    ));
    let mut build = super::tests::request();
    build.outputs = NonEmpty::new(vec![BuildOutput {
        target: BuildTarget::Mrpack,
        artifact: path("empty.mrpack"),
    }])
    .unwrap();
    let prepared = match engine.prepare(target.clone(), build).await.unwrap() {
        Preparation::Ready(value) => value,
        _ => panic!("empty pack needs no downloads"),
    };
    let permission = grant(&prepared);
    let mut operation = engine
        .start(prepared.authorize(permission).unwrap())
        .unwrap();
    assert!(matches!(
        &*operation.wait().await,
        OperationOutcome::Completed(ExecutionOutcome::Completed(ExecutionReceipt::Build(_)))
    ));
    let archive = fs::File::open(target.join("dist/empty.mrpack")).unwrap();
    let mut zip = zip::ZipArchive::new(archive).unwrap();
    let index: serde_json::Value =
        serde_json::from_reader(zip.by_name("modrinth.index.json").unwrap()).unwrap();
    assert_eq!(index["dependencies"]["minecraft"], "1.20.1");
    assert_eq!(index["files"].as_array().unwrap().len(), 0);
    let intent = fs::read(target.join("empack.yml")).unwrap();
    assert_eq!(
        DocumentCodec
            .decode_intent(&intent, "published")
            .unwrap()
            .intent()
            .metadata
            .name,
        "New pack $(literal)"
    );
    engine.shutdown().await;
}

#[tokio::test]
async fn initialization_refuses_directory_valued_template_seeds_without_mutation() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let (engine, _) = engine(state.path().join("state"));
    fs::create_dir_all(root.path().join("templates/client/instance.cfg.template")).unwrap();
    put(
        root.path(),
        "templates/client/instance.cfg.template/keep",
        b"untouched",
    );
    assert!(
        engine
            .prepare(
                root.path().to_path_buf(),
                request(LoaderKind::Vanilla, true)
            )
            .await
            .is_err()
    );
    assert!(!root.path().join("empack.yml").exists());
    assert_eq!(
        fs::read(
            root.path()
                .join("templates/client/instance.cfg.template/keep")
        )
        .unwrap(),
        b"untouched"
    );
    assert!(!state.path().join("state").exists());
    engine.shutdown().await;
}

#[cfg(unix)]
#[tokio::test]
async fn initialization_refuses_template_links_without_changing_outside_bytes() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let (engine, _) = engine(state.path().join("state"));
    put(outside.path(), "instance.cfg.template", b"outside");
    fs::create_dir_all(root.path().join("templates")).unwrap();
    std::os::unix::fs::symlink(outside.path(), root.path().join("templates/client")).unwrap();
    assert!(
        engine
            .prepare(
                root.path().to_path_buf(),
                request(LoaderKind::Vanilla, true)
            )
            .await
            .is_err()
    );
    assert!(!root.path().join("empack.yml").exists());
    assert_eq!(
        fs::read(outside.path().join("instance.cfg.template")).unwrap(),
        b"outside"
    );
    engine.shutdown().await;
}

#[tokio::test]
async fn initialization_preserves_user_template_destinations_and_common_precedence() {
    for name in [
        "client/instance.cfg",
        "client/INSTANCE.cfg",
        "common/instance.cfg.template",
    ] {
        let root = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        let (engine, _) = engine(state.path().join("state"));
        put(
            root.path(),
            &format!("templates/{name}"),
            b"user configuration",
        );
        let prepared = prepare(
            &engine,
            ProjectTarget::Existing(root.path().to_path_buf()),
            request(LoaderKind::Vanilla, false),
        )
        .await;
        assert!(!prepared.view().initialize().unwrap().files.changes().iter().any(|change|
            matches!(change.target(), ManagedPath::UserTemplate(path) if path.as_str() == "client/instance.cfg.template")));
        let permission = grant(&prepared);
        let mut operation = engine
            .start(prepared.authorize(permission).unwrap())
            .unwrap();
        assert!(matches!(
            &*operation.wait().await,
            OperationOutcome::Completed(ExecutionOutcome::Completed(ExecutionReceipt::Initialize(
                _
            )))
        ));
        let workspace = ProjectReader::new(RecoveryReader::new(state.path().join("state")))
            .capture_build(
                root.path(),
                &[],
                SnapshotLimits::default(),
                &crate::application::process_runtime::Cancellation::default(),
            )
            .unwrap();
        let rendered = crate::engine::templates::prepare_templates(
            &workspace,
            BuildTarget::ClientFull,
            &TemplateOptions::default(),
            &crate::application::process_runtime::Cancellation::default(),
        )
        .unwrap();
        let destination = if name == "client/INSTANCE.cfg" {
            "INSTANCE.cfg"
        } else {
            "instance.cfg"
        };
        let file = &rendered.files()[&path(destination)];
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut file.content.lease().open(), &mut bytes).unwrap();
        assert_eq!(bytes, b"user configuration");
        engine.shutdown().await;
    }
}

#[test]
fn initialization_rejects_seeds_with_colliding_rendered_destinations() {
    let request = request(LoaderKind::Vanilla, false);
    let project = request.candidate.project();
    for names in [
        ["client/instance.cfg", "client/instance.cfg.template"],
        ["client/INSTANCE.cfg", "client/instance.cfg.template"],
        ["server/config", "server/config/file.template"],
    ] {
        let seeds = names
            .into_iter()
            .map(|name| (path(name), b"seed".to_vec()))
            .collect();
        assert!(
            InitializeCandidate::new(
                project.intent().clone(),
                project.lock().runtime.clone(),
                seeds
            )
            .is_err()
        );
    }
}

#[tokio::test]
async fn initialization_rejects_a_new_common_template_after_preparation() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let (engine, _) = engine(state.path().join("state"));
    let prepared = prepare(
        &engine,
        ProjectTarget::Existing(root.path().to_path_buf()),
        request(LoaderKind::Vanilla, false),
    )
    .await;
    put(
        root.path(),
        "templates/common/instance.cfg",
        b"new user configuration",
    );
    let permission = grant(&prepared);
    let mut operation = engine
        .start(prepared.authorize(permission).unwrap())
        .unwrap();
    assert!(!matches!(
        &*operation.wait().await,
        OperationOutcome::Completed(ExecutionOutcome::Completed(_))
    ));
    assert!(!root.path().join("empack.yml").exists());
    assert!(
        !root
            .path()
            .join("templates/client/instance.cfg.template")
            .exists()
    );
    assert_eq!(
        fs::read(root.path().join("templates/common/instance.cfg")).unwrap(),
        b"new user configuration"
    );
    engine.shutdown().await;
}
