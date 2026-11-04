//! Live runtime probes and bounded launcher diagnostics. Missing HTTP/Java prerequisites fail opt-in checks.
use anyhow::Context;
use empack_core::model::{GameVersion, LoaderKind, LoaderVersion, RuntimeResolution};
use empack_lib::{
    application::{
        process_runtime::{Cancellation, execute_async},
        session::{ProcessOutput, ProcessStream},
    },
    engine::{
        acquisition::{HttpAcquisition, TransferLimits},
        artifacts::ArchiveLimits,
        content::SourceEvidencePolicy,
        resources::{ResourceGovernor, ResourceRequest},
        runtime::{OperationOutcome, OperationRuntime},
        server_runtime::installer::InstallerExecution,
        snapshot::SnapshotLimits,
    },
};
use std::{fs, path::PathBuf, time::Duration};
// Keep bounded, best-effort progress on failure: execute_async correctly reports
// retirement/timeout, but its failed result does not contain captured output.
async fn probe_launcher(
    command: std::process::Command,
    deadline: Duration,
) -> anyhow::Result<ProcessOutput> {
    let (sender, receiver) = std::sync::mpsc::sync_channel::<(ProcessStream, Vec<u8>)>(64);
    let observer = std::thread::spawn(move || {
        let mut tails = [Vec::new(), Vec::new()];
        for (stream, bytes) in receiver {
            let index = match stream {
                ProcessStream::Stdout => 0,
                ProcessStream::Stderr => 1,
            };
            let tail = &mut tails[index];
            tail.extend_from_slice(&bytes);
            let excess = tail.len().saturating_sub(64 * 1024);
            tail.drain(..excess);
        }
        tails
    });
    let result = execute_async(command, deadline, Cancellation::default(), Some(sender)).await;
    let tails = observer
        .join()
        .expect("launcher progress observer panicked");
    result.with_context(|| {
        format!(
            "Launcher failed; best-effort progress tails (at most 64 KiB per stream)\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&tails[0]),
            String::from_utf8_lossy(&tails[1]),
        )
    })
}

#[cfg(unix)]
#[tokio::test]
async fn native_runtime_probe_timeout_retains_partial_output() {
    let mut command = std::process::Command::new("/bin/sh");
    command.args([
        "-c",
        "printf 'launcher starting\\n'; printf 'runtime pending\\n' >&2; exec sleep 10",
    ]);
    let error = probe_launcher(command, Duration::from_secs(1))
        .await
        .unwrap_err();
    let diagnostic = format!("{error:#}");
    assert!(
        diagnostic.contains("timed out after 1 seconds"),
        "{diagnostic}"
    );
    assert!(diagnostic.contains("launcher starting"), "{diagnostic}");
    assert!(diagnostic.contains("runtime pending"), "{diagnostic}");
}

