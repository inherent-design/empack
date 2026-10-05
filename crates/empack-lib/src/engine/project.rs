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
    model::{AcquisitionSpec, ExpectedContent, ResolvedProject},
    path::{PathSyntax, PortableRelPath},
};
use std::path::Path;

pub struct WorkspaceSnapshot {
    root: ProjectReadRoot,
    native: NativeSnapshot,
    intent: DecodedIntent,
    prior_lock: Option<DecodedLock>,
}
impl WorkspaceSnapshot {
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
            .context("Project has no exact resolution lock")?
            .bind(&self.intent)
    }
    /// Decode backend metadata only from captured pack files, retaining native byte and object
    /// checks. This is an observation of the tree, not proof that an index includes every record.
    pub fn backend_files(&self, cancel: &Cancellation) -> Result<Vec<super::backend::BackendFile>> {
        self.root.check_binding()?;
        let mut result = Vec::new();
        let mut destinations = super::layout::CollisionIndex::default();
        for (path, observation) in self.native.entries() {
            let Some(relative) = path.as_str().strip_prefix("pack/") else {
                continue;
            };
            if !relative.ends_with(".pw.toml") {
                continue;
            }
            ensure!(
                matches!(observation, Observation::File(_)),
                "Backend metadata is not a regular file"
            );
            let bytes = read_document(&self.root, &self.native, path.as_str(), cancel)?
                .context("Captured backend metadata disappeared")?;
            let file = super::backend::BackendFile::parse(
                PortableRelPath::parse(relative, PathSyntax::ProjectContent)?,
                &bytes,
            )?;
            destinations.insert_file(file.destination.relative())?;
            result.push(file);
        }
        self.root.check_binding()?;
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
        let mut file = native::open_file(&parent, &leaf)?;
        ensure!(
            native::identity(&file)? == observed.object,
            "Source object changed before acquisition"
        );
        let acquired = verify_stream(
            &mut file,
            &ExpectedContent {
                digests: expected.and_then(|value| value.digests.clone()),
                size: Some(observed.bytes),
                accepted_observation: Some(content_id),
            },
            self.native.limits().file_bytes,
            policy,
            InitialObservation::RequireEvidence,
            cancel,
        )?;
        Ok((acquired, super::verification::content(observed).permissions))
    }
}
/// Receives only read-only journal access. No cache, tool or publication capability is present.
pub struct ProjectReader {
    recovery: RecoveryReader,
}
impl ProjectReader {
    pub fn new(recovery: RecoveryReader) -> Self {
        Self { recovery }
    }
    /// Bind standard build inputs and each declared local/archive source, including files outside
    /// managed namespaces. Only requested artifact destinations enter the read set, not retained
    /// unrelated distributions. Limits also apply to selected existing outputs; callers must budget
    /// their before-images. A second capture retains the exact first document revisions.
    pub fn capture_build(
        &self,
        selected: &Path,
        artifacts: &[PortableRelPath],
        limits: SnapshotLimits,
        cancel: &Cancellation,
    ) -> Result<WorkspaceSnapshot> {
        let documents = self.capture(selected, &[], limits, cancel)?;
        let project = documents.require_resolved()?;
        let mut scopes = ["pack", "overrides/client", "overrides/server", "templates"]
            .into_iter()
            .map(|name| PortableRelPath::parse(name, PathSyntax::ProjectContent))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        for artifact in artifacts {
            scopes.push(super::layout::ProjectLayout::path(
                &empack_core::files::ManagedPath::Artifact(artifact.clone()),
            )?);
        }
        for dependency in project.lock().dependencies.values() {
            for file in dependency.files.as_slice() {
                match &file.acquisition {
                    AcquisitionSpec::Local(path) => scopes.push(path.clone()),
                    AcquisitionSpec::Embedded { archive, .. } => scopes.push(archive.clone()),
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
        let captured = self.capture(selected, &scopes, limits, cancel)?;
        ensure!(
            documents.root.binding == captured.root.binding
                && documents.intent.raw_revision() == captured.intent.raw_revision()
                && documents.prior_lock.as_ref().map(DecodedLock::raw_revision)
                    == captured.prior_lock.as_ref().map(DecodedLock::raw_revision),
            "Project documents changed while selecting build inputs"
        );
        Ok(captured)
    }
    /// Documents are always in the read set. Extra scopes bind this operation's source and output inputs.
    pub fn capture(
        &self,
        selected: &Path,
        scopes: &[PortableRelPath],
        limits: SnapshotLimits,
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
        let native = root.capture(&scopes, limits, cancel)?;
        let intent_bytes = read_document(&root, &native, "empack.yml", cancel)?
            .context("Project has no empack.yml")?;
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
fn read_document(
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
