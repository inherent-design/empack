//! Discover metadata without granting opaque or unrelated records mutation authority.
use super::*;
use crate::engine::{backend::BackendFile, layout::ProjectLayout, source::CaptureFilter};
use cap_fs_ext::DirExt;
use empack_core::{files::ManagedPath, model::ContentLayer};
use std::collections::BTreeSet;

impl ProjectReader {
    /// Include exact recorded source files without broadening mutation ownership.
    pub fn capture_recorded_synchronization(
        &self,
        selected: &Path,
        proposed: Option<&ResolvedProject>,
        limits: SnapshotLimits,
        cancel: &Cancellation,
    ) -> Result<MutationSnapshot> {
        let captured =
            self.capture_synchronization_with_resolution(selected, proposed, limits, cancel)?;
        let mut workspace = captured.into_workspace();
        let candidate = crate::engine::synchronization::SynchronizationCandidate::prepare_input(
            workspace.intent(),
            workspace.prior_lock(),
            proposed,
        )?;
        let sources = candidate
            .project()
            .lock()
            .dependencies
            .values()
            .flat_map(|dependency| dependency.files.as_slice())
            .filter_map(|file| match &file.acquisition {
                AcquisitionSpec::Local(path) | AcquisitionSpec::Embedded { archive: path, .. } => {
                    Some(path.clone())
                }
                _ => None,
            })
            .collect::<BTreeSet<_>>();
        let _guard = self.recovery.enter(&workspace.root)?;
        let sources = sources.into_iter().collect::<Vec<_>>();
        for path in &sources {
            cancel.check()?;
            // A recorded source is a file, never authority to recursively inspect a directory.
            match native::parent(&workspace.root.directory, path)
                .and_then(|(parent, leaf)| native::open_file(&parent, &leaf))
            {
                Ok(file) => ensure!(file.metadata()?.is_file(), "Recorded source is not a file"),
                Err(error)
                    if error
                        .downcast_ref::<std::io::Error>()
                        .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) => {}
                Err(error) => return Err(error),
            }
        }
        if !sources.is_empty() {
            let next = workspace.root.capture(&sources, limits, cancel)?;
            workspace.native = workspace.native.merge(next)?;
        }
        ensure!(
            workspace.native.entries().len() <= limits.entries,
            "Recorded synchronization exceeds entry limit"
        );
        let bytes = workspace
            .native
            .entries()
            .values()
            .try_fold(0u64, |bytes, entry| {
                bytes
                    .checked_add(match entry {
                        Observation::File(file) => file.bytes,
                        _ => 0,
                    })
                    .context("Recorded synchronization size overflow")
            })?;
        ensure!(
            bytes <= limits.total_bytes,
            "Recorded synchronization exceeds byte limit"
        );
        workspace.root.revalidate(&workspace.native, cancel)?;
        Ok(MutationSnapshot { workspace })
    }

    /// Bind recorded placements and interpretable metadata for those exact destinations.
    pub fn capture_synchronization(
        &self,
        selected: &Path,
        limits: SnapshotLimits,
        cancel: &Cancellation,
    ) -> Result<MutationSnapshot> {
        self.capture_synchronization_with_resolution(selected, None, limits, cancel)
    }
    /// Fresh resolution extends capture to prior and next placements; it grants no write authority.
    pub fn capture_synchronization_with_resolution(
        &self,
        selected: &Path,
        proposed: Option<&ResolvedProject>,
        limits: SnapshotLimits,
        cancel: &Cancellation,
    ) -> Result<MutationSnapshot> {
        let pack = PortableRelPath::parse("pack", PathSyntax::ProjectContent)?;
        let controls = CaptureFilter::selected_mutation(&[])?;
        let documents =
            self.capture_selected(selected, &[pack], limits, Some(&controls), cancel)?;
        let candidate = crate::engine::synchronization::SynchronizationCandidate::prepare_input(
            documents.intent(),
            documents.prior_lock(),
            proposed,
        )?;
        let mut required = Vec::new();
        for dependency in candidate.project().lock().dependencies.values().chain(
            documents
                .prior_lock()
                .into_iter()
                .flat_map(|prior| prior.lock().dependencies.values()),
        ) {
            for file in dependency.files.as_slice() {
                for placement in file.placements.as_slice() {
                    required.push(ProjectLayout::path(&ManagedPath::Content {
                        layer: placement.layer,
                        path: placement.destination.relative().clone(),
                    })?);
                }
            }
        }
        self.capture_dependency_paths(selected, documents, required, limits, cancel)
    }
    /// Share exact destination discovery across restoration, addition and explicit updates.
    pub(super) fn capture_dependency_paths(
        &self,
        selected: &Path,
        mut documents: WorkspaceSnapshot,
        mut required: Vec<PortableRelPath>,
        limits: SnapshotLimits,
        cancel: &Cancellation,
    ) -> Result<MutationSnapshot> {
        let destinations = required
            .iter()
            .filter_map(|path| match ProjectLayout::classify(path) {
                Ok(ManagedPath::Content {
                    layer: ContentLayer::Common,
                    path,
                }) => Some(path),
                _ => None,
            })
            .collect();
        let _guard = self.recovery.enter(&documents.root)?;
        let discovered = discover_metadata(&documents, &destinations, limits, cancel)?;
        for snapshot in discovered {
            required.extend(snapshot.entries().iter().filter_map(|(path, observation)| {
                matches!(observation, Observation::File(_)).then_some(path.clone())
            }));
            documents.native = documents.native.merge(snapshot)?;
        }
        let filter = CaptureFilter::selected_mutation(&required)?;
        self.capture_mutation_paths_filtered(selected, documents, required, limits, &filter, cancel)
    }
}

