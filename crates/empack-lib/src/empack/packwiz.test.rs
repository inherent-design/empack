// Tests for packwiz integration

use super::*;
use crate::application::session::ProcessOutput;
use crate::application::session_mocks::{
    MockCommandSession, MockFileSystemProvider, MockProcessProvider, mock_root,
};
use std::path::PathBuf;

fn packwiz_env_lock() -> crate::test_support::EnvLockGuard<'static> {
    crate::test_support::env_lock().lock().unwrap()
}

#[test]
fn test_add_mod_modrinth_success() {
    let _env_lock = packwiz_env_lock();
    let mock_process = MockProcessProvider::new().with_packwiz_result(
        vec![
            "--pack-file".to_string(),
            mock_root()
                .join("workdir")
                .join("pack")
                .join("pack.toml")
                .to_string_lossy()
                .to_string(),
            "modrinth".to_string(),
            "add".to_string(),
            "--project-id".to_string(),
            "AANobbMI".to_string(),
            "-y".to_string(),
        ],
        Ok(ProcessOutput {
            stdout: "Added Sodium".to_string(),
            stderr: String::new(),
            success: true,
        }),
    );

    let session = MockCommandSession::new()
        .with_process(mock_process)
        .with_filesystem(
            MockFileSystemProvider::new().with_current_dir(mock_root().join("workdir")),
        );

    let mut metadata = PackwizMetadata::new(&session).unwrap();
    let result = metadata.add_mod("AANobbMI", ProjectPlatform::Modrinth);

    assert!(result.is_ok());
    let pack_file_str = mock_root()
        .join("workdir")
        .join("pack")
        .join("pack.toml")
        .to_string_lossy()
        .to_string();
    assert!(session.process_provider.verify_call(
        crate::empack::packwiz::PACKWIZ_BIN,
        &[
            "--pack-file",
            &pack_file_str,
            "modrinth",
            "add",
            "--project-id",
            "AANobbMI",
            "-y"
        ],
        &mock_root().join("workdir").join("pack")
    ));
}

#[test]
fn test_add_mod_curseforge_success() {
    let _env_lock = packwiz_env_lock();
    let mock_process = MockProcessProvider::new().with_packwiz_result(
        vec![
            "--pack-file".to_string(),
            mock_root()
                .join("workdir")
                .join("pack")
                .join("pack.toml")
                .to_string_lossy()
                .to_string(),
            "curseforge".to_string(),
            "add".to_string(),
            "--addon-id".to_string(),
            "123456".to_string(),
            "-y".to_string(),
        ],
        Ok(ProcessOutput {
            stdout: "Added mod".to_string(),
            stderr: String::new(),
            success: true,
        }),
    );

    let session = MockCommandSession::new()
        .with_process(mock_process)
        .with_filesystem(
            MockFileSystemProvider::new().with_current_dir(mock_root().join("workdir")),
        );

    let mut metadata = PackwizMetadata::new(&session).unwrap();
    let result = metadata.add_mod("123456", ProjectPlatform::CurseForge);

    assert!(result.is_ok());
    let pack_file_str = mock_root()
        .join("workdir")
        .join("pack")
        .join("pack.toml")
        .to_string_lossy()
        .to_string();
    assert!(session.process_provider.verify_call(
        crate::empack::packwiz::PACKWIZ_BIN,
        &[
            "--pack-file",
            &pack_file_str,
            "curseforge",
            "add",
            "--addon-id",
            "123456",
            "-y"
        ],
        &mock_root().join("workdir").join("pack")
    ));
}

#[test]
fn test_add_mod_failure() {
    let _env_lock = packwiz_env_lock();
    let mock_process = MockProcessProvider::new().with_packwiz_result(
        vec![
            "--pack-file".to_string(),
            mock_root()
                .join("workdir")
                .join("pack")
                .join("pack.toml")
                .to_string_lossy()
                .to_string(),
            "modrinth".to_string(),
            "add".to_string(),
            "--project-id".to_string(),
            "INVALID".to_string(),
            "-y".to_string(),
        ],
        Ok(ProcessOutput {
            stdout: String::new(),
            stderr: "Project not found".to_string(),
            success: false,
        }),
    );

    let session = MockCommandSession::new()
        .with_process(mock_process)
        .with_filesystem(
            MockFileSystemProvider::new().with_current_dir(mock_root().join("workdir")),
        );

    let mut metadata = PackwizMetadata::new(&session).unwrap();
    let result = metadata.add_mod("INVALID", ProjectPlatform::Modrinth);

    assert!(result.is_err());
    match result.unwrap_err() {
        PackwizError::CommandFailed { stderr, .. } => {
            assert_eq!(stderr, "Project not found");
        }
        _ => panic!("Wrong error type"),
    }
}

#[test]
fn test_remove_mod_success() {
    let _env_lock = packwiz_env_lock();
    let mock_process = MockProcessProvider::new().with_packwiz_result(
        vec![
            "--pack-file".to_string(),
            mock_root()
                .join("workdir")
                .join("pack")
                .join("pack.toml")
                .to_string_lossy()
                .to_string(),
            "remove".to_string(),
            "sodium".to_string(),
            "-y".to_string(),
        ],
        Ok(ProcessOutput {
            stdout: "Removed sodium".to_string(),
            stderr: String::new(),
            success: true,
        }),
    );

    let session = MockCommandSession::new()
        .with_process(mock_process)
        .with_filesystem(
            MockFileSystemProvider::new().with_current_dir(mock_root().join("workdir")),
        );

    let mut metadata = PackwizMetadata::new(&session).unwrap();
    let result = metadata.remove_mod("sodium");

    assert!(result.is_ok());
    let pack_file_str = mock_root()
        .join("workdir")
        .join("pack")
        .join("pack.toml")
        .to_string_lossy()
        .to_string();
    assert!(session.process_provider.verify_call(
        crate::empack::packwiz::PACKWIZ_BIN,
        &["--pack-file", &pack_file_str, "remove", "sodium", "-y"],
        &mock_root().join("workdir").join("pack")
    ));
}

#[test]
fn test_remove_mod_not_found() {
    let _env_lock = packwiz_env_lock();
    let mock_process = MockProcessProvider::new().with_packwiz_result(
        vec![
            "--pack-file".to_string(),
            mock_root()
                .join("workdir")
                .join("pack")
                .join("pack.toml")
                .to_string_lossy()
                .to_string(),
            "remove".to_string(),
            "nonexistent".to_string(),
            "-y".to_string(),
        ],
        Ok(ProcessOutput {
            stdout: String::new(),
            stderr: "Mod not found".to_string(),
            success: false,
        }),
    );

    let session = MockCommandSession::new()
        .with_process(mock_process)
        .with_filesystem(
            MockFileSystemProvider::new().with_current_dir(mock_root().join("workdir")),
        );

    let mut metadata = PackwizMetadata::new(&session).unwrap();
    let result = metadata.remove_mod("nonexistent");

    assert!(result.is_err());
    match result.unwrap_err() {
        PackwizError::CommandFailed { stderr, .. } => {
            assert!(
                stderr.contains("not found"),
                "Error should indicate mod not found, got: {}",
                stderr
            );
        }
        other => panic!("Expected CommandFailed error, got: {:?}", other),
    }
}

