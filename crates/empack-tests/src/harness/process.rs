use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crate::fixtures::WorkflowProjectFixture;

/// Resolve the empack binary path.
///
/// Checks in order:
/// - `EMPACK_E2E_BIN`
/// - llvm-cov instrumented builds first when coverage is active
/// - debug/release builds for normal local runs or fallback
/// - bare PATH fallback
pub fn empack_bin() -> PathBuf {
    if std::env::var_os("EMPACK_QUALIFY").is_some() {
        static QUALIFIED: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
        return QUALIFIED
            .get_or_init(|| {
                let selected = std::env::var_os("EMPACK_E2E_BIN").map(PathBuf::from);
                let digest = std::env::var("EMPACK_EXPECT_BINARY_SHA256").ok();
                let version = std::env::var("EMPACK_EXPECT_VERSION").ok();
                validate_qualification_binary(
                    selected.as_deref(),
                    digest.as_deref(),
                    version.as_deref(),
                )
                .expect("qualification requires the exact selected artifact")
            })
            .clone();
    }
    let selected = if let Some(bin) = std::env::var_os("EMPACK_E2E_BIN") {
        PathBuf::from(bin)
    } else {
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let target_root = manifest.join("../../target");
        let exe = if cfg!(windows) { ".exe" } else { "" };
        let coverage_active = std::env::var_os("LLVM_PROFILE_FILE").is_some()
            || std::env::var_os("CARGO_LLVM_COV").is_some();
        find_empack_bin(&target_root, exe, coverage_active)
            .unwrap_or_else(|| PathBuf::from(format!("empack{exe}")))
    };
    static ANNOUNCED: std::sync::Once = std::sync::Once::new();
    ANNOUNCED.call_once(|| {
        eprintln!(
            "empack test executable: {} (local discovery)",
            selected.display()
        )
    });
    selected
}

fn validate_qualification_binary(
    selected: Option<&Path>,
    expected_digest: Option<&str>,
    expected_version: Option<&str>,
) -> anyhow::Result<PathBuf> {
    use anyhow::{Context, ensure};
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let selected = selected.context("EMPACK_E2E_BIN is required; discovery is disabled")?;
    ensure!(
        selected.is_absolute(),
        "Qualification executable must be absolute"
    );
    let selected = selected.canonicalize()?;
    ensure!(
        selected.is_file(),
        "Qualification executable must be a regular file"
    );
    let expected_digest = expected_digest.context("EMPACK_EXPECT_BINARY_SHA256 is required")?;
    ensure!(
        expected_digest.len() == 64 && expected_digest.bytes().all(|b| b.is_ascii_hexdigit()),
        "Expected SHA-256 must contain 64 hexadecimal digits"
    );
    let expected_version = expected_version.context("EMPACK_EXPECT_VERSION is required")?;
    ensure!(
        !expected_version.is_empty(),
        "Expected version must not be empty"
    );
    let mut file = std::fs::File::open(&selected)?;
    let mut digest = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    let actual: String = digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    ensure!(
        actual.eq_ignore_ascii_case(expected_digest),
        "Selected executable SHA-256 differs from qualification input"
    );
    let root = tempfile::tempdir()?;
    let mut command = Command::new(&selected);
    configure_command_env(&mut command, root.path());
    command.current_dir(root.path()).arg("--version");
    let output = assert_cmd::Command::from_std(command)
        .timeout(std::time::Duration::from_secs(10))
        .output()?;
    ensure!(
        output.status.success(),
        "Selected executable version probe failed"
    );
    ensure!(
        String::from_utf8(output.stdout)?.trim() == format!("empack {expected_version}"),
        "Selected executable version differs from qualification input"
    );
    eprintln!(
        "empack test executable: {} sha256={actual} version={expected_version} (qualification)",
        selected.display()
    );
    Ok(selected)
}

