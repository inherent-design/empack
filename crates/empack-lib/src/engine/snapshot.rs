//! Bounded native observations. Read sets bind bytes, membership and expected absence.
use super::io::copy_bounded;
use super::native::{self, ObjectIdentity};
use crate::application::process_runtime::Cancellation;
use anyhow::{Context, Result, ensure};
use cap_fs_ext::DirExt;
use cap_std::fs::Dir;
use empack_core::path::{PathSyntax, PortableRelPath};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    path::{Path, PathBuf},
};

/// Limits apply while reading, including files that grow after metadata inspection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotLimits {
    pub entries: usize,
    pub depth: usize,
    pub file_bytes: u64,
    pub total_bytes: u64,
}
impl Default for SnapshotLimits {
    fn default() -> Self {
        Self {
            entries: 100_000,
            depth: 128,
            file_bytes: 8 * 1024 * 1024 * 1024,
            total_bytes: 64 * 1024 * 1024 * 1024,
        }
    }
}

/// An opened project root grants reads only through this adapter.
pub struct ProjectReadRoot {
    pub(super) directory: Dir,
    location: PathBuf,
    pub(super) binding: ObjectIdentity,
}
impl ProjectReadRoot {
    pub(super) fn open_child(parent: &Self, child: &str) -> Result<Self> {
        PortableRelPath::parse(child, PathSyntax::ArtifactName)?;
        parent.check_binding()?;
        let directory = parent.directory.open_dir_nofollow(child)?;
        native::reject_reparse(&directory.try_clone()?.into_std_file())?;
        let binding = native::directory_identity(&directory)?;
        let root = Self {
            directory,
            binding,
            location: parent.location.join(child),
        };
        root.check_binding()?;
        Ok(root)
    }
    /// Ambient authority is used only for the explicitly selected project root.
    pub fn open(selected: &Path) -> Result<Self> {
        let location = selected
            .canonicalize()
            .context("Cannot locate project root")?;
        let directory = Dir::open_ambient_dir(&location, cap_std::ambient_authority())?;
        native::reject_reparse(&directory.try_clone()?.into_std_file())?;
        let binding = native::directory_identity(&directory)?;
        Ok(Self {
            directory,
            location,
            binding,
        })
    }

    pub(super) fn check_binding(&self) -> Result<()> {
        let current = Dir::open_ambient_dir(&self.location, cap_std::ambient_authority())
            .context("Project root disappeared")?;
        ensure!(
            native::directory_identity(&current)? == self.binding,
            "Project root was replaced"
        );
        Ok(())
    }

    /// Observe only selected managed paths. Overlapping scopes are rejected.
    /// The caller must hold project coordination and gate hot journals before capture.
    pub fn capture(
        &self,
        scopes: &[PortableRelPath],
        limits: SnapshotLimits,
        cancel: &Cancellation,
    ) -> Result<NativeSnapshot> {
        self.capture_filtered(scopes, limits, None, cancel)
    }

    pub(super) fn capture_filtered(
        &self,
        scopes: &[PortableRelPath],
        limits: SnapshotLimits,
        filter: Option<&super::source::CaptureFilter>,
        cancel: &Cancellation,
    ) -> Result<NativeSnapshot> {
        self.capture_filtered_omitting(scopes, limits, filter, &BTreeSet::new(), cancel)
    }

    /// Recovery compares project inputs without journal-owned publication scratch.
    /// Omitted paths must come from a validated journal, never caller selection.
    pub(super) fn capture_filtered_omitting(
        &self,
        scopes: &[PortableRelPath],
        limits: SnapshotLimits,
        filter: Option<&super::source::CaptureFilter>,
        omitted: &BTreeSet<String>,
        cancel: &Cancellation,
    ) -> Result<NativeSnapshot> {
        self.check_binding()?;
        let selected: BTreeSet<_> = scopes.iter().cloned().collect();
        ensure!(selected.len() == scopes.len(), "Duplicate snapshot scope");
        for path in &selected {
            for ancestor in ancestors(path.as_str()) {
                ensure!(
                    !selected.iter().any(|other| other.as_str() == ancestor),
                    "Overlapping snapshot scopes"
                );
            }
        }
        let mut capture = Capture {
            entries: BTreeMap::new(),
            total: 0,
            limits,
            cancel,
            omitted,
            filter: filter
                .map(|policy| Ok::<_, anyhow::Error>((policy, policy.matcher()?)))
                .transpose()?,
        };
        for path in scopes {
            cancel.check()?;
            capture.scope(&self.directory, path)?;
        }
        self.check_binding()?;
        Ok(NativeSnapshot {
            root: self.binding,
            groups: vec![CaptureGroup {
                scopes: scopes.to_vec(),
                limits,
                filter: filter.cloned(),
            }],
            entries: capture.entries,
        })
    }

