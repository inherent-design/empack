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
        server_runtime::{VanillaServerPlan, library::LibraryServerPlan},
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
    let transport = HttpAcquisition::new()?;
    let owner = OperationRuntime::new(
        ResourceGovernor::new(ResourceRequest {
            jobs: 2,
            memory_bytes: 128 << 20,
            scratch_bytes: 512 << 20,
            open_files: 64,
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
    let executable = if cfg!(windows) { "java.exe" } else { "java" };
    let java = std::env::var_os("JAVA_HOME")
        .map(|home| PathBuf::from(home).join("bin").join(executable))
        .unwrap_or_else(|| executable.into());
    let mut command = std::process::Command::new(java);
    command
        .args(["-jar", "server.jar", "--help"])
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
    anyhow::ensure!(
        !root.path().join("eula.txt").exists(),
        "Help probe unexpectedly created an EULA file"
    );
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
