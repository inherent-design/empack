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

    match format {
        ArchiveFormat::Zip => create_zip_archive(source_dir, output_path),
        ArchiveFormat::TarGz => create_tar_gz_archive(source_dir, output_path),
        ArchiveFormat::SevenZ => create_7z_archive(source_dir, output_path),
    }
}

fn create_zip_archive(source_dir: &Path, output_path: &Path) -> Result<(), ArchiveError> {
    use std::io::{Read, Write};
    use zip::write::SimpleFileOptions;
    use zip::CompressionMethod;

    let file = File::create(output_path).map_err(|e| ArchiveError::Io {
        path: output_path.to_path_buf(),
        source: e,
    })?;
    let mut zip_writer = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);

    let mut has_entries = false;

    fn walk_dir(
        base: &Path,
        current: &Path,
        writer: &mut zip::ZipWriter<File>,
        options: SimpleFileOptions,
        has_entries: &mut bool,
    ) -> Result<(), ArchiveError> {
        let entries = std::fs::read_dir(current).map_err(|e| ArchiveError::Io {
            path: current.to_path_buf(),
            source: e,
        })?;

        for entry in entries {
            let entry = entry.map_err(|e| ArchiveError::Io {
                path: current.to_path_buf(),
                source: e,
            })?;
            let path = entry.path();
            let relative = path
                .strip_prefix(base)
                .expect("path must be under base");
            let name = relative.to_string_lossy();

            if path.is_dir() {
                writer
                    .add_directory(format!("{}/", name), options)
                    .map_err(ArchiveError::Zip)?;
                walk_dir(base, &path, writer, options, has_entries)?;
            } else {
                *has_entries = true;
                writer
                    .start_file(name.to_string(), options)
                    .map_err(ArchiveError::Zip)?;
                let mut f = File::open(&path).map_err(|e| ArchiveError::Io {
                    path: path.clone(),
                    source: e,
                })?;
                let mut buffer = Vec::new();
                f.read_to_end(&mut buffer).map_err(|e| ArchiveError::Io {
                    path: path.clone(),
                    source: e,
                })?;
                writer.write_all(&buffer).map_err(|e| ArchiveError::Io {
                    path: path.clone(),
                    source: e,
                })?;
            }
        }
        Ok(())
    }

    walk_dir(source_dir, source_dir, &mut zip_writer, options, &mut has_entries)?;

    if !has_entries {
        drop(zip_writer);
        let _ = std::fs::remove_file(output_path);
        return Err(ArchiveError::EmptySource(source_dir.to_path_buf()));
    }

    zip_writer.finish().map_err(ArchiveError::Zip)?;
    Ok(())
}

fn create_tar_gz_archive(source_dir: &Path, output_path: &Path) -> Result<(), ArchiveError> {
    use flate2::write::GzEncoder;
    use flate2::Compression;

    let file = File::create(output_path).map_err(|e| ArchiveError::Io {
        path: output_path.to_path_buf(),
        source: e,
    })?;
    let enc = GzEncoder::new(file, Compression::default());
    let mut tar_builder = tar::Builder::new(enc);
    tar_builder
        .append_dir_all(".", source_dir)
        .map_err(|e| ArchiveError::Io {
            path: source_dir.to_path_buf(),
            source: e,
        })?;
    tar_builder.finish().map_err(|e| ArchiveError::Io {
        path: output_path.to_path_buf(),
        source: e,
    })?;
    Ok(())
}

fn create_7z_archive(source_dir: &Path, output_path: &Path) -> Result<(), ArchiveError> {
    sevenz_rust2::compress_to_path(source_dir, output_path)
        .map_err(|e| ArchiveError::SevenZ(e.to_string()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    include!("archive.test.rs");
}
