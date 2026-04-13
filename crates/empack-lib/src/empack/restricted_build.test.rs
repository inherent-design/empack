use super::*;
use crate::application::session::FileMetadata;
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

fn recent_file_metadata(len: usize, timestamp_ms: u64) -> FileMetadata {
    FileMetadata {
        is_directory: false,
        len: len as u64,
        modified_unix_ms: Some(timestamp_ms),
        created_unix_ms: Some(timestamp_ms),
    }
}

fn modified_only_file_metadata(len: usize, timestamp_ms: u64) -> FileMetadata {
    FileMetadata {
        is_directory: false,
        len: len as u64,
        modified_unix_ms: Some(timestamp_ms),
        created_unix_ms: None,
    }
}

fn sample_resourcepack_restricted_mod(workdir: &Path) -> RestrictedModInfo {
    RestrictedModInfo {
        name: "No Enchant Glint".to_string(),
        url: "https://www.curseforge.com/minecraft/texture-packs/no-enchant-glint/download/4660358"
            .to_string(),
        dest_path: workdir
            .join("dist")
            .join("client-full")
            .join("resourcepacks")
            .join("No_Enchant_Glint.zip")
            .to_string_lossy()
            .to_string(),
    }
}

fn sample_deceasedcraft_resourcepack_restricted_mod(workdir: &Path) -> RestrictedModInfo {
    RestrictedModInfo {
        name: "No Enchant Glint".to_string(),
        url: "https://www.curseforge.com/minecraft/texture-packs/no-enchant-glint/download/4660358"
            .to_string(),
        dest_path: workdir
            .join("dist")
            .join("client-full")
            .join("resourcepacks")
            .join("§6No Enchant Glint 1.20.1.zip")
            .to_string_lossy()
            .to_string(),
    }
}

fn baseline_snapshot(path: PathBuf, metadata: &FileMetadata) -> PendingRestrictedCandidateSnapshot {
    PendingRestrictedCandidateSnapshot {
        path: path.to_string_lossy().to_string(),
        len: metadata.len,
        modified_unix_ms: metadata.modified_unix_ms,
        created_unix_ms: metadata.created_unix_ms,
    }
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
    assert!(loaded.candidate_baseline.is_empty());
}

#[test]
fn persist_pending_build_round_trips_candidate_baseline() {
    let _guard = crate::test_support::env_lock().lock().unwrap();
    let cache_root = TempDir::new().expect("cache root tempdir");
    let _cache_dir = unsafe { EnvVarGuard::set("EMPACK_CACHE_DIR", cache_root.path()) };

    let workdir = mock_root().join("restricted-build-baseline-roundtrip");
    let downloads_dir = workdir.join("downloads");
    let provider = MockFileSystemProvider::new()
        .with_current_dir(workdir.clone())
        .with_configured_project(workdir.clone())
        .with_binary_file_and_metadata(
            downloads_dir.join("existing.zip"),
            b"existing bytes".to_vec(),
            recent_file_metadata("existing bytes".len(), 123_456),
        );

    let mut pending = save_pending_build(
        &provider,
        &workdir,
        &[BuildTarget::ClientFull],
        ArchiveFormat::Zip,
        &[sample_resourcepack_restricted_mod(&workdir)],
    )
    .expect("save pending build");
    pending.candidate_baseline =
        capture_candidate_baseline(&provider, std::slice::from_ref(&downloads_dir))
            .expect("capture baseline");

    persist_pending_build(&provider, &workdir, &pending).expect("persist pending build");
    let loaded = load_pending_build(&provider, &workdir)
        .expect("load pending build")
        .expect("pending build exists");

    assert_eq!(loaded, pending);
    assert_eq!(loaded.candidate_baseline.len(), 1);
}

