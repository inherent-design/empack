use super::*;
use empack_core::model::{GameVersion, LoaderVersion};
use std::io::{Cursor, Write};
fn content(bytes: &[u8]) -> AcquiredContent {
    verify_stream(
        &mut &*bytes,
        &ExpectedContent {
            digests: None,
            size: None,
            accepted_observation: None,
        },
        1 << 20,
        SourceEvidencePolicy::Compatibility,
        InitialObservation::Accepted,
        &Cancellation::default(),
    )
    .unwrap()
}
fn runtime(version: &str) -> RuntimeResolution {
    RuntimeResolution {
        minecraft: GameVersion::parse("1.20.1").unwrap(),
        loader: LoaderKind::Fabric,
        loader_version: Some(LoaderVersion::parse(version).unwrap()),
    }
}
fn metadata(version: &str) -> Value {
    serde_json::json!({"id":format!("fabric-loader-{version}-1.20.1"),"inheritsFrom":"1.20.1","mainClass":"net.fabricmc.KnotServer","arguments":{"game":[]},"libraries":[{"name":format!("net.fabricmc:fabric-loader:{version}"),"url":"https://maven.fabricmc.net/"},{"name":"net.fabricmc:intermediary:1.20.1","url":"https://maven.fabricmc.net/"}]})
}
fn plan(value: &Value, version: &str) -> Result<LibraryServerPlan> {
    LibraryServerPlan::from_metadata(runtime(version), &content(&serde_json::to_vec(value)?))
}
fn contract(main: &str, shaded: bool) -> LauncherContract {
    LauncherContract {
        kind: LibraryKind::Fabric,
        launch_main: main.into(),
        declared_main: None,
        shaded,
    }
}
#[test]
fn catalog_binds_runtime_coordinates_and_preserves_legacy_launcher_layout() {
    for (version, shaded) in [
        ("0.11.7", true),
        ("0.12.5", true),
        ("0.12.6", false),
        ("0.16.0", false),
    ] {
        let plan = plan(&metadata(version), version).unwrap();
        assert_eq!(plan.launcher.shaded, shaded);
        assert_eq!(
            plan.libraries[0].url,
            format!(
                "https://maven.fabricmc.net/net/fabricmc/fabric-loader/{version}/fabric-loader-{version}.jar"
            )
        );
        assert!(plan.libraries[0].expected.digests.is_none());
    }
    for (name, value) in [
        ("id", Value::from("wrong")),
        ("inheritsFrom", Value::from("wrong")),
        ("mainClass", Value::from("bad\ncommand")),
    ] {
        let mut data = metadata("0.16.0");
        data[name] = value;
        assert!(plan(&data, "0.16.0").is_err());
    }
    for coordinate in [
        "evil:../../outside:1",
        "net.fabricmc:fabric-loader:0.15.0",
        "group:artifact:version:unhandled",
        "group:artifact:.\n",
    ] {
        let mut data = metadata("0.16.0");
        data["libraries"][0]["name"] = coordinate.into();
        assert!(plan(&data, "0.16.0").is_err());
    }
    let mut duplicate = metadata("0.16.0");
    duplicate["libraries"][1] = duplicate["libraries"][0].clone();
    assert!(plan(&duplicate, "0.16.0").is_err());
    let mut arguments = metadata("0.16.0");
    arguments["arguments"]["jvm"] = serde_json::json!(["-Dignored=true"]);
    assert!(plan(&arguments, "0.16.0").is_err());
}
fn jar(extra: bool) -> AcquiredContent {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in [
        (
            "META-INF/MANIFEST.MF",
            b"Manifest-Version: 1.0\r\nMain-Class: net.fabricmc.Launcher\r\n\r\n".as_slice(),
        ),
        (
            "net/fabricmc/Launcher.class",
            &[0xca, 0xfe, 0xba, 0xbe, 0, 0, 0, 61],
        ),
        (
            "net/fabricmc/KnotServer.class",
            &[0xca, 0xfe, 0xba, 0xbe, 0, 0, 0, 61],
        ),
        (
            "META-INF/services/example.Service",
            if extra {
                b"example.B\nexample.A\n"
            } else {
                b"example.A # comment\n"
            },
        ),
        ("META-INF/OLD.SF", b"invalid after shading"),
        ("shared.txt", if extra { b"second" } else { b"first" }),
    ] {
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(bytes).unwrap();
    }
    content(&zip.finish().unwrap().into_inner())
}
#[test]
fn generated_launchers_preserve_classpaths_and_shaded_service_union() {
    let paths = [
        "libraries/net/fabricmc/fabric-loader/0.11.7/fabric-loader-0.11.7.jar",
        "libraries/extra/extra.jar",
    ]
    .map(|name| PortableRelPath::parse(name, PathSyntax::ArchiveMember).unwrap());
    for shaded in [false, true] {
        let (content, main) = launcher::assemble(
            &paths,
            &[jar(false), jar(true)],
            &contract("net.fabricmc.KnotServer", shaded),
            ArchiveLimits::default(),
            2 << 20,
            &Cancellation::default(),
        )
        .unwrap();
        assert_eq!(main, "net.fabricmc.Launcher");
        let mut jar =
            JarReader::open(&content, ArchiveLimits::default(), &Cancellation::default()).unwrap();
        let attributes = jar.main_attributes().unwrap();
        assert_eq!(attributes["main-class"], main);
        assert_eq!(attributes.contains_key("class-path"), !shaded);
        if shaded {
            let mut services = String::new();
            jar.archive
                .by_name("META-INF/services/example.Service")
                .unwrap()
                .read_to_string(&mut services)
                .unwrap();
            assert_eq!(services, "example.A\nexample.B\n");
            assert!(jar.archive.by_name("META-INF/OLD.SF").is_err());
            let mut shared = String::new();
            jar.archive
                .by_name("shared.txt")
                .unwrap()
                .read_to_string(&mut shared)
                .unwrap();
            assert_eq!(shared, "first");
        } else {
            assert_eq!(jar.archive.len(), 2);
            assert_eq!(
                attributes["class-path"],
                paths
                    .iter()
                    .map(|path| path.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
            );
        }
    }
}
#[test]
fn missing_classes_cancellation_and_output_limits_cannot_produce_launchers() {
    let paths = [PortableRelPath::parse(
        "libraries/net/fabricmc/fabric-loader/0.16.0/loader.jar",
        PathSyntax::ArchiveMember,
    )
    .unwrap()];
    let libraries = [jar(false)];
    assert!(
        launcher::assemble(
            &paths,
            &libraries,
            &contract("net.fabricmc.Absent", false),
            ArchiveLimits::default(),
            2 << 20,
            &Cancellation::default()
        )
        .is_err()
    );
    assert!(
        launcher::assemble(
            &paths,
            &libraries,
            &contract("net.fabricmc.KnotServer", true),
            ArchiveLimits::default(),
            32,
            &Cancellation::default()
        )
        .is_err()
    );
    let cancel = Cancellation::default();
    cancel.cancel();
    assert!(
        launcher::assemble(
            &paths,
            &libraries,
            &contract("net.fabricmc.KnotServer", false),
            ArchiveLimits::default(),
            2 << 20,
            &cancel
        )
        .is_err()
    );
}

#[tokio::test]
async fn composed_runtime_rejects_wrong_bytes_and_retains_exact_library_evidence() {
    use crate::engine::{
        resources::{ResourceGovernor, ResourceRequest},
        runtime::{OperationOutcome, OperationRuntime},
    };
    for (family, corrupt) in [
        (LoaderKind::Fabric, false),
        (LoaderKind::Fabric, true),
        (LoaderKind::Quilt, false),
        (LoaderKind::Quilt, true),
    ] {
        let governor = ResourceGovernor::new(ResourceRequest {
            jobs: 2,
            memory_bytes: 128 << 20,
            scratch_bytes: 16 << 20,
            open_files: 16,
        });
        let operation = OperationRuntime::new(governor.clone(), 1);
        let mut handle = operation
            .start(move |mut scope| async move {
                let mut plan = match family {
                    LoaderKind::Quilt => quilt_plan(&quilt_metadata()).unwrap(),
                    _ => plan(&metadata("0.16.0"), "0.16.0").unwrap(),
                };
                let expected = jar(false);
                for library in &mut plan.libraries {
                    library.expected.digests = Some(expected.observed_digests().clone());
                    library.expected.size = Some(expected.lease().len());
                }
                let libraries = vec![if corrupt { jar(true) } else { expected.clone() }, expected];
                Ok(plan
                    .finish(
                        crate::engine::server_runtime::tests::prepared_fixture(),
                        libraries,
                        &mut scope,
                        ArchiveLimits::default(),
                    )
                    .await)
            })
            .unwrap();
        let outcome = handle.wait().await;
        operation.shutdown().await;
        match &*outcome {
            OperationOutcome::Completed(Ok(runtime)) => {
                assert!(!corrupt);
                assert_eq!(runtime.runtime().loader, family);
                let Some(LoaderRuntimeEvidence::Libraries(evidence)) =
                    runtime.evidence().loader.as_ref()
                else {
                    panic!("missing library runtime evidence")
                };
                assert_eq!(runtime.evidence().main_class, "net.minecraft.server.Main");
                assert_eq!(runtime.launcher_main_class(), "net.fabricmc.Launcher");
                assert_eq!(evidence.family, family);
                assert_eq!(evidence.libraries.len(), 2);
                assert!(!evidence.shaded);
                assert_eq!(runtime.files().len(), 5);
                let launcher = &runtime.files()
                    [&PortableRelPath::parse("server.jar", PathSyntax::ProjectContent).unwrap()];
                assert_eq!(evidence.launcher, launcher.content.lease().id());
                assert_eq!(
                    governor.status().reserved.scratch_bytes,
                    launcher.content.lease().len()
                );
            }
            OperationOutcome::Completed(Err(_)) => assert!(corrupt),
            OperationOutcome::Failed(error) => panic!("{error}"),
        }
        drop(outcome);
        drop(handle);
        drop(operation);
        assert_eq!(governor.status().reserved.scratch_bytes, 0);
    }
}

fn quilt_metadata() -> Value {
    let mut value = metadata("0.26.3");
    value["id"] = "quilt-loader-0.26.3-1.20.1".into();
    value["launcherMainClass"] = "net.fabricmc.Launcher".into();
    value["libraries"][0]["name"] = "org.quiltmc:quilt-loader:0.26.3".into();
    value["libraries"][0]["url"] = "https://maven.quiltmc.org/repository/release/".into();
    value
}
fn quilt_plan(value: &Value) -> Result<LibraryServerPlan> {
    let mut runtime = runtime("0.26.3");
    runtime.loader = LoaderKind::Quilt;
    LibraryServerPlan::from_metadata(runtime, &content(&serde_json::to_vec(value)?))
}
#[test]
fn quilt_catalog_requires_its_own_identity_and_declared_launcher() {
    let plan = quilt_plan(&quilt_metadata()).unwrap();
    assert_eq!(plan.launcher.kind, LibraryKind::Quilt);
    assert!(!plan.launcher.shaded);
    assert_eq!(
        plan.launcher.declared_main.as_deref(),
        Some("net.fabricmc.Launcher")
    );
    assert_eq!(
        plan.libraries[0].url,
        "https://maven.quiltmc.org/repository/release/org/quiltmc/quilt-loader/0.26.3/quilt-loader-0.26.3.jar"
    );
    for field in ["id", "launcherMainClass"] {
        let mut value = quilt_metadata();
        value[field] = Value::Null;
        assert!(quilt_plan(&value).is_err());
    }
    let mut value = quilt_metadata();
    value["libraries"][0]["name"] = "net.fabricmc:fabric-loader:0.26.3".into();
    assert!(quilt_plan(&value).is_err());
    let mut value = quilt_metadata();
    value["libraries"].as_array_mut().unwrap().push(serde_json::json!({"name":"org.quiltmc:hashed:1.19","url":"https://maven.quiltmc.org/repository/release/"}));
    assert!(quilt_plan(&value).is_err());
}
#[test]
fn quilt_launcher_uses_declared_class_and_its_own_properties() {
    let plan = quilt_plan(&quilt_metadata()).unwrap();
    let paths = plan
        .libraries
        .iter()
        .map(|library| library.path.clone())
        .collect::<Vec<_>>();
    let (content, main) = launcher::assemble(
        &paths,
        &[jar(false), jar(false)],
        &plan.launcher,
        ArchiveLimits::default(),
        2 << 20,
        &Cancellation::default(),
    )
    .unwrap();
    assert_eq!(main, "net.fabricmc.Launcher");
    let mut jar =
        JarReader::open(&content, ArchiveLimits::default(), &Cancellation::default()).unwrap();
    assert!(
        jar.archive
            .by_name("fabric-server-launch.properties")
            .is_err()
    );
    let mut properties = String::new();
    jar.archive
        .by_name("quilt-server-launch.properties")
        .unwrap()
        .read_to_string(&mut properties)
        .unwrap();
    assert_eq!(properties, "launch.mainClass=net.fabricmc.KnotServer\n");
}
