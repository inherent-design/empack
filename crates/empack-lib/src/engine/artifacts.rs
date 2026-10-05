//! Independent byte-inventory verification for candidate distribution containers.
use super::{io::copy_bounded, layout::CollisionIndex};
use crate::application::process_runtime::Cancellation;
use anyhow::{Result, ensure};
use empack_core::{
    digest::ContentId,
    files::{FileContent, FilePermissions},
    model::DistributionArchive,
    path::{PathSyntax, PortableRelPath},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{self, Read, Seek, SeekFrom},
};

/// Parsing budgets include actual decoded bytes, not only archive declarations.
#[derive(Debug, Clone, Copy)]
pub struct ArchiveLimits {
    pub compressed_bytes: u64,
    pub file_bytes: u64,
    pub total_bytes: u64,
    pub entries: usize,
    pub depth: usize,
}
impl Default for ArchiveLimits {
    fn default() -> Self {
        Self {
            compressed_bytes: 64 * 1024 * 1024 * 1024,
            file_bytes: 8 * 1024 * 1024 * 1024,
            total_bytes: 64 * 1024 * 1024 * 1024,
            entries: 100_000,
            depth: 128,
        }
    }
}
pub trait ArchiveRead: Read + Seek {}
impl<T: Read + Seek> ArchiveRead for T {}
/// Container bytes and exact file inventory checked. Empty directories are allowed.
/// This is not pack semantic or redistribution proof.
#[derive(Debug)]
pub struct VerifiedArchive {
    content: ContentId,
    bytes: u64,
    members: usize,
    unpacked_bytes: u64,
    directories: BTreeSet<PortableRelPath>,
}
impl VerifiedArchive {
    pub fn content(&self) -> &ContentId {
        &self.content
    }
    pub fn len(&self) -> u64 {
        self.bytes
    }
    pub fn is_empty(&self) -> bool {
        self.bytes == 0
    }
    pub fn members(&self) -> usize {
        self.members
    }
    pub fn unpacked_bytes(&self) -> u64 {
        self.unpacked_bytes
    }
    pub fn directories(&self) -> &BTreeSet<PortableRelPath> {
        &self.directories
    }
}
struct Check<'a> {
    expected: &'a BTreeMap<PortableRelPath, FileContent>,
    observed: BTreeMap<PortableRelPath, FileContent>,
    collisions: CollisionIndex,
    directories: BTreeSet<PortableRelPath>,
    limits: ArchiveLimits,
    entries: usize,
    total: u64,
    cancel: &'a Cancellation,
}
impl Check<'_> {
    fn entry(
        &mut self,
        name: &str,
        directory: bool,
        declared: u64,
        permissions: FilePermissions,
        source: &mut dyn Read,
    ) -> Result<()> {
        self.cancel.check()?;
        self.entries = self
            .entries
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("Archive entry count overflow"))?;
        ensure!(
            self.entries <= self.limits.entries,
            "Archive exceeds entry limit"
        );
        let name = if directory {
            name.strip_suffix('/').unwrap_or(name)
        } else {
            name
        };
        if directory && matches!(name, "." | "./") {
            ensure!(declared == 0, "Root directory entry contains data");
            return Ok(());
        }
        // TAR commonly spells members ./name. Resolve only leading current-directory syntax.
        let mut name = name;
        while let Some(relative) = name.strip_prefix("./") {
            name = relative;
        }
        let path = PortableRelPath::parse(name, PathSyntax::ArchiveMember)?;
        ensure!(
            path.components().count() <= self.limits.depth,
            "Archive entry exceeds depth limit"
        );
        if directory {
            ensure!(declared == 0, "Directory entry contains data");
            self.collisions.insert_directory(&path)?;
            ensure!(self.directories.insert(path), "Duplicate archive directory");
            return Ok(());
        }
        self.collisions.insert_file(&path)?;
        let expected = self
            .expected
            .get(&path)
            .ok_or_else(|| anyhow::anyhow!("Unexpected archive member: {}", path.as_str()))?;
        ensure!(
            declared == expected.bytes && declared <= self.limits.file_bytes,
            "Archive member size differs from expected content: {}",
            path.as_str()
        );
        let remaining = self
            .limits
            .total_bytes
            .checked_sub(self.total)
            .ok_or_else(|| anyhow::anyhow!("Archive exceeds decoded byte limit"))?;
        let (hash, bytes) = copy_bounded(
            source,
            &mut io::sink(),
            declared.min(remaining),
            self.cancel,
        )?;
        ensure!(
            bytes == declared && hash == *expected.content.bytes(),
            "Archive member bytes differ from expected content: {}",
            path.as_str()
        );
        ensure!(
            permissions == expected.permissions,
            "Archive member permissions differ from output intent: {}",
            path.as_str()
        );
        self.total += bytes;
        self.observed.insert(
            path,
            FileContent {
                content: ContentId::from_sha256(hash),
                bytes,
                permissions,
            },
        );
        Ok(())
    }
}
fn modes(mode: u32) -> Result<FilePermissions> {
    ensure!(
        mode & 0o170000 == 0 || mode & 0o170000 == 0o100000,
        "Archive contains a non-regular file"
    );
    Ok(FilePermissions {
        readonly: mode & 0o222 == 0,
        executable: mode & 0o100 != 0,
    })
}
/// Expected inventory comes from planning. A valid CRC or successful exporter is not completeness.
/// Call in an admitted blocking task. These are member/output budgets, not a process memory ceiling:
/// the 7z dependency does not expose a decoder allocation limit. Untrusted acquisition needs its
/// separate input policy; the packaging adapter verifies only its privately generated candidates.
pub fn verify_archive(
    source: &mut dyn ArchiveRead,
    format: DistributionArchive,
    expected: &BTreeMap<PortableRelPath, FileContent>,
    limits: ArchiveLimits,
    cancel: &Cancellation,
) -> Result<VerifiedArchive> {
    cancel.check()?;
    let size = source.seek(SeekFrom::End(0))?;
    ensure!(
        size <= limits.compressed_bytes,
        "Archive exceeds compressed byte limit"
    );
    source.rewind()?;
    let (original_hash, original_size) =
        copy_bounded(source, &mut io::sink(), limits.compressed_bytes, cancel)?;
    ensure!(original_size == size, "Archive changed before inspection");
    source.rewind()?;
    let mut collisions = CollisionIndex::default();
    for path in expected.keys() {
        collisions.insert_file(path)?;
    }
    let mut check = Check {
        expected,
        observed: BTreeMap::new(),
        collisions: CollisionIndex::default(),
        directories: BTreeSet::new(),
        limits,
        entries: 0,
        total: 0,
        cancel,
    };
    match format {
        DistributionArchive::Zip => {
            preflight_zip(source, size, limits, cancel)?;
            source.rewind()?;
            let mut archive = zip::ZipArchive::new(&mut *source)?;
            ensure!(
                archive.len() <= limits.entries,
                "Archive exceeds entry limit"
            );
            for index in 0..archive.len() {
                let mut entry = archive.by_index(index)?;
                ensure!(
                    !entry.encrypted() && !entry.is_symlink(),
                    "Encrypted or linked archive member is unsupported"
                );
                let name = entry.name().to_owned();
                let directory = entry.is_dir();
                let permissions = if directory {
                    FilePermissions {
                        readonly: false,
                        executable: false,
                    }
                } else {
                    modes(entry.unix_mode().unwrap_or(0o644))?
                };
                check.entry(&name, directory, entry.size(), permissions, &mut entry)?;
            }
        }
        DistributionArchive::TarGz => {
            let decoder = flate2::read::GzDecoder::new(&mut *source);
            let mut archive = tar::Archive::new(decoder);
            for entry in archive.entries()? {
                let mut entry = entry?;
                let directory = entry.header().entry_type().is_dir();
                ensure!(
                    directory || entry.header().entry_type().is_file(),
                    "Archive contains a link or special member"
                );
                let name = std::str::from_utf8(&entry.path_bytes())?.to_owned();
                let permissions = if directory {
                    FilePermissions {
                        readonly: false,
                        executable: false,
                    }
                } else {
                    modes(entry.header().mode()?)?
                };
                check.entry(&name, directory, entry.size(), permissions, &mut entry)?;
            }
            // TAR stops at its zero blocks before the gzip trailer. Drain the decoder so
            // a truncated stream or incorrect gzip CRC cannot receive a verified receipt.
            copy_bounded(
                &mut archive.into_inner(),
                &mut io::sink(),
                limits.total_bytes,
                cancel,
            )?;
        }
        DistributionArchive::SevenZip => {
            let mut archive =
                sevenz_rust2::ArchiveReader::new(&mut *source, sevenz_rust2::Password::empty())?;
            archive.set_thread_count(1);
            archive.for_each_entries(|entry, bytes| {
                let result = (|| {
                    ensure!(
                        !entry.is_anti_item && entry.windows_attributes & 0x400 == 0,
                        "Archive contains a removal or reparse entry"
                    );
                    let unix =
                        entry.has_windows_attributes && entry.windows_attributes & 0x8000 != 0;
                    if unix && entry.is_directory {
                        let kind = (entry.windows_attributes >> 16) & 0o170000;
                        ensure!(kind == 0 || kind == 0o040000, "Invalid directory file kind");
                    }
                    let permissions = if unix && !entry.is_directory {
                        modes(entry.windows_attributes >> 16)?
                    } else {
                        FilePermissions {
                            readonly: entry.has_windows_attributes
                                && entry.windows_attributes & 1 != 0,
                            executable: false,
                        }
                    };
                    check.entry(
                        &entry.name,
                        entry.is_directory,
                        entry.size,
                        permissions,
                        bytes,
                    )
                })();
                result
                    .map(|()| true)
                    .map_err(|error| sevenz_rust2::Error::from(io::Error::other(error)))
            })?;
        }
    }
    ensure!(
        check.observed.len() == expected.len(),
        "Archive omits expected members"
    );
    source.rewind()?;
    let (hash, bytes) = copy_bounded(source, &mut io::sink(), limits.compressed_bytes, cancel)?;
    ensure!(
        bytes == size && hash == original_hash,
        "Archive changed during verification"
    );
    Ok(VerifiedArchive {
        content: ContentId::from_sha256(hash),
        bytes,
        members: check.observed.len(),
        unpacked_bytes: check.total,
        directories: check.directories,
    })
}
// zip's name index collapses duplicate central-directory names. Inspect bounded raw records first.
fn preflight_zip(
    source: &mut dyn ArchiveRead,
    size: u64,
    limits: ArchiveLimits,
    cancel: &Cancellation,
) -> Result<()> {
    let count = size.min(65535 + 22) as usize;
    source.seek(SeekFrom::End(-(count as i64)))?;
    let mut tail = vec![0; count];
    source.read_exact(&mut tail)?;
    let eocd = (0..count.saturating_sub(21))
        .rev()
        .find(|&offset| {
            tail[offset..].starts_with(b"PK\x05\x06")
                && offset
                    + 22
                    + usize::from(u16::from_le_bytes([tail[offset + 20], tail[offset + 21]]))
                    == count
        })
        .ok_or_else(|| anyhow::anyhow!("Missing ZIP end record"))?;
    let u16_at = |offset: usize| u16::from_le_bytes([tail[eocd + offset], tail[eocd + offset + 1]]);
    let u32_at = |offset: usize| {
        u32::from_le_bytes(tail[eocd + offset..eocd + offset + 4].try_into().unwrap())
    };
    ensure!(
        u16_at(4) == 0 && u16_at(6) == 0,
        "Multi-disk ZIP is unsupported"
    );
    let mut entries = u64::from(u16_at(10));
    let mut central_size = u64::from(u32_at(12));
    let mut central = u64::from(u32_at(16));
    let end_position = size - count as u64 + eocd as u64;
    if entries == u64::from(u16::MAX)
        || central_size == u64::from(u32::MAX)
        || central == u64::from(u32::MAX)
    {
        ensure!(end_position >= 20, "Invalid ZIP64 locator");
        source.seek(SeekFrom::Start(end_position - 20))?;
        let mut locator = [0u8; 20];
        source.read_exact(&mut locator)?;
        ensure!(
            locator[..4] == *b"PK\x06\x07"
                && locator[4..8] == [0; 4]
                && u32::from_le_bytes(locator[16..20].try_into().unwrap()) == 1,
            "Invalid ZIP64 locator"
        );
        source.seek(SeekFrom::Start(u64::from_le_bytes(
            locator[8..16].try_into().unwrap(),
        )))?;
        let mut record = [0u8; 56];
        source.read_exact(&mut record)?;
        ensure!(
            record[..4] == *b"PK\x06\x06" && record[16..24] == [0; 8],
            "Invalid ZIP64 end record"
        );
        entries = u64::from_le_bytes(record[32..40].try_into().unwrap());
        central_size = u64::from_le_bytes(record[40..48].try_into().unwrap());
        central = u64::from_le_bytes(record[48..56].try_into().unwrap());
    }
    ensure!(
        entries <= limits.entries as u64,
        "Archive exceeds entry limit"
    );
    // Bound parser metadata before ZipArchive allocates its name index.
    ensure!(
        central_size <= 16 * 1024 * 1024,
        "ZIP metadata exceeds 16 MiB limit"
    );
    let central_end = central
        .checked_add(central_size)
        .ok_or_else(|| anyhow::anyhow!("ZIP directory overflow"))?;
    ensure!(
        central_end <= end_position,
        "ZIP directory extends beyond its end record"
    );
    source.seek(SeekFrom::Start(central))?;
    let mut names = std::collections::BTreeSet::new();
    for _ in 0..entries {
        cancel.check()?;
        let mut header = [0u8; 46];
        source.read_exact(&mut header)?;
        ensure!(
            header[..4] == *b"PK\x01\x02",
            "Invalid ZIP central directory member"
        );
        let length = usize::from(u16::from_le_bytes(header[28..30].try_into().unwrap()));
        let extra = u16::from_le_bytes(header[30..32].try_into().unwrap());
        let comment = u16::from_le_bytes(header[32..34].try_into().unwrap());
        let mut name = vec![0; length];
        source.read_exact(&mut name)?;
        ensure!(names.insert(name), "Duplicate ZIP central directory member");
        source.seek(SeekFrom::Current(i64::from(extra) + i64::from(comment)))?;
        ensure!(
            source.stream_position()? <= central_end,
            "ZIP member exceeds directory bounds"
        );
    }
    ensure!(
        source.stream_position()? == central_end,
        "ZIP directory count differs from its records"
    );
    Ok(())
}

#[cfg(test)]
mod tests;

mod writer;
pub use writer::{EmptyArchiveSource, package_directory, write_archive};