    /// Detect raw byte changes, replacement, membership changes and newly occupied destinations.
    pub fn revalidate(&self, snapshot: &NativeSnapshot, cancel: &Cancellation) -> Result<()> {
        ensure!(
            snapshot.root == self.binding,
            "Snapshot belongs to another project root"
        );
        let mut current = None;
        for group in &snapshot.groups {
            let group =
                self.capture_filtered(&group.scopes, group.limits, group.filter.as_ref(), cancel)?;
            current = Some(match current {
                None => group,
                Some(previous) => NativeSnapshot::merge(previous, group)?,
            });
        }
        let current = current.context("Snapshot has no capture groups")?;
        ensure!(
            current.entries == snapshot.entries,
            "Project inputs changed since preparation"
        );
        Ok(())
    }
}

/// Observed bytes, including raw document bytes. This is data, never write permission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileObservation {
    pub content: [u8; 32],
    pub bytes: u64,
    pub readonly: bool,
    #[cfg(unix)]
    pub mode: u32,
    pub(super) object: ObjectIdentity,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Observation {
    Absent,
    /// An ancestor is bound without reading unrelated siblings.
    Ancestor(DirectoryBinding),
    File(FileObservation),
    Directory {
        members: BTreeSet<String>,
        #[doc(hidden)]
        binding: DirectoryBinding,
    },
}
/// Opaque native directory binding; callers cannot forge a snapshot from it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirectoryBinding(ObjectIdentity);

#[derive(Debug)]
pub(super) struct CaptureGroup {
    pub scopes: Vec<PortableRelPath>,
    pub limits: SnapshotLimits,
    pub filter: Option<super::source::CaptureFilter>,
}