#[test]
fn test_refresh_index_success() {
    let _env_lock = packwiz_env_lock();
    let mock_process = MockProcessProvider::new().with_packwiz_result(
        vec![
            "--pack-file".to_string(),
            mock_root()
                .join("workdir")
                .join("pack")
                .join("pack.toml")
                .to_string_lossy()
                .to_string(),
            "refresh".to_string(),
        ],
        Ok(ProcessOutput {
            stdout: "Refreshed index".to_string(),
            stderr: String::new(),
            success: true,
        }),
    );

    let session = MockCommandSession::new()
        .with_process(mock_process)
        .with_filesystem(
            MockFileSystemProvider::new().with_current_dir(mock_root().join("workdir")),
        );

    let mut metadata = PackwizMetadata::new(&session).unwrap();
    let result = metadata.refresh_index();

    assert!(result.is_ok());
    let pack_file_str = mock_root()
        .join("workdir")
        .join("pack")
        .join("pack.toml")
        .to_string_lossy()
        .to_string();
    assert!(session.process_provider.verify_call(
        crate::empack::packwiz::PACKWIZ_BIN,
        &["--pack-file", &pack_file_str, "refresh"],
        &mock_root().join("workdir").join("pack")
    ));
}

#[test]
fn test_refresh_index_hash_mismatch() {
    let _env_lock = packwiz_env_lock();
    let mock_process = MockProcessProvider::new().with_packwiz_result(
        vec![
            "--pack-file".to_string(),
            mock_root()
                .join("workdir")
                .join("pack")
                .join("pack.toml")
                .to_string_lossy()
                .to_string(),
            "refresh".to_string(),
        ],
        Ok(ProcessOutput {
            stdout: String::new(),
            stderr: "Error: Hash mismatch for mods/sodium.pw.toml".to_string(),
            success: false,
        }),
    );

    let session = MockCommandSession::new()
        .with_process(mock_process)
        .with_filesystem(
            MockFileSystemProvider::new().with_current_dir(mock_root().join("workdir")),
        );

    let mut metadata = PackwizMetadata::new(&session).unwrap();
    let result = metadata.refresh_index();

    assert!(result.is_err());
    match result.unwrap_err() {
        PackwizError::HashMismatchError(msg) => {
            assert!(msg.contains("Hash mismatch"));
        }
        _ => panic!("Wrong error type"),
    }
}

#[test]
fn test_refresh_index_pack_format_error() {
    let _env_lock = packwiz_env_lock();
    let mock_process = MockProcessProvider::new().with_packwiz_result(
        vec![
            "--pack-file".to_string(),
            mock_root()
                .join("workdir")
                .join("pack")
                .join("pack.toml")
                .to_string_lossy()
                .to_string(),
            "refresh".to_string(),
        ],
        Ok(ProcessOutput {
            stdout: String::new(),
            stderr: "Error: pack format 'packwiz:1.1.0' is not supported".to_string(),
            success: false,
        }),
    );

    let session = MockCommandSession::new()
        .with_process(mock_process)
        .with_filesystem(
            MockFileSystemProvider::new().with_current_dir(mock_root().join("workdir")),
        );

    let mut metadata = PackwizMetadata::new(&session).unwrap();
    let result = metadata.refresh_index();

    assert!(result.is_err());
    match result.unwrap_err() {
        PackwizError::PackFormatError(msg) => {
            assert!(msg.contains("pack format"));
            assert!(msg.contains("not supported"));
        }
        _ => panic!("Wrong error type"),
    }
}

#[test]
fn test_export_mrpack_success() {
    let _env_lock = packwiz_env_lock();
    let output_path = mock_root().join("workdir").join("dist").join("pack.mrpack");
    let mock_process = MockProcessProvider::new().with_packwiz_result(
        vec![
            "--pack-file".to_string(),
            mock_root()
                .join("workdir")
                .join("pack")
                .join("pack.toml")
                .to_string_lossy()
                .to_string(),
            "modrinth".to_string(),
            "export".to_string(),
            "-o".to_string(),
            mock_root()
                .join("workdir")
                .join("dist")
                .join("pack.mrpack")
                .to_string_lossy()
                .to_string(),
        ],
        Ok(ProcessOutput {
            stdout: "Exported pack.mrpack".to_string(),
            stderr: String::new(),
            success: true,
        }),
    );

    let session = MockCommandSession::new()
        .with_process(mock_process)
        .with_filesystem(
            MockFileSystemProvider::new().with_current_dir(mock_root().join("workdir")),
        );

    let mut metadata = PackwizMetadata::new(&session).unwrap();
    let result = metadata.export_mrpack(&output_path);

    assert!(result.is_ok());
    let pack_file_str = mock_root()
        .join("workdir")
        .join("pack")
        .join("pack.toml")
        .to_string_lossy()
        .to_string();
    let output_str = output_path.to_string_lossy().to_string();
    assert!(session.process_provider.verify_call(
        crate::empack::packwiz::PACKWIZ_BIN,
        &[
            "--pack-file",
            &pack_file_str,
            "modrinth",
            "export",
            "-o",
            &output_str
        ],
        &mock_root().join("workdir").join("pack")
    ));
}

#[test]
fn test_packwiz_unavailable() {
    let _env_lock = packwiz_env_lock();
    // Set EMPACK_PACKWIZ_BIN to a non-existent path so resolve_packwiz_binary()
    // fails early without attempting a network download.
    let _guard = TestEnvGuard::set("EMPACK_PACKWIZ_BIN", "/nonexistent/packwiz-tx");

    let mock_process = MockProcessProvider::new().with_packwiz_unavailable();

    let session = MockCommandSession::new()
        .with_process(mock_process)
        .with_filesystem(
            MockFileSystemProvider::new().with_current_dir(mock_root().join("workdir")),
        );

    let mut metadata = PackwizMetadata::new(&session).unwrap();
    let result = metadata.add_mod("AANobbMI", ProjectPlatform::Modrinth);

    assert!(result.is_err());
    match result.unwrap_err() {
        PackwizError::NotAvailable(msg) => {
            assert!(
                msg.contains("not") || msg.contains("failed") || msg.contains("does not exist"),
                "Expected unavailability message, got: {msg}"
            );
        }
        other => panic!("Expected NotAvailable error, got: {:?}", other),
    }
}

#[test]
fn test_packwiz_env_override_existing_path_is_used() {
    let _env_lock = packwiz_env_lock();
    let temp_dir = tempfile::tempdir().unwrap();
    let override_path = temp_dir.path().join("custom-packwiz-tx");
    std::fs::write(&override_path, b"stub").unwrap();

    let _guard = TestEnvGuard::set("EMPACK_PACKWIZ_BIN", override_path.to_str().unwrap());

    let workdir = mock_root().join("workdir");
    let expected_pack_file = workdir.join("pack").join("pack.toml");
    let expected_pack_file_str = expected_pack_file.to_string_lossy().to_string();
    let mock_process = MockProcessProvider::new().with_result(
        override_path.to_string_lossy().to_string(),
        vec![
            "--pack-file".to_string(),
            expected_pack_file_str.clone(),
            "modrinth".to_string(),
            "add".to_string(),
            "--project-id".to_string(),
            "AANobbMI".to_string(),
            "-y".to_string(),
        ],
        Ok(ProcessOutput {
            stdout: "Added Sodium".to_string(),
            stderr: String::new(),
            success: true,
        }),
    );

    let session = MockCommandSession::new()
        .with_process(mock_process)
        .with_filesystem(MockFileSystemProvider::new().with_current_dir(workdir.clone()));

    let mut metadata = PackwizMetadata::new(&session).unwrap();
    let result = metadata.add_mod("AANobbMI", ProjectPlatform::Modrinth);

    assert!(result.is_ok());
    assert!(session.process_provider.verify_call(
        override_path.to_str().unwrap(),
        &[
            "--pack-file",
            expected_pack_file_str.as_str(),
            "modrinth",
            "add",
            "--project-id",
            "AANobbMI",
            "-y"
        ],
        &workdir.join("pack")
    ));
}

/// RAII guard that sets an environment variable for the duration of a test
/// and restores the previous value on drop.
struct TestEnvGuard {
    key: String,
    prev: Option<String>,
}

impl TestEnvGuard {
    fn set(key: &str, value: &str) -> Self {
        let prev = std::env::var(key).ok();
        unsafe { std::env::set_var(key, value) };
        Self {
            key: key.to_string(),
            prev,
        }
    }
}