async fn verify_live_runtime(game: &str, loader: Option<(&str, LoaderKind)>) -> anyhow::Result<()> {
    verify_live_distribution(game, loader, false).await
}
async fn verify_live_distribution(
    game: &str,
    loader: Option<(&str, LoaderKind)>,
    managed: bool,
) -> anyhow::Result<()> {
    let runtime = RuntimeResolution {
        minecraft: GameVersion::parse(game)?,
        loader: loader.map_or(LoaderKind::Vanilla, |(_, kind)| kind),
        loader_version: loader
            .map(|(version, _)| LoaderVersion::parse(version))
            .transpose()?,
    };
    let executable = if cfg!(windows) { "java.exe" } else { "java" };
    let java_home = if matches!(game, "1.12.2" | "1.7.10")
        || (game == "1.16.5" && runtime.loader == LoaderKind::Forge)
    {
        Some(std::env::var_os("EMPACK_TEST_JAVA8_HOME").ok_or_else(|| {
            anyhow::anyhow!("Historical runtime probes require EMPACK_TEST_JAVA8_HOME")
        })?)
    } else {
        std::env::var_os("JAVA_HOME")
    };
    let java = java_home
        .map(|home| PathBuf::from(home).join("bin").join(executable))
        .unwrap_or_else(|| executable.into());
    let root = tempfile::tempdir()?;
    package_runtime(runtime, java.clone(), root.path(), managed).await?;
    let entrypoint = |name: &str| {
        if cfg!(windows) {
            let mut command = std::process::Command::new("cmd.exe");
            command.args(["/D", "/C"]).arg(format!("{name}.bat"));
            command
        } else {
            let mut command = std::process::Command::new("bash");
            command.arg(format!("{name}.sh"));
            command
        }
    };
    let mut command = entrypoint("start");
    if java.is_absolute() {
        command.env(
            "JAVA_HOME",
            java.parent().and_then(std::path::Path::parent).unwrap(),
        );
    }
    let state = tempfile::tempdir()?;
    if managed {
        let name = if cfg!(windows) {
            "empack.exe"
        } else {
            "empack"
        };
        let binary = std::env::var_os("EMPACK_E2E_BIN")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../../target/debug")
                    .join(name)
            });
        anyhow::ensure!(
            binary.file_name().is_some_and(|value| value == name),
            "Managed launcher needs the normal empack executable name"
        );
        anyhow::ensure!(
            binary.is_absolute() && binary.is_file(),
            "Invalid EMPACK_E2E_BIN"
        );
        let paths =
            std::env::join_paths(std::iter::once(binary.parent().unwrap().to_owned()).chain(
                std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
            ))?;
        command
            .env("PATH", paths)
            .env("EMPACK_STATE_DIR", state.path().join("state"))
            .env("EMPACK_CACHE_DIR", state.path().join("cache"));
    }
    let historical = matches!(game, "1.12.2" | "1.7.10");
    command.env("EMPACK_NO_PAUSE", "1").current_dir(root.path());
    if managed {
        // First installation verifies/copies the complete native inventory. It is
        // not Minecraft startup, and must finish before measuring that boundary.
        // Retain a separate finite budget and test the generated installer itself.
        let mut install = entrypoint("install_pack");
        install.current_dir(root.path());
        for (key, value) in command.get_envs() {
            match value {
                Some(value) => {
                    install.env(key, value);
                }
                None => {
                    install.env_remove(key);
                }
            }
        }
        let started = std::time::Instant::now();
        let installed = probe_launcher(install, Duration::from_secs(300))
            .await
            .context("Generated first-installation phase failed")?;
        anyhow::ensure!(
            installed.success,
            "Generated installer failed: {}",
            installed.error_output()
        );
        anyhow::ensure!(
            root.path().join(".empack/instance.json").is_file(),
            "Installer omitted completed ownership"
        );
        anyhow::ensure!(
            fs::read(root.path().join("game/config/runtime-check.txt"))? == b"current content",
            "Installer omitted selected content"
        );
        eprintln!(
            "Generated first installation completed in {:?}",
            started.elapsed()
        );
    }
    if !historical {
        command.arg("--help");
    }
    let started = std::time::Instant::now();
    let output = probe_launcher(command, Duration::from_secs(90))
        .await
        .context("Generated server startup phase failed")?;
    eprintln!("Generated startup completed in {:?}", started.elapsed());
    anyhow::ensure!(
        output.success,
        "Actual server launcher failed: {}",
        output.error_output()
    );
    anyhow::ensure!(
        fs::read(root.path().join("game/config/runtime-check.txt"))? == b"current content",
        "Distribution omitted current game content"
    );
    if managed {
        anyhow::ensure!(
            root.path().join(".empack/instance.json").is_file(),
            "Managed launch omitted completed ownership"
        );
    }
    if historical {
        let eula = fs::read_to_string(root.path().join("game/eula.txt"))?;
        anyhow::ensure!(
            eula.lines().any(|line| line.trim() == "eula=false"),
            "Historical startup did not preserve EULA refusal"
        );
        anyhow::ensure!(
            output.stdout.contains("You need to agree to the EULA"),
            "Historical server did not reach its EULA gate: {}",
            output.error_output()
        );
    } else {
        anyhow::ensure!(
            !root.path().join("game/eula.txt").exists(),
            "Help probe unexpectedly created an EULA file"
        );
        anyhow::ensure!(
            (output.stdout.contains("--help") || output.stderr.contains("--help"))
                && (output.stdout.contains("--port") || output.stderr.contains("--port")),
            "Launcher did not reach Minecraft help: {}",
            output.error_output()
        );
    }
    Ok(())
}
// Exercise the published distribution, not merely loose copies of prepared runtime files.
async fn package_runtime(
    runtime: RuntimeResolution,
    java: PathBuf,
    destination: &std::path::Path,
    managed: bool,
) -> anyhow::Result<()> {
    use empack_core::{distribution::Recipe, model::NonEmpty};
    use empack_core::{
        inventory::OptionalPolicy,
        model::{ResolutionLock, ResolvedProject},
        path::{PathSyntax, PortableRelPath},
    };
    use empack_lib::engine::{
        api::{
            BuildOutput, BuildRequest, Engine, EngineConfig, ExecutionGrant, ExecutionOutcome,
            ExecutionReceipt, NetworkPermission, OperationResources, Preparation,
        },
        documents::DocumentCodec,
        mrpack::OptionalConversion,
        templates::TemplateOptions,
    };
    let project = tempfile::tempdir()?;
    let host = tempfile::tempdir()?;
    let codec = DocumentCodec;
    let mut intent = codec.decode_intent(b"schema: 5\npack: {id: fixture, name: Runtime, version: test}\nruntime: {java: 21, minecraft: '1.20.1', loader: {kind: vanilla}}\nbuild: {profiles: {server: {target: server, delivery: bundled, environment: server, updates: snapshot, format: zip}}, default: [server]}\ndependencies: {}\nlayout: {}\nextensions: {}\n", "runtime-smoke")?.intent().clone();
    intent.runtime.minecraft = runtime.minecraft.clone();
    intent.runtime.loader = runtime.loader;
    intent.runtime.loader_version = runtime.loader_version.clone();
    if managed {
        intent.metadata.id = "runtime.smoke".into();
        intent.runtime.java_major = 21;
        intent.installation_policies = Default::default();
    }
    let bytes = codec.encode_intent(&intent)?;
    let revision = codec
        .decode_intent(&bytes, "runtime-smoke")?
        .semantic_revision();
    let resolved = ResolvedProject::validate(
        intent,
        ResolutionLock {
            acceptable_versions: Vec::new(),
            intent_revision: revision,
            resolver: "runtime-smoke".into(),
            dependencies: Default::default(),
            required_edges: Default::default(),
            coverage: Default::default(),
            runtime: runtime.clone(),
        },
        revision,
    )?;
    fs::write(project.path().join("empack.yml"), bytes)?;
    fs::write(
        project.path().join("empack.lock"),
        codec.encode_lock(&resolved)?,
    )?;
    fs::create_dir_all(project.path().join("pack/config"))?;
    fs::write(
        project.path().join("pack/config/runtime-check.txt"),
        b"current content",
    )?;
    let artifact = PortableRelPath::parse("server.zip", PathSyntax::ArtifactName)?;
    let work = ResourceRequest {
        jobs: 1,
        memory_bytes: 64 << 20,
        scratch_bytes: 128 << 20,
        open_files: 64,
    };
    let engine = Engine::new(
        EngineConfig {
            state_root: host.path().join("state"),
            retained_operations: 2,
            resources: OperationResources {
                capture: work,
                prepared: ResourceRequest {
                    jobs: 0,
                    memory_bytes: 4 << 20,
                    scratch_bytes: 0,
                    open_files: 1,
                },
                local_acquisition: work,
                acquired: ResourceRequest {
                    jobs: 0,
                    memory_bytes: 4 << 20,
                    scratch_bytes: 16 << 20,
                    open_files: 16,
                },
                assembly: ResourceRequest {
                    jobs: 1,
                    memory_bytes: 128 << 20,
                    scratch_bytes: 2 << 30,
                    open_files: 1200,
                },
                receipt: ResourceRequest {
                    jobs: 0,
                    memory_bytes: 16 << 20,
                    scratch_bytes: 0,
                    open_files: 0,
                },
            },
            snapshot: SnapshotLimits {
                entries: 1024,
                depth: 32,
                file_bytes: 128 << 20,
                total_bytes: 512 << 20,
            },
            archive: ArchiveLimits::default(),
            transfer: TransferLimits {
                file_bytes: 128 << 20,
                transfer_bytes: 128 << 20,
                deadline: Duration::from_secs(45),
                ..TransferLimits::default()
            },
            installer: InstallerExecution {
                java,
                deadline: Duration::from_secs(240),
                heap_megabytes: 1024,
                output: SnapshotLimits {
                    entries: 1024,
                    depth: 32,
                    file_bytes: 128 << 20,
                    total_bytes: 512 << 20,
                },
            },
        },
        ResourceGovernor::new(ResourceRequest {
            jobs: 2,
            memory_bytes: 3 << 30,
            scratch_bytes: 3 << 30,
            open_files: 2400,
        }),
    )?;
    let request = BuildRequest {
        clean: false,
        outputs: NonEmpty::new(vec![BuildOutput {
            archive: empack_core::model::DistributionArchive::Zip,
            target: if managed {
                Recipe::SERVER_REFERENCES
            } else {
                Recipe::SERVER_BUNDLED
            },
            artifact,
        }])?,
        optional: OptionalPolicy::Preserve,
        mrpack_optional: OptionalConversion::RejectMetadataLoss,
        templates: TemplateOptions::default(),
        evidence: SourceEvidencePolicy::Compatibility,
    };
    let prepared = match engine.prepare(project.path().to_owned(), request).await? {
        Preparation::Ready(value) => value,
        Preparation::NeedsInput(_) => {
            anyhow::bail!("Runtime smoke unexpectedly needs manual content")
        }
    };
    anyhow::ensure!(
        !host.path().join("state").exists(),
        "Preparation created host state"
    );
    let grant = ExecutionGrant {
        plan: prepared.view().plan(),
        network: NetworkPermission::Allow,
        run_installer: true,
        run_runtime: false,
        replacement: None,
    };
    let mut handle = engine.start(prepared.authorize(grant)?)?;
    let result = handle.wait().await;
    engine.shutdown().await;
    match &*result {
        OperationOutcome::Completed(ExecutionOutcome::Completed(ExecutionReceipt::Build(
            receipt,
        ))) => {
            anyhow::ensure!(
                receipt.artifacts.len() == 1 && receipt.artifacts[0].server_runtime.is_some(),
                "Published archive lacks runtime evidence"
            );
        }
        OperationOutcome::Completed(
            ExecutionOutcome::FailedBeforePublication(error)
            | ExecutionOutcome::ExecutionUncertain(error),
        ) => anyhow::bail!("Runtime build failed: {error:#}"),
        OperationOutcome::Completed(ExecutionOutcome::RecoveryRequired { cause, .. }) => {
            anyhow::bail!("Runtime publication requires recovery: {cause:#}")
        }
        _ => anyhow::bail!("Runtime build did not complete"),
    }
    zip::ZipArchive::new(fs::File::open(project.path().join("dist/server.zip"))?)?
        .extract(destination)?;
    if managed {
        anyhow::ensure!(
            !destination.join("game/config/runtime-check.txt").exists(),
            "Managed content must be activated through the engine"
        );
    }
    Ok(())
}