/// Immutable native evidence bound to one retained project root.
#[derive(Debug)]
pub struct NativeSnapshot {
    root: ObjectIdentity,
    groups: Vec<CaptureGroup>,
    entries: BTreeMap<PortableRelPath, Observation>,
}
impl NativeSnapshot {
    /// Persisted comparison data only. A matching hash does not reconstruct read authority.
    pub(super) fn fingerprint(&self) -> [u8; 32] {
        use sha2::{Digest, Sha256};
        fn bytes(hash: &mut Sha256, value: &[u8]) {
            hash.update((value.len() as u64).to_le_bytes());
            hash.update(value);
        }
        fn object(hash: &mut Sha256, value: ObjectIdentity) {
            hash.update(value.volume.to_le_bytes());
            hash.update(value.object.to_le_bytes());
            match value.created {
                Some((seconds, nanos)) => {
                    hash.update([1]);
                    hash.update(seconds.to_le_bytes());
                    hash.update(nanos.to_le_bytes());
                }
                None => hash.update([0]),
            }
        }
        let mut hash = Sha256::new();
        hash.update(b"empack-native-inputs-v1");
        object(&mut hash, self.root);
        hash.update((self.entries.len() as u64).to_le_bytes());
        for (path, observed) in &self.entries {
            bytes(&mut hash, path.as_str().as_bytes());
            match observed {
                Observation::Absent => hash.update([0]),
                Observation::Ancestor(binding) => {
                    hash.update([1]);
                    object(&mut hash, binding.0);
                }
                Observation::File(file) => {
                    hash.update([2]);
                    object(&mut hash, file.object);
                    hash.update(file.content);
                    hash.update(file.bytes.to_le_bytes());
                    hash.update([u8::from(file.readonly)]);
                    #[cfg(unix)]
                    hash.update(file.mode.to_le_bytes());
                }
                Observation::Directory { members, binding } => {
                    hash.update([3]);
                    object(&mut hash, binding.0);
                    hash.update((members.len() as u64).to_le_bytes());
                    for member in members {
                        bytes(&mut hash, member.as_bytes());
                    }
                }
            }
        }
        hash.finalize().into()
    }
    /// Filtered directory membership establishes absence only for included destinations.
    /// A missing ancestor observed directly remains independent evidence of absence.
    pub(super) fn membership_covers(&self, path: &PortableRelPath) -> Result<bool> {
        for group in &self.groups {
            if !group.scopes.iter().any(|scope| {
                path == scope || path.as_str().starts_with(&format!("{}/", scope.as_str()))
            }) {
                continue;
            }
            match &group.filter {
                Some(policy) if !policy.includes(&policy.matcher()?, path, false) => {}
                _ => return Ok(true),
            }
        }
        Ok(false)
    }
    pub(super) fn groups(&self) -> impl Iterator<Item = &CaptureGroup> {
        self.groups.iter()
    }
    /// Preserve each read group's own budget while combining evidence for one publication.
    pub(super) fn merge(mut self, other: Self) -> Result<Self> {
        ensure!(
            self.root == other.root,
            "Cannot combine different project roots"
        );
        for (path, observed) in other.entries {
            if let Some(previous) = self.entries.get(&path) {
                match (previous, &observed) {
                    (a, b) if a == b => continue,
                    (Observation::Directory { binding, .. }, Observation::Ancestor(other))
                        if binding == other =>
                    {
                        continue;
                    }
                    (Observation::Ancestor(previous), Observation::Directory { binding, .. })
                        if previous == binding => {}
                    _ => anyhow::bail!("Input changed between capture groups: {}", path.as_str()),
                }
            }
            self.entries.insert(path, observed);
        }
        self.groups.extend(other.groups);
        Ok(self)
    }
    pub fn entries(&self) -> &BTreeMap<PortableRelPath, Observation> {
        &self.entries
    }
}

