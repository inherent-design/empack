//! Read-only normalized project preparation, gated by native publication recovery state.
use super::{
    content::{AcquiredContent, InitialObservation, SourceEvidencePolicy, verify_stream},
    documents::{DecodedIntent, DecodedLock, DocumentCodec, MAX_DOCUMENT_BYTES},
    io::copy_bounded,
    native,
    publication::RecoveryReader,
    snapshot::{NativeSnapshot, Observation, ProjectReadRoot, SnapshotLimits},
};
use crate::application::process_runtime::Cancellation;
use anyhow::{Context, Result, ensure};
use empack_core::{
    digest::ContentId,
    files::FilePermissions,
    model::{AcquisitionSpec, DocumentRevision, ExpectedContent, ResolvedProject},
    path::{PathSyntax, PortableRelPath},
};
use std::path::Path;

mod creation;
mod synchronization;
pub use creation::NewProjectSnapshot;

/// Required authoring state is distinct from an I/O or publication failure.
#[derive(Debug, thiserror::Error)]
pub enum ProjectDocumentsError {
    #[error("Project has no empack.yml")]
    MissingIntent,
    #[error("Project has no exact resolution lock")]
    MissingLock,
}

/// Managed replacement inputs may be empty or contain an invalid prior document.
/// Capturing them grants no authority to remove or overwrite files.
pub struct ReplacementSnapshot {
    root: ProjectReadRoot,
    native: NativeSnapshot,
    recovery: RecoveryReader,
    limits: SnapshotLimits,
}
impl ReplacementSnapshot {
    pub fn observations(&self) -> &NativeSnapshot {
        &self.native
    }
    /// Observe only explicit seed destinations. Existing regular files are user-owned;
    /// directories and links cannot authorize a seed or broaden the replacement footprint.
    pub(super) fn capture_seed_files(
        mut self,
        paths: &[PortableRelPath],
        cancel: &Cancellation,
    ) -> Result<Self> {
        let _guard = self.recovery.enter(&self.root)?;
        let missing: Vec<_> = paths
            .iter()
            .filter(|path| !self.native.entries().contains_key(*path))
            .cloned()
            .collect();
        if !missing.is_empty() {
            let captured = self.root.capture(&missing, self.limits, cancel)?;
            self.native = self.native.merge(captured)?;
        }
        for path in paths {
            ensure!(
                matches!(
                    self.native.entries().get(path),
                    Some(Observation::File(_) | Observation::Absent)
                ),
                "Scaffold destination is not a regular file or absent: {}",
                path.as_str()
            );
        }
        self.root.revalidate(&self.native, cancel)?;
        Ok(self)
    }
    pub(super) fn seed_file_is_missing(&self, path: &PortableRelPath) -> bool {
        matches!(self.native.entries().get(path), Some(Observation::Absent))
    }
    /// Bind entries that can collide with seed outputs, including common-layer alternatives.
    /// Unrelated templates are filtered before opening their contents or validating their names.
    pub(super) fn capture_seed_templates(
        mut self,
        paths: &[PortableRelPath],
        cancel: &Cancellation,
    ) -> Result<Self> {
        if paths.is_empty() {
            return Ok(self);
        }
        let mut scopes = std::collections::BTreeSet::from([PortableRelPath::parse(
            "templates/common",
            PathSyntax::ProjectContent,
        )?]);
        for path in paths {
            let relative = path
                .as_str()
                .strip_prefix("templates/")
                .context("Template outside its namespace")?;
            let layer = relative
                .split_once('/')
                .context("Template lacks a layer")?
                .0;
            ensure!(
                matches!(layer, "common" | "client" | "server"),
                "Unknown template layer"
            );
            scopes.insert(PortableRelPath::parse(
                &format!("templates/{layer}"),
                PathSyntax::ProjectContent,
            )?);
        }
        if scopes
            .iter()
            .all(|scope| self.native.entries().contains_key(scope))
        {
            return Ok(self);
        }
        let _guard = self.recovery.enter(&self.root)?;
        let filter = super::source::CaptureFilter::template_seeds(paths)?;
        let captured = self.root.capture_filtered(
            &scopes.iter().cloned().collect::<Vec<_>>(),
            self.limits,
            Some(&filter),
            cancel,
        )?;
        for scope in &scopes {
            ensure!(
                matches!(
                    captured.entries().get(scope),
                    Some(Observation::Directory { .. } | Observation::Absent)
                ),
                "Template layer is not a directory or absent"
            );
        }
        self.native = self.native.merge(captured)?;
        self.root.revalidate(&self.native, cancel)?;
        Ok(self)
    }
    pub(super) fn template_seed_is_missing(&self, path: &PortableRelPath) -> Result<bool> {
        let (layer, destination, _) = super::templates::template_address(path.as_str())?
            .context("Unknown template seed layer")?;
        for (observed, kind) in self.native.entries() {
            if !matches!(kind, Observation::File(_)) {
                continue;
            }
            let Some(relative) = observed.as_str().strip_prefix("templates/") else {
                continue;
            };
            let Some((existing_layer, existing_destination, _)) =
                super::templates::template_address(relative)?
            else {
                continue;
            };
            if existing_layer != empack_core::model::ContentLayer::Common && existing_layer != layer
            {
                continue;
            }
            let mut collisions = super::layout::CollisionIndex::default();
            collisions.insert_file(&existing_destination)?;
            if collisions.insert_file(&destination).is_err() {
                return Ok(false);
            }
        }
        Ok(true)
    }
    /// Explicit incoming paths need unfiltered absence evidence. Filtered directory membership
    /// cannot establish that an ignored destination is absent or authorize overwriting it.
    pub(super) fn complete_for(
        mut self,
        targets: &[PortableRelPath],
        cancel: &Cancellation,
    ) -> Result<Self> {
        let _guard = self.recovery.enter(&self.root)?;
        let missing: Vec<_> = targets
            .iter()
            .filter(|path| !self.native.entries().contains_key(*path))
            .cloned()
            .collect();
        if !missing.is_empty() {
            let additional = self.root.capture(&missing, self.limits, cancel)?;
            for path in &missing {
                ensure!(
                    !matches!(
                        additional.entries().get(path),
                        Some(Observation::File(_) | Observation::Directory { .. })
                    ),
                    "Import destination was not captured as owned content: {}",
                    path.as_str()
                );
            }
            self.native = self.native.merge(additional)?;
        }
        self.root.revalidate(&self.native, cancel)?;
        Ok(self)
    }
    pub(super) fn into_native(self) -> (ProjectReadRoot, NativeSnapshot) {
        (self.root, self.native)
    }
}
/// Managed mutation capture includes every locked placement even when source rules exclude it.
/// Acquisition sources outside managed placements are not deletion authority.
pub struct MutationSnapshot {
    workspace: WorkspaceSnapshot,
}
impl MutationSnapshot {
    pub(in crate::engine) fn workspace(&self) -> &WorkspaceSnapshot {
        &self.workspace
    }
    pub(in crate::engine) fn require_revision(
        &self,
        revision: Option<ProjectRevision>,
    ) -> Result<()> {
        ensure!(
            revision.is_none_or(|expected| self.workspace.revision() == expected),
            "Dependency choices belong to changed project documents; resolve a fresh request"
        );
        Ok(())
    }
    pub(super) fn into_workspace(self) -> WorkspaceSnapshot {
        self.workspace
    }
}

