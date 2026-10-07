//! Read-only recovery planning binds the journal and the exact visible effects before approval.
use super::*;
use crate::engine::verification;
use empack_core::{
    digest::ContentId,
    files::{FilePermissions, FilePlan, ObservedPath},
};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryAction {
    Finish,
    Restore,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryKind {
    Publication,
    Creation,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryStatus {
    pub kind: RecoveryKind,
    pub operation: String,
    /// Finishing this journal already means completing a previously requested restoration.
    pub restoring: bool,
}
/// Private captured state cannot be deserialized into recovery authority.
pub struct PreparedRecovery {
    root: ObjectIdentity,
    revision: [u8; 32],
    status: RecoveryStatus,
    action: RecoveryAction,
    observed: BTreeMap<PortableRelPath, Option<FileObservation>>,
    files: FilePlan,
    scratch: u64,
}
impl PreparedRecovery {
    pub fn status(&self) -> &RecoveryStatus {
        &self.status
    }
    pub fn action(&self) -> RecoveryAction {
        self.action
    }
    pub fn files(&self) -> &FilePlan {
        &self.files
    }
    pub fn scratch_bytes(&self) -> u64 {
        self.scratch
    }
    pub(super) fn check(&self, root: &ProjectReadRoot, journal: &Journal) -> Result<()> {
        ensure!(
            root.binding == self.root && revision(journal)? == self.revision,
            "Recovery journal changed after preparation"
        );
        root.check_binding()?;
        for change in &journal.changes {
            let path = PortableRelPath::parse(&change.target, PathSyntax::ProjectContent)?;
            ensure!(
                self.observed.get(&path) == Some(&current_file(root, &path, change)?),
                "Recovery input changed after preparation: {}",
                path.as_str()
            );
        }
        Ok(())
    }
}
fn revision(journal: &Journal) -> Result<[u8; 32]> {
    Ok(Sha256::digest(serde_json::to_vec(journal)?).into())
}
pub(in crate::engine::publication) fn file_content(value: &Fingerprint) -> FileContent {
    FileContent {
        content: ContentId::from_sha256(value.sha256),
        bytes: value.bytes,
        permissions: FilePermissions {
            readonly: value.readonly,
            executable: value.executable,
        },
    }
}
impl Publisher {
    fn read_recovery(&self, root: &ProjectReadRoot) -> Result<Option<(Dir, File, Journal)>> {
        root.check_binding()?;
        let state = match self.host.open_dir_nofollow(root_key(root)?) {
            Ok(state) => state,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        native::reject_reparse(&state.try_clone()?.into_std_file())?;
        // A journal is published only after its coordination file exists. Never create one here.
        let lock = native::open_file(&state, "operation.lock")?;
        lock.try_lock_shared()
            .context("Project publication is busy")?;
        let Some(journal) = load_journal(&state)? else {
            return Ok(None);
        };
        validate_journal(&journal, root)?;
        if journal.committed {
            return Ok(None);
        }
        Ok(Some((state, lock, journal)))
    }
    pub fn inspect_recovery(&self, root: &ProjectReadRoot) -> Result<Option<RecoveryStatus>> {
        Ok(self
            .read_recovery(root)?
            .map(|(_, _lock, journal)| RecoveryStatus {
                kind: RecoveryKind::Publication,
                operation: journal.operation,
                restoring: journal.restoring,
            }))
    }
    /// Validate retained images and visible inputs without creating state or changing the project.
    pub fn prepare_recovery(
        &self,
        root: &ProjectReadRoot,
        action: RecoveryAction,
    ) -> Result<Option<PreparedRecovery>> {
        let Some((state, _lock, journal)) = self.read_recovery(root)? else {
            return Ok(None);
        };
        let retained = state.open_dir_nofollow(&journal.operation)?;
        native::reject_reparse(&retained.try_clone()?.into_std_file())?;
        preflight_recovery(
            root,
            &retained,
            &journal,
            action == RecoveryAction::Finish || journal.restoring,
        )?;
        let mut observations = BTreeMap::new();
        let mut observed = BTreeMap::new();
        let mut desired = BTreeMap::new();
        let mut removals = BTreeSet::new();
        let mut scratch = 0u64;
        for change in &journal.changes {
            let path = PortableRelPath::parse(&change.target, PathSyntax::ProjectContent)?;
            let target = ProjectLayout::classify(&path)?;
            let current = current_file(root, &path, change)?;
            observed.insert(
                target.clone(),
                current
                    .as_ref()
                    .map(|value| ObservedPath::File(content(value)))
                    .unwrap_or(ObservedPath::Absent),
            );
            observations.insert(path, current);
            let after = if action == RecoveryAction::Restore && !journal.restoring {
                &change.before
            } else {
                &change.after
            };
            if let Some(after) = after {
                desired.insert(target, file_content(after));
                let copies = if action == RecoveryAction::Restore && !journal.restoring {
                    2
                } else {
                    1
                };
                scratch = scratch
                    .checked_add(
                        after
                            .bytes
                            .checked_mul(copies)
                            .context("Recovery scratch size overflow")?,
                    )
                    .context("Recovery scratch size overflow")?;
            } else {
                removals.insert(target);
            }
        }
        let files = verification::plan_mutation_files(&observed, &desired, &removals)?;
        root.check_binding()?;
        Ok(Some(PreparedRecovery {
            root: root.binding,
            revision: revision(&journal)?,
            status: RecoveryStatus {
                kind: RecoveryKind::Publication,
                operation: journal.operation,
                restoring: journal.restoring,
            },
            action,
            observed: observations,
            files,
            scratch,
        }))
    }
    /// Recheck the approved journal and observations while holding exclusive publication ownership.
    pub fn recover_prepared(
        &self,
        root: &ProjectReadRoot,
        prepared: PreparedRecovery,
    ) -> Result<PublicationReceipt> {
        match prepared.action {
            RecoveryAction::Finish => self.recover_checked(root, Some(&prepared)),
            RecoveryAction::Restore => self.restore_checked(root, Some(&prepared), &mut |_| Ok(())),
        }
    }
}
