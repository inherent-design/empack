//! Observe exact author-owned destinations and sources without foreign metadata discovery.
use super::*;
use crate::engine::{layout::ProjectLayout, source::CaptureFilter};
use empack_core::files::ManagedPath;
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

    /// Bind only recorded placements for exact native ownership.
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
        let documents = self.capture(selected, &[], limits, cancel)?;
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
        documents: WorkspaceSnapshot,
        required: Vec<PortableRelPath>,
        limits: SnapshotLimits,
        cancel: &Cancellation,
    ) -> Result<MutationSnapshot> {
        let filter = CaptureFilter::native_mutation(&required)?;
        self.capture_mutation_paths_filtered(selected, documents, required, limits, &filter, cancel)
    }
}
