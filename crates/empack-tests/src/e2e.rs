use std::path::{Path, PathBuf};
use std::process::Command;

/// Resolve the empack binary path.
///
/// Checks in order: `EMPACK_E2E_BIN` env var, llvm-cov instrumented
/// binary, debug build, release build, bare PATH.
pub fn empack_bin() -> PathBuf {
    if let Ok(bin) = std::env::var("EMPACK_E2E_BIN") {
        return PathBuf::from(bin);
    }

    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let target_root = manifest.join("../../target");
    let exe = if cfg!(windows) { ".exe" } else { "" };

    for cov_dir in &["llvm-cov-target/debug", "llvm-cov-target/release"] {
        let candidate = target_root.join(format!("{cov_dir}/empack{exe}"));
        if candidate.exists() {
            return candidate;
        }
    }

    for profile in &["debug", "release"] {
        let candidate = target_root.join(format!("{profile}/empack{exe}"));
        if candidate.exists() {
            return candidate;
        }
    }

    PathBuf::from(format!("empack{exe}"))
}

pub fn has_packwiz() -> bool {
    Command::new("packwiz")
        .arg("--help")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok()
}

pub fn has_java() -> bool {
    Command::new("java")
        .arg("-version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok()
}

pub fn has_cf_key() -> bool {
    std::env::var("EMPACK_KEY_CURSEFORGE").is_ok()
}

/// Return early from a test when packwiz is not in PATH.
#[macro_export]
macro_rules! skip_if_no_packwiz {
    () => {
        if !$crate::e2e::has_packwiz() {
            eprintln!("SKIP: packwiz not in PATH");
            return;
        }
    };
}

/// Return early from a test when Java is not in PATH.
#[macro_export]
macro_rules! skip_if_no_java {
    () => {
        $crate::skip_if_no_packwiz!();
        if !$crate::e2e::has_java() {
            eprintln!("SKIP: java not in PATH");
            return;
        }
    };
}

/// Return early from a test when the CurseForge API key is not set.
#[macro_export]
macro_rules! skip_if_no_cf_key {
    () => {
        $crate::skip_if_no_packwiz!();
        if !$crate::e2e::has_cf_key() {
            eprintln!("SKIP: EMPACK_KEY_CURSEFORGE not set");
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

    /// Working directory for this project.
    pub fn dir(&self) -> &Path {
        &self.root
    }

    /// Build an empack Command pre-configured with NO_COLOR and the
    /// project working directory.
    pub fn cmd(&self) -> Command {
        let mut cmd = Command::new(empack_bin());
        cmd.current_dir(&self.root);
        cmd.env("NO_COLOR", "1");
        cmd
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

/// Build an assert_cmd Command from the resolved empack binary.
///
/// Prefers the llvm-cov instrumented binary when available so E2E
/// tests contribute to coverage reports.
pub fn empack_assert_cmd() -> assert_cmd::Command {
    let mut cmd = assert_cmd::Command::new(empack_bin());
    cmd.env("NO_COLOR", "1");
    cmd
}

/// Build an empack Command pointed at a specific directory with NO_COLOR.
pub fn empack_cmd(workdir: &Path) -> Command {
    let mut cmd = Command::new(empack_bin());
    cmd.current_dir(workdir);
    cmd.env("NO_COLOR", "1");
    cmd
}

/// Initialize an empack project non-interactively.
///
/// Panics if the init command fails.
pub fn init_project(parent: &Path, name: &str, loader: &str, mc_version: &str) -> PathBuf {
    let status = empack_cmd(parent)
        .args([
            "init", "--yes",
            "--modloader", loader,
            "--mc-version", mc_version,
            name,
        ])
        .status()
        .expect("failed to spawn empack init");
    assert!(status.success(), "empack init exited with {}", status);
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
