//! Encoders consume retained private inputs and verify output before returning it.
use super::*;
use crate::engine::{snapshot::Observation, staging::FrozenStage};
use std::{fs::File, io::Write};

#[derive(Debug, thiserror::Error)]
#[error("Archive source contains no files")]
pub struct EmptyArchiveSource;

/// Transitional path adapter for existing build callers. It grants no project-document writer.
pub fn package_directory(
    selected: &std::path::Path,
    output: &mut File,
    format: DistributionArchive,
    cancel: &Cancellation,
) -> Result<VerifiedArchive> {
    use crate::engine::snapshot::{ProjectReadRoot, SnapshotLimits};
    use crate::engine::staging::MutableStage;
    let root = ProjectReadRoot::open(selected)?;
    let scopes = directory_members(&root)?;
    let snapshot = root.capture(&scopes, SnapshotLimits::default(), cancel)?;
    let expected: BTreeMap<_, _> = snapshot
        .entries()
        .iter()
        .filter_map(|(path, entry)| {
            let Observation::File(file) = entry else {
                return None;
            };
            #[cfg(unix)]
            let executable = file.mode & 0o100 != 0;
            #[cfg(not(unix))]
            let executable = false;
            Some((
                path.clone(),
                FileContent {
                    content: ContentId::from_sha256(file.content),
                    bytes: file.bytes,
                    permissions: FilePermissions {
                        readonly: file.readonly,
                        executable,
                    },
                },
            ))
        })
        .collect();
    if expected.is_empty() {
        return Err(EmptyArchiveSource.into());
    }
    let mut stage = MutableStage::from_snapshot(&root, &snapshot, cancel)?
        .freeze(SnapshotLimits::default(), cancel)?;
    let result = write_archive(
        &mut stage,
        output,
        format,
        &expected,
        ArchiveLimits::default(),
        cancel,
    )?;
    root.revalidate(&snapshot, cancel)?;
    ensure!(
        directory_members(&root)? == scopes,
        "Archive source membership changed"
    );
    Ok(result)
}
fn directory_members(
    root: &crate::engine::snapshot::ProjectReadRoot,
) -> Result<Vec<PortableRelPath>> {
    let mut paths = Vec::new();
    for entry in root.directory.entries()? {
        ensure!(
            paths.len() < ArchiveLimits::default().entries,
            "Archive source exceeds entry limit"
        );
        let name = entry?
            .file_name()
            .into_string()
            .map_err(|_| anyhow::anyhow!("Nonportable source name"))?;
        paths.push(PortableRelPath::parse(&name, PathSyntax::ProjectContent)?);
    }
    paths.sort();
    Ok(paths)
}

