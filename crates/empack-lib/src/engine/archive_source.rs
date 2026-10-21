//! A retained, bounded ZIP reader for declared embedded content. No filesystem destinations exist here.
use super::{
    artifacts::{ArchiveLimits, ArchiveRead, modes, preflight_zip},
    content::{AcquiredContent, InitialObservation, SourceEvidencePolicy, verify_stream},
    layout::CollisionIndex,
};
use crate::application::process_runtime::Cancellation;
use anyhow::{Context, Result, ensure};
use empack_core::{
    files::FilePermissions,
    model::ExpectedContent,
    path::{ArchiveMemberName, PathSyntax, PortableRelPath},
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
/// Retains the source reader; opening once amortizes metadata checks and I/O.
pub struct ZipContentSource {
    archive: zip::ZipArchive<Box<dyn ArchiveRead + Send>>,
    files: BTreeMap<PortableRelPath, ArchiveMember>,
    limits: ArchiveLimits,
    extracted: u64,
    evidence: Option<super::content::ArchiveEvidence>,
}
impl ZipContentSource {
    /// Raw directory checks precede the ZIP library's allocation/name index. All metadata is
    /// checked, even for members the caller will not extract. Run in an admitted blocking worker.
    pub fn open(
        content: &AcquiredContent,
        limits: ArchiveLimits,
        cancel: &Cancellation,
    ) -> Result<Self> {
        let mut source = Self::from_reader(
            Box::new(content.lease().open()),
            content.lease().len(),
            limits,
            cancel,
        )?;
        source.evidence = Some(super::content::ArchiveEvidence::captured(content));
        Ok(source)
    }
    /// Verify an opaque ZIP/JAR without granting member extraction authority.
    /// Its case-sensitive namespace is independent of native destination rules.
    pub(in crate::engine) fn inspect_opaque(
        content: &AcquiredContent,
        limits: ArchiveLimits,
        cancel: &Cancellation,
    ) -> Result<Vec<ArchiveMemberName>> {
        let mut collisions = CollisionIndex::exact_members();
        let mut indexed = index_zip(
            Box::new(content.lease().open()),
            content.lease().len(),
            limits,
            cancel,
            |name, directory| {
                let name = ArchiveMemberName::parse(name)?;
                collisions.insert_member(&name, directory)?;
                Ok(name)
            },
        )?;
        verify_members(
            &mut indexed.archive,
            indexed.files.values(),
            limits,
            &mut 0,
            cancel,
        )?;
        Ok(indexed.files.into_keys().collect())
    }
    /// Read a captured native archive without duplicating its compressed bytes in scratch.
    /// The owning workflow must retain and revalidate the snapshot before publishing members.
    pub(in crate::engine) fn open_captured(
        workspace: &super::project::WorkspaceSnapshot,
        path: &PortableRelPath,
        limits: ArchiveLimits,
        cancel: &Cancellation,
    ) -> Result<Self> {
        let (file, expected, _) = workspace.open_observed_input(path, None)?;
        Self::from_reader(
            Box::new(file),
            expected.size.expect("captured size"),
            limits,
            cancel,
        )
    }
    fn from_reader(
        reader: Box<dyn ArchiveRead + Send>,
        bytes: u64,
        limits: ArchiveLimits,
        cancel: &Cancellation,
    ) -> Result<Self> {
        let mut collisions = CollisionIndex::default();
        let indexed = index_zip(reader, bytes, limits, cancel, |name, directory| {
            let path = PortableRelPath::parse(name, PathSyntax::ArchiveMember)?;
            if directory {
                collisions.insert_directory(&path)?;
            } else {
                collisions.insert_file(&path)?;
            }
            Ok(path)
        })?;
        Ok(Self {
            archive: indexed.archive,
            files: indexed.files,
            limits,
            extracted: 0,
            evidence: None,
        })
    }
    pub(in crate::engine) fn member(&self, path: &PortableRelPath) -> Option<&ArchiveMember> {
        self.files.get(path)
    }
    pub fn files(&self) -> impl Iterator<Item = (&PortableRelPath, &ArchiveMember)> {
        self.files.iter()
    }
    /// Read every member through the decoder and CRC verifier without extracting to disk.
    /// These reads share the extraction allowance; an error cannot reset consumed work.
    pub fn verify_members(&mut self, cancel: &Cancellation) -> Result<()> {
        verify_members(
            &mut self.archive,
            self.files.values(),
            self.limits,
            &mut self.extracted,
            cancel,
        )
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
        let policy = self
            .evidence
            .as_ref()
            .map(|evidence| evidence.member_policy(expected, policy))
            .transpose()?
            .unwrap_or(policy);
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
        let content = result?;
        let content = match &self.evidence {
            Some(evidence) => evidence.bind(content, member.clone()),
            None => content,
        };
        Ok((content, observed.permissions))
    }
}
struct IndexedArchive<N> {
    archive: zip::ZipArchive<Box<dyn ArchiveRead + Send>>,
    files: BTreeMap<N, ArchiveMember>,
}

fn index_zip<N: Ord>(
    mut reader: Box<dyn ArchiveRead + Send>,
    bytes: u64,
    limits: ArchiveLimits,
    cancel: &Cancellation,
    mut member_name: impl FnMut(&str, bool) -> Result<N>,
) -> Result<IndexedArchive<N>> {
    ensure!(
        bytes <= limits.compressed_bytes,
        "Source archive exceeds compressed byte limit"
    );
    preflight_zip(&mut reader, bytes, limits, cancel)?;
    reader.rewind()?;
    let mut archive = zip::ZipArchive::new(reader)?;
    ensure!(
        archive.len() <= limits.entries,
        "Source archive exceeds entry limit"
    );
    let mut files = BTreeMap::new();
    let mut total = 0u64;
    for index in 0..archive.len() {
        cancel.check()?;
        let entry = archive.by_index(index)?;
        ensure!(
            !entry.encrypted() && !entry.is_symlink(),
            "Encrypted or linked source member is unsupported"
        );
        let decoded_name = entry.name().context("Invalid source archive member name")?;
        let name = if entry.is_dir() {
            decoded_name
                .strip_suffix('/')
                .context("Nonportable source directory name")?
        } else {
            decoded_name.as_ref()
        };
        ensure!(
            name.split('/').count() <= limits.depth,
            "Source archive member exceeds depth limit"
        );
        let path = member_name(name, entry.is_dir())?;
        if entry.is_dir() {
            ensure!(entry.size() == 0, "Source archive directory contains bytes");
            ensure!(
                entry
                    .unix_mode()
                    .is_none_or(|mode| matches!(mode & 0o170000, 0 | 0o040000)),
                "Source directory has another file kind"
            );
            continue;
        }
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
    Ok(IndexedArchive { archive, files })
}

fn verify_members<'a>(
    archive: &mut zip::ZipArchive<Box<dyn ArchiveRead + Send>>,
    members: impl Iterator<Item = &'a ArchiveMember>,
    limits: ArchiveLimits,
    extracted: &mut u64,
    cancel: &Cancellation,
) -> Result<()> {
    for member in members {
        cancel.check()?;
        ensure!(
            member.bytes <= limits.total_bytes.saturating_sub(*extracted),
            "Source archive verification allowance exhausted"
        );
        let mut entry = archive.by_index(member.index)?;
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
        *extracted = extracted
            .checked_add(charged)
            .context("Source archive verified-size overflow")?;
        ensure!(
            *extracted <= limits.total_bytes,
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
