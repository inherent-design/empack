//! Native archive operations for zip, tar.gz, and 7z formats.
//!
//! Provides create and extract functions that replace external tool dependencies
//! (`zip`, `unzip`, `tar`) with native Rust crate implementations. All functions
//! operate on real filesystem paths and are tested with real temp directories.

use std::fs::File;
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Supported archive formats for distribution packaging.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveFormat {
    Zip,
    TarGz,
    SevenZ,
}

impl ArchiveFormat {
    /// Returns the file extension for this format (without leading dot).
    pub fn extension(&self) -> &str {
        match self {
            ArchiveFormat::Zip => "zip",
            ArchiveFormat::TarGz => "tar.gz",
            ArchiveFormat::SevenZ => "7z",
        }
    }
}

impl std::fmt::Display for ArchiveFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.extension())
    }
}

/// Errors produced by archive operations.
#[derive(Debug, Error)]
pub enum ArchiveError {
    #[error("IO error at {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("zip error: {0}")]
    Zip(#[from] zip::result::ZipError),

    #[error("7z compression error: {0}")]
    SevenZ(String),

    #[error("archive verification failed: {0:#}")]
    Verification(#[from] anyhow::Error),

    #[error("source directory is empty: {0}")]
    EmptySource(PathBuf),

    #[error("source directory does not exist: {0}")]
    SourceNotFound(PathBuf),
}

/// Extract a zip archive to a directory.
///
/// Creates `output_dir` if it does not exist. Preserves directory structure
/// and sanitizes paths to prevent zip-slip attacks (handled by the `zip`
/// crate internally).
pub fn extract_zip(archive_path: &Path, output_dir: &Path) -> Result<(), ArchiveError> {
    let file = File::open(archive_path).map_err(|e| ArchiveError::Io {
        path: archive_path.to_path_buf(),
        source: e,
    })?;
    let mut archive = zip::ZipArchive::new(file)?;
    std::fs::create_dir_all(output_dir).map_err(|e| ArchiveError::Io {
        path: output_dir.to_path_buf(),
        source: e,
    })?;
    archive.extract(output_dir)?;
    Ok(())
}

/// Create an archive from a directory in the specified format.
///
/// The archive contains the contents of `source_dir` with relative paths
/// rooted at `source_dir` itself (i.e., `source_dir/foo.txt` becomes
/// `foo.txt` in the archive).
///
/// Returns `ArchiveError::SourceNotFound` if the directory does not exist,
/// or `ArchiveError::EmptySource` if it contains no files.
pub fn create_archive(
    source_dir: &Path,
    output_path: &Path,
    format: ArchiveFormat,
) -> Result<(), ArchiveError> {
    if !source_dir.exists() {
        return Err(ArchiveError::SourceNotFound(source_dir.to_path_buf()));
    }

    let parent = output_path
        .parent()
        .filter(|value| !value.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let source = source_dir
        .canonicalize()
        .map_err(|source| ArchiveError::Io {
            path: source_dir.to_owned(),
            source,
        })?;
    let destination = parent.canonicalize().map_err(|source| ArchiveError::Io {
        path: parent.to_owned(),
        source,
    })?;
    if destination.starts_with(&source) {
        return Err(ArchiveError::Verification(anyhow::anyhow!(
            "Archive output must be outside its input tree"
        )));
    }
    let mut candidate =
        tempfile::NamedTempFile::new_in(parent).map_err(|source| ArchiveError::Io {
            path: output_path.to_owned(),
            source,
        })?;
    let format = match format {
        ArchiveFormat::Zip => empack_core::model::DistributionArchive::Zip,
        ArchiveFormat::TarGz => empack_core::model::DistributionArchive::TarGz,
        ArchiveFormat::SevenZ => empack_core::model::DistributionArchive::SevenZip,
    };
    crate::engine::artifacts::package_directory(
        source_dir,
        candidate.as_file_mut(),
        format,
        &crate::application::process_runtime::Cancellation::default(),
    )
    .map_err(|error| {
        if error.is::<crate::engine::artifacts::EmptyArchiveSource>() {
            ArchiveError::EmptySource(source_dir.to_owned())
        } else {
            ArchiveError::Verification(error)
        }
    })?;
    candidate
        .as_file()
        .sync_all()
        .map_err(|source| ArchiveError::Io {
            path: output_path.to_owned(),
            source,
        })?;
    candidate
        .persist(output_path)
        .map_err(|error| ArchiveError::Io {
            path: output_path.to_owned(),
            source: error.error,
        })?;
    Ok(())
}

/// Publish a ZIP with named file replacements, streaming the existing entries.
/// Callers validate each source and destination against their workflow roots.
pub fn overlay_zip(archive_path: &Path, additions: &[(PathBuf, String)]) -> anyhow::Result<()> {
    let parent = archive_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Archive has no parent"))?;
    let temporary = tempfile::NamedTempFile::new_in(parent)?;
    let mut input = zip::ZipArchive::new(File::open(archive_path)?)?;
    let mut output = zip::ZipWriter::new(temporary.reopen()?);
    let names: std::collections::HashSet<_> =
        additions.iter().map(|(_, name)| name.as_str()).collect();
    anyhow::ensure!(
        names.len() == additions.len(),
        "Duplicate archive overlay destination"
    );
    for index in 0..input.len() {
        let entry = input.by_index(index)?;
        if !names.contains(entry.name()) {
            output.raw_copy_file(entry)?;
        }
    }
    for (source, name) in additions {
        crate::empack::paths::validate_relative_destination(Path::new(""), Path::new(name))?;
        output.start_file(name, zip::write::SimpleFileOptions::default())?;
        std::io::copy(&mut File::open(source)?, &mut output)?;
    }
    output.finish()?.sync_all()?;
    drop(input);
    temporary.persist(archive_path).map_err(|e| e.error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    include!("archive.test.rs");
}