fn find_empack_bin(target_root: &Path, exe: &str, coverage_active: bool) -> Option<PathBuf> {
    let profiles: &[&str] = if coverage_active {
        &[
            "llvm-cov-target/debug",
            "llvm-cov-target/release",
            "debug",
            "release",
        ]
    } else {
        &["debug", "release"]
    };
    profiles
        .iter()
        .map(|profile| target_root.join(format!("{profile}/empack{exe}")))
        .find(|candidate| candidate.is_file())
}

pub fn has_java() -> bool {
    command_succeeds(Command::new("java").arg("-version"))
}

fn command_succeeds(command: &mut Command) -> bool {
    command
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// Resolve the same CurseForge credential default used by the CLI.
pub fn curseforge_key() -> String {
    std::env::var("EMPACK_KEY_CURSEFORGE").unwrap_or_else(|_| {
        empack_lib::application::config::defaults::CURSEFORGE_API_CLIENT_KEY.to_string()
    })
}

pub fn has_cf_key() -> bool {
    !curseforge_key().trim().is_empty()
}

/// Report an unavailable prerequisite, failing strict verification runs.
pub fn prerequisite_available(available: bool, reason: &str) -> bool {
    if !available {
        assert!(
            std::env::var_os("EMPACK_E2E_STRICT").is_none()
                && std::env::var_os("EMPACK_QUALIFY").is_none(),
            "Missing E2E prerequisite: {reason}"
        );
        eprintln!("SKIP: {reason}");
    }
    available
}

// Preserve only process/tool essentials. Product settings and fixture controls must
// be supplied by the test that owns them, not inherited from the developer's shell.
const CHILD_ENVIRONMENT: &[&str] = &[
    "PATH",
    "Path",
    "SystemRoot",
    "SYSTEMROOT",
    "WINDIR",
    "COMSPEC",
    "PATHEXT",
    "JAVA_HOME",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
    "LLVM_PROFILE_FILE",
];

fn configure_command_env(cmd: &mut Command, workdir: &Path) {
    let inherited = std::env::vars_os().collect::<Vec<_>>();
    configure_environment(cmd, workdir, &inherited);
}

fn configure_environment(
    cmd: &mut Command,
    workdir: &Path,
    inherited: &[(std::ffi::OsString, std::ffi::OsString)],
) {
    cmd.env_clear();
    for (key, value) in inherited {
        if CHILD_ENVIRONMENT.iter().any(|allowed| key == allowed) {
            cmd.env(key, value);
        }
    }
    // Do not precreate state/cache: inspection and help tests need to observe
    // whether the product creates them. Every project gets independent paths.
    let home = workdir.join(".harness-home");
    cmd.env("TMPDIR", workdir)
        .env("TMP", workdir)
        .env("TEMP", workdir)
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("APPDATA", home.join("roaming"))
        .env("LOCALAPPDATA", home.join("local"))
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("XDG_DATA_HOME", home.join("data"))
        .env("XDG_CACHE_HOME", home.join("cache"))
        .env("EMPACK_CACHE_DIR", workdir.join(".empack-cache"))
        .env("EMPACK_STATE_DIR", workdir.join(".host-state"))
        .env("NO_COLOR", "1")
        .env("LANG", "C")
        .env("LC_ALL", "C")
        .env("TZ", "UTC");
}

/// Return early from a test when Java is not in PATH.
#[macro_export]
macro_rules! skip_if_no_java {
    () => {
        if !$crate::e2e::prerequisite_available($crate::e2e::has_java(), "java -version failed") {
            return;
        }
    };
}

/// Return early from a test when the CurseForge API key is not set.
#[macro_export]
macro_rules! skip_if_no_cf_key {
    () => {
        if !$crate::e2e::prerequisite_available(
            $crate::e2e::has_cf_key(),
            "CurseForge API key unavailable",
        ) {
            return;
        }
    };
}

/// Isolated test project backed by a temporary directory.
///
/// The TempDir is held for the lifetime of this struct; dropping it
/// cleans up all files. Use `dir()` for the working directory and
/// `cmd()` for a pre-configured empack Command.
pub struct TestProject {
    _tmp: tempfile::TempDir,
    root: PathBuf,
}

impl Default for TestProject {
    fn default() -> Self {
        Self::new()
    }
}

impl TestProject {
    /// Create a new empty test project directory.
    pub fn new() -> Self {
        let tmp = tempfile::TempDir::new().expect("failed to create temp dir");
        let root = tmp.path().to_path_buf();
        Self { _tmp: tmp, root }
    }

    /// Create a test project with an initialized empack pack.
    pub fn initialized(name: &str, loader: &str, mc_version: &str) -> Self {
        let project = Self::new();
        init_project(&project.root, name, loader, mc_version);
        Self {
            root: project.root.join(name),
            _tmp: project._tmp,
        }
    }

    /// Create a test project from the shared workflow fixture without live init.
    pub fn workflow_fixture(name: &str, loader: &str, mc_version: &str) -> Self {
        let project = Self::new();
        let mut fixture = WorkflowProjectFixture::new(name.to_string());
        fixture.loader = loader.to_string();
        fixture.minecraft_version = mc_version.to_string();
        fixture
            .write_to(&project.root)
            .unwrap_or_else(|e| panic!("failed to write workflow fixture: {e}"));
        project
    }

    /// Supply explicit publisher/runtime policy for native-consumer fixtures.
    pub fn configure_native_distribution(&self, pack_id: &str, java_major: u16) {
        use empack_core::model::ResolvedProject;
        use empack_lib::engine::documents::DocumentCodec;
        let manifest = self.root.join("empack.yml");
        let lock_path = self.root.join("empack.lock");
        let decoded = empack_lib::testing::read_authoring(&manifest).unwrap();
        let prior = DocumentCodec
            .decode_lock(&std::fs::read(&lock_path).unwrap(), &decoded, "test")
            .unwrap();
        let mut intent = prior.intent().clone();
        intent.metadata.id = pack_id.into();
        intent.runtime.java_major = java_major;
        let bytes = DocumentCodec.encode_intent(&intent).unwrap();
        let revision = DocumentCodec
            .decode_intent(&bytes, "test")
            .unwrap()
            .semantic_revision();
        let mut lock = prior.lock().clone();
        lock.intent_revision = revision;
        let project = ResolvedProject::validate(intent, lock, revision).unwrap();
        empack_lib::testing::write_authoring(&manifest, project.intent()).unwrap();
        std::fs::write(lock_path, DocumentCodec.encode_lock(&project).unwrap()).unwrap();
    }

    /// Working directory for this project.
    pub fn dir(&self) -> &Path {
        &self.root
    }

    /// Build an empack Command pre-configured with NO_COLOR and the
    /// project working directory.
    pub fn cmd(&self) -> Command {
        let mut cmd = Command::new(empack_bin());
        cmd.current_dir(&self.root);
        configure_command_env(&mut cmd, &self.root);
        cmd
    }

    pub fn run_output(&self, args: &[&str]) -> Output {
        run_empack_output(&self.root, args)
    }

    /// Assert a file relative to the project root contains the expected string.
    pub fn assert_contains(&self, relative: &str, expected: &str) {
        assert_file_contains(&self.root.join(relative), expected);
    }

    /// Assert a file relative to the project root exists.
    pub fn assert_exists(&self, relative: &str) {
        assert_file_exists(&self.root.join(relative));
    }
}

/// Build an assertion command with the same isolated environment as all CLI tests.
pub fn empack_assert_cmd(workdir: &Path) -> assert_cmd::Command {
    assert_cmd::Command::from_std(empack_cmd(workdir))
}

/// Bound offline CLI fixtures and reject accidental external acquisition through
/// the product's ordinary proxy configuration. This is not an OS network sandbox.
pub fn offline_command(workdir: &Path) -> assert_cmd::Command {
    let mut command = empack_assert_cmd(workdir);
    command
        .env("EMPACK_COLOR", "never")
        .env("EMPACK_NET_TIMEOUT", "1")
        .env("HTTP_PROXY", "http://127.0.0.1:9")
        .env("HTTPS_PROXY", "http://127.0.0.1:9")
        .env("ALL_PROXY", "http://127.0.0.1:9")
        .env("NO_PROXY", "")
        .timeout(std::time::Duration::from_secs(30));
    command
}

/// Expected artifact version, independent of the test crate's Cargo version.
/// Qualification supplies the expectation; local runs probe their selected executable.
pub fn expected_cli_version() -> String {
    if let Ok(version) = std::env::var("EMPACK_EXPECT_VERSION") {
        return version;
    }
    assert!(
        std::env::var_os("EMPACK_QUALIFY").is_none(),
        "qualification requires an expected version"
    );
    let project = TestProject::new();
    let output = empack_assert_cmd(project.dir())
        .arg("--version")
        .timeout(std::time::Duration::from_secs(10))
        .output()
        .expect("probe selected executable version");
    assert!(output.status.success(), "version probe failed: {output:?}");
    let text = String::from_utf8(output.stdout).expect("UTF-8 version");
    let version = text
        .trim()
        .strip_prefix("empack ")
        .expect("empack version prefix");
    assert!(!version.is_empty() && !version.contains(char::is_whitespace));
    version.to_owned()
}

/// Development binaries scaffold the declared compatibility version; released
/// binaries scaffold their own version. Neither value comes from this test crate.
pub fn expected_automation_version() -> String {
    let version = expected_cli_version();
    if version == "0.0.0-dev" {
        "0.8.0-beta".to_owned()
    } else {
        version
    }
}

/// Build an empack Command pointed at a specific directory with NO_COLOR.
pub fn empack_cmd(workdir: &Path) -> Command {
    let mut cmd = Command::new(empack_bin());
    cmd.current_dir(workdir);
    configure_command_env(&mut cmd, workdir);
    cmd
}

fn format_output_for_debug(output: &Output) -> String {
    format!(
        "status: {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn run_empack_output(workdir: &Path, args: &[&str]) -> Output {
    empack_cmd(workdir)
        .args(args)
        .output()
        .expect("failed to spawn empack command")
}

/// Initialize an empack project non-interactively.
///
/// Panics if the init command fails.
pub fn init_project(parent: &Path, name: &str, loader: &str, mc_version: &str) -> PathBuf {
    let output = run_empack_output(
        parent,
        &[
            "init",
            "--yes",
            "--modloader",
            loader,
            "--mc-version",
            mc_version,
            name,
        ],
    );
    assert!(
        output.status.success(),
        "empack init failed\n{}",
        format_output_for_debug(&output)
    );
    parent.join(name)
}

/// Assert a file exists and contains the expected substring.
pub fn assert_file_contains(path: &Path, expected: &str) {
    let content = std::fs::read_to_string(path)
        .unwrap_or_else(|_| panic!("failed to read {}", path.display()));
    assert!(
        content.contains(expected),
        "{} does not contain '{}'\ncontent:\n{}",
        path.display(),
        expected,
        content
    );
}

/// Assert a file exists at the given path.
pub fn assert_file_exists(path: &Path) {
    assert!(path.exists(), "expected file at {}", path.display());
}

/// Initialization publishes coherent intent and an exact runtime lock.
pub fn assert_project_initialized(project_root: &Path) {
    let _ = read_project(project_root);
}
pub fn read_project(project_root: &Path) -> empack_core::model::ResolvedProject {
    let codec = empack_lib::engine::documents::DocumentCodec;
    let intent = empack_lib::testing::read_authoring(&project_root.join("empack.yml")).unwrap();
    codec
        .decode_lock(
            &std::fs::read(project_root.join("empack.lock")).unwrap(),
            &intent,
            "empack.lock",
        )
        .unwrap()
}
fn loader_name(loader: empack_core::model::LoaderKind) -> &'static str {
    use empack_core::model::LoaderKind;
    match loader {
        LoaderKind::Vanilla => "none",
        LoaderKind::Fabric => "fabric",
        LoaderKind::Quilt => "quilt",
        LoaderKind::Forge => "forge",
        LoaderKind::NeoForge => "neoforge",
    }
}
pub fn assert_project_loader(project_root: &Path, expected_loader: &str) {
    assert_eq!(
        loader_name(read_project(project_root).intent().runtime.loader),
        expected_loader
    );
}
pub fn assert_project_minecraft_version(project_root: &Path, expected_version: &str) {
    assert_eq!(
        read_project(project_root)
            .intent()
            .runtime
            .minecraft
            .as_str(),
        expected_version
    );
}
pub fn assert_project_loader_absent(project_root: &Path) {
    assert_eq!(
        read_project(project_root).intent().runtime.loader,
        empack_core::model::LoaderKind::Vanilla
    );
}
pub fn assert_project_datapack_folder(project_root: &Path, expected_folder: &str) {
    assert_eq!(
        read_project(project_root)
            .intent()
            .layout
            .get(&empack_core::model::ContentKind::DataPack)
            .map(|p| p.as_str()),
        Some(expected_folder)
    );
}
pub fn assert_locked_loader_version(project_root: &Path, loader: &str, expected_version: &str) {
    let project = read_project(project_root);
    assert_eq!(loader_name(project.lock().runtime.loader), loader);
    assert_eq!(
        project
            .lock()
            .runtime
            .loader_version
            .as_ref()
            .map(|v| v.as_str()),
        Some(expected_version)
    );
}
pub fn assert_locked_minecraft_version(project_root: &Path, expected_version: &str) {
    assert_eq!(
        read_project(project_root).lock().runtime.minecraft.as_str(),
        expected_version
    );
}
pub fn assert_locked_loader_version_prefix(
    project_root: &Path,
    loader: &str,
    expected_prefix: &str,
) {
    let project = read_project(project_root);
    assert_eq!(loader_name(project.lock().runtime.loader), loader);
    assert!(
        project
            .lock()
            .runtime
            .loader_version
            .as_ref()
            .is_some_and(|v| v.as_str().starts_with(expected_prefix))
    );
}

/// Assert a dist artifact with the given suffix exists and return its path.
pub fn assert_dist_artifact_suffix(project_root: &Path, suffix: &str) -> PathBuf {
    let dist_dir = project_root.join("dist");
    let entries = std::fs::read_dir(&dist_dir)
        .unwrap_or_else(|_| panic!("failed to read {}", dist_dir.display()));

    for entry in entries.filter_map(|entry| entry.ok()) {
        let path = entry.path();
        if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with(suffix))
        {
            return path;
        }
    }

    panic!(
        "expected dist artifact ending with '{}' under {}",
        suffix,
        dist_dir.display()
    );
}

