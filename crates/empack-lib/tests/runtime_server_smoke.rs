//! Explicit live runtime probes. Missing HTTP/Java prerequisites fail this opt-in suite.
use empack_core::model::{GameVersion, LoaderKind, LoaderVersion, RuntimeResolution};
use empack_lib::{
    application::process_runtime::{Cancellation, execute_async},
    engine::{
        acquisition::{HttpAcquisition, TransferLimits},
        artifacts::ArchiveLimits,
        content::SourceEvidencePolicy,
        resources::{ResourceGovernor, ResourceRequest},
        runtime::{OperationOutcome, OperationRuntime},
        server_runtime::{
            ServerLaunch, VanillaServerPlan,
            installer::{InstallerExecution, InstallerServerPlan},
            library::LibraryServerPlan,
        },
        snapshot::SnapshotLimits,
    },
};
use std::{fs, path::PathBuf, time::Duration};
async fn verify_live_runtime(game: &str, loader: Option<(&str, LoaderKind)>) -> anyhow::Result<()> {
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
    let installer_java = java.clone();
    let installer = matches!(runtime.loader, LoaderKind::Forge | LoaderKind::NeoForge);
    let transport = HttpAcquisition::new()?;
    let owner = OperationRuntime::new(
        ResourceGovernor::new(ResourceRequest {
            jobs: 2,
            memory_bytes: if installer { 2 << 30 } else { 128 << 20 },
            scratch_bytes: if installer { 2 << 30 } else { 512 << 20 },
            open_files: if installer { 1100 } else { 64 },
        }),
        1,
    );
    let mut handle = owner.start(move |mut scope| async move {
        let limits = TransferLimits {
            file_bytes: 128 << 20,
            transfer_bytes: 128 << 20,
            deadline: Duration::from_secs(45),
            ..TransferLimits::default()
        };
        Ok(async {
            if runtime.loader == LoaderKind::Vanilla {
                VanillaServerPlan::resolve(&transport, &mut scope, runtime, limits)
                    .await?
                    .acquire(
                        &transport,
                        &mut scope,
                        limits,
                        ArchiveLimits::default(),
                        SourceEvidencePolicy::Compatibility,
                    )
                    .await
            } else if installer {
                InstallerServerPlan::resolve(
                    &transport,
                    &mut scope,
                    runtime,
                    limits,
                    ArchiveLimits::default(),
                    SourceEvidencePolicy::Compatibility,
                )
                .await?
                .prepare(
                    &transport,
                    &mut scope,
                    limits,
                    ArchiveLimits::default(),
                    SourceEvidencePolicy::Compatibility,
                    InstallerExecution {
                        java: installer_java,
                        deadline: Duration::from_secs(240),
                        heap_megabytes: 1024,
                        output: SnapshotLimits {
                            entries: 1024,
                            depth: 32,
                            file_bytes: 128 << 20,
                            total_bytes: 512 << 20,
                        },
                    },
                )
                .await
            } else {
                LibraryServerPlan::resolve(&transport, &mut scope, runtime, limits)
                    .await?
                    .acquire(
                        &transport,
                        &mut scope,
                        limits,
                        ArchiveLimits::default(),
                        SourceEvidencePolicy::Compatibility,
                    )
                    .await
            }
        }
        .await)
    })?;
    let terminal = handle.wait().await;
    owner.shutdown().await;
    let prepared = match &*terminal {
        OperationOutcome::Completed(Ok(runtime)) => runtime,
        OperationOutcome::Completed(Err(error)) => {
            anyhow::bail!("Runtime preparation failed: {error:#}")
        }
        OperationOutcome::Failed(error) => anyhow::bail!("Runtime operation failed: {error}"),
    };
    let root = tempfile::tempdir()?;
    for (relative, file) in prepared.files() {
        let destination = root.path().join(relative.as_str());
        fs::create_dir_all(destination.parent().unwrap())?;
        file.content.lease().copy_verified(
            &mut fs::File::create(destination)?,
            &Cancellation::default(),
        )?;
    }
    let mut command = std::process::Command::new(java);
    match prepared.launch() {
        ServerLaunch::Jar(path) => {
            command.args(["-jar", path.as_str()]);
        }
        ServerLaunch::Arguments { unix, windows } => {
            let path = if cfg!(windows) { windows } else { unix };
            command
                .arg("@user_jvm_args.txt")
                .arg(format!("@{}", path.as_str()));
        }
    }
    let historical = matches!(game, "1.12.2" | "1.7.10");
    command
        .arg(if historical { "nogui" } else { "--help" })
        .current_dir(root.path());
    let output = execute_async(
        command,
        Duration::from_secs(90),
        Cancellation::default(),
        None,
    )
    .await?;
    anyhow::ensure!(
        output.success,
        "Actual server launcher failed: {}",
        output.error_output()
    );
    if historical {
        let eula = fs::read_to_string(root.path().join("eula.txt"))?;
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
            !root.path().join("eula.txt").exists(),
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
