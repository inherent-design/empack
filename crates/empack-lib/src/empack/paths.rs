//! Workflow checks for generated files. The filesystem provider remains path-transparent.

use anyhow::{Context, Result, bail};
use std::path::{Component, Path};

/// Reject metadata that cannot form a portable, single filename component.
pub fn validate_filename(value: &str) -> Result<()> {
    empack_core::path::ArtifactStem::parse(value)
        .with_context(|| format!("Invalid artifact filename component: {value:?}"))?;
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

/// A file under the managed pack tree, validated against the current filesystem.
/// Revalidate immediately before mutation; this is not a filesystem capability.
#[derive(Debug, Clone)]
pub struct TrackedProjectFile(std::path::PathBuf);

impl TrackedProjectFile {
    pub fn validate(
        fs: &dyn crate::application::session::FileSystemProvider,
        root: &Path,
        relative: &str,
    ) -> Result<Self> {
        let spelling = relative;
        let relative = Path::new(relative);
        anyhow::ensure!(
            relative.is_relative(),
            "Tracked local dependency path must be relative"
        );
        anyhow::ensure!(
            !relative
                .components()
                .any(|c| matches!(c, Component::ParentDir)),
            "Tracked local dependency path escapes the project directory"
        );
        let inside = relative.strip_prefix("pack").map_err(|_| {
            anyhow::anyhow!("Tracked local build inputs must be stored under pack/")
        })?;
        anyhow::ensure!(
            !inside.as_os_str().is_empty(),
            "Tracked local dependency must name a file under pack/"
        );
        empack_core::path::InstallDestination::parse(spelling)?;
        let path = root.join(relative);
        fs.validate_output_path(root, &path)?;
        anyhow::ensure!(
            !fs.exists(&path) || fs.is_regular_file(&path),
            "Tracked local dependency is not a regular file: {}",
            path.display()
        );
        Ok(Self(path))
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
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
    fn reserved_device_names_are_rejected_on_every_host() {
        for name in ["CON", "nul.jar", "COM1.zip", "lpt9", "CONIN$", "LPT².txt"] {
            assert!(validate_filename(name).is_err(), "{name}");
        }
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
