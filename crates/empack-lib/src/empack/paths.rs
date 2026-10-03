//! Workflow checks for generated files. The filesystem provider remains path-transparent.

use anyhow::{Result, bail};
use std::path::{Component, Path};

/// Reject metadata that cannot form a portable, single filename component.
pub fn validate_filename(value: &str) -> Result<()> {
    if value.is_empty()
        || matches!(value, "." | "..")
        || value.ends_with(['.', ' '])
        || value
            .chars()
            .any(|c| c.is_control() || "/\\:<>\"|?*".contains(c))
    {
        bail!("Invalid artifact filename component: {value:?}");
    }
    Ok(())
}

/// Require a destination below its workflow root without parent traversal.
pub fn validate_relative_destination(root: &Path, destination: &Path) -> Result<()> {
    let relative = destination.strip_prefix(root).map_err(|_| {
        anyhow::anyhow!(
            "Output path is outside {}: {}",
            root.display(),
            destination.display()
        )
    })?;
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        bail!("Invalid output destination: {}", destination.display());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_cannot_be_a_path() {
        for value in [
            "../escape",
            "/absolute",
            "..\\escape",
            "C:escape",
            "a\nb",
            "..",
            "",
            "name.",
        ] {
            assert!(validate_filename(value).is_err(), "{value:?}");
        }
        assert!(validate_filename("A Pack $(literal)").is_ok());
    }

    #[test]
    fn reject_parent_traversal_even_with_matching_prefix() {
        let root = Path::new("project/dist");
        assert!(validate_relative_destination(root, &root.join("../outside.jar")).is_err());
        assert!(validate_relative_destination(root, &root.join("mods/file.jar")).is_ok());
    }
    #[cfg(unix)]
    #[test]
    fn live_output_paths_reject_symlinked_ancestors() {
        use crate::application::session::{FileSystemProvider, LiveFileSystemProvider};
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), root.path().join("dist")).unwrap();
        assert!(
            LiveFileSystemProvider
                .validate_output_path(root.path(), &root.path().join("dist/mods/file.jar"))
                .is_err()
        );
        assert!(!outside.path().join("mods").exists());
    }
}
