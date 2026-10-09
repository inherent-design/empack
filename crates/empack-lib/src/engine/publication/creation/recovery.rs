//! Creation recovery binds an absent destination or an already visible retained root.
use super::*;
use crate::engine::publication::{RecoveryKind, RecoveryStatus};

pub struct PreparedCreationRecovery {
    parent: ObjectIdentity,
    child: String,
    revision: [u8; 32],
    visible: bool,
    status: RecoveryStatus,
    files: FilePlan,
}
impl PreparedCreationRecovery {
    pub fn status(&self) -> &RecoveryStatus {
        &self.status
    }
    pub fn files(&self) -> &FilePlan {
        &self.files
    }
    pub(super) fn check(
        &self,
        parent: &ProjectReadRoot,
        child: &str,
        journal: &CreationJournal,
    ) -> Result<()> {
        parent.check_binding()?;
        ensure!(
            parent.binding == self.parent
                && child == self.child
                && revision(journal)? == self.revision,
            "Creation recovery changed after preparation"
        );
        let root = selected_root(parent, child)?;
        ensure!(
            root.is_some() == self.visible,
            "Creation destination changed after preparation"
        );
        if let Some(root) = root {
            ensure!(
                Binding::from(root.binding) == journal.binding,
                "Creation root changed after preparation"
            );
        }
        Ok(())
    }
}
struct ReadCreation {
    parent: ProjectReadRoot,
    child: String,
    journal: CreationJournal,
    visible: bool,
    _locks: Vec<File>,
}
fn revision(journal: &CreationJournal) -> Result<[u8; 32]> {
    Ok(Sha256::digest(serde_json::to_vec(journal)?).into())
}
fn selected_root(parent: &ProjectReadRoot, child: &str) -> Result<Option<ProjectReadRoot>> {
    match ProjectReadRoot::open_child(parent, child) {
        Ok(root) => Ok(Some(root)),
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) =>
        {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}
fn shared_lock(state: &Dir) -> Result<File> {
    let file = native::open_file(state, "operation.lock")?;
    file.try_lock_shared().context("Project creation is busy")?;
    Ok(file)
}
fn read(publisher: &Publisher, selected: &Path) -> Result<Option<ReadCreation>> {
    let (parent, child) = creation_selection(selected)?;
    let root = selected_root(&parent, &child)?;
    if let Some(root) = &root
        && let Some(state) = bound_state(publisher, &root.binding.into(), false)?
    {
        let guard = shared_lock(&state)?;
        if let Some(journal) = load_creation(&state)? {
            validate_creation_structure(&journal)?;
            ensure!(
                journal.binding == Binding::from(root.binding),
                "Creation journal belongs to another root"
            );
            if journal.committed {
                return Ok(None);
            }
            return Ok(Some(ReadCreation {
                parent,
                child,
                journal,
                visible: true,
                _locks: vec![guard],
            }));
        }
    }
    let Some(state) = creation_state(publisher, &parent, &child, false)? else {
        return Ok(None);
    };
    let mut guards = vec![shared_lock(&state)?];
    let Some(initial) = load_creation(&state)? else {
        return Ok(None);
    };
    validate_creation(&parent, &child, &initial)?;
    let journal = if let Some(state) = bound_state(publisher, &initial.binding, false)? {
        guards.push(shared_lock(&state)?);
        match load_creation(&state)? {
            Some(journal) => {
                match_creation(&initial, &journal)?;
                journal
            }
            None => initial,
        }
    } else {
        initial
    };
    if journal.committed {
        return Ok(None);
    }
    Ok(Some(ReadCreation {
        parent,
        child,
        journal,
        visible: root.is_some(),
        _locks: guards,
    }))
}
impl Publisher {
    pub fn inspect_creation_recovery(&self, selected: &Path) -> Result<Option<RecoveryStatus>> {
        Ok(read(self, selected)?.map(|read| RecoveryStatus {
            operation: read.journal.operation,
            restoring: false,
            kind: RecoveryKind::Creation,
        }))
    }
    pub fn prepare_creation_recovery(
        &self,
        selected: &Path,
    ) -> Result<Option<PreparedCreationRecovery>> {
        let Some(read) = read(self, selected)? else {
            return Ok(None);
        };
        let location = if read.visible {
            &read.child
        } else {
            &read.journal.candidate
        };
        verify_created_root(
            &read.parent,
            location,
            &read.journal,
            &Cancellation::default(),
        )?;
        let mut observed = BTreeMap::new();
        let mut desired = BTreeMap::new();
        for (path, expected) in &read.journal.expected {
            let target = ProjectLayout::classify(&PortableRelPath::parse(
                path,
                PathSyntax::ProjectContent,
            )?)?;
            let expected = crate::engine::publication::recovery::file_content(expected);
            observed.insert(
                target.clone(),
                if read.visible {
                    ObservedPath::File(expected.clone())
                } else {
                    ObservedPath::Absent
                },
            );
            desired.insert(target, expected);
        }
        let files = crate::engine::verification::plan_mutation_files(
            &observed,
            &desired,
            &BTreeSet::new(),
        )?;
        Ok(Some(PreparedCreationRecovery {
            parent: read.parent.binding,
            child: read.child,
            revision: revision(&read.journal)?,
            visible: read.visible,
            status: RecoveryStatus {
                operation: read.journal.operation,
                restoring: false,
                kind: RecoveryKind::Creation,
            },
            files,
        }))
    }
    pub fn recover_prepared_creation(
        &self,
        selected: &Path,
        prepared: PreparedCreationRecovery,
    ) -> Result<PublicationReceipt> {
        self.recover_new_checked(selected, Some(&prepared))
    }
}