struct Capture<'a> {
    entries: BTreeMap<PortableRelPath, Observation>,
    total: u64,
    limits: SnapshotLimits,
    cancel: &'a Cancellation,
    omitted: &'a BTreeSet<String>,
    filter: Option<(
        &'a super::source::CaptureFilter,
        super::source::SourceFilter,
    )>,
}
impl Capture<'_> {
    fn insert(&mut self, path: PortableRelPath, value: Observation) -> Result<()> {
        if let Some(existing) = self.entries.get(&path) {
            ensure!(*existing == value, "Ancestor changed during capture");
            return Ok(());
        }
        ensure!(
            self.entries.len() < self.limits.entries,
            "Snapshot exceeds entry limit"
        );
        self.entries.insert(path, value);
        Ok(())
    }
    fn scope(&mut self, root: &Dir, path: &PortableRelPath) -> Result<()> {
        let mut directory = root.try_clone()?;
        let mut prefix = String::new();
        let mut components = path.components().peekable();
        let mut depth = 0;
        while let Some(component) = components.next() {
            self.cancel.check()?;
            depth += 1;
            ensure!(depth <= self.limits.depth, "Snapshot exceeds depth limit");
            if components.peek().is_none() {
                return self.visit(&directory, component, path.clone(), depth);
            }
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(component);
            let ancestor = PortableRelPath::parse(&prefix, PathSyntax::ProjectContent)?;
            match directory.open_dir_nofollow(component) {
                Ok(next) => {
                    native::reject_reparse(&next.try_clone()?.into_std_file())?;
                    self.insert(
                        ancestor,
                        Observation::Ancestor(DirectoryBinding(native::directory_identity(&next)?)),
                    )?;
                    directory = next;
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    self.insert(ancestor, Observation::Absent)?;
                    return self.insert(path.clone(), Observation::Absent);
                }
                Err(error) => return Err(error.into()),
            }
        }
        unreachable!("portable paths are nonempty")
    }
    fn visit(
        &mut self,
        parent: &Dir,
        leaf: &str,
        path: PortableRelPath,
        depth: usize,
    ) -> Result<()> {
        self.cancel.check()?;
        ensure!(depth <= self.limits.depth, "Snapshot exceeds depth limit");
        if self.omitted.contains(path.as_str()) {
            return Ok(());
        }
        let metadata = match parent.symlink_metadata(leaf) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return self.insert(path, Observation::Absent);
            }
            Err(error) => return Err(error.into()),
        };
        if self
            .filter
            .as_ref()
            .is_some_and(|(policy, matcher)| !policy.includes(matcher, &path, metadata.is_dir()))
        {
            return Ok(());
        }
        ensure!(
            !metadata.file_type().is_symlink(),
            "Managed input is a link: {}",
            path.as_str()
        );
        if metadata.is_dir() {
            let directory = parent.open_dir_nofollow(leaf)?;
            native::reject_reparse(&directory.try_clone()?.into_std_file())?;
            let binding = DirectoryBinding(native::directory_identity(&directory)?);
            let mut members = BTreeSet::new();
            let mut seen = 0;
            for entry in directory.entries()? {
                self.cancel.check()?;
                let name = entry?.file_name();
                if name.to_str().is_some_and(|name| {
                    self.omitted
                        .contains(&format!("{}/{}", path.as_str(), name))
                }) {
                    continue;
                }
                ensure!(
                    seen < self.limits.entries.saturating_sub(self.entries.len()),
                    "Snapshot exceeds entry limit"
                );
                seen += 1;
                if let Some((policy, matcher)) = &self.filter {
                    let metadata = directory.symlink_metadata(&name)?;
                    let child = std::path::Path::new(path.as_str()).join(&name);
                    if !policy.includes_native(matcher, &child, metadata.is_dir()) {
                        continue;
                    }
                }
                let name = name
                    .into_string()
                    .map_err(|_| anyhow::anyhow!("Nonportable managed filename"))?;
                PortableRelPath::parse(&name, PathSyntax::ArtifactName)?;
                PortableRelPath::parse(
                    &format!("{}/{}", path.as_str(), name),
                    PathSyntax::ProjectContent,
                )?;
                members.insert(name);
            }
            self.insert(
                path.clone(),
                Observation::Directory {
                    members: members.clone(),
                    binding,
                },
            )?;
            for name in members {
                let child = PortableRelPath::parse(
                    &format!("{}/{}", path.as_str(), name),
                    PathSyntax::ProjectContent,
                )?;
                self.visit(&directory, &name, child, depth + 1)?;
            }
        } else {
            ensure!(
                metadata.is_file(),
                "Managed input is not a regular file: {}",
                path.as_str()
            );
            let mut file = native::open_file(parent, leaf)?;
            let maximum = self
                .limits
                .file_bytes
                .min(self.limits.total_bytes.saturating_sub(self.total));
            let observation = observe_file(&mut file, maximum, self.cancel)?;
            self.total += observation.bytes;
            self.insert(path, Observation::File(observation))?;
        }
        Ok(())
    }
}

pub(super) fn observe_file(
    file: &mut File,
    maximum: u64,
    cancel: &Cancellation,
) -> Result<FileObservation> {
    let metadata = file.metadata()?;
    ensure!(metadata.is_file(), "Managed input is not a regular file");
    ensure!(metadata.len() <= maximum, "Snapshot exceeds byte limit");
    let object = native::identity(file)?;
    let (content, bytes) = copy_bounded(file, &mut std::io::sink(), maximum, cancel)?;
    let after = file.metadata()?;
    ensure!(
        after.len() == metadata.len()
            && bytes == metadata.len()
            && after.modified()? == metadata.modified()?,
        "File changed while capturing snapshot"
    );
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    Ok(FileObservation {
        content,
        bytes,
        readonly: metadata.permissions().readonly(),
        #[cfg(unix)]
        mode: metadata.permissions().mode(),
        object,
    })
}

fn ancestors(path: &str) -> impl Iterator<Item = &str> {
    path.match_indices('/').map(|(index, _)| &path[..index])
}

#[cfg(test)]
mod tests;