impl Drop for TestEnvGuard {
    fn drop(&mut self) {
        match &self.prev {
            Some(v) => unsafe { std::env::set_var(&self.key, v) },
            None => unsafe { std::env::remove_var(&self.key) },
        }
    }
}

#[test]
fn test_installer_success() {
    let bootstrap_jar_path = PathBuf::from("/cache/packwiz-installer-bootstrap.jar");
    let installer_jar_path = PathBuf::from("/cache/packwiz-installer.jar");
    let mock_process = MockProcessProvider::new().with_result(
        "java".to_string(),
        vec![
            "-jar".to_string(),
            "/cache/packwiz-installer-bootstrap.jar".to_string(),
            "--bootstrap-main-jar".to_string(),
            "/cache/packwiz-installer.jar".to_string(),
            "-g".to_string(),
            "-s".to_string(),
            "both".to_string(),
            mock_root()
                .join("workdir")
                .join("pack")
                .join("pack.toml")
                .to_string_lossy()
                .to_string(),
        ],
        Ok(ProcessOutput {
            stdout: "Downloaded 5 mods".to_string(),
            stderr: String::new(),
            success: true,
        }),
    );

    let session = MockCommandSession::new()
        .with_process(mock_process)
        .with_filesystem(
            MockFileSystemProvider::new().with_current_dir(mock_root().join("workdir")),
        );

    let installer = PackwizInstaller::new(&session, bootstrap_jar_path, installer_jar_path);
    let result = installer.install_mods("both", &mock_root().join("workdir"));

    assert!(result.is_ok());
    let pack_file_str = mock_root()
        .join("workdir")
        .join("pack")
        .join("pack.toml")
        .to_string_lossy()
        .to_string();
    assert!(session.process_provider.verify_call(
        "java",
        &[
            "-jar",
            "/cache/packwiz-installer-bootstrap.jar",
            "--bootstrap-main-jar",
            "/cache/packwiz-installer.jar",
            "-g",
            "-s",
            "both",
            &pack_file_str
        ],
        &mock_root().join("workdir")
    ));
}

#[test]
fn test_installer_invalid_side() {
    let bootstrap_jar_path = PathBuf::from("/cache/packwiz-installer-bootstrap.jar");
    let installer_jar_path = PathBuf::from("/cache/packwiz-installer.jar");

    let session = MockCommandSession::new();

    let installer = PackwizInstaller::new(&session, bootstrap_jar_path, installer_jar_path);
    let result = installer.install_mods("invalid", &mock_root().join("workdir"));

    assert!(result.is_err());
    match result.unwrap_err() {
        PackwizError::CommandFailed { stderr, .. } => {
            assert!(stderr.contains("Invalid side"));
        }
        _ => panic!("Wrong error type"),
    }
}

#[test]
fn test_installer_download_failure() {
    let bootstrap_jar_path = PathBuf::from("/cache/packwiz-installer-bootstrap.jar");
    let installer_jar_path = PathBuf::from("/cache/packwiz-installer.jar");
    let mock_process = MockProcessProvider::new().with_result(
        "java".to_string(),
        vec![
            "-jar".to_string(),
            "/cache/packwiz-installer-bootstrap.jar".to_string(),
            "--bootstrap-main-jar".to_string(),
            "/cache/packwiz-installer.jar".to_string(),
            "-g".to_string(),
            "-s".to_string(),
            "client".to_string(),
            mock_root()
                .join("workdir")
                .join("pack")
                .join("pack.toml")
                .to_string_lossy()
                .to_string(),
        ],
        Ok(ProcessOutput {
            stdout: String::new(),
            stderr: "Network error: timeout downloading mod".to_string(),
            success: false,
        }),
    );

    let session = MockCommandSession::new()
        .with_process(mock_process)
        .with_filesystem(
            MockFileSystemProvider::new().with_current_dir(mock_root().join("workdir")),
        );

    let installer = PackwizInstaller::new(&session, bootstrap_jar_path, installer_jar_path);
    let result = installer.install_mods("client", &mock_root().join("workdir"));

    assert!(result.is_err());
    match result.unwrap_err() {
        PackwizError::CommandFailed { stderr, .. } => {
            assert!(stderr.contains("Network error"));
        }
        _ => panic!("Wrong error type"),
    }
}

#[test]
fn test_check_installer_available_uses_filesystem_provider() {
    let bootstrap_jar_path = PathBuf::from("/cache/packwiz-installer-bootstrap.jar");
    let installer_jar_path = PathBuf::from("/cache/packwiz-installer.jar");

    let session = MockCommandSession::new().with_filesystem(
        MockFileSystemProvider::new()
            .with_current_dir(mock_root().join("workdir"))
            .with_file(bootstrap_jar_path.clone(), "jar".to_string()),
    );

    let installer = PackwizInstaller::new(&session, bootstrap_jar_path, installer_jar_path);

    assert!(installer.check_installer_available().unwrap());
}

#[test]
fn test_mock_packwiz_ops_init_writes_supported_loader_line() {
    let workdir = mock_root().join("workdir");
    let ops = MockPackwizOps::new().with_current_dir(workdir.clone());

    ops.run_packwiz_init(
        &workdir,
        "Test Pack",
        "Test Author",
        "1.0.0",
        "fabric",
        "1.21.4",
        "0.15.11",
    )
    .unwrap();

    let fs = ops.filesystem.lock().unwrap();
    let pack_toml = fs
        .get(&workdir.join("pack").join("pack.toml"))
        .expect("pack.toml should be created");
    assert!(pack_toml.contains("name = \"Test Pack\""));
    assert!(pack_toml.contains("minecraft = \"1.21.4\""));
    assert!(pack_toml.contains("fabric = \"0.15.11\""));
    assert!(fs.contains_key(&workdir.join("pack").join("index.toml")));
}

#[test]
fn test_mock_packwiz_ops_init_omits_loader_line_for_none() {
    let workdir = mock_root().join("workdir");
    let ops = MockPackwizOps::new().with_current_dir(workdir.clone());

    ops.run_packwiz_init(
        &workdir,
        "Vanilla Pack",
        "Test Author",
        "1.0.0",
        "none",
        "1.21.4",
        "",
    )
    .unwrap();

    let fs = ops.filesystem.lock().unwrap();
    let pack_toml = fs
        .get(&workdir.join("pack").join("pack.toml"))
        .expect("pack.toml should be created");
    assert!(pack_toml.contains("minecraft = \"1.21.4\""));
    assert!(!pack_toml.contains("fabric = "));
    assert!(!pack_toml.contains("forge = "));
    assert!(!pack_toml.contains("neoforge = "));
    assert!(!pack_toml.contains("quilt = "));
}

#[test]
fn test_mock_packwiz_ops_refresh_requires_pack_file() {
    let workdir = mock_root().join("workdir");
    let ops = MockPackwizOps::new().with_current_dir(workdir.clone());

    let result = ops.run_packwiz_refresh(&workdir);
    assert!(matches!(
        result,
        Err(crate::empack::state::StateError::MissingFile { .. })
    ));
}

#[test]
fn test_mock_packwiz_ops_failing_init_returns_io_error() {
    let workdir = mock_root().join("workdir");
    let ops = MockPackwizOps::new()
        .with_current_dir(workdir.clone())
        .with_failing_init();

    let result = ops.run_packwiz_init(
        &workdir,
        "Broken Pack",
        "Test Author",
        "1.0.0",
        "fabric",
        "1.21.4",
        "0.15.11",
    );

    assert!(matches!(
        result,
        Err(crate::empack::state::StateError::IoError { .. })
    ));
}