/// Count `.pw.toml` files recursively under `pack_root/mods`.
///
/// Returns 0 if the directory does not exist.
pub fn count_pw_toml_files(pack_root: &Path) -> usize {
    let mods_dir = pack_root.join("mods");
    std::fs::read_dir(mods_dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter(|e| e.path().extension().is_some_and(|ext| ext == "toml"))
                .count()
        })
        .unwrap_or(0)
}

/// Create a minimal Modrinth mrpack archive for local import tests.
pub fn write_local_mrpack(
    archive_path: &Path,
    pack_name: &str,
    version_id: &str,
    minecraft_version: &str,
    loader_id: &str,
    loader_version: &str,
) -> anyhow::Result<()> {
    let mut dependencies = serde_json::Map::new();
    dependencies.insert(
        "minecraft".to_string(),
        serde_json::Value::String(minecraft_version.to_string()),
    );
    dependencies.insert(
        loader_id.to_string(),
        serde_json::Value::String(loader_version.to_string()),
    );

    let manifest = serde_json::json!({
        "formatVersion": 1,
        "game": "minecraft",
        "name": pack_name,
        "versionId": version_id,
        "summary": "Local test fixture",
        "files": [],
        "dependencies": dependencies,
    });

    crate::fixtures::write_zip(
        archive_path,
        &[(
            "modrinth.index.json",
            &serde_json::to_vec_pretty(&manifest)?,
        )],
    )
}

