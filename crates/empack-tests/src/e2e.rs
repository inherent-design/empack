use std::path::{Path, PathBuf};
use std::process::Command;

/// Resolve the empack binary path.
///
/// Uses `EMPACK_E2E_BIN` env var if set; falls back to searching PATH
/// for the binary, or the cargo target directory.
pub fn empack_bin() -> PathBuf {
    if let Ok(bin) = std::env::var("EMPACK_E2E_BIN") {
        return PathBuf::from(bin);
    }

    let cargo_target = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/release/empack");
    if cargo_target.exists() {
        return cargo_target;
    }

    let debug_target = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/debug/empack");
    if debug_target.exists() {
        return debug_target;
    }

    PathBuf::from("empack")
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

/// Initialize an empack project non-interactively.
///
/// Panics if the init command fails.
pub fn init_project(parent: &Path, name: &str, loader: &str, mc_version: &str) -> PathBuf {
    let status = Command::new(empack_bin())
        .args([
            "init", "--yes",
            "--modloader", loader,
            "--mc-version", mc_version,
            name,
        ])
        .current_dir(parent)
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