/// A read precondition for choices derived from one native project's documents.
/// This value grants no mutation authority and cannot be restored from serialized input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProjectRevision {
    root: native::ObjectIdentity,
    intent: DocumentRevision,
    lock: Option<DocumentRevision>,
}

pub struct WorkspaceSnapshot {
    root: ProjectReadRoot,
    native: NativeSnapshot,
    intent: DecodedIntent,
    prior_lock: Option<DecodedLock>,
}
impl WorkspaceSnapshot {
    pub fn revision(&self) -> ProjectRevision {
        ProjectRevision {
            root: self.root.binding,
            intent: self.intent.raw_revision(),
            lock: self.prior_lock.as_ref().map(DecodedLock::raw_revision),
        }
    }
    pub(super) fn into_native(self) -> (ProjectReadRoot, NativeSnapshot) {
        (self.root, self.native)
    }
    pub(super) fn read_document(
        &self,
        path: &PortableRelPath,
        cancel: &Cancellation,
    ) -> Result<Option<Vec<u8>>> {
        read_document(&self.root, &self.native, path.as_str(), cancel)
    }
    pub fn intent(&self) -> &DecodedIntent {
        &self.intent
    }
    pub fn prior_lock(&self) -> Option<&DecodedLock> {
        self.prior_lock.as_ref()
    }
    pub fn observations(&self) -> &NativeSnapshot {
        &self.native
    }
    pub fn root(&self) -> &ProjectReadRoot {
        &self.root
    }
    /// A stale lock remains available for reconciliation but cannot silently satisfy build input.
    pub fn require_resolved(&self) -> Result<ResolvedProject> {
        self.prior_lock
            .as_ref()
            .context(ProjectDocumentsError::MissingLock)?
            .bind(&self.intent)
    }
    /// Enumerate native source layers without interpreting foreign metadata names.
    pub(in crate::engine) fn source_entries(
        &self,
        cancel: &Cancellation,
    ) -> Result<Vec<super::source::SourceEntry>> {
        use empack_core::{model::ContentLayer, path::InstallDestination};
        let filter = super::source::SourceFilter::author(&self.intent.intent().source_excludes)?;
        let mut result = Vec::new();
        for (prefix, layer) in [
            ("pack/", ContentLayer::Common),
            ("overrides/common/", ContentLayer::CommonOverride),
            ("overrides/client/", ContentLayer::Client),
            ("overrides/server/", ContentLayer::Server),
        ] {
            let base =
                PortableRelPath::parse(prefix.trim_end_matches('/'), PathSyntax::ProjectContent)?;
            ensure!(
                matches!(
                    self.native.entries().get(&base),
                    Some(Observation::Directory { .. } | Observation::Absent)
                ),
                "Native source layer was not captured"
            );
            for (path, entry) in self.native.entries() {
                cancel.check()?;
                if !matches!(entry, Observation::File(_)) {
                    continue;
                }
                let Some(relative) = path.as_str().strip_prefix(prefix) else {
                    continue;
                };
                let destination = InstallDestination::parse(relative)?;
                if filter.includes(destination.relative(), false) {
                    result.push(super::source::SourceEntry {
                        path: path.clone(),
                        destination,
                        layer,
                    });
                }
            }
        }
        Ok(result)
    }
    /// Copy one captured regular input into a verified private lease. This cannot read an
    /// uncaptured path or publish bytes, and every source declaration remains enforced.
    pub fn acquire_file(
        &self,
        path: &PortableRelPath,
        expected: Option<&ExpectedContent>,
        policy: SourceEvidencePolicy,
        cancel: &Cancellation,
    ) -> Result<(AcquiredContent, FilePermissions)> {
        let (mut file, effective, permissions) = self.open_observed_input(path, expected)?;
        let maximum = effective.size.expect("captured size");
        let acquired = verify_stream(
            &mut file,
            &effective,
            maximum,
            policy,
            InitialObservation::RequireEvidence,
            cancel,
        )?;
        Ok((acquired, permissions))
    }
    /// Check captured bytes without retaining another copy merely to authorize their removal.
    pub(super) fn verify_file(
        &self,
        path: &PortableRelPath,
        expected: &ExpectedContent,
        cancel: &Cancellation,
    ) -> Result<empack_core::digest::DigestSet> {
        let (mut file, effective, _) = self.open_observed_input(path, Some(expected))?;
        let maximum = effective.size.expect("captured size");
        Ok(super::content::verify_observation(
            &mut file,
            &effective,
            maximum,
            SourceEvidencePolicy::Compatibility,
            InitialObservation::RequireEvidence,
            cancel,
        )?
        .observed)
    }
    pub(in crate::engine) fn open_observed_input(
        &self,
        path: &PortableRelPath,
        expected: Option<&ExpectedContent>,
    ) -> Result<(std::fs::File, ExpectedContent, FilePermissions)> {
        let Some(Observation::File(observed)) = self.native.entries().get(path) else {
            anyhow::bail!("Input is not a captured regular file: {}", path.as_str());
        };
        let content_id = ContentId::from_sha256(observed.content);
        if let Some(expected) = expected {
            ensure!(
                expected.size.is_none_or(|size| size == observed.bytes),
                "Source size differs from lock"
            );
            ensure!(
                expected
                    .accepted_observation
                    .as_ref()
                    .is_none_or(|id| id == &content_id),
                "Source differs from accepted lock observation"
            );
        }
        self.root.check_binding()?;
        let (parent, leaf) = native::parent(&self.root.directory, path)?;
        let file = native::open_file(&parent, &leaf)?;
        ensure!(
            native::identity(&file)? == observed.object,
            "Source object changed before acquisition"
        );
        let effective = ExpectedContent {
            digests: expected.and_then(|value| value.digests.clone()),
            size: Some(observed.bytes),
            accepted_observation: Some(content_id),
        };
        Ok((
            file,
            effective,
            super::verification::content(observed).permissions,
        ))
    }
}
/// Receives only read-only journal access. No cache, tool or publication capability is present.
pub struct ProjectReader {
    recovery: RecoveryReader,
}
enum AdditionCapture<'a> {
    Add,
    Adopt,
    Replace(&'a empack_core::addition::ReplacementSelection),
}
impl ProjectReader {
    pub fn new(recovery: RecoveryReader) -> Self {
        Self { recovery }
    }
    /// Observe only authoring documents and managed content roots, without requiring valid
    /// prior schemas. Templates, distributions and unrelated root files remain outside replacement.
    pub fn capture_replacement(
        &self,
        selected: &Path,
        limits: SnapshotLimits,
        cancel: &Cancellation,
    ) -> Result<ReplacementSnapshot> {
        cancel.check()?;
        let root = ProjectReadRoot::open(selected)?;
        let _initial_guard = self.recovery.enter(&root)?;
        let scopes = [
            "empack.yml",
            "empack.lock",
            "pack",
            "overrides/common",
            "overrides/client",
            "overrides/server",
        ]
        .into_iter()
        .map(|path| PortableRelPath::parse(path, PathSyntax::ProjectContent))
        .collect::<std::result::Result<Vec<_>, _>>()?;
        let document = PortableRelPath::parse("empack.yml", PathSyntax::ProjectContent)?;
        let prior = root.capture(&[document], limits, cancel)?;
        let excludes = read_document(&root, &prior, "empack.yml", cancel)?
            .and_then(|bytes| {
                DocumentCodec
                    .decode_intent(&bytes, "replacement source")
                    .ok()
            })
            .map(|document| document.intent().source_excludes.clone())
            .unwrap_or_default();
        let capture_filter = super::source::CaptureFilter::author(&excludes, &[])?;
        let native = root.capture_filtered(&scopes, limits, Some(&capture_filter), cancel)?;
        let native = prior.merge(native)?;
        let _final_guard = self.recovery.enter(&root)?;
        root.revalidate(&native, cancel)?;
        Ok(ReplacementSnapshot {
            root,
            native,
            recovery: self.recovery.clone(),
            limits,
        })
    }
    /// Bind standard build inputs and each declared local/archive source, including files outside
    /// managed namespaces. Only requested artifact destinations enter the read set, not retained
    /// unrelated distributions. Artifact budgets are separate from source-file limits.
    /// A second capture retains the exact first document revisions.
    pub fn capture_build(
        &self,
        selected: &Path,
        artifacts: &[PortableRelPath],
        limits: SnapshotLimits,
        cancel: &Cancellation,
    ) -> Result<WorkspaceSnapshot> {
        let archive = super::artifacts::ArchiveLimits::default();
        self.capture_build_with_limits(
            selected,
            artifacts,
            limits,
            SnapshotLimits {
                file_bytes: archive.compressed_bytes,
                total_bytes: archive.compressed_bytes,
                entries: limits.entries,
                depth: limits.depth,
            },
            cancel,
        )
    }
    pub fn capture_build_with_limits(
        &self,
        selected: &Path,
        artifacts: &[PortableRelPath],
        limits: SnapshotLimits,
        artifact_limits: SnapshotLimits,
        cancel: &Cancellation,
    ) -> Result<WorkspaceSnapshot> {
        self.capture_build_selection(selected, artifacts, false, limits, artifact_limits, cancel)
    }
    /// Cleanup captures the whole artifact namespace and binds its membership to approval.
    pub(in crate::engine) fn capture_build_selection(
        &self,
        selected: &Path,
        artifacts: &[PortableRelPath],
        clean: bool,
        limits: SnapshotLimits,
        artifact_limits: SnapshotLimits,
        cancel: &Cancellation,
    ) -> Result<WorkspaceSnapshot> {
        let documents = self.capture(selected, &[], limits, cancel)?;
        let project = documents.require_resolved()?;
        let mut required_sources = Vec::new();
        let mut scopes = [
            "pack",
            "overrides/common",
            "overrides/client",
            "overrides/server",
            "templates",
        ]
        .into_iter()
        .map(|name| PortableRelPath::parse(name, PathSyntax::ProjectContent))
        .collect::<std::result::Result<Vec<_>, _>>()?;
        for dependency in project.lock().dependencies.values() {
            for file in dependency.files.as_slice() {
                match &file.acquisition {
                    AcquisitionSpec::Local(path) => {
                        scopes.push(path.clone());
                        required_sources.push(path.clone());
                    }
                    AcquisitionSpec::Embedded { archive, .. } => {
                        scopes.push(archive.clone());
                        required_sources.push(archive.clone());
                    }
                    _ => {}
                }
            }
        }
        scopes.sort();
        scopes.dedup();
        let names: std::collections::BTreeSet<_> =
            scopes.iter().map(PortableRelPath::as_str).collect();
        let scopes: Vec<_> = scopes
            .iter()
            .filter(|path| {
                !path
                    .as_str()
                    .match_indices('/')
                    .any(|(index, _)| names.contains(&path.as_str()[..index]))
            })
            .cloned()
            .collect();
        for dependency in project.lock().dependencies.values() {
            for file in dependency.files.as_slice() {
                for placement in file.placements.as_slice() {
                    required_sources.push(super::layout::ProjectLayout::path(
                        &empack_core::files::ManagedPath::Content {
                            layer: placement.layer,
                            path: placement.destination.relative().clone(),
                        },
                    )?);
                }
            }
        }
        let filter = super::source::CaptureFilter::author(
            &documents.intent.intent().source_excludes,
            &required_sources,
        )?;
        let mut captured =
            self.capture_selected(selected, &scopes, limits, Some(&filter), cancel)?;
        // The rule bytes used to select traversal remain exact read-set inputs.
        captured.native = documents.native.merge(captured.native)?;
        if clean || !artifacts.is_empty() {
            // A build may observe old outputs, but cannot replace an input under another role.
            // Use portable collision rules too: case aliases must not bypass this on Windows.
            let mut ownership = super::layout::CollisionIndex::default();
            for input in &scopes {
                ownership.insert_file(input)?;
            }
            let output_scopes = if clean {
                vec![PortableRelPath::parse("dist", PathSyntax::ProjectContent)?]
            } else {
                artifacts
                    .iter()
                    .map(|artifact| {
                        super::layout::ProjectLayout::path(
                            &empack_core::files::ManagedPath::Artifact(artifact.clone()),
                        )
                    })
                    .collect::<Result<Vec<_>>>()?
            };
            for output in &output_scopes {
                ownership
                    .insert_file(output)
                    .context("Build output overlaps a captured source scope")?;
            }
            let _guard = self.recovery.enter(&captured.root)?;
            let outputs = captured
                .root
                .capture(&output_scopes, artifact_limits, cancel)?;
            captured.native = captured.native.merge(outputs)?;
            captured.root.revalidate(&captured.native, cancel)?;
        }
        ensure!(
            documents.root.binding == captured.root.binding
                && documents.intent.raw_revision() == captured.intent.raw_revision()
                && documents.prior_lock.as_ref().map(DecodedLock::raw_revision)
                    == captured.prior_lock.as_ref().map(DecodedLock::raw_revision),
            "Project documents changed while selecting build inputs"
        );
        Ok(captured)
    }
    /// Capture managed placements for semantic mutations. Templates, artifacts and external
    /// acquisition sources remain outside this operation; locked placements override ignore rules.
    pub fn capture_mutation(
        &self,
        selected: &Path,
        limits: SnapshotLimits,
        cancel: &Cancellation,
    ) -> Result<MutationSnapshot> {
        self.capture_removal_inputs(selected, None, limits, cancel)
    }
    /// Resolve read-only user selectors before capturing only their managed file placements.
    pub fn capture_removal(
        &self,
        selected: &Path,
        selectors: &empack_core::model::NonEmpty<super::removal::RemovalSelector>,
        mode: empack_core::removal::RemovalMode,
        evidence: empack_core::removal::RemovalEvidencePolicy,
        limits: SnapshotLimits,
        cancel: &Cancellation,
    ) -> Result<MutationSnapshot> {
        self.capture_removal_inputs(selected, Some((selectors, mode, evidence)), limits, cancel)
    }
    fn capture_removal_inputs(
        &self,
        selected: &Path,
        selection: Option<(
            &empack_core::model::NonEmpty<super::removal::RemovalSelector>,
            empack_core::removal::RemovalMode,
            empack_core::removal::RemovalEvidencePolicy,
        )>,
        limits: SnapshotLimits,
        cancel: &Cancellation,
    ) -> Result<MutationSnapshot> {
        let documents = self.capture(selected, &[], limits, cancel)?;
        let project = documents.require_resolved()?;
        let selected_keys = selection
            .map(|(selectors, mode, evidence)| {
                let keys = super::removal::resolve_selections(&project, selectors)?;
                Ok::<_, anyhow::Error>(
                    super::removal::logical_plan(&project, &keys, mode, evidence)?.removed(),
                )
            })
            .transpose()?;
        let mut required = Vec::new();
        for (key, dependency) in &project.lock().dependencies {
            if selected_keys
                .as_ref()
                .is_some_and(|keys| !keys.contains(key))
            {
                continue;
            }
            for file in dependency.files.as_slice() {
                for placement in file.placements.as_slice() {
                    required.push(super::layout::ProjectLayout::path(
                        &empack_core::files::ManagedPath::Content {
                            layer: placement.layer,
                            path: placement.destination.relative().clone(),
                        },
                    )?);
                }
            }
        }
        self.capture_dependency_paths(selected, documents, required, limits, cancel)
    }
    /// Bind old and new placements for the canonical addition without reading unrelated payloads.
    pub fn capture_addition(
        &self,
        selected: &Path,
        group: &empack_core::addition::AdditionGroup,
        limits: SnapshotLimits,
        cancel: &Cancellation,
    ) -> Result<MutationSnapshot> {
        self.capture_addition_mode(selected, group, limits, AdditionCapture::Add, cancel)
    }
    /// Observe selected installed placements without comparing them
    /// with old byte assertions. This nominates adoption inputs; it grants no write authority.
    pub fn capture_observed_dependencies(
        &self,
        selected: &Path,
        keys: &empack_core::model::NonEmpty<empack_core::model::DependencyKey>,
        limits: SnapshotLimits,
        cancel: &Cancellation,
    ) -> Result<WorkspaceSnapshot> {
        let documents = self.capture(selected, &[], limits, cancel)?;
        let current = documents.require_resolved()?;
        let mut required = Vec::new();
        for key in keys.as_slice() {
            let dependency =
                current.lock().dependencies.get(key).with_context(|| {
                    format!("Unknown installed dependency key: {}", key.as_str())
                })?;
            for placement in dependency
                .files
                .as_slice()
                .iter()
                .flat_map(|file| file.placements.as_slice())
            {
                required.push(super::layout::ProjectLayout::path(
                    &empack_core::files::ManagedPath::Content {
                        layer: placement.layer,
                        path: placement.destination.relative().clone(),
                    },
                )?);
            }
        }
        Ok(self
            .capture_dependency_paths(selected, documents, required, limits, cancel)?
            .into_workspace())
    }

