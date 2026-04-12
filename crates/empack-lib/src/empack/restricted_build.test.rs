use super::*;
use crate::application::session_mocks::{MockFileSystemProvider, mock_root};
use std::ffi::OsString;
use std::path::Path;
use tempfile::TempDir;

struct EnvVarGuard {
    key: &'static str,
    previous: Option<OsString>,
}

impl EnvVarGuard {
    unsafe fn set(key: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
        let previous = std::env::var_os(key);
        unsafe {
            std::env::set_var(key, value);
        }
        Self { key, previous }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        unsafe {
            match &self.previous {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }
}

fn restricted_dest(workdir: &Path, target: &str, filename: &str) -> String {
    workdir
        .join("dist")
        .join(target)
        .join("mods")
        .join(filename)
        .to_string_lossy()
        .to_string()
}

fn sample_restricted_mods(workdir: &Path) -> Vec<RestrictedModInfo> {
    vec![
        RestrictedModInfo {
            name: "Entity Culling".to_string(),
            url: "https://www.curseforge.com/minecraft/mc-mods/entityculling/download/4763646"
                .to_string(),
            dest_path: restricted_dest(workdir, "client-full", "entityculling.jar"),
        },
        RestrictedModInfo {
            name: "Entity Culling".to_string(),
            url: "https://www.curseforge.com/minecraft/mc-mods/entityculling/download/4763646"
                .to_string(),
            dest_path: restricted_dest(workdir, "server-full", "entityculling.jar"),
        },
    ]
}

#[test]
fn save_and_load_pending_build_round_trips() {
    let _guard = crate::test_support::env_lock().lock().unwrap();
    let cache_root = TempDir::new().expect("cache root tempdir");
    let _cache_dir = unsafe { EnvVarGuard::set("EMPACK_CACHE_DIR", cache_root.path()) };

    let workdir = mock_root().join("restricted-build-roundtrip");
    let provider = MockFileSystemProvider::new()
        .with_current_dir(workdir.clone())
        .with_configured_project(workdir.clone());

    let saved = save_pending_build(
        &provider,
        &workdir,
        &[BuildTarget::ClientFull],
        ArchiveFormat::Zip,
        &sample_restricted_mods(&workdir),
    )
    .expect("save pending build");
    let loaded = load_pending_build(&provider, &workdir)
        .expect("load pending build")
        .expect("pending build exists");

    assert_eq!(loaded, saved);
    assert_eq!(loaded.schema_version, 1);
    assert_eq!(loaded.targets, vec!["client-full"]);
    assert_eq!(loaded.entries.len(), 2);
    assert_eq!(loaded.entries[0].filename, "entityculling.jar");
}

#[test]
fn validate_pending_build_detects_fingerprint_mismatch() {
    let _guard = crate::test_support::env_lock().lock().unwrap();
    let cache_root = TempDir::new().expect("cache root tempdir");
    let _cache_dir = unsafe { EnvVarGuard::set("EMPACK_CACHE_DIR", cache_root.path()) };

    let workdir = mock_root().join("restricted-build-fingerprint");
    let provider = MockFileSystemProvider::new()
        .with_current_dir(workdir.clone())
        .with_configured_project(workdir.clone());

    let pending = save_pending_build(
        &provider,
        &workdir,
        &[BuildTarget::ClientFull],
        ArchiveFormat::Zip,
        &sample_restricted_mods(&workdir),
    )
    .expect("save pending build");

    provider
        .write_file(&workdir.join("empack.yml"), "empack:\n  name: changed\n")
        .expect("rewrite empack.yml");

    let stale = validate_pending_build(&provider, &workdir, &pending)
        .expect("validate pending build")
        .expect("stale reason");
    assert!(stale.contains("project files changed"));
}

#[test]
fn validate_pending_build_detects_missing_target_dir() {
    let _guard = crate::test_support::env_lock().lock().unwrap();
    let cache_root = TempDir::new().expect("cache root tempdir");
    let _cache_dir = unsafe { EnvVarGuard::set("EMPACK_CACHE_DIR", cache_root.path()) };

    let workdir = mock_root().join("restricted-build-missing-target");
    let provider = MockFileSystemProvider::new()
        .with_current_dir(workdir.clone())
        .with_configured_project(workdir.clone());

    let pending = save_pending_build(
        &provider,
        &workdir,
        &[BuildTarget::ClientFull],
        ArchiveFormat::Zip,
        &sample_restricted_mods(&workdir),
    )
    .expect("save pending build");

    let stale = validate_pending_build(&provider, &workdir, &pending)
        .expect("validate pending build")
        .expect("stale reason");
    assert!(stale.contains("required build directory is missing"));
}

#[test]
fn validate_pending_build_allows_missing_future_full_build_dirs_for_mrpack_restrictions() {
    let _guard = crate::test_support::env_lock().lock().unwrap();
    let cache_root = TempDir::new().expect("cache root tempdir");
    let _cache_dir = unsafe { EnvVarGuard::set("EMPACK_CACHE_DIR", cache_root.path()) };

    let workdir = mock_root().join("restricted-build-mrpack-only");
    let provider = MockFileSystemProvider::new()
        .with_current_dir(workdir.clone())
        .with_configured_project(workdir.clone());

    let pending = save_pending_build(
        &provider,
        &workdir,
        &[BuildTarget::Mrpack, BuildTarget::ClientFull, BuildTarget::ServerFull],
        ArchiveFormat::Zip,
        &[RestrictedModInfo {
            name: "Bee Fix".to_string(),
            url: "https://www.curseforge.com/minecraft/mc-mods/bee-fix/download/4618962"
                .to_string(),
            dest_path: cache_root
                .path()
                .join("packwiz")
                .join("cache")
                .join("import")
                .join("BeeFix-1.20-1.0.7.jar")
                .to_string_lossy()
                .to_string(),
        }],
    )
    .expect("save pending build");

    assert_eq!(
        validate_pending_build(&provider, &workdir, &pending).expect("validate pending build"),
        None
    );
}

#[test]
fn restricted_cache_dir_is_stable_per_project_root() {
    let _guard = crate::test_support::env_lock().lock().unwrap();
    let cache_root = TempDir::new().expect("cache root tempdir");
    let _cache_dir = unsafe { EnvVarGuard::set("EMPACK_CACHE_DIR", cache_root.path()) };

    let first_workdir = mock_root().join("first-project");
    let second_workdir = mock_root().join("second-project");

    let first = restricted_cache_dir(&first_workdir).expect("first cache dir");
    let first_again = restricted_cache_dir(&first_workdir).expect("first cache dir again");
    let second = restricted_cache_dir(&second_workdir).expect("second cache dir");

    assert_eq!(first, first_again);
    assert_ne!(first, second);
    assert!(first.starts_with(cache_root.path()));
    let hashed = first
        .file_name()
        .expect("cache dir should have hashed final component")
        .to_string_lossy()
        .to_string();
    assert_eq!(hashed.len(), 64);
    assert_eq!(hashed, hex_sha256(first_workdir.to_string_lossy().as_bytes()));
}

#[test]
fn imports_downloads_into_cache_and_restores_to_every_destination() {
    let _guard = crate::test_support::env_lock().lock().unwrap();
    let cache_root = TempDir::new().expect("cache root tempdir");
    let _cache_dir = unsafe { EnvVarGuard::set("EMPACK_CACHE_DIR", cache_root.path()) };

    let workdir = mock_root().join("restricted-build-stage");
    let downloads_dir = workdir.join("downloads");
    let provider = MockFileSystemProvider::new()
        .with_current_dir(workdir.clone())
        .with_configured_project(workdir.clone())
        .with_file(
            downloads_dir.join("entityculling.jar"),
            "cached mod bytes".to_string(),
        );

    let pending = save_pending_build(
        &provider,
        &workdir,
        &[BuildTarget::ClientFull, BuildTarget::ServerFull],
        ArchiveFormat::Zip,
        &sample_restricted_mods(&workdir),
    )
    .expect("save pending build");

    provider
        .create_dir_all(&workdir.join("dist").join("client-full"))
        .expect("create client-full");
    provider
        .create_dir_all(&workdir.join("dist").join("server-full"))
        .expect("create server-full");

    import_matching_downloads_into_cache(&provider, &pending, std::slice::from_ref(&downloads_dir))
        .expect("import downloads into cache");
    assert!(
        provider.exists(&pending.restricted_cache_path().join("entityculling.jar")),
        "download should be imported into the managed restricted cache"
    );

    let missing = stage_cached_entries_to_destinations(&provider, &pending)
        .expect("stage cached entries to destinations");
    assert!(missing.is_empty());
    assert_eq!(
        provider
            .read_bytes(&PathBuf::from(&pending.entries[0].dest_path))
            .expect("read first restored file"),
        b"cached mod bytes"
    );
    assert_eq!(
        provider
            .read_bytes(&PathBuf::from(&pending.entries[1].dest_path))
            .expect("read second restored file"),
        b"cached mod bytes"
    );
}