#[tokio::test]
#[ignore = "live official HTTP endpoints and Java 17/21; run mise run smoke:runtime"]
async fn runtime_server_vanilla() {
    verify_live_runtime("1.20.1", None).await.unwrap();
}
#[tokio::test]
#[ignore = "live official HTTP endpoints and Java 17/21; run mise run smoke:runtime"]
async fn runtime_server_fabric_classpath() {
    verify_live_runtime("1.20.1", Some(("0.16.0", LoaderKind::Fabric)))
        .await
        .unwrap();
}
#[tokio::test]
#[ignore = "live official HTTP endpoints and Java 17/21; run mise run smoke:runtime"]
async fn runtime_server_fabric_shaded() {
    verify_live_runtime("1.16.5", Some(("0.11.7", LoaderKind::Fabric)))
        .await
        .unwrap();
}

#[tokio::test]
#[ignore = "live official HTTP endpoints and Java 17/21; run mise run smoke:runtime"]
async fn runtime_server_quilt() {
    verify_live_runtime("1.20.1", Some(("0.26.3", LoaderKind::Quilt)))
        .await
        .unwrap();
}

#[tokio::test]
#[ignore = "live official Forge/NeoForge Maven; run mise run smoke:runtime"]
async fn runtime_installer_profiles_bind_modern_and_historical_releases() {
    use empack_lib::engine::server_runtime::installer::InstallerServerPlan;
    for (game, loader, version) in [
        ("1.20.1", LoaderKind::Forge, "47.4.0"),
        ("1.12.2", LoaderKind::Forge, "14.23.5.2860"),
        ("1.16.5", LoaderKind::Forge, "36.2.39"),
        ("1.7.10", LoaderKind::Forge, "10.13.4.1614"),
        ("1.21.1", LoaderKind::NeoForge, "21.1.209"),
        ("1.20.1", LoaderKind::NeoForge, "47.1.106"),
    ] {
        let runtime = RuntimeResolution {
            minecraft: GameVersion::parse(game).unwrap(),
            loader,
            loader_version: Some(LoaderVersion::parse(version).unwrap()),
        };
        let owner = OperationRuntime::new(
            ResourceGovernor::new(ResourceRequest {
                jobs: 2,
                memory_bytes: 128 << 20,
                scratch_bytes: 128 << 20,
                open_files: 16,
            }),
            1,
        );
        let mut handle = owner
            .start(move |mut scope| async move {
                Ok(InstallerServerPlan::resolve(
                    &HttpAcquisition::new().unwrap(),
                    &mut scope,
                    runtime,
                    TransferLimits {
                        file_bytes: 32 << 20,
                        transfer_bytes: 32 << 20,
                        deadline: Duration::from_secs(45),
                        ..TransferLimits::default()
                    },
                    ArchiveLimits::default(),
                    SourceEvidencePolicy::Compatibility,
                )
                .await)
            })
            .unwrap();
        let outcome = handle.wait().await;
        owner.shutdown().await;
        match &*outcome {
            OperationOutcome::Completed(Ok(plan)) => {
                assert!(!plan.contract().libraries.is_empty());
                assert!(plan.expected().digests.is_some());
            }
            OperationOutcome::Completed(Err(error)) => {
                panic!("{game} {loader:?} {version}: {error:#}")
            }
            OperationOutcome::Failed(error) => panic!("Installer operation failed: {error}"),
        }
    }
}