/// Live restricted imports must retain exact restart state and expose read-only inspection
/// and explicit cleanup. A generic provider/network failure cannot satisfy this contract.
pub fn assert_pending_import_lifecycle(
    workdir: &Path,
    target: &str,
    state: &Path,
    output: &Output,
) {
    assert!(!output.status.success());
    let diagnostic = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        diagnostic.contains("content obligations need explicit input"),
        "{diagnostic}"
    );
    assert!(
        diagnostic.contains("Import continuation was saved"),
        "{diagnostic}"
    );
    let target_path = workdir.join(target);
    assert!(
        !target_path.exists(),
        "incomplete import must not create its destination"
    );
    let records = std::fs::read_dir(state.join("pending-imports"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|value| value == "json"))
        .collect::<Vec<_>>();
    assert_eq!(records.len(), 1);
    let original = std::fs::read(&records[0]).unwrap();
    let record: serde_json::Value = serde_json::from_slice(&original).unwrap();
    assert_eq!(record["schema"], 1);
    let id = record["archive"]
        .as_array()
        .unwrap()
        .iter()
        .map(|byte| format!("{:02x}", byte.as_u64().unwrap()))
        .collect::<String>();
    let archive = state
        .join("pending-import-content")
        .join(format!("{id}.blob"));
    assert_eq!(
        std::fs::metadata(&archive).unwrap().len(),
        record["archive_bytes"].as_u64().unwrap()
    );
    let preview = empack_cmd(workdir)
        .env("EMPACK_STATE_DIR", state)
        .args(["init", "--continue", "--dry-run", target])
        .output()
        .unwrap();
    assert!(!preview.status.success());
    let diagnostic = format!(
        "{}{}",
        String::from_utf8_lossy(&preview.stdout),
        String::from_utf8_lossy(&preview.stderr)
    );
    assert!(
        diagnostic.contains("content obligations need explicit input"),
        "{diagnostic}"
    );
    assert_eq!(std::fs::read(&records[0]).unwrap(), original);
    assert!(!target_path.exists());
    for dry in [true, false] {
        let mut command = empack_cmd(workdir);
        command
            .env("EMPACK_STATE_DIR", state)
            .arg("--workdir")
            .arg(&target_path)
            .args(["--yes", "clean", "import"]);
        if dry {
            command.arg("--dry-run");
        }
        let cleanup = command.output().unwrap();
        assert!(
            cleanup.status.success(),
            "{}",
            format_output_for_debug(&cleanup)
        );
        if dry {
            assert_eq!(std::fs::read(&records[0]).unwrap(), original);
        } else {
            assert!(!records[0].exists());
        }
        assert!(!target_path.exists());
        assert!(
            archive.exists(),
            "explicit record cleanup retains verified source content"
        );
    }
}