/// Write only into a caller-owned private candidate. Expected files are supplied independently.
/// Portable permissions come from intent, including executable bits on Windows hosts.
pub fn write_archive(
    stage: &mut FrozenStage,
    output: &mut File,
    format: DistributionArchive,
    expected: &BTreeMap<PortableRelPath, FileContent>,
    limits: ArchiveLimits,
    cancel: &Cancellation,
) -> Result<VerifiedArchive> {
    let mut directories = BTreeSet::new();
    let mut files = BTreeSet::new();
    for (path, observed) in stage.inventory() {
        match observed {
            Observation::File(file) => {
                let intended = expected
                    .get(path)
                    .ok_or_else(|| anyhow::anyhow!("Unplanned archive input: {}", path.as_str()))?;
                ensure!(
                    file.bytes == intended.bytes && file.content == *intended.content.bytes(),
                    "Archive input differs from inventory: {}",
                    path.as_str()
                );
                files.insert(path.clone());
            }
            Observation::Directory { .. } | Observation::Ancestor(_) => {
                directories.insert(path.clone());
            }
            Observation::Absent => {}
        }
    }
    ensure!(
        files.len() == expected.len(),
        "Archive input omits expected files"
    );
    ensure!(
        files.len().saturating_add(directories.len()) <= limits.entries,
        "Archive input exceeds entry limit"
    );
    let total = expected.values().try_fold(0u64, |total, file| {
        ensure!(
            file.bytes <= limits.file_bytes,
            "Archive input exceeds file limit"
        );
        total
            .checked_add(file.bytes)
            .ok_or_else(|| anyhow::anyhow!("Archive input size overflow"))
    })?;
    ensure!(
        total <= limits.total_bytes,
        "Archive input exceeds decoded byte limit"
    );
    cancel.check()?;
    output.set_len(0)?;
    output.rewind()?;
    let mut bounded = BoundedOutput {
        inner: &mut *output,
        position: 0,
        maximum: limits.compressed_bytes,
        cancel,
    };
    match format {
        DistributionArchive::Zip => {
            let mut writer = zip::ZipWriter::new(&mut bounded);
            for path in &directories {
                cancel.check()?;
                writer.add_directory(
                    format!("{}/", path.as_str()),
                    zip::write::SimpleFileOptions::default().unix_permissions(0o755),
                )?;
            }
            for (path, file) in expected {
                cancel.check()?;
                writer.start_file(
                    path.as_str(),
                    zip::write::SimpleFileOptions::default()
                        .unix_permissions(mode(file.permissions))
                        .large_file(file.bytes >= u64::from(u32::MAX)),
                )?;
                stage.copy_verified(path, &mut writer, cancel)?;
            }
            writer.finish()?;
        }
        DistributionArchive::TarGz => {
            let encoder =
                flate2::write::GzEncoder::new(&mut bounded, flate2::Compression::default());
            let mut writer = tar::Builder::new(encoder);
            for path in &directories {
                cancel.check()?;
                let mut header = tar::Header::new_gnu();
                header.set_entry_type(tar::EntryType::Directory);
                header.set_mode(0o755);
                header.set_size(0);
                header.set_mtime(0);
                header.set_cksum();
                writer.append_data(&mut header, path.as_str(), io::empty())?;
            }
            for (path, file) in expected {
                let mut header = tar::Header::new_gnu();
                header.set_entry_type(tar::EntryType::Regular);
                header.set_mode(mode(file.permissions));
                header.set_size(file.bytes);
                header.set_mtime(0);
                header.set_cksum();
                writer.append_data(
                    &mut header,
                    path.as_str(),
                    Cancellable {
                        input: stage.reader(path)?,
                        cancel,
                    },
                )?;
            }
            writer.into_inner()?.finish()?;
        }
        DistributionArchive::SevenZip => {
            let mut writer = sevenz_rust2::ArchiveWriter::new(&mut bounded)?;
            for path in &directories {
                cancel.check()?;
                let mut entry = sevenz_rust2::ArchiveEntry::new_directory(path.as_str());
                entry.has_windows_attributes = true;
                entry.windows_attributes = (0o040755 << 16) | 0x8010;
                writer.push_archive_entry(entry, None::<io::Empty>)?;
            }
            for (path, file) in expected {
                let mut entry = sevenz_rust2::ArchiveEntry::new_file(path.as_str());
                entry.size = file.bytes;
                entry.has_windows_attributes = true;
                entry.windows_attributes = ((0o100000 | mode(file.permissions)) << 16)
                    | 0x8000
                    | u32::from(file.permissions.readonly);
                writer.push_archive_entry(
                    entry,
                    Some(Cancellable {
                        input: stage.reader(path)?,
                        cancel,
                    }),
                )?;
            }
            writer.finish()?;
        }
    }
    output.flush()?;
    let verified = verify_archive(output, format, expected, limits, cancel)?;
    ensure!(
        verified.directories() == &directories,
        "Archive directory inventory changed"
    );
    Ok(verified)
}

struct BoundedOutput<'a> {
    inner: &'a mut File,
    position: u64,
    maximum: u64,
    cancel: &'a Cancellation,
}
impl Write for BoundedOutput<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.cancel.check().map_err(io::Error::other)?;
        if bytes.is_empty() {
            return Ok(0);
        }
        let count = (self.maximum.saturating_sub(self.position)).min(bytes.len() as u64) as usize;
        if count == 0 {
            return Err(io::Error::other("Encoded archive exceeds byte limit"));
        }
        let written = self.inner.write(&bytes[..count])?;
        self.position += written as u64;
        Ok(written)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}
impl Seek for BoundedOutput<'_> {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.position = self.inner.seek(position)?;
        if self.position > self.maximum {
            return Err(io::Error::other("Archive seek exceeds byte limit"));
        }
        Ok(self.position)
    }
}
fn mode(permissions: FilePermissions) -> u32 {
    let base = if permissions.readonly { 0o444 } else { 0o644 };
    base | if permissions.executable { 0o111 } else { 0 }
}
struct Cancellable<'a, R> {
    input: R,
    cancel: &'a Cancellation,
}
impl<R: Read> Read for Cancellable<'_, R> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.cancel.check().map_err(io::Error::other)?;
        self.input.read(bytes)
    }
}