#[test]
fn test_live_packwiz_ops_legacy_forge_init_uses_none_then_patches_versions() {
    let workdir = mock_root().join("workdir");
    let pack_dir = workdir.join("pack");
    let pack_toml_path = pack_dir.join("pack.toml");
    let pack_toml = r#"name = "Legacy Forge Pack"
author = "Test Author"
version = "1.0.0"
pack-format = "packwiz:1.1.0"

[index]
file = "index.toml"
hash-format = "sha256"
hash = ""

[versions]
minecraft = "1.7.10"
"#;

    let process = MockProcessProvider::new()
        .with_packwiz_result(
            vec![
                "init".to_string(),
                "--name".to_string(),
                "Legacy Forge Pack".to_string(),
                "--author".to_string(),
                "Test Author".to_string(),
                "--version".to_string(),
                "1.0.0".to_string(),
                "--mc-version".to_string(),
                "1.7.10".to_string(),
                "--modloader".to_string(),
                "none".to_string(),
                "-y".to_string(),
            ],
            Ok(ProcessOutput {
                stdout: "pack.toml created".to_string(),
                stderr: String::new(),
                success: true,
            }),
        )
        .with_packwiz_result(
            vec![
                "--pack-file".to_string(),
                pack_toml_path.to_string_lossy().to_string(),
                "refresh".to_string(),
            ],
            Ok(ProcessOutput {
                stdout: "Refreshed index".to_string(),
                stderr: String::new(),
                success: true,
            }),
        );

    let filesystem = MockFileSystemProvider::new()
        .with_current_dir(workdir.clone())
        .with_file(pack_toml_path.clone(), pack_toml.to_string());

    let ops = LivePackwizOps::new(&process, &filesystem, PACKWIZ_BIN);
    ops.run_packwiz_init(
        &workdir,
        "Legacy Forge Pack",
        "Test Author",
        "1.0.0",
        "forge",
        "1.7.10",
        "10.13.4.1614",
    )
    .expect("legacy forge init should succeed via workaround");

    let updated = filesystem.read_to_string(&pack_toml_path).unwrap();
    assert!(updated.contains("minecraft = \"1.7.10\""));
    assert!(updated.contains("forge = \"10.13.4.1614\""));
    assert!(process.verify_call(
        PACKWIZ_BIN,
        &[
            "init",
            "--name",
            "Legacy Forge Pack",
            "--author",
            "Test Author",
            "--version",
            "1.0.0",
            "--mc-version",
            "1.7.10",
            "--modloader",
            "none",
            "-y"
        ],
        &pack_dir
    ));
    assert!(process.verify_call(
        PACKWIZ_BIN,
        &["--pack-file", &pack_toml_path.to_string_lossy(), "refresh"],
        &workdir
    ));
}

#[test]
fn test_live_packwiz_ops_pre_boundary_1710_forge_init_uses_direct_forge_version() {
    let workdir = mock_root().join("workdir");
    let pack_dir = workdir.join("pack");
    let process = MockProcessProvider::new().with_packwiz_result(
        vec![
            "init".to_string(),
            "--name".to_string(),
            "Early Legacy Forge Pack".to_string(),
            "--author".to_string(),
            "Test Author".to_string(),
            "--version".to_string(),
            "1.0.0".to_string(),
            "--mc-version".to_string(),
            "1.7.10".to_string(),
            "--modloader".to_string(),
            "forge".to_string(),
            "-y".to_string(),
            "--forge-version".to_string(),
            "10.13.2.1291".to_string(),
        ],
        Ok(ProcessOutput {
            stdout: "pack.toml created".to_string(),
            stderr: String::new(),
            success: true,
        }),
    );

    let filesystem = MockFileSystemProvider::new()
        .with_current_dir(workdir.clone())
        .with_file(pack_dir.join(".placeholder"), String::new());

    let ops = LivePackwizOps::new(&process, &filesystem, PACKWIZ_BIN);
    ops.run_packwiz_init(
        &workdir,
        "Early Legacy Forge Pack",
        "Test Author",
        "1.0.0",
        "forge",
        "1.7.10",
        "10.13.2.1291",
    )
    .expect("pre-boundary 1.7.10 forge init should use the direct packwiz path");

    assert!(process.verify_call(
        PACKWIZ_BIN,
        &[
            "init",
            "--name",
            "Early Legacy Forge Pack",
            "--author",
            "Test Author",
            "--version",
            "1.0.0",
            "--mc-version",
            "1.7.10",
            "--modloader",
            "forge",
            "-y",
            "--forge-version",
            "10.13.2.1291"
        ],
        &pack_dir
    ));
}

#[test]
fn test_packwiz_version_helpers_report_available_and_unavailable() {
    let workdir = mock_root().join("workdir");
    let packwiz_path = mock_root()
        .join("bin")
        .join(PACKWIZ_BIN)
        .to_string_lossy()
        .to_string();

    let process = MockProcessProvider::new().with_result(
        "go".to_string(),
        vec![
            "version".to_string(),
            "-m".to_string(),
            packwiz_path.clone(),
        ],
        Ok(ProcessOutput {
            stdout: "path\tgithub.com/example/packwiz\nmod github.com/example/packwiz v0.4.2\n"
                .to_string(),
            stderr: String::new(),
            success: true,
        }),
    );

    let (available, version) = check_packwiz_available(&process, &workdir).unwrap();
    assert!(available);
    assert_eq!(version, "v0.4.2");

    let unavailable = MockProcessProvider::new().with_packwiz_unavailable();
    let (available, version) = check_packwiz_available(&unavailable, &workdir).unwrap();
    assert!(!available);
    assert_eq!(version, "not found");
}

#[test]
fn test_uses_legacy_forge_init_workaround_switches_at_exact_boundary() {
    assert!(!uses_legacy_forge_init_workaround(
        "forge",
        "1.7.10",
        "10.13.2.1291"
    ));
    assert!(uses_legacy_forge_init_workaround(
        "forge",
        "1.7.10",
        "10.13.2.1300"
    ));
    assert!(uses_legacy_forge_init_workaround(
        "forge",
        "1.7.10",
        "10.13.4.1614-1.7.10"
    ));
    assert!(!uses_legacy_forge_init_workaround(
        "fabric",
        "1.7.10",
        "0.14.0"
    ));
}

#[test]
fn test_get_packwiz_version_returns_none_without_mod_line() {
    let workdir = mock_root().join("workdir");
    let packwiz_path = mock_root()
        .join("bin")
        .join(PACKWIZ_BIN)
        .to_string_lossy()
        .to_string();

    let process = MockProcessProvider::new().with_result(
        "go".to_string(),
        vec![
            "version".to_string(),
            "-m".to_string(),
            packwiz_path.clone(),
        ],
        Ok(ProcessOutput {
            stdout: "github.com/example/packwiz\ndep github.com/example dep".to_string(),
            stderr: String::new(),
            success: true,
        }),
    );

    assert_eq!(get_packwiz_version(&process, &packwiz_path, &workdir), None);
}

#[test]
fn test_get_packwiz_version_returns_none_on_command_failure() {
    let workdir = mock_root().join("workdir");
    let packwiz_path = mock_root()
        .join("bin")
        .join(PACKWIZ_BIN)
        .to_string_lossy()
        .to_string();

    let process = MockProcessProvider::new().with_result(
        "go".to_string(),
        vec![
            "version".to_string(),
            "-m".to_string(),
            packwiz_path.clone(),
        ],
        Ok(ProcessOutput {
            stdout: String::new(),
            stderr: "go: failed to inspect binary".to_string(),
            success: false,
        }),
    );

    assert_eq!(get_packwiz_version(&process, &packwiz_path, &workdir), None);
}