    /// Adoption may establish the first lock, after verifying every proposed payload.
    pub fn capture_adoption(
        &self,
        selected: &Path,
        group: &empack_core::addition::AdditionGroup,
        limits: SnapshotLimits,
        cancel: &Cancellation,
    ) -> Result<MutationSnapshot> {
        self.capture_addition_mode(selected, group, limits, AdditionCapture::Adopt, cancel)
    }
    /// Replacement binds both the selected prior objects and every requested new placement.
    pub fn capture_dependency_replacement(
        &self,
        selected: &Path,
        group: &empack_core::addition::AdditionGroup,
        selection: &empack_core::addition::ReplacementSelection,
        limits: SnapshotLimits,
        cancel: &Cancellation,
    ) -> Result<MutationSnapshot> {
        self.capture_addition_mode(
            selected,
            group,
            limits,
            AdditionCapture::Replace(selection),
            cancel,
        )
    }
    fn capture_addition_mode(
        &self,
        selected: &Path,
        group: &empack_core::addition::AdditionGroup,
        limits: SnapshotLimits,
        mode: AdditionCapture<'_>,
        cancel: &Cancellation,
    ) -> Result<MutationSnapshot> {
        let documents = self.capture(selected, &[], limits, cancel)?;
        let current = documents
            .prior_lock()
            .map(|lock| lock.bind(documents.intent()))
            .transpose()?;
        let candidate = match mode {
            AdditionCapture::Adopt => super::addition::AdditionCandidate::prepare_adoption(
                documents.intent(),
                documents.prior_lock(),
                group,
            )?,
            AdditionCapture::Add => super::addition::AdditionCandidate::prepare(
                documents.intent(),
                documents.prior_lock().context("Addition requires a lock")?,
                group,
            )?,
            AdditionCapture::Replace(selection) => {
                super::addition::AdditionCandidate::prepare_replacement(
                    documents.intent(),
                    documents
                        .prior_lock()
                        .context("Replacement requires a lock")?,
                    group,
                    selection,
                )?
            }
        };
        let mut required = Vec::new();
        for key in candidate.plan().bindings().values() {
            for dependency in [
                current
                    .as_ref()
                    .and_then(|current| current.lock().dependencies.get(key)),
                candidate.project().lock().dependencies.get(key),
            ]
            .into_iter()
            .flatten()
            {
                for file in dependency.files.as_slice() {
                    for placement in file.placements.as_slice() {
                        required.push(super::layout::ProjectLayout::path(
                            &empack_core::files::ManagedPath::Content {
                                layer: placement.layer,
                                path: placement.destination.relative().clone(),
                            },
                        )?);
                    }
                }
            }
        }
        self.capture_dependency_paths(selected, documents, required, limits, cancel)
    }
    #[allow(clippy::too_many_arguments)]
    fn capture_mutation_paths_filtered(
        &self,
        selected: &Path,
        documents: WorkspaceSnapshot,
        required: Vec<PortableRelPath>,
        limits: SnapshotLimits,
        filter: &super::source::CaptureFilter,
        cancel: &Cancellation,
    ) -> Result<MutationSnapshot> {
        let scopes = [
            "pack",
            "overrides/common",
            "overrides/client",
            "overrides/server",
        ]
        .into_iter()
        .filter(|name| {
            *name == "pack"
                || required
                    .iter()
                    .any(|path| path.as_str().starts_with(&format!("{name}/")))
        })
        .map(|name| PortableRelPath::parse(name, PathSyntax::ProjectContent))
        .collect::<std::result::Result<Vec<_>, _>>()?;
        let workspace = self.capture_selected(selected, &scopes, limits, Some(filter), cancel)?;
        ensure!(
            documents.root.binding == workspace.root.binding
                && documents.intent.raw_revision() == workspace.intent.raw_revision()
                && documents.prior_lock.as_ref().map(DecodedLock::raw_revision)
                    == workspace.prior_lock.as_ref().map(DecodedLock::raw_revision),
            "Project documents changed while selecting mutation inputs"
        );
        // Captured aliases must not turn into false absence on a case-sensitive host or
        // select differently spelled native objects on a case-insensitive one. This index
        // checks spelling only; actual file kinds remain subject to removal verification.
        let mut spelling = super::layout::CollisionIndex::default();
        for path in workspace.native.entries().keys().chain(required.iter()) {
            spelling.insert_directory(path)?;
        }
        documents.root.revalidate(&documents.native, cancel)?;
        Ok(MutationSnapshot { workspace })
    }
    /// Documents are always in the read set. Extra scopes bind this operation's source and output inputs.
    pub fn capture(
        &self,
        selected: &Path,
        scopes: &[PortableRelPath],
        limits: SnapshotLimits,
        cancel: &Cancellation,
    ) -> Result<WorkspaceSnapshot> {
        self.capture_selected(selected, scopes, limits, None, cancel)
    }
    fn capture_selected(
        &self,
        selected: &Path,
        scopes: &[PortableRelPath],
        limits: SnapshotLimits,
        filter: Option<&super::source::CaptureFilter>,
        cancel: &Cancellation,
    ) -> Result<WorkspaceSnapshot> {
        cancel.check()?;
        let root = ProjectReadRoot::open(selected)?;
        let _initial_guard = self.recovery.enter(&root)?;
        let mut scopes = scopes.to_vec();
        for name in ["empack.yml", "empack.lock"] {
            let path = PortableRelPath::parse(name, PathSyntax::ProjectContent)?;
            if !scopes.contains(&path) {
                scopes.push(path);
            }
        }
        let native = root.capture_filtered(&scopes, limits, filter, cancel)?;
        let intent_bytes = read_document(&root, &native, "empack.yml", cancel)?
            .context(ProjectDocumentsError::MissingIntent)?;
        let intent = DocumentCodec.decode_intent(&intent_bytes, "empack.yml")?;
        let prior_lock = read_document(&root, &native, "empack.lock", cancel)?
            .map(|bytes| DocumentCodec.decode_prior_lock(&bytes, "empack.lock"))
            .transpose()?;
        // The host coordination file may have been created after the first read-only probe.
        let _final_guard = self.recovery.enter(&root)?;
        root.revalidate(&native, cancel)?;
        Ok(WorkspaceSnapshot {
            root,
            native,
            intent,
            prior_lock,
        })
    }
}
pub(super) fn read_document(
    root: &ProjectReadRoot,
    snapshot: &NativeSnapshot,
    name: &str,
    cancel: &Cancellation,
) -> Result<Option<Vec<u8>>> {
    let path = PortableRelPath::parse(name, PathSyntax::ProjectContent)?;
    let expected = match snapshot.entries().get(&path) {
        Some(Observation::Absent) => return Ok(None),
        Some(Observation::File(file)) => file,
        _ => anyhow::bail!("Project document is not a regular file: {name}"),
    };
    ensure!(
        expected.bytes <= MAX_DOCUMENT_BYTES as u64,
        "Project document exceeds size limit: {name}"
    );
    let (parent, leaf) = native::parent(&root.directory, &path)?;
    let mut file = native::open_file(&parent, &leaf)?;
    ensure!(
        native::identity(&file)? == expected.object,
        "Project document changed before decoding: {name}"
    );
    let mut bytes = Vec::with_capacity(expected.bytes as usize);
    let (digest, count) = copy_bounded(&mut file, &mut bytes, expected.bytes, cancel)?;
    ensure!(
        digest == expected.content && count == expected.bytes,
        "Project document bytes changed before decoding: {name}"
    );
    Ok(Some(bytes))
}
#[cfg(test)]
mod tests;
