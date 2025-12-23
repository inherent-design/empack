// Tests for packwiz integration

use super::*;
use crate::application::session::ProcessOutput;
use crate::application::session_mocks::{
    MockCommandSession, MockFileSystemProvider, MockProcessProvider,
};
use std::path::PathBuf;

#[test]
fn test_add_mod_modrinth_success() {
    let mock_process = MockProcessProvider::new().with_packwiz_result(
        vec![
            "--pack-file".to_string(),
            "/test/workdir/pack/pack.toml".to_string(),
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
            MockFileSystemProvider::new().with_current_dir(PathBuf::from("/test/workdir")),
        );

    let mut metadata = PackwizMetadata::new(&session).unwrap();
    let result = metadata.add_mod("AANobbMI", ProjectPlatform::Modrinth);

    assert!(result.is_ok());
    assert!(session.process_provider.verify_call(
        "packwiz",
        &[
            "--pack-file",
            "/test/workdir/pack/pack.toml",
            "modrinth",
            "add",
            "--project-id",
            "AANobbMI",
            "-y"
        ],
        &PathBuf::from("/test/workdir/pack")
    ));
}

#[test]
fn test_add_mod_curseforge_success() {
    let mock_process = MockProcessProvider::new().with_packwiz_result(
        vec![
            "--pack-file".to_string(),
            "/test/workdir/pack/pack.toml".to_string(),
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
            MockFileSystemProvider::new().with_current_dir(PathBuf::from("/test/workdir")),
        );

    let mut metadata = PackwizMetadata::new(&session).unwrap();
    let result = metadata.add_mod("123456", ProjectPlatform::CurseForge);

    assert!(result.is_ok());
}

#[test]
fn test_add_mod_failure() {
    let mock_process = MockProcessProvider::new().with_packwiz_result(
        vec![
            "--pack-file".to_string(),
            "/test/workdir/pack/pack.toml".to_string(),
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
            MockFileSystemProvider::new().with_current_dir(PathBuf::from("/test/workdir")),
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
    let mock_process = MockProcessProvider::new().with_packwiz_result(
        vec![
            "--pack-file".to_string(),
            "/test/workdir/pack/pack.toml".to_string(),
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
            MockFileSystemProvider::new().with_current_dir(PathBuf::from("/test/workdir")),
        );

    let mut metadata = PackwizMetadata::new(&session).unwrap();
    let result = metadata.remove_mod("sodium");

    assert!(result.is_ok());
    assert!(session.process_provider.verify_call(
        "packwiz",
        &[
            "--pack-file",
            "/test/workdir/pack/pack.toml",
            "remove",
            "sodium",
            "-y"
        ],
        &PathBuf::from("/test/workdir/pack")
    ));
}

#[test]
fn test_remove_mod_not_found() {
    let mock_process = MockProcessProvider::new().with_packwiz_result(
        vec![
            "--pack-file".to_string(),
            "/test/workdir/pack/pack.toml".to_string(),
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
            MockFileSystemProvider::new().with_current_dir(PathBuf::from("/test/workdir")),
        );

    let mut metadata = PackwizMetadata::new(&session).unwrap();
    let result = metadata.remove_mod("nonexistent");

    assert!(result.is_err());
}

#[test]
fn test_refresh_index_success() {
    let mock_process = MockProcessProvider::new().with_packwiz_result(
        vec![
            "--pack-file".to_string(),
            "/test/workdir/pack/pack.toml".to_string(),
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
            MockFileSystemProvider::new().with_current_dir(PathBuf::from("/test/workdir")),
        );

    let mut metadata = PackwizMetadata::new(&session).unwrap();
    let result = metadata.refresh_index();

    assert!(result.is_ok());
}

#[test]
fn test_refresh_index_hash_mismatch() {
    let mock_process = MockProcessProvider::new().with_packwiz_result(
        vec![
            "--pack-file".to_string(),
            "/test/workdir/pack/pack.toml".to_string(),
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
            MockFileSystemProvider::new().with_current_dir(PathBuf::from("/test/workdir")),
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
    let mock_process = MockProcessProvider::new().with_packwiz_result(
        vec![
            "--pack-file".to_string(),
            "/test/workdir/pack/pack.toml".to_string(),
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
            MockFileSystemProvider::new().with_current_dir(PathBuf::from("/test/workdir")),
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
    let output_path = PathBuf::from("/test/workdir/dist/pack.mrpack");
    let mock_process = MockProcessProvider::new().with_packwiz_result(
        vec![
            "--pack-file".to_string(),
            "/test/workdir/pack/pack.toml".to_string(),
            "modrinth".to_string(),
            "export".to_string(),
            "-o".to_string(),
            "/test/workdir/dist/pack.mrpack".to_string(),
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
            MockFileSystemProvider::new().with_current_dir(PathBuf::from("/test/workdir")),
        );

    let mut metadata = PackwizMetadata::new(&session).unwrap();
    let result = metadata.export_mrpack(&output_path);

    assert!(result.is_ok());
}

#[test]
fn test_packwiz_unavailable() {
    let mock_process = MockProcessProvider::new().with_packwiz_unavailable();

    let session = MockCommandSession::new()
        .with_process(mock_process)
        .with_filesystem(
            MockFileSystemProvider::new().with_current_dir(PathBuf::from("/test/workdir")),
        );

    let mut metadata = PackwizMetadata::new(&session).unwrap();
    let result = metadata.add_mod("AANobbMI", ProjectPlatform::Modrinth);

    assert!(result.is_err());
    match result.unwrap_err() {
        PackwizError::NotAvailable(msg) => {
            assert!(msg.contains("not found"));
        }
        _ => panic!("Wrong error type"),
    }
}

#[test]
fn test_installer_success() {
    let jar_path = PathBuf::from("/cache/packwiz-installer-bootstrap.jar");
    let mock_process = MockProcessProvider::new().with_result(
        "java".to_string(),
        vec![
            "-jar".to_string(),
            "/cache/packwiz-installer-bootstrap.jar".to_string(),
            "-g".to_string(),
            "-s".to_string(),
            "both".to_string(),
            "--pack-folder".to_string(),
            "pack".to_string(),
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
            MockFileSystemProvider::new().with_current_dir(PathBuf::from("/test/workdir")),
        );

    let installer = PackwizInstaller::new(&session, jar_path.clone()).unwrap();
    let result = installer.install_mods("both", &PathBuf::from("/test/workdir"));

    assert!(result.is_ok());
    assert!(session.process_provider.verify_call(
        "java",
        &[
            "-jar",
            "/cache/packwiz-installer-bootstrap.jar",
            "-g",
            "-s",
            "both",
            "--pack-folder",
            "pack"
        ],
        &PathBuf::from("/test/workdir")
    ));
}

#[test]
fn test_installer_invalid_side() {
    let jar_path = PathBuf::from("/cache/packwiz-installer-bootstrap.jar");

    let session = MockCommandSession::new();

    let installer = PackwizInstaller::new(&session, jar_path).unwrap();
    let result = installer.install_mods("invalid", &PathBuf::from("/test/workdir"));

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
    let jar_path = PathBuf::from("/cache/packwiz-installer-bootstrap.jar");
    let mock_process = MockProcessProvider::new().with_result(
        "java".to_string(),
        vec![
            "-jar".to_string(),
            "/cache/packwiz-installer-bootstrap.jar".to_string(),
            "-g".to_string(),
            "-s".to_string(),
            "client".to_string(),
            "--pack-folder".to_string(),
            "pack".to_string(),
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
            MockFileSystemProvider::new().with_current_dir(PathBuf::from("/test/workdir")),
        );

    let installer = PackwizInstaller::new(&session, jar_path).unwrap();
    let result = installer.install_mods("client", &PathBuf::from("/test/workdir"));

    assert!(result.is_err());
    match result.unwrap_err() {
        PackwizError::CommandFailed { stderr, .. } => {
            assert!(stderr.contains("Network error"));
        }
        _ => panic!("Wrong error type"),
    }
}

#[test]
fn test_cached_packwiz_check() {
    let mock_process = MockProcessProvider::new()
        .with_packwiz_result(
            vec![
                "--pack-file".to_string(),
                "/test/workdir/pack/pack.toml".to_string(),
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
                "/test/workdir/pack/pack.toml".to_string(),
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
            MockFileSystemProvider::new().with_current_dir(PathBuf::from("/test/workdir")),
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