#[test]
fn test_cached_packwiz_check() {
    let _env_lock = packwiz_env_lock();
    let mock_process = MockProcessProvider::new()
        .with_packwiz_result(
            vec![
                "--pack-file".to_string(),
                mock_root()
                    .join("workdir")
                    .join("pack")
                    .join("pack.toml")
                    .to_string_lossy()
                    .to_string(),
                "modrinth".to_string(),
                "add".to_string(),
                "--project-id".to_string(),
                "mod1".to_string(),
                "-y".to_string(),
            ],
            Ok(ProcessOutput {
                stdout: "Added mod1".to_string(),
                stderr: String::new(),
                success: true,
            }),
        )
        .with_packwiz_result(
            vec![
                "--pack-file".to_string(),
                mock_root()
                    .join("workdir")
                    .join("pack")
                    .join("pack.toml")
                    .to_string_lossy()
                    .to_string(),
                "modrinth".to_string(),
                "add".to_string(),
                "--project-id".to_string(),
                "mod2".to_string(),
                "-y".to_string(),
            ],
            Ok(ProcessOutput {
                stdout: "Added mod2".to_string(),
                stderr: String::new(),
                success: true,
            }),
        );

    let session = MockCommandSession::new()
        .with_process(mock_process)
        .with_filesystem(
            MockFileSystemProvider::new().with_current_dir(mock_root().join("workdir")),
        );

    let mut metadata = PackwizMetadata::new(&session).unwrap();

    // First call should check packwiz availability
    let result1 = metadata.add_mod("mod1", ProjectPlatform::Modrinth);
    assert!(result1.is_ok());

    // Second call should use cached availability (no additional check)
    let result2 = metadata.add_mod("mod2", ProjectPlatform::Modrinth);
    assert!(result2.is_ok());

    // Both calls should succeed - the important behavior is that the second
    // call doesn't fail due to check_packwiz being called again
    assert!(result1.is_ok() && result2.is_ok());
}

#[test]
fn test_refresh_index_reports_generic_failure() {
    let _env_lock = packwiz_env_lock();
    let mock_process = MockProcessProvider::new().with_packwiz_result(
        vec![
            "--pack-file".to_string(),
            mock_root()
                .join("workdir")
                .join("pack")
                .join("pack.toml")
                .to_string_lossy()
                .to_string(),
            "refresh".to_string(),
        ],
        Ok(ProcessOutput {
            stdout: String::new(),
            stderr: "permission denied".to_string(),
            success: false,
        }),
    );

    let session = MockCommandSession::new()
        .with_process(mock_process)
        .with_filesystem(
            MockFileSystemProvider::new().with_current_dir(mock_root().join("workdir")),
        );

    let mut metadata = PackwizMetadata::new(&session).unwrap();
    let result = metadata.refresh_index();

    assert!(result.is_err());
    match result.unwrap_err() {
        PackwizError::CommandFailed { command, stderr } => {
            assert_eq!(command, "packwiz refresh");
            assert_eq!(stderr, "permission denied");
        }
        other => panic!("Expected CommandFailed error, got: {:?}", other),
    }
}

/// Test: Packwiz parser robustness - malformed pack.toml handling
///
/// Validates error handling when pack.toml has invalid TOML syntax or missing required fields.
/// This tests packwiz's error reporting, not direct parsing (empack delegates to packwiz).
#[test]
fn test_packwiz_malformed_pack_toml() {
    let _env_lock = packwiz_env_lock();
    // Simulate packwiz returning error due to malformed pack.toml
    let mock_process = MockProcessProvider::new().with_packwiz_result(
        vec![
            "--pack-file".to_string(),
            mock_root()
                .join("workdir")
                .join("pack")
                .join("pack.toml")
                .to_string_lossy()
                .to_string(),
            "refresh".to_string(),
        ],
        Ok(ProcessOutput {
            stdout: String::new(),
            stderr:
                "Error: failed to parse pack.toml: expected '=' after table key at line 3 column 1"
                    .to_string(),
            success: false,
        }),
    );

    let session = MockCommandSession::new()
        .with_process(mock_process)
        .with_filesystem(
            MockFileSystemProvider::new().with_current_dir(mock_root().join("workdir")),
        );

    let mut metadata = PackwizMetadata::new(&session).unwrap();
    let result = metadata.refresh_index();

    // Should propagate error from packwiz
    assert!(result.is_err());
    match result.unwrap_err() {
        PackwizError::CommandFailed { stderr, .. } => {
            assert!(
                stderr.contains("parse") && stderr.contains("pack.toml"),
                "Error should indicate pack.toml parsing issue, got: {}",
                stderr
            );
        }
        _ => panic!("Expected CommandFailed error for malformed pack.toml"),
    }
}

/// Test: Packwiz parser robustness - missing required fields in pack.toml
#[test]
fn test_packwiz_pack_toml_missing_fields() {
    let _env_lock = packwiz_env_lock();
    // Simulate packwiz returning error due to missing required pack.toml fields
    let mock_process = MockProcessProvider::new().with_packwiz_result(
        vec![
            "--pack-file".to_string(),
            mock_root()
                .join("workdir")
                .join("pack")
                .join("pack.toml")
                .to_string_lossy()
                .to_string(),
            "modrinth".to_string(),
            "export".to_string(),
            "-o".to_string(),
            mock_root()
                .join("pack.mrpack")
                .to_string_lossy()
                .to_string(),
        ],
        Ok(ProcessOutput {
            stdout: String::new(),
            stderr: "Error: pack.toml is missing required field: name".to_string(),
            success: false,
        }),
    );

    let session = MockCommandSession::new()
        .with_process(mock_process)
        .with_filesystem(
            MockFileSystemProvider::new().with_current_dir(mock_root().join("workdir")),
        );

    let mut metadata = PackwizMetadata::new(&session).unwrap();
    let result = metadata.export_mrpack(&mock_root().join("pack.mrpack"));

    // Should propagate error from packwiz
    assert!(result.is_err());
    match result.unwrap_err() {
        PackwizError::CommandFailed { stderr, .. } => {
            assert!(
                stderr.contains("missing required field"),
                "Error should indicate missing field, got: {}",
                stderr
            );
        }
        _ => panic!("Expected CommandFailed error for missing pack.toml fields"),
    }
}

/// Test: Packwiz parser robustness - invalid TOML syntax
#[test]
fn test_packwiz_invalid_toml_syntax() {
    let _env_lock = packwiz_env_lock();
    // Simulate packwiz failing due to completely invalid TOML syntax
    let mock_process = MockProcessProvider::new().with_packwiz_result(
        vec![
            "--pack-file".to_string(),
            mock_root().join("workdir").join("pack").join("pack.toml").to_string_lossy().to_string(),
            "refresh".to_string(),
        ],
        Ok(ProcessOutput {
            stdout: String::new(),
            stderr: "Error: invalid TOML value, unexpected newline\nexpected an equals, found a newline at line 1".to_string(),
            success: false,
        }),
    );

    let session = MockCommandSession::new()
        .with_process(mock_process)
        .with_filesystem(
            MockFileSystemProvider::new().with_current_dir(mock_root().join("workdir")),
        );

    let mut metadata = PackwizMetadata::new(&session).unwrap();
    let result = metadata.refresh_index();

    // Should propagate packwiz error
    assert!(result.is_err());

    let err = result.unwrap_err();
    let err_msg = format!("{:?}", err);

    // Error should contain information about TOML or parse failure
    assert!(
        err_msg.contains("TOML") || err_msg.contains("parse") || err_msg.contains("invalid"),
        "Error should indicate TOML/parse issue, got: {}",
        err_msg
    );
}

// ── parse_installer_restricted_output tests ─────────────────────────────

#[test]
fn test_parse_single_restricted_mod() {
    let output = "\
Failed to download modpack, the following errors were encountered:
OptiFine_1.20.1_HD_U_I6.jar:
java.lang.Exception: This mod is excluded from the CurseForge API and must be downloaded manually.
Please go to https://www.curseforge.com/minecraft/mc-mods/optifine/download/4912891 and save this file to /tmp/pack/.minecraft/mods/OptiFine_1.20.1_HD_U_I6.jar
\tat link.infra.packwiz.installer.DownloadTask.download(DownloadTask.java:42)";

    let results = parse_installer_restricted_output(output);

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].name, "OptiFine_1.20.1_HD_U_I6.jar");
    assert_eq!(
        results[0].url,
        "https://www.curseforge.com/minecraft/mc-mods/optifine/download/4912891"
    );
    assert_eq!(
        results[0].dest_path,
        "/tmp/pack/.minecraft/mods/OptiFine_1.20.1_HD_U_I6.jar"
    );
}