#[test]
fn load_pending_build_defaults_missing_candidate_baseline() {
    let _guard = crate::test_support::env_lock().lock().unwrap();
    let cache_root = TempDir::new().expect("cache root tempdir");
    let _cache_dir = unsafe { EnvVarGuard::set("EMPACK_CACHE_DIR", cache_root.path()) };

    let workdir = mock_root().join("restricted-build-missing-baseline");
    let provider = MockFileSystemProvider::new()
        .with_current_dir(workdir.clone())
        .with_configured_project(workdir.clone());

    let pending = save_pending_build(
        &provider,
        &workdir,
        &[BuildTarget::ClientFull],
        ArchiveFormat::Zip,
        &[sample_resourcepack_restricted_mod(&workdir)],
    )
    .expect("save pending build");

    let state_path = pending_state_path(&workdir);
    let mut json: serde_json::Value =
        serde_json::from_str(&provider.read_to_string(&state_path).expect("read pending json"))
            .expect("parse pending json");
    json.as_object_mut()
        .expect("pending json object")
        .remove("candidate_baseline");
    provider
        .write_file(
            &state_path,
            &serde_json::to_string_pretty(&json).expect("serialize pending json"),
        )
        .expect("rewrite pending json");

    let loaded = load_pending_build(&provider, &workdir)
        .expect("load pending build")
        .expect("pending build exists");

    assert!(loaded.candidate_baseline.is_empty());
    assert_eq!(loaded.entries, pending.entries);
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

    import_matching_downloads_into_cache(
        &provider,
        &workdir,
        &pending,
        std::slice::from_ref(&downloads_dir),
    )
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

#[test]
fn import_matching_downloads_into_cache_imports_recent_unicode_named_zip_when_exact_filename_is_missing(
) {
    let _guard = crate::test_support::env_lock().lock().unwrap();
    let cache_root = TempDir::new().expect("cache root tempdir");
    let _cache_dir = unsafe { EnvVarGuard::set("EMPACK_CACHE_DIR", cache_root.path()) };

    let workdir = mock_root().join("restricted-build-unicode-recent");
    let downloads_dir = workdir.join("downloads");
    let bytes = b"manual resource pack bytes".to_vec();
    let provider = MockFileSystemProvider::new()
        .with_current_dir(workdir.clone())
        .with_configured_project(workdir.clone())
        .with_binary_file_and_metadata(
            downloads_dir.join("§6No Enchant Glint 1.20.1.zip"),
            bytes.clone(),
            recent_file_metadata(bytes.len(), 205_000),
        );

    let mut pending = save_pending_build(
        &provider,
        &workdir,
        &[BuildTarget::ClientFull],
        ArchiveFormat::Zip,
        &[sample_resourcepack_restricted_mod(&workdir)],
    )
    .expect("save pending build");
    pending.recorded_at_unix_ms = Some(200_000);

    import_matching_downloads_into_cache(
        &provider,
        &workdir,
        &pending,
        std::slice::from_ref(&downloads_dir),
    )
    .expect("import recent unicode download");

    assert_eq!(
        provider
            .read_bytes(&pending.restricted_cache_path().join("No_Enchant_Glint.zip"))
            .expect("read cached file"),
        bytes
    );
}

#[test]
fn import_matching_downloads_into_cache_imports_exact_deceasedcraft_filename_when_present() {
    let _guard = crate::test_support::env_lock().lock().unwrap();
    let cache_root = TempDir::new().expect("cache root tempdir");
    let _cache_dir = unsafe { EnvVarGuard::set("EMPACK_CACHE_DIR", cache_root.path()) };

    let workdir = mock_root().join("restricted-build-exact-deceasedcraft");
    let downloads_dir = workdir.join("downloads");
    let exact_name = "§6No Enchant Glint 1.20.1.zip";
    let bytes = b"exact deceasedcraft bytes".to_vec();
    let provider = MockFileSystemProvider::new()
        .with_current_dir(workdir.clone())
        .with_configured_project(workdir.clone())
        .with_binary_file_and_metadata(
            downloads_dir.join(exact_name),
            bytes.clone(),
            recent_file_metadata(bytes.len(), 205_000),
        );

    let pending = save_pending_build(
        &provider,
        &workdir,
        &[BuildTarget::ClientFull],
        ArchiveFormat::Zip,
        &[sample_deceasedcraft_resourcepack_restricted_mod(&workdir)],
    )
    .expect("save pending build");

    import_matching_downloads_into_cache(
        &provider,
        &workdir,
        &pending,
        std::slice::from_ref(&downloads_dir),
    )
    .expect("import exact deceasedcraft filename");

    assert_eq!(
        provider
            .read_bytes(&pending.restricted_cache_path().join(exact_name))
            .expect("read cached exact file"),
        bytes
    );
}

#[test]
fn import_matching_downloads_into_cache_ignores_old_unicode_zip_candidates() {
    let _guard = crate::test_support::env_lock().lock().unwrap();
    let cache_root = TempDir::new().expect("cache root tempdir");
    let _cache_dir = unsafe { EnvVarGuard::set("EMPACK_CACHE_DIR", cache_root.path()) };

    let workdir = mock_root().join("restricted-build-unicode-old");
    let downloads_dir = workdir.join("downloads");
    let bytes = b"old resource pack bytes".to_vec();
    let provider = MockFileSystemProvider::new()
        .with_current_dir(workdir.clone())
        .with_configured_project(workdir.clone())
        .with_binary_file_and_metadata(
            downloads_dir.join("§6No Enchant Glint 1.20.1.zip"),
            bytes,
            recent_file_metadata(23, 150_000),
        );

    let mut pending = save_pending_build(
        &provider,
        &workdir,
        &[BuildTarget::ClientFull],
        ArchiveFormat::Zip,
        &[sample_resourcepack_restricted_mod(&workdir)],
    )
    .expect("save pending build");
    pending.recorded_at_unix_ms = Some(200_000);

    import_matching_downloads_into_cache(
        &provider,
        &workdir,
        &pending,
        std::slice::from_ref(&downloads_dir),
    )
    .expect("import old unicode candidate");

    assert!(
        !provider.exists(&pending.restricted_cache_path().join("No_Enchant_Glint.zip")),
        "old candidate should not be imported"
    );
    assert_eq!(missing_cached_entries(&provider, &pending).len(), 1);
}

#[test]
fn import_matching_downloads_into_cache_uses_modified_time_when_created_time_is_unavailable() {
    let _guard = crate::test_support::env_lock().lock().unwrap();
    let cache_root = TempDir::new().expect("cache root tempdir");
    let _cache_dir = unsafe { EnvVarGuard::set("EMPACK_CACHE_DIR", cache_root.path()) };

    let workdir = mock_root().join("restricted-build-modified-fallback");
    let downloads_dir = workdir.join("downloads");
    let bytes = b"modified-only bytes".to_vec();
    let provider = MockFileSystemProvider::new()
        .with_current_dir(workdir.clone())
        .with_configured_project(workdir.clone())
        .with_binary_file_and_metadata(
            downloads_dir.join("§6No Enchant Glint 1.20.1.zip"),
            bytes.clone(),
            modified_only_file_metadata(bytes.len(), 205_000),
        );

    let mut pending = save_pending_build(
        &provider,
        &workdir,
        &[BuildTarget::ClientFull],
        ArchiveFormat::Zip,
        &[sample_resourcepack_restricted_mod(&workdir)],
    )
    .expect("save pending build");
    pending.recorded_at_unix_ms = Some(200_000);

    import_matching_downloads_into_cache(
        &provider,
        &workdir,
        &pending,
        std::slice::from_ref(&downloads_dir),
    )
    .expect("import modified-only candidate");

    assert_eq!(
        provider
            .read_bytes(&pending.restricted_cache_path().join("No_Enchant_Glint.zip"))
            .expect("read cached file"),
        bytes
    );
}

#[test]
fn import_matching_downloads_into_cache_collapses_duplicate_candidates_by_sha256() {
    let _guard = crate::test_support::env_lock().lock().unwrap();
    let cache_root = TempDir::new().expect("cache root tempdir");
    let _cache_dir = unsafe { EnvVarGuard::set("EMPACK_CACHE_DIR", cache_root.path()) };

    let workdir = mock_root().join("restricted-build-deduped-hash");
    let downloads_a = workdir.join("downloads-a");
    let downloads_b = workdir.join("downloads-b");
    let bytes = b"duplicate candidate bytes".to_vec();
    let provider = MockFileSystemProvider::new()
        .with_current_dir(workdir.clone())
        .with_configured_project(workdir.clone())
        .with_binary_file_and_metadata(
            downloads_a.join("§6No Enchant Glint 1.20.1.zip"),
            bytes.clone(),
            recent_file_metadata(bytes.len(), 205_000),
        )
        .with_binary_file_and_metadata(
            downloads_b.join("No Enchant Glint copy.zip"),
            bytes.clone(),
            recent_file_metadata(bytes.len(), 206_000),
        );

    let mut pending = save_pending_build(
        &provider,
        &workdir,
        &[BuildTarget::ClientFull],
        ArchiveFormat::Zip,
        &[sample_resourcepack_restricted_mod(&workdir)],
    )
    .expect("save pending build");
    pending.recorded_at_unix_ms = Some(200_000);

    import_matching_downloads_into_cache(
        &provider,
        &workdir,
        &pending,
        &[downloads_a, downloads_b],
    )
    .expect("import duplicate candidates");

    assert!(
        provider.exists(&pending.restricted_cache_path().join("No_Enchant_Glint.zip")),
        "duplicate candidates with identical hashes should collapse into one import"
    );
}

#[test]
fn import_matching_downloads_into_cache_does_not_guess_when_multiple_distinct_recent_zip_candidates_exist(
) {
    let _guard = crate::test_support::env_lock().lock().unwrap();
    let cache_root = TempDir::new().expect("cache root tempdir");
    let _cache_dir = unsafe { EnvVarGuard::set("EMPACK_CACHE_DIR", cache_root.path()) };

    let workdir = mock_root().join("restricted-build-ambiguous-recent");
    let downloads_dir = workdir.join("downloads");
    let provider = MockFileSystemProvider::new()
        .with_current_dir(workdir.clone())
        .with_configured_project(workdir.clone())
        .with_binary_file_and_metadata(
            downloads_dir.join("§6No Enchant Glint 1.20.1.zip"),
            b"first bytes".to_vec(),
            recent_file_metadata(11, 205_000),
        )
        .with_binary_file_and_metadata(
            downloads_dir.join("Other recent pack.zip"),
            b"second bytes".to_vec(),
            recent_file_metadata(12, 206_000),
        );

    let mut pending = save_pending_build(
        &provider,
        &workdir,
        &[BuildTarget::ClientFull],
        ArchiveFormat::Zip,
        &[sample_resourcepack_restricted_mod(&workdir)],
    )
    .expect("save pending build");
    pending.recorded_at_unix_ms = Some(200_000);

    import_matching_downloads_into_cache(
        &provider,
        &workdir,
        &pending,
        std::slice::from_ref(&downloads_dir),
    )
    .expect("scan ambiguous candidates");

    assert!(
        !provider.exists(&pending.restricted_cache_path().join("No_Enchant_Glint.zip")),
        "ambiguous recent candidates should not be guessed"
    );
    assert_eq!(missing_cached_entries(&provider, &pending).len(), 1);
}

#[test]
fn import_matching_downloads_into_cache_ignores_preexisting_recent_zip_noise_when_baseline_exists() {
    let _guard = crate::test_support::env_lock().lock().unwrap();
    let cache_root = TempDir::new().expect("cache root tempdir");
    let _cache_dir = unsafe { EnvVarGuard::set("EMPACK_CACHE_DIR", cache_root.path()) };

    let workdir = mock_root().join("restricted-build-baseline-noise");
    let downloads_dir = workdir.join("downloads");
    let noise_a = downloads_dir.join("noise-a.zip");
    let noise_b = downloads_dir.join("noise-b.zip");
    let noise_c = downloads_dir.join("noise-c.zip");
    let target_path = downloads_dir.join("§6No Enchant Glint 1.20.1.zip");
    let noise_a_meta = recent_file_metadata(7, 205_000);
    let noise_b_meta = recent_file_metadata(7, 206_000);
    let noise_c_meta = recent_file_metadata(7, 207_000);
    let provider = MockFileSystemProvider::new()
        .with_current_dir(workdir.clone())
        .with_configured_project(workdir.clone())
        .with_binary_file_and_metadata(noise_a.clone(), b"noise-a".to_vec(), noise_a_meta.clone())
        .with_binary_file_and_metadata(noise_b.clone(), b"noise-b".to_vec(), noise_b_meta.clone())
        .with_binary_file_and_metadata(noise_c.clone(), b"noise-c".to_vec(), noise_c_meta.clone());

    let mut pending = save_pending_build(
        &provider,
        &workdir,
        &[BuildTarget::ClientFull],
        ArchiveFormat::Zip,
        &[sample_deceasedcraft_resourcepack_restricted_mod(&workdir)],
    )
    .expect("save pending build");
    pending.candidate_baseline = vec![
        baseline_snapshot(noise_a, &noise_a_meta),
        baseline_snapshot(noise_b, &noise_b_meta),
        baseline_snapshot(noise_c, &noise_c_meta),
    ];

    provider
        .write_bytes(&target_path, b"manual resource pack bytes")
        .expect("write target file");
    provider.set_file_metadata(
        target_path,
        recent_file_metadata("manual resource pack bytes".len(), 208_000),
    );

    import_matching_downloads_into_cache(
        &provider,
        &workdir,
        &pending,
        std::slice::from_ref(&downloads_dir),
    )
    .expect("import with baseline noise");

    assert_eq!(
        provider
            .read_bytes(
                &pending
                    .restricted_cache_path()
                    .join("§6No Enchant Glint 1.20.1.zip")
            )
            .expect("read cached target"),
        b"manual resource pack bytes"
    );
}

#[test]
fn missing_cached_entries_treats_preexisting_unchanged_cache_as_missing_when_baseline_exists() {
    let _guard = crate::test_support::env_lock().lock().unwrap();
    let cache_root = TempDir::new().expect("cache root tempdir");
    let _cache_dir = unsafe { EnvVarGuard::set("EMPACK_CACHE_DIR", cache_root.path()) };

    let workdir = mock_root().join("restricted-build-stale-cache-missing");
    let provider = MockFileSystemProvider::new()
        .with_current_dir(workdir.clone())
        .with_configured_project(workdir.clone());

    let mut pending = save_pending_build(
        &provider,
        &workdir,
        &[BuildTarget::ClientFull],
        ArchiveFormat::Zip,
        &[sample_resourcepack_restricted_mod(&workdir)],
    )
    .expect("save pending build");
    let cache_path = pending.restricted_cache_path().join("No_Enchant_Glint.zip");
    let cache_meta = recent_file_metadata("stale cache bytes".len(), 200_000);
    provider
        .write_bytes(&cache_path, b"stale cache bytes")
        .expect("write stale cache bytes");
    provider.set_file_metadata(cache_path.clone(), cache_meta.clone());
    pending.candidate_baseline = vec![baseline_snapshot(cache_path, &cache_meta)];

    assert_eq!(missing_cached_entries(&provider, &pending).len(), 1);
}

#[test]
fn import_matching_downloads_into_cache_refreshes_preexisting_stale_cache_from_exact_candidate() {
    let _guard = crate::test_support::env_lock().lock().unwrap();
    let cache_root = TempDir::new().expect("cache root tempdir");
    let _cache_dir = unsafe { EnvVarGuard::set("EMPACK_CACHE_DIR", cache_root.path()) };

    let workdir = mock_root().join("restricted-build-stale-cache-exact-refresh");
    let downloads_dir = workdir.join("downloads");
    let provider = MockFileSystemProvider::new()
        .with_current_dir(workdir.clone())
        .with_configured_project(workdir.clone());

    let mut pending = save_pending_build(
        &provider,
        &workdir,
        &[BuildTarget::ClientFull],
        ArchiveFormat::Zip,
        &[sample_resourcepack_restricted_mod(&workdir)],
    )
    .expect("save pending build");
    let cache_path = pending.restricted_cache_path().join("No_Enchant_Glint.zip");
    let cache_meta = recent_file_metadata("stale cache bytes".len(), 200_000);
    provider
        .write_bytes(&cache_path, b"stale cache bytes")
        .expect("write stale cache bytes");
    provider.set_file_metadata(cache_path.clone(), cache_meta.clone());
    pending.candidate_baseline = vec![baseline_snapshot(cache_path.clone(), &cache_meta)];

    provider
        .write_bytes(&downloads_dir.join("No_Enchant_Glint.zip"), b"fresh exact bytes")
        .expect("write fresh exact download");
    provider.set_file_metadata(
        downloads_dir.join("No_Enchant_Glint.zip"),
        recent_file_metadata("fresh exact bytes".len(), 205_000),
    );

    import_matching_downloads_into_cache(
        &provider,
        &workdir,
        &pending,
        std::slice::from_ref(&downloads_dir),
    )
    .expect("refresh stale cache from exact candidate");

    assert_eq!(
        provider.read_bytes(&cache_path).expect("read refreshed cache"),
        b"fresh exact bytes"
    );
    assert!(missing_cached_entries(&provider, &pending).is_empty());
}

#[test]
fn import_matching_downloads_into_cache_refreshes_preexisting_stale_cache_from_unique_baseline_candidate(
) {
    let _guard = crate::test_support::env_lock().lock().unwrap();
    let cache_root = TempDir::new().expect("cache root tempdir");
    let _cache_dir = unsafe { EnvVarGuard::set("EMPACK_CACHE_DIR", cache_root.path()) };

    let workdir = mock_root().join("restricted-build-stale-cache-fallback-refresh");
    let downloads_dir = workdir.join("downloads");
    let provider = MockFileSystemProvider::new()
        .with_current_dir(workdir.clone())
        .with_configured_project(workdir.clone());

    let mut pending = save_pending_build(
        &provider,
        &workdir,
        &[BuildTarget::ClientFull],
        ArchiveFormat::Zip,
        &[sample_resourcepack_restricted_mod(&workdir)],
    )
    .expect("save pending build");
    let cache_path = pending.restricted_cache_path().join("No_Enchant_Glint.zip");
    let cache_meta = recent_file_metadata("stale cache bytes".len(), 200_000);
    provider
        .write_bytes(&cache_path, b"stale cache bytes")
        .expect("write stale cache bytes");
    provider.set_file_metadata(cache_path.clone(), cache_meta.clone());
    pending.candidate_baseline = vec![baseline_snapshot(cache_path.clone(), &cache_meta)];

    let variant_path = downloads_dir.join("§6No Enchant Glint 1.20.1.zip");
    provider
        .write_bytes(&variant_path, b"fresh fallback bytes")
        .expect("write fallback candidate");
    provider.set_file_metadata(
        variant_path,
        recent_file_metadata("fresh fallback bytes".len(), 205_000),
    );

    import_matching_downloads_into_cache(
        &provider,
        &workdir,
        &pending,
        std::slice::from_ref(&downloads_dir),
    )
    .expect("refresh stale cache from unique fallback candidate");

    assert_eq!(
        provider.read_bytes(&cache_path).expect("read refreshed cache"),
        b"fresh fallback bytes"
    );
    assert!(missing_cached_entries(&provider, &pending).is_empty());
}

#[test]
fn import_matching_downloads_into_cache_leaves_preexisting_stale_cache_unresolved_without_new_candidate(
) {
    let _guard = crate::test_support::env_lock().lock().unwrap();
    let cache_root = TempDir::new().expect("cache root tempdir");
    let _cache_dir = unsafe { EnvVarGuard::set("EMPACK_CACHE_DIR", cache_root.path()) };

    let workdir = mock_root().join("restricted-build-stale-cache-unresolved");
    let provider = MockFileSystemProvider::new()
        .with_current_dir(workdir.clone())
        .with_configured_project(workdir.clone());

    let mut pending = save_pending_build(
        &provider,
        &workdir,
        &[BuildTarget::ClientFull],
        ArchiveFormat::Zip,
        &[sample_resourcepack_restricted_mod(&workdir)],
    )
    .expect("save pending build");
    let cache_path = pending.restricted_cache_path().join("No_Enchant_Glint.zip");
    let cache_meta = recent_file_metadata("stale cache bytes".len(), 200_000);
    provider
        .write_bytes(&cache_path, b"stale cache bytes")
        .expect("write stale cache bytes");
    provider.set_file_metadata(cache_path.clone(), cache_meta.clone());
    pending.candidate_baseline = vec![baseline_snapshot(cache_path.clone(), &cache_meta)];

    import_matching_downloads_into_cache(&provider, &workdir, &pending, &[])
        .expect("scan without new candidates");

    assert_eq!(
        provider.read_bytes(&cache_path).expect("read unchanged cache"),
        b"stale cache bytes"
    );
    assert_eq!(missing_cached_entries(&provider, &pending).len(), 1);
}

#[test]
fn missing_cached_entries_legacy_pending_still_trusts_existing_cache() {
    let _guard = crate::test_support::env_lock().lock().unwrap();
    let cache_root = TempDir::new().expect("cache root tempdir");
    let _cache_dir = unsafe { EnvVarGuard::set("EMPACK_CACHE_DIR", cache_root.path()) };

    let workdir = mock_root().join("restricted-build-legacy-cache-trust");
    let provider = MockFileSystemProvider::new()
        .with_current_dir(workdir.clone())
        .with_configured_project(workdir.clone());

    let pending = save_pending_build(
        &provider,
        &workdir,
        &[BuildTarget::ClientFull],
        ArchiveFormat::Zip,
        &[sample_resourcepack_restricted_mod(&workdir)],
    )
    .expect("save pending build");
    let cache_path = pending.restricted_cache_path().join("No_Enchant_Glint.zip");
    provider
        .write_bytes(&cache_path, b"legacy cache bytes")
        .expect("write legacy cache bytes");

    assert!(missing_cached_entries(&provider, &pending).is_empty());
}

#[test]
fn import_matching_downloads_into_cache_keeps_blocking_when_multiple_new_distinct_zip_candidates_exist_after_baseline(
) {
    let _guard = crate::test_support::env_lock().lock().unwrap();
    let cache_root = TempDir::new().expect("cache root tempdir");
    let _cache_dir = unsafe { EnvVarGuard::set("EMPACK_CACHE_DIR", cache_root.path()) };

    let workdir = mock_root().join("restricted-build-baseline-ambiguous");
    let downloads_dir = workdir.join("downloads");
    let baseline_path = downloads_dir.join("preexisting.zip");
    let baseline_meta = recent_file_metadata("baseline".len(), 200_000);
    let provider = MockFileSystemProvider::new()
        .with_current_dir(workdir.clone())
        .with_configured_project(workdir.clone())
        .with_binary_file_and_metadata(
            baseline_path.clone(),
            b"baseline".to_vec(),
            baseline_meta.clone(),
        );

    let mut pending = save_pending_build(
        &provider,
        &workdir,
        &[BuildTarget::ClientFull],
        ArchiveFormat::Zip,
        &[sample_resourcepack_restricted_mod(&workdir)],
    )
    .expect("save pending build");
    pending.candidate_baseline = vec![baseline_snapshot(baseline_path, &baseline_meta)];

    let first = downloads_dir.join("§6No Enchant Glint 1.20.1.zip");
    let second = downloads_dir.join("Another recent resource pack.zip");
    provider
        .write_bytes(&first, b"first bytes")
        .expect("write first candidate");
    provider.set_file_metadata(first, recent_file_metadata("first bytes".len(), 205_000));
    provider
        .write_bytes(&second, b"second bytes")
        .expect("write second candidate");
    provider.set_file_metadata(second, recent_file_metadata("second bytes".len(), 206_000));

    import_matching_downloads_into_cache(
        &provider,
        &workdir,
        &pending,
        std::slice::from_ref(&downloads_dir),
    )
    .expect("scan ambiguous post-baseline candidates");

    assert!(
        !provider.exists(
            &pending
                .restricted_cache_path()
                .join("No_Enchant_Glint.zip")
        ),
        "multiple new distinct candidates should remain ambiguous"
    );
    assert_eq!(missing_cached_entries(&provider, &pending).len(), 1);
}

#[test]
fn import_matching_downloads_into_cache_scans_managed_cache_for_recent_variant_names() {
    let _guard = crate::test_support::env_lock().lock().unwrap();
    let cache_root = TempDir::new().expect("cache root tempdir");
    let _cache_dir = unsafe { EnvVarGuard::set("EMPACK_CACHE_DIR", cache_root.path()) };

    let workdir = mock_root().join("restricted-build-cache-variant");
    let provider = MockFileSystemProvider::new()
        .with_current_dir(workdir.clone())
        .with_configured_project(workdir.clone());

    let mut pending = save_pending_build(
        &provider,
        &workdir,
        &[BuildTarget::ClientFull],
        ArchiveFormat::Zip,
        &[sample_resourcepack_restricted_mod(&workdir)],
    )
    .expect("save pending build");
    pending.recorded_at_unix_ms = Some(200_000);

    let variant_path = pending
        .restricted_cache_path()
        .join("§6No Enchant Glint 1.20.1.zip");
    provider
        .create_dir_all(&pending.restricted_cache_path())
        .expect("create cache dir");
    provider
        .write_bytes(&variant_path, b"cached variant bytes")
        .expect("write cache variant");
    provider.set_file_metadata(
        variant_path,
        recent_file_metadata("cached variant bytes".len(), 205_000),
    );

    import_matching_downloads_into_cache(&provider, &workdir, &pending, &[])
        .expect("import cache variant");

    assert_eq!(
        provider
            .read_bytes(&pending.restricted_cache_path().join("No_Enchant_Glint.zip"))
            .expect("read normalized cache target"),
        b"cached variant bytes"
    );
}
