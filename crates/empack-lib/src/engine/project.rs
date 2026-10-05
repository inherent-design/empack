//! Read-only normalized project preparation, gated by native publication recovery state.
use super::{
    documents::{DecodedIntent, DecodedLock, DocumentCodec, MAX_DOCUMENT_BYTES},
    io::copy_bounded,
    native,
    publication::RecoveryReader,
    snapshot::{NativeSnapshot, Observation, ProjectReadRoot, SnapshotLimits},
};
use crate::application::process_runtime::Cancellation;
use anyhow::{Context, Result, ensure};
use empack_core::{
    model::ResolvedProject,
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
}
/// Receives only read-only journal access. No cache, tool or publication capability is present.
pub struct ProjectReader {
    recovery: RecoveryReader,
}
impl ProjectReader {
    pub fn new(recovery: RecoveryReader) -> Self {
        Self { recovery }
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