#[test]
fn test_parse_multiple_restricted_mods() {
    let output = "\
Failed to download modpack, the following errors were encountered:
OptiFine.jar:
java.lang.Exception: This mod is excluded from the CurseForge API and must be downloaded manually.
Please go to https://www.curseforge.com/minecraft/mc-mods/optifine/download/111 and save this file to /mods/OptiFine.jar
\tat link.infra.packwiz.installer.DownloadTask.download(DownloadTask.java:42)
Replay-Mod.jar:
java.lang.Exception: This mod is excluded from the CurseForge API and must be downloaded manually.
Please go to https://www.curseforge.com/minecraft/mc-mods/replay-mod/download/222 and save this file to /mods/Replay-Mod.jar
\tat link.infra.packwiz.installer.DownloadTask.download(DownloadTask.java:42)
Vivecraft.jar:
java.lang.Exception: This mod is excluded from the CurseForge API and must be downloaded manually.
Please go to https://www.curseforge.com/minecraft/mc-mods/vivecraft/download/333 and save this file to /mods/Vivecraft.jar
\tat link.infra.packwiz.installer.DownloadTask.download(DownloadTask.java:42)";

    let results = parse_installer_restricted_output(output);

    assert_eq!(results.len(), 3);
    assert_eq!(results[0].name, "OptiFine.jar");
    assert_eq!(results[1].name, "Replay-Mod.jar");
    assert_eq!(results[2].name, "Vivecraft.jar");
    assert_eq!(
        results[0].url,
        "https://www.curseforge.com/minecraft/mc-mods/optifine/download/111"
    );
    assert_eq!(
        results[1].url,
        "https://www.curseforge.com/minecraft/mc-mods/replay-mod/download/222"
    );
    assert_eq!(
        results[2].url,
        "https://www.curseforge.com/minecraft/mc-mods/vivecraft/download/333"
    );
}

#[test]
fn test_parse_no_restricted_mods() {
    let output = "\
Downloading installer... Done!
Installing modpack...
Downloaded 42 mods
All mods installed successfully.";

    let results = parse_installer_restricted_output(output);

    assert!(results.is_empty());
}

#[test]
fn test_parse_interleaved_stack_trace() {
    let output = "\
Failed to download modpack, the following errors were encountered:
OptiFine.jar:
java.lang.Exception: This mod is excluded from the CurseForge API and must be downloaded manually.
\tat link.infra.packwiz.installer.DownloadTask.download(DownloadTask.java:42)
\tat link.infra.packwiz.installer.DownloadTask.call(DownloadTask.java:30)
\tat java.base/java.util.concurrent.FutureTask.run(FutureTask.java:264)
Please go to https://www.curseforge.com/minecraft/mc-mods/optifine/download/999 and save this file to /mods/OptiFine.jar
\tat java.base/java.util.concurrent.ThreadPoolExecutor.runWorker(ThreadPoolExecutor.java:1136)";

    let results = parse_installer_restricted_output(output);

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].name, "OptiFine.jar");
    assert_eq!(
        results[0].url,
        "https://www.curseforge.com/minecraft/mc-mods/optifine/download/999"
    );
    assert_eq!(results[0].dest_path, "/mods/OptiFine.jar");
}

#[test]
fn test_parse_empty_url_not_pushed() {
    let output = "\
Failed to download modpack, the following errors were encountered:
OptiFine.jar:
java.lang.Exception: This mod is excluded from the CurseForge API and must be downloaded manually.
\tat link.infra.packwiz.installer.DownloadTask.download(DownloadTask.java:42)
\tat link.infra.packwiz.installer.DownloadTask.call(DownloadTask.java:30)
\tat java.base/java.util.concurrent.FutureTask.run(FutureTask.java:264)
\tat java.base/java.util.concurrent.ThreadPoolExecutor.runWorker(ThreadPoolExecutor.java:1136)
\tat java.base/java.util.concurrent.ThreadPoolExecutor$Worker.run(ThreadPoolExecutor.java:635)
\tat java.base/java.lang.Thread.run(Thread.java:842)";

    let results = parse_installer_restricted_output(output);

    assert!(
        results.is_empty(),
        "should not produce an entry when URL line is beyond the 5-line lookahead"
    );
}

#[test]
fn test_parse_name_from_stdout_line() {
    let output = "\
Failed to download modpack, the following errors were encountered:
MyCustomMod-1.2.3.jar:
java.lang.Exception: This mod is excluded from the CurseForge API and must be downloaded manually.
Please go to https://www.curseforge.com/minecraft/mc-mods/mycustommod/download/555 and save this file to /mods/MyCustomMod-1.2.3.jar
\tat link.infra.packwiz.installer.DownloadTask.download(DownloadTask.java:42)";

    let results = parse_installer_restricted_output(output);

    assert_eq!(results.len(), 1);
    assert_eq!(
        results[0].name, "MyCustomMod-1.2.3.jar",
        "name should come from the preceding stdout line, not the exception line"
    );
}

#[test]
fn test_parse_excluded_on_first_line_falls_back_to_destination_filename() {
    let output = "\
java.lang.Exception: This mod is excluded from the CurseForge API and must be downloaded manually.
Please go to https://www.curseforge.com/minecraft/mc-mods/unknown/download/1 and save this file to /mods/unknown.jar";

    let results = parse_installer_restricted_output(output);

    assert_eq!(results.len(), 1);
    assert_eq!(
        results[0].name, "unknown.jar",
        "should fall back to the destination filename when no name line is present"
    );
    assert_eq!(
        results[0].url,
        "https://www.curseforge.com/minecraft/mc-mods/unknown/download/1"
    );
}

#[test]
fn test_parse_dedupes_duplicate_restricted_entries_and_ignores_stack_frame_names() {
    let output = "\
Failed to download modpack, the following errors were encountered:
java.lang.Exception: This mod is excluded from the CurseForge API and must be downloaded manually.
Please go to https://www.curseforge.com/minecraft/mc-mods/entityculling/files/4763646 and save this file to /mods/entityculling-fabric-1.6.2-mc1.20.1.jar
\tat link.infra.packwiz.installer.bootstrap.Main.main(Main.java:46)
java.lang.Exception: This mod is excluded from the CurseForge API and must be downloaded manually.
Please go to https://www.curseforge.com/minecraft/mc-mods/entityculling/files/4763646 and save this file to /mods/entityculling-fabric-1.6.2-mc1.20.1.jar
\tat link.infra.packwiz.installer.bootstrap.Main.main(Main.java:46)";

    let results = parse_installer_restricted_output(output);

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].name, "entityculling-fabric-1.6.2-mc1.20.1.jar");
    assert_eq!(
        results[0].url,
        "https://www.curseforge.com/minecraft/mc-mods/entityculling/download/4763646"
    );
    assert_eq!(
        results[0].dest_path,
        "/mods/entityculling-fabric-1.6.2-mc1.20.1.jar"
    );
}

#[test]
fn test_parse_url_at_lookahead_boundary() {
    let output = "\
Failed to download modpack, the following errors were encountered:
BoundaryMod.jar:
java.lang.Exception: This mod is excluded from the CurseForge API and must be downloaded manually.
\tat stack.trace.line1(File.java:1)
\tat stack.trace.line2(File.java:2)
\tat stack.trace.line3(File.java:3)
\tat stack.trace.line4(File.java:4)
Please go to https://www.curseforge.com/minecraft/mc-mods/boundary/download/42 and save this file to /mods/BoundaryMod.jar";

    let results = parse_installer_restricted_output(output);

    assert_eq!(
        results.len(),
        1,
        "URL on the 5th line after excluded should be found"
    );
    assert_eq!(results[0].name, "BoundaryMod.jar");
    assert_eq!(
        results[0].url,
        "https://www.curseforge.com/minecraft/mc-mods/boundary/download/42"
    );
}

