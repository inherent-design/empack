use anyhow::Result;
use empack_lib::application::session::{FileMetadata, FileSystemProvider, ProcessOutput};
use empack_lib::application::{BuildArgs, Commands, execute_command_with_session};
use empack_lib::display::Display;
use empack_lib::empack::RestrictedModInfo;
use empack_lib::empack::archive::ArchiveFormat;
use empack_lib::terminal::TerminalCapabilities;

use empack_tests::test_env::MockSessionBuilder;

#[tokio::test]
async fn e2e_build_continue_resumes_restricted_full_build() -> Result<()> {
    let project_name = "continue-workflow";
    let workdir = empack_lib::application::session_mocks::mock_root().join("workdir");
    let bootstrap_jar = workdir
        .join("cache")
        .join("packwiz-installer-bootstrap.jar");
    let installer_jar = workdir.join("cache").join("packwiz-installer.jar");
    let pack_toml = workdir
        .join("dist")
        .join("client-full")
        .join("pack")
        .join("pack.toml");

    let java_key = (
        "java".to_string(),
        vec![
            "-jar".to_string(),
            bootstrap_jar.to_string_lossy().to_string(),
            "--bootstrap-main-jar".to_string(),
            installer_jar.to_string_lossy().to_string(),
            "-g".to_string(),
            "-s".to_string(),
            "both".to_string(),
            pack_toml.to_string_lossy().to_string(),
        ],
    );

    let mut session = MockSessionBuilder::new()
        .with_empack_project(project_name, "1.21.1", "fabric")
        .with_pre_cached_jars()
        .build();

    session.process_provider.results.insert(
        java_key.clone(),
        Ok(ProcessOutput {
            stdout:
                "Failed to download modpack, the following errors were encountered:\nOptiFine.jar:"
                    .to_string(),
            stderr: format!(
                "java.lang.Exception: This mod is excluded from the CurseForge API and must be downloaded manually.\nPlease go to https://www.curseforge.com/minecraft/mc-mods/optifine/files/4912891 and save this file to {}\n\tat link.infra.packwiz.installer.DownloadTask.download(DownloadTask.java:42)",
                workdir
                    .join("dist")
                    .join("client-full")
                    .join("mods")
                    .join("OptiFine.jar")
                    .to_string_lossy()
            ),
            success: false,
        }),
    );

    Display::init_or_get(TerminalCapabilities::minimal());

    let first = execute_command_with_session(
        Commands::Build(BuildArgs {
            targets: vec!["client-full".to_string()],
            ..Default::default()
        }),
        &session,
    )
    .await;

    assert!(
        first.is_err(),
        "first build should stop for restricted download: {first:?}"
    );

    let pending =
        empack_lib::empack::restricted_build::load_pending_build(session.filesystem(), &workdir)?
            .expect("pending restricted build should be recorded");
    session.filesystem().write_bytes(
        &pending.restricted_cache_path().join("OptiFine.jar"),
        b"cached bytes",
    )?;

    session.process_provider.results.remove(&java_key);

    let second = execute_command_with_session(
        Commands::Build(BuildArgs {
            continue_build: true,
            ..Default::default()
        }),
        &session,
    )
    .await;

    assert!(second.is_ok(), "continue build should succeed: {second:?}");
    assert!(
        session.filesystem().exists(
            &workdir
                .join("dist")
                .join("client-full")
                .join("mods")
                .join("OptiFine.jar")
        ),
        "restricted jar should be restored into the continued distribution tree"
    );
    assert!(
        session.filesystem().exists(
            &workdir
                .join("dist")
                .join(format!("{project_name}-v1.0.0-client-full.zip"))
        ),
        "continued build should produce the client-full archive"
    );
    assert!(
        empack_lib::empack::restricted_build::load_pending_build(session.filesystem(), &workdir)?
            .is_none(),
        "pending restricted state should be cleared after continue succeeds"
    );

    Ok(())
}

#[tokio::test]
async fn e2e_build_continue_imports_recent_unicode_variant_download() -> Result<()> {
    let project_name = "continue-unicode-variant";
    let workdir = empack_lib::application::session_mocks::mock_root().join("workdir");
    let downloads_dir = workdir.join("manual-downloads");
    let variant_name = "§6No Enchant Glint 1.20.1.zip";
    let cache_filename = "No_Enchant_Glint.zip";
    let download_bytes = b"manual resource pack bytes";

    let session = MockSessionBuilder::new()
        .with_empack_project(project_name, "1.21.1", "fabric")
        .build();

    let pending = empack_lib::empack::restricted_build::save_pending_build(
        session.filesystem(),
        &workdir,
        &[empack_lib::primitives::BuildTarget::Mrpack],
        ArchiveFormat::Zip,
        &[RestrictedModInfo {
            name: "No Enchant Glint".to_string(),
            url: "https://www.curseforge.com/minecraft/texture-packs/no-enchant-glint/download/4660358"
                .to_string(),
            dest_path: workdir
                .join("packwiz-cache")
                .join("import")
                .join(cache_filename)
                .to_string_lossy()
                .to_string(),
        }],
    )?;

    session
        .filesystem_provider
        .write_bytes(&downloads_dir.join(variant_name), download_bytes)?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    session.filesystem_provider.set_file_metadata(
        downloads_dir.join(variant_name),
        FileMetadata {
            is_directory: false,
            len: download_bytes.len() as u64,
            modified_unix_ms: Some(now),
            created_unix_ms: Some(now),
        },
    );

    Display::init_or_get(TerminalCapabilities::minimal());

    let result = execute_command_with_session(
        Commands::Build(BuildArgs {
            continue_build: true,
            downloads_dir: Some(downloads_dir.to_string_lossy().to_string()),
            ..Default::default()
        }),
        &session,
    )
    .await;

    assert!(
        result.is_ok(),
        "continue build should import a recent Unicode variant download: {result:?}"
    );
    assert!(
        session
            .filesystem()
            .exists(&pending.restricted_cache_path().join(cache_filename)),
        "variant download should be copied into the managed restricted cache"
    );
    assert!(
        session.filesystem().exists(
            &workdir
                .join("dist")
                .join(format!("{project_name}-v1.0.0.mrpack"))
        ),
        "continued build should produce the mrpack archive"
    );
    assert!(
        empack_lib::empack::restricted_build::load_pending_build(session.filesystem(), &workdir)?
            .is_none(),
        "pending restricted state should be cleared after continue succeeds"
    );

    Ok(())
}