#[tokio::test]
#[ignore = "live official Forge installer and Java 17; run mise run smoke:runtime"]
async fn runtime_server_forge() {
    verify_live_runtime("1.20.1", Some(("47.4.0", LoaderKind::Forge)))
        .await
        .unwrap();
}

#[tokio::test]
#[ignore = "live official NeoForge installer and Java 21; run mise run smoke:runtime"]
async fn runtime_server_neoforge() {
    verify_live_runtime("1.21.1", Some(("21.1.209", LoaderKind::NeoForge)))
        .await
        .unwrap();
}

#[tokio::test]
#[ignore = "live official early NeoForge installer and Java 17/21; run mise run smoke:runtime"]
async fn runtime_server_neoforge_early() {
    verify_live_runtime("1.20.1", Some(("47.1.106", LoaderKind::NeoForge)))
        .await
        .unwrap();
}

#[tokio::test]
#[ignore = "live historical Forge and EMPACK_TEST_JAVA8_HOME; run mise run smoke:runtime"]
async fn runtime_server_forge_112() {
    verify_live_runtime("1.12.2", Some(("14.23.5.2860", LoaderKind::Forge)))
        .await
        .unwrap();
}

#[tokio::test]
#[ignore = "live historical Forge and EMPACK_TEST_JAVA8_HOME; run mise run smoke:runtime"]
async fn runtime_server_forge_1710() {
    verify_live_runtime("1.7.10", Some(("10.13.4.1614", LoaderKind::Forge)))
        .await
        .unwrap();
}

#[tokio::test]
#[ignore = "live historical Forge and EMPACK_TEST_JAVA8_HOME; run mise run smoke:runtime"]
async fn runtime_server_forge_116() {
    verify_live_runtime("1.16.5", Some(("36.2.39", LoaderKind::Forge)))
        .await
        .unwrap();
}

#[tokio::test]
#[ignore = "live official runtime, Java 21 and a fresh EMPACK_E2E_BIN"]
async fn runtime_managed_server_vanilla() {
    verify_live_distribution("1.20.1", None, true)
        .await
        .unwrap();
}
#[tokio::test]
#[ignore = "live official NeoForge installer, Java 21 and a fresh EMPACK_E2E_BIN"]
async fn runtime_managed_server_neoforge() {
    verify_live_distribution("1.21.1", Some(("21.1.209", LoaderKind::NeoForge)), true)
        .await
        .unwrap();
}