#[test]
fn test_parse_url_beyond_lookahead_boundary() {
    let output = "\
Failed to download modpack, the following errors were encountered:
TooFarMod.jar:
java.lang.Exception: This mod is excluded from the CurseForge API and must be downloaded manually.
\tat stack.trace.line1(File.java:1)
\tat stack.trace.line2(File.java:2)
\tat stack.trace.line3(File.java:3)
\tat stack.trace.line4(File.java:4)
\tat stack.trace.line5(File.java:5)
Please go to https://www.curseforge.com/minecraft/mc-mods/toofar/download/42 and save this file to /mods/TooFarMod.jar";

    let results = parse_installer_restricted_output(output);

    assert!(
        results.is_empty(),
        "URL on the 6th line after excluded should be outside the 5-line lookahead"
    );
}

#[test]
fn test_parse_empty_output() {
    let results = parse_installer_restricted_output("");
    assert!(results.is_empty());
}

#[test]
fn test_restricted_destination_filename_extracts_basename() {
    assert_eq!(
        restricted_destination_filename("/tmp/pack/.minecraft/mods/OptiFine.jar"),
        Some("OptiFine.jar".to_string())
    );
}

#[test]
fn test_restricted_curseforge_file_id_supports_files_and_download_urls() {
    assert_eq!(
        restricted_curseforge_file_id(
            "https://www.curseforge.com/minecraft/mc-mods/optifine/files/4912891"
        ),
        Some(4_912_891)
    );
    assert_eq!(
        restricted_curseforge_file_id(
            "https://www.curseforge.com/minecraft/mc-mods/optifine/download/4912891"
        ),
        Some(4_912_891)
    );
}

#[test]
fn test_restricted_curseforge_file_id_rejects_malformed_urls() {
    assert_eq!(
        restricted_curseforge_file_id("https://www.curseforge.com/minecraft/mc-mods/optifine"),
        None
    );
    assert_eq!(
        restricted_curseforge_file_id(
            "https://www.curseforge.com/minecraft/mc-mods/optifine/files/not-a-number"
        ),
        None
    );
}

#[test]
fn test_normalize_curseforge_manual_download_url_rewrites_files_endpoint() {
    assert_eq!(
        normalize_curseforge_manual_download_url(
            "https://www.curseforge.com/minecraft/mc-mods/optifine/files/4912891"
        ),
        "https://www.curseforge.com/minecraft/mc-mods/optifine/download/4912891"
    );
}

#[test]
fn test_normalize_curseforge_manual_download_url_preserves_download_endpoint() {
    assert_eq!(
        normalize_curseforge_manual_download_url(
            "https://www.curseforge.com/minecraft/mc-mods/optifine/download/4912891"
        ),
        "https://www.curseforge.com/minecraft/mc-mods/optifine/download/4912891"
    );
}

#[test]
fn test_normalize_curseforge_manual_download_url_preserves_non_curseforge_url() {
    assert_eq!(
        normalize_curseforge_manual_download_url("https://example.com/file.jar"),
        "https://example.com/file.jar"
    );
}