/// Discovery candidates are not a read set. Only a successfully parsed record naming a
/// selected destination is retained, then revalidated against final capture. Never follow links.
fn discover_metadata(
    workspace: &WorkspaceSnapshot,
    destinations: &BTreeSet<PortableRelPath>,
    limits: SnapshotLimits,
    cancel: &Cancellation,
) -> Result<Vec<NativeSnapshot>> {
    let mut result = Vec::new();
    if destinations.is_empty() {
        return Ok(result);
    }
    let pack = match workspace.root.directory.open_dir_nofollow("pack") {
        Ok(pack) => pack,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(result),
        Err(error) => return Err(error.into()),
    };
    native::reject_reparse(&pack.try_clone()?.into_std_file())?;
    let filter = workspace.source_filter(cancel)?;
    let mut pending = vec![(String::new(), 0usize)];
    let mut seen = 0usize;
    let mut remaining = limits.total_bytes;
    while let Some((prefix, depth)) = pending.pop() {
        cancel.check()?;
        ensure!(
            depth <= limits.depth,
            "Metadata discovery exceeds depth limit"
        );
        let directory = if prefix.is_empty() {
            pack.try_clone()?
        } else {
            let path = PortableRelPath::parse(&prefix, PathSyntax::ProjectContent)?;
            let Ok((parent, leaf)) = native::parent(&pack, &path) else {
                continue;
            };
            let Ok(directory) = parent.open_dir_nofollow(&leaf) else {
                continue;
            };
            if native::reject_reparse(&directory.try_clone()?.into_std_file()).is_err() {
                continue;
            }
            directory
        };
        let entries = match directory.entries() {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries {
            cancel.check()?;
            seen += 1;
            ensure!(
                seen <= limits.entries,
                "Metadata discovery exceeds entry limit"
            );
            let Ok(entry) = entry else { continue };
            let Ok(name) = entry.file_name().into_string() else {
                continue;
            };
            let relative = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            let Ok(relative) = PortableRelPath::parse(&relative, PathSyntax::ProjectContent) else {
                continue;
            };
            let Ok(metadata) = directory.symlink_metadata(&name) else {
                continue;
            };
            if metadata.file_type().is_symlink() || !filter.includes(&relative, metadata.is_dir()) {
                continue;
            }
            if metadata.is_dir() {
                pending.push((relative.as_str().to_owned(), depth + 1));
                continue;
            }
            if !metadata.is_file()
                || !name.ends_with(".pw.toml")
                || metadata.len() > limits.file_bytes.min(MAX_DOCUMENT_BYTES as u64)
            {
                continue;
            }
            ensure!(
                metadata.len() <= remaining,
                "Metadata discovery exceeds byte limit"
            );
            let path = PortableRelPath::parse(
                &format!("pack/{}", relative.as_str()),
                PathSyntax::ProjectContent,
            )?;
            let bounded = SnapshotLimits {
                file_bytes: limits
                    .file_bytes
                    .min(MAX_DOCUMENT_BYTES as u64)
                    .min(remaining),
                total_bytes: remaining,
                ..limits
            };
            // A race, unreadable file, or unsupported filesystem object establishes no ownership.
            let Ok(snapshot) = workspace
                .root
                .capture(std::slice::from_ref(&path), bounded, cancel)
            else {
                remaining = remaining.saturating_sub(bounded.file_bytes);
                cancel.check()?;
                continue;
            };
            if let Some(Observation::File(file)) = snapshot.entries().get(&path) {
                remaining = remaining.saturating_sub(file.bytes);
            }
            let Ok(Some(bytes)) = read_document(&workspace.root, &snapshot, path.as_str(), cancel)
            else {
                cancel.check()?;
                continue;
            };
            let Ok(record) = BackendFile::parse(relative, &bytes) else {
                continue;
            };
            if destinations.contains(record.destination.relative()) {
                result.push(snapshot);
            }
        }
    }
    workspace.root.check_binding()?;
    cancel.check()?;
    Ok(result)
}
