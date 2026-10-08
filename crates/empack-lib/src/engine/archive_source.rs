//! A retained, bounded ZIP reader for declared embedded content. No filesystem destinations exist here.
use super::{
    artifacts::{ArchiveLimits, modes, preflight_zip},
    content::{
        AcquiredContent, ContentReader, InitialObservation, SourceEvidencePolicy, verify_stream,
    },
    layout::CollisionIndex,
};
use crate::application::process_runtime::Cancellation;
use anyhow::{Context, Result, ensure};
use empack_core::{
    files::FilePermissions,
    model::ExpectedContent,
    path::{PathSyntax, PortableRelPath},
};
use std::{
    collections::BTreeMap,
    io::{self, Read, Seek},
};

#[derive(Clone, Copy)]
pub struct ArchiveMember {
    pub bytes: u64,
    pub permissions: FilePermissions,
    index: usize,
}
/// Owns the source lease through its reader; opening once amortizes metadata checks and I/O.
pub struct ZipContentSource {
    archive: zip::ZipArchive<ContentReader>,
    files: BTreeMap<PortableRelPath, ArchiveMember>,
    limits: ArchiveLimits,
    extracted: u64,
}
impl ZipContentSource {
    /// Raw directory checks precede the ZIP library's allocation/name index. All metadata is
    /// checked, even for members the caller will not extract. Run in an admitted blocking worker.
    pub fn open(
        content: &AcquiredContent,
        limits: ArchiveLimits,
        cancel: &Cancellation,
    ) -> Result<Self> {
        ensure!(
            content.lease().len() <= limits.compressed_bytes,
            "Source archive exceeds compressed byte limit"
        );
        let mut reader = content.lease().open();
        preflight_zip(&mut reader, content.lease().len(), limits, cancel)?;
        reader.rewind()?;
        let mut archive = zip::ZipArchive::new(reader)?;
        ensure!(
            archive.len() <= limits.entries,
            "Source archive exceeds entry limit"
        );
        let mut files = BTreeMap::new();
        let mut collisions = CollisionIndex::default();
        let mut total = 0u64;
        for index in 0..archive.len() {
            cancel.check()?;
            let entry = archive.by_index(index)?;
            ensure!(
                !entry.encrypted() && !entry.is_symlink(),
                "Encrypted or linked source member is unsupported"
            );
            let name = if entry.is_dir() {
                entry
                    .name()
                    .strip_suffix('/')
                    .context("Nonportable source directory name")?
            } else {
                entry.name()
            };
            let path = PortableRelPath::parse(name, PathSyntax::ArchiveMember)?;
            ensure!(
                path.components().count() <= limits.depth,
                "Source archive member exceeds depth limit"
            );
            if entry.is_dir() {
                ensure!(entry.size() == 0, "Source archive directory contains bytes");
                ensure!(
                    entry
                        .unix_mode()
                        .is_none_or(|mode| matches!(mode & 0o170000, 0 | 0o040000)),
                    "Source directory has another file kind"
                );
                collisions.insert_directory(&path)?;
                continue;
            }
            collisions.insert_file(&path)?;
            let permissions = modes(entry.unix_mode().unwrap_or(0o644))?;
            total = total
                .checked_add(entry.size())
                .context("Source archive expanded-size overflow")?;
            ensure!(
                entry.size() <= limits.file_bytes && total <= limits.total_bytes,
                "Source archive exceeds expanded byte limit"
            );
            files.insert(
                path,
                ArchiveMember {
                    index,
                    bytes: entry.size(),
                    permissions,
                },
            );
        }
        Ok(Self {
            archive,
            files,
            limits,
            extracted: 0,
        })
    }
    pub fn files(&self) -> impl Iterator<Item = (&PortableRelPath, &ArchiveMember)> {
        self.files.iter()
    }
    /// Read every member through the decoder and CRC verifier without extracting to disk.
    /// These reads share the extraction allowance; an error cannot reset consumed work.
    pub fn verify_members(&mut self, cancel: &Cancellation) -> Result<()> {
        for member in self.files.values() {
            cancel.check()?;
            ensure!(
                member.bytes <= self.limits.total_bytes.saturating_sub(self.extracted),
                "Source archive verification allowance exhausted"
            );
            let mut entry = self.archive.by_index(member.index)?;
            let mut input = CountedRead {
                input: &mut entry,
                count: 0,
            };
            let result = super::io::copy_bounded(&mut input, &mut io::sink(), member.bytes, cancel);
            let charged = if result.is_err() {
                input.count.max(member.bytes)
            } else {
                input.count
            };
            self.extracted = self
                .extracted
                .checked_add(charged)
                .context("Source archive verified-size overflow")?;
            ensure!(
                self.extracted <= self.limits.total_bytes,
                "Source archive exceeds actual verification allowance"
            );
            let (_, bytes) = result?;
            ensure!(
                bytes == member.bytes,
                "Source member size differs from its directory record"
            );
        }
        Ok(())
    }
    /// Actual reads, including failed attempts, consume the extraction allowance. A member is
    /// usable only after CRC, all source declarations and size checks pass in private storage.
    pub fn acquire(
        &mut self,
        member: &PortableRelPath,
        expected: &ExpectedContent,
        policy: SourceEvidencePolicy,
        initial: InitialObservation,
        cancel: &Cancellation,
    ) -> Result<(AcquiredContent, FilePermissions)> {
        let observed = self
            .files
            .get(member)
            .context("Declared archive member is missing or not a file")?;
        let maximum = self
            .limits
            .file_bytes
            .min(self.limits.total_bytes.saturating_sub(self.extracted));
        ensure!(
            observed.bytes <= maximum,
            "Source archive extraction allowance exhausted"
        );
        ensure!(
            expected.size.is_none_or(|size| size == observed.bytes),
            "Source member size differs from declaration"
        );
        let maximum = observed.bytes;
        let mut entry = self.archive.by_index(observed.index)?;
        let mut input = CountedRead {
            input: &mut entry,
            count: 0,
        };
        let result = verify_stream(
            &mut input,
            &ExpectedContent {
                size: Some(observed.bytes),
                ..expected.clone()
            },
            maximum,
            policy,
            initial,
            cancel,
        );
        // A decoder can return an error after writing into a buffer without reporting a count.
        // Charge the declared member size on failure rather than allowing repeated free attempts.
        let charged = if result.is_err() {
            input.count.max(observed.bytes)
        } else {
            input.count
        };
        self.extracted = self
            .extracted
            .checked_add(charged)
            .context("Source archive extracted-size overflow")?;
        ensure!(
            self.extracted <= self.limits.total_bytes,
            "Source archive exceeds actual extraction allowance"
        );
        Ok((result?, observed.permissions))
    }
}
struct CountedRead<'a> {
    input: &'a mut dyn Read,
    count: u64,
}
impl Read for CountedRead<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let count = self.input.read(buffer)?;
        self.count = self
            .count
            .checked_add(count as u64)
            .ok_or_else(|| io::Error::other("Source archive byte count overflow"))?;
        Ok(count)
    }
}
#[cfg(test)]
mod tests;