#[test]
fn test_parse_export_restricted_output_parses_manual_downloads() {
    let output = "\
Refreshing index... 100 % [===============================================] done
Reading external files...
Retrieving 319 external files...
Disclaimer: you are responsible for ensuring you comply with ALL the licenses.
Found 2 manual downloads; these mods are unable to be downloaded by packwiz (due to API limitations) and must be manually downloaded:
No Potion Particles (Itsme64's no potion particles [1.5.1].zip) from https://www.curseforge.com/minecraft/texture-packs/no-potion-particles/files/4607236
Bee Fix (BeeFix-1.20-1.0.7.jar) from https://www.curseforge.com/minecraft/mc-mods/bee-fix/files/4618962
Once you have done so, place these files in /Users/test/Library/Caches/packwiz/cache/import and re-run this command.";

    let results = parse_export_restricted_output(output);

    assert_eq!(results.len(), 2);
    assert_eq!(results[0].name, "No Potion Particles");
    assert_eq!(
        results[0].url,
        "https://www.curseforge.com/minecraft/texture-packs/no-potion-particles/download/4607236"
    );
    assert_eq!(
        results[0].dest_path,
        "/Users/test/Library/Caches/packwiz/cache/import/Itsme64's no potion particles [1.5.1].zip"
    );
    assert_eq!(results[1].name, "Bee Fix");
    assert_eq!(
        results[1].url,
        "https://www.curseforge.com/minecraft/mc-mods/bee-fix/download/4618962"
    );
    assert_eq!(
        results[1].dest_path,
        "/Users/test/Library/Caches/packwiz/cache/import/BeeFix-1.20-1.0.7.jar"
    );
}

#[test]
fn test_parse_export_restricted_output_preserves_windows_import_separator_style() {
    let output = "\
Found 1 manual downloads; these mods are unable to be downloaded by packwiz (due to API limitations) and must be manually downloaded:
Bee Fix (BeeFix-1.20-1.0.7.jar) from https://www.curseforge.com/minecraft/mc-mods/bee-fix/files/4618962
Once you have done so, place these files in C:\\Users\\test\\AppData\\Local\\packwiz\\cache\\import and re-run this command.";

    let results = parse_export_restricted_output(output);

    assert_eq!(results.len(), 1);
    assert_eq!(
        results[0].dest_path,
        "C:\\Users\\test\\AppData\\Local\\packwiz\\cache\\import\\BeeFix-1.20-1.0.7.jar"
    );
}

#[test]
fn test_parse_export_restricted_output_accepts_non_http_url_schemes() {
    let output = "\
Found 1 manual downloads; these mods are unable to be downloaded by packwiz (due to API limitations) and must be manually downloaded:
Bee Fix (BeeFix-1.20-1.0.7.jar) from ftp://downloads.example.com/BeeFix-1.20-1.0.7.jar
Once you have done so, place these files in /Users/test/Library/Caches/packwiz/cache/import and re-run this command.";

    let results = parse_export_restricted_output(output);

    assert_eq!(results.len(), 1);
    assert_eq!(
        results[0].url,
        "ftp://downloads.example.com/BeeFix-1.20-1.0.7.jar"
    );
    assert_eq!(
        results[0].dest_path,
        "/Users/test/Library/Caches/packwiz/cache/import/BeeFix-1.20-1.0.7.jar"
    );
}

#[test]
fn test_parse_export_restricted_output_requires_import_dir_line() {
    let output = "\
Found 1 manual downloads; these mods are unable to be downloaded by packwiz (due to API limitations) and must be manually downloaded:
Bee Fix (BeeFix-1.20-1.0.7.jar) from https://www.curseforge.com/minecraft/mc-mods/bee-fix/files/4618962";

    assert!(parse_export_restricted_output(output).is_empty());
}

#[test]
fn test_parse_export_restricted_output_handles_packed_progress_output() {
    let output = "\
Refreshing index... 52 % [=========================>------------------------] 0s \u{1b}[1A\u{1b}[JRefreshing index... 100 % [===============================================] done Reading external files... Retrieving 319 external files... Found 2 manual downloads; these mods are unable to be downloaded by packwiz (due to API limitations) and must be manually downloaded: Bee Fix (BeeFix-1.20-1.0.7.jar) from https://www.curseforge.com/minecraft/mc-mods/bee-fix/files/4618962 No Potion Particles (Itsme64's no potion particles [1.5.1].zip) from https://www.curseforge.com/minecraft/texture-packs/no-potion-particles/files/4607236 Once you have done so, place these files in /Users/test/Library/Caches/packwiz/cache/import and re-run this command.";

    let results = parse_export_restricted_output(output);

    assert_eq!(results.len(), 2);
    assert_eq!(results[0].name, "Bee Fix");
    assert_eq!(
        results[0].url,
        "https://www.curseforge.com/minecraft/mc-mods/bee-fix/download/4618962"
    );
    assert_eq!(
        results[1].url,
        "https://www.curseforge.com/minecraft/texture-packs/no-potion-particles/download/4607236"
    );
}

// ── get_installed_mods .pw.toml filter tests ───────────────────────────

#[test]
fn test_get_installed_mods_only_includes_pw_toml_files() {
    let workdir = mock_root().join("workdir");
    let mods_dir = workdir.join("pack").join("mods");

    let fs = MockFileSystemProvider::new()
        .with_current_dir(workdir.clone())
        // Valid .pw.toml files; should be included
        .with_file(
            mods_dir.join("fabric-api.pw.toml"),
            "name = \"Fabric API\"".to_string(),
        )
        .with_file(
            mods_dir.join("sodium.pw.toml"),
            "name = \"Sodium\"".to_string(),
        )
        // Plain .toml files; should NOT be included
        .with_file(mods_dir.join("config.toml"), "key = \"value\"".to_string())
        .with_file(
            mods_dir.join("mod-settings.toml"),
            "setting = true".to_string(),
        )
        // Empty slug (.pw.toml with no prefix); should NOT be included
        .with_file(mods_dir.join(".pw.toml"), "empty = true".to_string())
        // Non-toml file; should NOT be included
        .with_file(mods_dir.join("some-file.txt"), "text".to_string());

    let process = MockProcessProvider::new();
    let ops = LivePackwizOps::new(&process, &fs, PACKWIZ_BIN);

    let installed = ops.get_installed_mods(&workdir).unwrap();

    assert_eq!(
        installed.len(),
        2,
        "expected exactly 2 mods, got: {:?}",
        installed
    );
    assert!(
        installed.contains("fabric-api"),
        "should contain fabric-api"
    );
    assert!(installed.contains("sodium"), "should contain sodium");
    assert!(!installed.contains("config"), "should NOT contain config");
    assert!(
        !installed.contains("mod-settings"),
        "should NOT contain mod-settings"
    );
    assert!(!installed.contains(""), "should NOT contain empty slug");
    assert!(
        !installed.contains("some-file"),
        "should NOT contain non-toml files"
    );
}

// ── write_pack_toml_options tests ────────────────────────────────────

#[test]
fn test_pack_toml_options_merge() {
    let workdir = mock_root().join("workdir");
    let pack_toml_path = workdir.join("pack").join("pack.toml");

    let existing = r#"name = "Test Pack"
author = "Test Author"
version = "1.0.0"
pack-format = "packwiz:1.1.0"

[index]
file = "index.toml"
hash-format = "sha256"
hash = ""

[versions]
minecraft = "1.20.1"
fabric = "0.14.21"
"#;

    let fs = MockFileSystemProvider::new()
        .with_current_dir(workdir.clone())
        .with_file(pack_toml_path.clone(), existing.to_string());

    let versions = vec!["1.20".to_string(), "1.20.2".to_string()];
    let result = write_pack_toml_options(&pack_toml_path, Some("datapacks"), Some(&versions), &fs);
    assert!(result.is_ok(), "write_pack_toml_options failed: {result:?}");

    let updated = fs.read_to_string(&pack_toml_path).unwrap();
    let doc: toml::Table = toml::from_str(&updated).unwrap();

    assert_eq!(
        doc.get("name").and_then(|v| v.as_str()),
        Some("Test Pack"),
        "name should be preserved",
    );
    assert!(
        doc.get("versions").is_some(),
        "[versions] should be preserved"
    );
    assert!(doc.get("index").is_some(), "[index] should be preserved");

    let options = doc.get("options").expect("[options] should exist");
    assert_eq!(
        options.get("datapack-folder").and_then(|v| v.as_str()),
        Some("datapacks"),
    );
    let agv = options
        .get("acceptable-game-versions")
        .and_then(|v| v.as_array())
        .expect("acceptable-game-versions should be an array");
    let agv_strs: Vec<&str> = agv.iter().filter_map(|v| v.as_str()).collect();
    assert_eq!(agv_strs, vec!["1.20", "1.20.2"]);
}

#[test]
fn test_pack_toml_options_preserves_other_keys() {
    let workdir = mock_root().join("workdir");
    let pack_toml_path = workdir.join("pack").join("pack.toml");

    let existing = r#"name = "Test Pack"
pack-format = "packwiz:1.1.0"

[index]
file = "index.toml"
hash-format = "sha256"
hash = ""

[versions]
minecraft = "1.20.1"

[options]
no-internal-hashes = true
"#;

    let fs = MockFileSystemProvider::new()
        .with_current_dir(workdir.clone())
        .with_file(pack_toml_path.clone(), existing.to_string());

    let result = write_pack_toml_options(&pack_toml_path, Some("datapacks"), None, &fs);
    assert!(result.is_ok(), "write_pack_toml_options failed: {result:?}");

    let updated = fs.read_to_string(&pack_toml_path).unwrap();
    let doc: toml::Table = toml::from_str(&updated).unwrap();

    let options = doc.get("options").expect("[options] should exist");
    assert_eq!(
        options.get("no-internal-hashes").and_then(|v| v.as_bool()),
        Some(true),
        "pre-existing options key should be preserved",
    );
    assert_eq!(
        options.get("datapack-folder").and_then(|v| v.as_str()),
        Some("datapacks"),
    );
    assert!(
        options.get("acceptable-game-versions").is_none(),
        "acceptable-game-versions should not be injected when None",
    );
}

#[test]
fn test_pack_toml_options_none_params_are_no_ops() {
    let workdir = mock_root().join("workdir");
    let pack_toml_path = workdir.join("pack").join("pack.toml");

    let existing = r#"name = "Test Pack"
pack-format = "packwiz:1.1.0"

[index]
file = "index.toml"
hash-format = "sha256"
hash = ""

[versions]
minecraft = "1.20.1"
"#;

    let fs = MockFileSystemProvider::new()
        .with_current_dir(workdir.clone())
        .with_file(pack_toml_path.clone(), existing.to_string());

    let result = write_pack_toml_options(&pack_toml_path, None, None, &fs);
    assert!(result.is_ok());

    let updated = fs.read_to_string(&pack_toml_path).unwrap();
    assert_eq!(
        updated, existing,
        "file should be unchanged when both params are None",
    );
}

#[test]
fn test_pack_toml_versions_merge() {
    let workdir = mock_root().join("workdir");
    let pack_toml_path = workdir.join("pack").join("pack.toml");

    let existing = r#"name = "Legacy Forge Pack"
pack-format = "packwiz:1.1.0"

[index]
file = "index.toml"
hash-format = "sha256"
hash = ""

[versions]
minecraft = "1.7.10"
"#;

    let fs = MockFileSystemProvider::new()
        .with_current_dir(workdir.clone())
        .with_file(pack_toml_path.clone(), existing.to_string());

    write_pack_toml_versions(&pack_toml_path, &[("forge", "10.13.4.1614")], &fs)
        .expect("should merge forge version into pack.toml");

    let updated = fs.read_to_string(&pack_toml_path).unwrap();
    let doc: toml::Table = toml::from_str(&updated).unwrap();
    let versions = doc
        .get("versions")
        .and_then(|value| value.as_table())
        .expect("[versions] should exist");
    assert_eq!(
        versions.get("minecraft").and_then(|value| value.as_str()),
        Some("1.7.10")
    );
    assert_eq!(
        versions.get("forge").and_then(|value| value.as_str()),
        Some("10.13.4.1614")
    );
}