#[cfg(test)]
mod harness_tests {
    use super::*;

    #[test]
    fn project_fixture_creates_temporary_root() {
        assert!(TestProject::new().dir().exists());
    }

    #[test]
    fn coverage_uses_instrumented_binary_even_with_existing_debug_build() {
        let root = tempfile::tempdir().unwrap();
        for profile in ["debug", "release", "llvm-cov-target/debug"] {
            let dir = root.path().join(profile);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("empack"), "fixture").unwrap();
        }
        assert_eq!(
            find_empack_bin(root.path(), "", true).unwrap(),
            root.path().join("llvm-cov-target/debug/empack")
        );
        assert_eq!(
            find_empack_bin(root.path(), "", false).unwrap(),
            root.path().join("debug/empack")
        );
    }

    #[test]
    fn ordinary_runs_do_not_select_stale_coverage_binary() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("llvm-cov-target/debug");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("empack.exe"), "fixture").unwrap();
        assert!(find_empack_bin(root.path(), ".exe", false).is_none());
        assert_eq!(
            find_empack_bin(root.path(), ".exe", true).unwrap(),
            dir.join("empack.exe")
        );
    }

    #[test]
    fn prerequisite_probe_requires_successful_exit() {
        let exe = std::env::current_exe().unwrap();
        assert!(command_succeeds(Command::new(&exe).arg("--list")));
        assert!(!command_succeeds(
            Command::new(&exe).arg("--invalid-prerequisite-probe")
        ));
        assert!(!command_succeeds(&mut Command::new(
            "nonexistent-empack-test-program"
        )));
    }
    #[test]
    fn qualification_refuses_missing_relative_and_wrong_digest_inputs() {
        assert!(validate_qualification_binary(None, None, None).is_err());
        assert!(validate_qualification_binary(Some(Path::new("empack")), None, None).is_err());
        let executable = std::env::current_exe().unwrap();
        let error = validate_qualification_binary(
            Some(&executable),
            Some(&"0".repeat(64)),
            Some("0.8.4-beta"),
        )
        .unwrap_err();
        assert!(error.to_string().contains("SHA-256 differs"));
    }

    #[cfg(unix)]
    #[test]
    fn qualification_checks_version_after_matching_bytes() {
        use sha2::{Digest, Sha256};
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let executable = root.path().join("empack fixture");
        let bytes = b"#!/bin/sh\nprintf 'empack 0.8.4-beta\\n'\n";
        std::fs::write(&executable, bytes).unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755)).unwrap();
        let digest: String = Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        assert_eq!(
            validate_qualification_binary(Some(&executable), Some(&digest), Some("0.8.4-beta"))
                .unwrap(),
            executable.canonicalize().unwrap()
        );
        let error =
            validate_qualification_binary(Some(&executable), Some(&digest), Some("0.8.3-beta"))
                .unwrap_err();
        assert!(error.to_string().contains("version differs"));
    }

    #[test]
    fn child_environment_excludes_host_configuration_and_test_authority() {
        use std::ffi::OsString;
        let root = tempfile::tempdir().unwrap();
        let inherited: Vec<(OsString, OsString)> = [
            ("PATH", "/fixture/bin"),
            ("HOME", "/private/home"),
            ("TMPDIR", "/private/tmp"),
            ("EMPACK_E2E_BIN", "/wrong/binary"),
            ("EMPACK_QUALIFY", "1"),
            ("EMPACK_WORKDIR", "/wrong/project"),
            ("EMPACK_YES", "1"),
            ("EMPACK_KEY_CURSEFORGE", "private"),
            ("EMPACK_TEST_FAULT", "1"),
            ("HTTPS_PROXY", "http://wrong"),
        ]
        .into_iter()
        .map(|(k, v)| (k.into(), v.into()))
        .collect();
        let mut command = Command::new("unused");
        configure_environment(&mut command, root.path(), &inherited);
        let env = command
            .get_envs()
            .filter_map(|(k, v)| v.map(|v| (k.to_owned(), v.to_owned())))
            .collect::<std::collections::BTreeMap<_, _>>();
        assert_eq!(
            env.get(std::ffi::OsStr::new("PATH")).unwrap(),
            "/fixture/bin"
        );
        assert_eq!(
            env.get(std::ffi::OsStr::new("HOME")).unwrap(),
            &root.path().join(".harness-home").into_os_string()
        );
        for key in ["TMPDIR", "TMP", "TEMP"] {
            assert_eq!(env.get(std::ffi::OsStr::new(key)).unwrap(), root.path());
        }
        for key in [
            "EMPACK_E2E_BIN",
            "EMPACK_QUALIFY",
            "EMPACK_WORKDIR",
            "EMPACK_YES",
            "EMPACK_KEY_CURSEFORGE",
            "EMPACK_TEST_FAULT",
            "HTTPS_PROXY",
        ] {
            assert!(!env.contains_key(std::ffi::OsStr::new(key)), "{key}");
        }
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    }
}
