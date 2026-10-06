//! Publish a complete new root with one no-replace directory rename.
use super::*;
use crate::engine::{project::NewProjectSnapshot, staging::FrozenStage};
use empack_core::files::{FilePlan, ManagedPath, ObservedPath};
use sha2::{Digest, Sha256};

struct CandidateCleanup(Option<Dir>);
impl Drop for CandidateCleanup {
    fn drop(&mut self) {
        if let Some(directory) = self.0.take() {
            // This handle owns newly created private scratch, never a selected project tree.
            let _ = directory.remove_open_dir_all();
        }
    }
}

/// Only complete, already verified create-only file plans can become new project roots.
pub struct PreparedRootCreation {
    target: NewProjectSnapshot,
    plan: FilePlan,
    stage: FrozenStage,
}
impl PreparedRootCreation {
    pub(crate) fn from_verified(
        target: NewProjectSnapshot,
        change: VerifiedFileChange,
    ) -> Result<Self> {
        let (plan, stage, _) = change.into_parts();
        ensure!(
            plan.expected().contains_key(&ManagedPath::IntentDocument)
                && plan.expected().contains_key(&ManagedPath::LockDocument),
            "New root requires complete project documents"
        );
        ensure!(
            plan.changes().len() == plan.expected().len()
                && plan.changes().iter().all(|change| matches!(
                    change,
                    FileChange::Replace {
                        before: ObservedPath::Absent,
                        ..
                    }
                )),
            "New root requires a complete create-only candidate"
        );
        Ok(Self {
            target,
            plan,
            stage,
        })
    }
    pub fn plan(&self) -> &FilePlan {
        &self.plan
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CreationJournal {
    schema: u32,
    parent: Binding,
    child: String,
    operation: String,
    candidate: String,
    binding: Binding,
    expected: BTreeMap<String, Fingerprint>,
    limits: SnapshotLimits,
    committed: bool,
}
impl Publisher {
    pub fn publish_new(
        &self,
        prepared: PreparedRootCreation,
        cancel: &Cancellation,
    ) -> Result<PublicationReceipt> {
        self.publish_new_with_hook(prepared, cancel, &mut |_| Ok(()))
    }
    fn publish_new_with_hook(
        &self,
        prepared: PreparedRootCreation,
        cancel: &Cancellation,
        hook: &mut dyn FnMut(PublicationPoint) -> Result<()>,
    ) -> Result<PublicationReceipt> {
        let PreparedRootCreation {
            target,
            plan,
            mut stage,
        } = prepared;
        target.revalidate(cancel)?;
        let state = creation_state(self, &target.parent, target.child().as_str(), true)?.unwrap();
        let _lock = lock(&state)?;
        if let Some(previous) = load_creation(&state)? {
            validate_creation(&target.parent, target.child().as_str(), &previous)?;
            ensure!(
                authoritative_creation(self, &previous)?
                    .is_some_and(|(_, record)| record.committed),
                "New project has retained creation state; inspect recovery before retrying"
            );
        }
        target.parent.revalidate(&target.absence, cancel)?;
        let operation = format!(
            "op-{:x}-{:x}-{:x}",
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos(),
            std::process::id(),
            NEXT_OPERATION.fetch_add(1, Ordering::Relaxed)
        );
        let candidate = format!(".empack-create-{operation}");
        target.parent.directory.create_dir(&candidate)?;
        let directory = target.parent.directory.open_dir_nofollow(&candidate)?;
        let mut cleanup = CandidateCleanup(Some(directory.try_clone()?));
        #[cfg(windows)]
        crate::engine::windows_privacy::protect_empty_temporary(&directory)?;
        private_directory(&directory)?;
        let binding = native::directory_identity(&directory)?;
        ensure!(
            binding.created.is_some(),
            "Filesystem lacks durable directory creation identity"
        );
        // Candidate identity and explicit expected paths delimit this owned scratch tree.
        let expected = plan
            .expected()
            .iter()
            .map(|(path, file)| {
                Ok((
                    ProjectLayout::path(path)?.as_str().to_owned(),
                    Fingerprint::from(file),
                ))
            })
            .collect::<Result<BTreeMap<_, _>>>()?;
        let limits = creation_limits(&expected)?;
        for (name, expected) in &expected {
            cancel.check()?;
            let path = PortableRelPath::parse(name, PathSyntax::ProjectContent)?;
            let (parent, leaf) = publication_parent(&directory, &path)?;
            let mut output = new_retained_file(&parent, &leaf)?;
            stage.copy_verified(&path, &mut output, cancel)?;
            let mut permissions = output.metadata()?.permissions();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                permissions.set_mode(
                    if expected.readonly { 0o400 } else { 0o600 }
                        | if expected.executable { 0o100 } else { 0 },
                );
            }
            #[cfg(not(unix))]
            permissions.set_readonly(expected.readonly);
            output.set_permissions(permissions)?;
            output.sync_all()?;
            sync_directory(&parent)?;
        }
        sync_directory(&directory)?;
        sync_directory(&target.parent.directory)?;
        let mut journal = CreationJournal {
            schema: 1,
            parent: target.parent.binding.into(),
            child: target.child().as_str().into(),
            operation,
            candidate,
            binding: binding.into(),
            expected,
            limits,
            committed: false,
        };
        verify_created_root(&target.parent, &journal.candidate, &journal, cancel)?;
        hook(PublicationPoint::RecoveryDataDurable)?;
        target.parent.revalidate(&target.absence, cancel)?;
        // Once durable intent exists, cancellation is deferred until the root is recoverable.
        // A failed journal sync can still have published its intent file. Retain recovery bytes
        // before attempting that boundary; only pre-intent failures discard private scratch.
        let root_state = bound_state(self, &journal.binding, true)?.unwrap();
        let _root_lock = lock(&root_state)?;
        cleanup.0.take();
        write_creation(&state, &journal).with_context(|| RecoveryRequired {
            operation: journal.operation.clone(),
        })?;
        hook(PublicationPoint::CreationIndexDurable).with_context(|| RecoveryRequired {
            operation: journal.operation.clone(),
        })?;
        write_creation(&root_state, &journal).with_context(|| RecoveryRequired {
            operation: journal.operation.clone(),
        })?;
        hook(PublicationPoint::IntentDurable).with_context(|| RecoveryRequired {
            operation: journal.operation.clone(),
        })?;
        finish_creation(&target.parent, &root_state, &mut journal, hook).with_context(|| {
            RecoveryRequired {
                operation: journal.operation.clone(),
            }
        })
    }

    /// Complete retained new-root publication without reacquiring bytes or rerunning tools.
    pub fn recover_new(&self, selected: &Path) -> Result<PublicationReceipt> {
        let (parent, child) = creation_selection(selected)?;
        let state = creation_state(self, &parent, &child, false)?
            .context("No retained project creation")?;
        let _lock = lock(&state)?;
        let initial = load_creation(&state)?.context("No retained project creation")?;
        validate_creation(&parent, &child, &initial)?;
        let root_state = bound_state(self, &initial.binding, true)?.unwrap();
        let _root_lock = lock(&root_state)?;
        let mut journal = match load_creation(&root_state)? {
            Some(record) => {
                match_creation(&initial, &record)?;
                record
            }
            None => {
                write_creation(&root_state, &initial)?;
                initial
            }
        };
        finish_creation(&parent, &root_state, &mut journal, &mut |_| Ok(())).with_context(|| {
            RecoveryRequired {
                operation: journal.operation.clone(),
            }
        })
    }
}
impl RecoveryReader {
    pub(in crate::engine) fn enter_new(
        &self,
        parent: &ProjectReadRoot,
        child: &str,
    ) -> Result<ProjectReadGuard> {
        let empty = || ProjectReadGuard { _lock: None };
        let Some(publisher) = Publisher::open_existing(&self.host_state)? else {
            return Ok(empty());
        };
        let Some(state) = creation_state(&publisher, parent, child, false)? else {
            return Ok(empty());
        };
        let file = match native::open_file(&state, "operation.lock") {
            Ok(file) => Some(file),
            Err(error)
                if error
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) =>
            {
                None
            }
            Err(error) => return Err(error),
        };
        if let Some(file) = &file {
            file.try_lock_shared().context("Project creation is busy")?;
        }
        if let Some(journal) = load_creation(&state)? {
            validate_creation(parent, child, &journal)?;
            ensure!(
                authoritative_creation(&publisher, &journal)?
                    .is_some_and(|(_, record)| record.committed),
                "Project requires creation recovery before ordinary reads"
            );
        }
        Ok(ProjectReadGuard { _lock: file })
    }
}
fn match_creation(initial: &CreationJournal, record: &CreationJournal) -> Result<()> {
    let mut expected = initial.clone();
    expected.committed = record.committed;
    ensure!(
        expected == *record,
        "Creation journal identity or inventory changed"
    );
    Ok(())
}
fn authoritative_creation(
    publisher: &Publisher,
    initial: &CreationJournal,
) -> Result<Option<(Dir, CreationJournal)>> {
    let Some(state) = bound_state(publisher, &initial.binding, false)? else {
        return Ok(None);
    };
    let Some(record) = load_creation(&state)? else {
        return Ok(None);
    };
    match_creation(initial, &record)?;
    Ok(Some((state, record)))
}
pub(super) fn requires_root_recovery(state: &Dir, root: &ProjectReadRoot) -> Result<bool> {
    let Some(journal) = load_creation(state)? else {
        return Ok(false);
    };
    validate_creation_structure(&journal)?;
    ensure!(
        journal.binding == Binding::from(root.binding),
        "Creation journal belongs to another root"
    );
    Ok(!journal.committed)
}
fn bound_state(publisher: &Publisher, binding: &Binding, create: bool) -> Result<Option<Dir>> {
    state_by_key(publisher, &binding_key(binding)?, create)
}

fn creation_selection(selected: &Path) -> Result<(ProjectReadRoot, String)> {
    ensure!(selected.is_absolute(), "Project selection must be absolute");
    let child = selected
        .file_name()
        .and_then(|name| name.to_str())
        .context("Project child name is not portable")?;
    PortableRelPath::parse(child, PathSyntax::ArtifactName)?;
    Ok((
        ProjectReadRoot::open(
            selected
                .parent()
                .context("Project requires an existing parent")?,
        )?,
        child.into(),
    ))
}
fn creation_key(parent: &ProjectReadRoot, child: &str) -> Result<String> {
    Ok(format!(
        "create-{}-{}",
        root_key(parent)?,
        empack_core::digest::ExpectedDigest::Sha256(Sha256::digest(child.as_bytes()).into()).hex()
    ))
}
fn creation_state(
    publisher: &Publisher,
    parent: &ProjectReadRoot,
    child: &str,
    create: bool,
) -> Result<Option<Dir>> {
    state_by_key(publisher, &creation_key(parent, child)?, create)
}
fn state_by_key(publisher: &Publisher, key: &str, create: bool) -> Result<Option<Dir>> {
    if create {
        match publisher.host.create_dir(key) {
            Ok(()) => sync_directory(&publisher.host)?,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
    }
    let state = match publisher.host.open_dir_nofollow(key) {
        Ok(state) => state,
        Err(error) if !create && error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if create {
        private_directory(&state)?;
    }
    Ok(Some(state))
}
fn write_creation(state: &Dir, journal: &CreationJournal) -> Result<()> {
    let bytes = serde_json::to_vec(journal)?;
    ensure!(
        bytes.len() as u64 <= JOURNAL_LIMIT,
        "Creation journal exceeds size limit"
    );
    let (name, mut output) = create_temporary(state)?;
    let result = (|| {
        output.write_all(&bytes)?;
        output.sync_all()?;
        state.rename(&name, state, "creation.json")?;
        sync_directory(state)
    })();
    if result.is_err() {
        let _ = state.remove_file(&name);
    }
    result
}
fn load_creation(state: &Dir) -> Result<Option<CreationJournal>> {
    use std::io::Read;
    let file = match native::open_file(state, "creation.json") {
        Ok(file) => file,
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) =>
        {
            return Ok(None);
        }
        Err(error) => return Err(error),
    };
    ensure!(
        file.metadata()?.len() <= JOURNAL_LIMIT,
        "Creation journal exceeds size limit"
    );
    let mut bytes = Vec::new();
    file.take(JOURNAL_LIMIT + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= JOURNAL_LIMIT,
        "Creation journal exceeds size limit"
    );
    Ok(Some(serde_json::from_slice(&bytes)?))
}
fn validate_creation(
    parent: &ProjectReadRoot,
    child: &str,
    journal: &CreationJournal,
) -> Result<()> {
    ensure!(
        journal.schema == 1
            && journal.parent == Binding::from(parent.binding)
            && journal.child == child,
        "Creation journal target mismatch"
    );
    validate_creation_structure(journal)
}
fn validate_creation_structure(journal: &CreationJournal) -> Result<()> {
    ensure!(journal.schema == 1, "Unknown creation journal schema");
    PortableRelPath::parse(&journal.child, PathSyntax::ArtifactName)?;
    PortableRelPath::parse(&journal.operation, PathSyntax::ArtifactName)?;
    ensure!(
        journal.operation.starts_with("op-")
            && journal.candidate == format!(".empack-create-{}", journal.operation),
        "Invalid creation candidate ownership"
    );
    ensure!(
        journal.expected.contains_key("empack.yml") && journal.expected.contains_key("empack.lock"),
        "Creation journal lacks project documents"
    );
    let mut collisions = CollisionIndex::default();
    for name in journal.expected.keys() {
        let path = PortableRelPath::parse(name, PathSyntax::ProjectContent)?;
        ProjectLayout::classify(&path)?;
        collisions.insert_file(&path)?;
    }
    let limits = creation_limits(&journal.expected)?;
    ensure!(
        journal.limits.entries == limits.entries
            && journal.limits.depth == limits.depth
            && journal.limits.file_bytes == limits.file_bytes
            && journal.limits.total_bytes == limits.total_bytes,
        "Creation observation limits differ from expected inventory"
    );
    Ok(())
}
fn creation_limits(expected: &BTreeMap<String, Fingerprint>) -> Result<SnapshotLimits> {
    let mut paths = BTreeSet::new();
    let mut bytes = 0u64;
    let mut largest = 0;
    let mut depth = 0;
    for (name, file) in expected {
        let mut prefix = String::new();
        for (index, part) in name.split('/').enumerate() {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(part);
            paths.insert(prefix.clone());
            depth = depth.max(index + 1);
        }
        bytes = bytes
            .checked_add(file.bytes)
            .context("Creation size overflow")?;
        largest = largest.max(file.bytes);
    }
    Ok(SnapshotLimits {
        entries: paths.len(),
        depth,
        file_bytes: largest,
        total_bytes: bytes,
    })
}
fn verify_created_root(
    parent: &ProjectReadRoot,
    child: &str,
    journal: &CreationJournal,
    cancel: &Cancellation,
) -> Result<()> {
    parent.check_binding()?;
    let root = ProjectReadRoot::open_child(parent, child)?;
    ensure!(
        Binding::from(root.binding) == journal.binding,
        "Created root identity changed"
    );
    let tops: BTreeSet<_> = journal
        .expected
        .keys()
        .map(|path| path.split('/').next().unwrap().to_owned())
        .collect();
    let actual: BTreeSet<_> = root
        .directory
        .entries()?
        .map(|entry| {
            entry?
                .file_name()
                .into_string()
                .map_err(|_| std::io::Error::other("Nonportable created filename"))
        })
        .collect::<std::io::Result<_>>()?;
    ensure!(actual == tops, "Created root membership changed");
    let scopes = tops
        .iter()
        .map(|name| {
            PortableRelPath::parse(name, PathSyntax::ProjectContent).map_err(anyhow::Error::from)
        })
        .collect::<Result<Vec<_>>>()?;
    let snapshot = root.capture(&scopes, journal.limits, cancel)?;
    let files: BTreeMap<_, _> = snapshot
        .entries()
        .iter()
        .filter_map(|(path, observation)| {
            if let Observation::File(file) = observation {
                Some((path.as_str().to_owned(), Fingerprint::from(&content(file))))
            } else {
                None
            }
        })
        .collect();
    ensure!(
        files.len() == journal.expected.len()
            && files
                .iter()
                .all(|(path, file)| fingerprints_match(Some(file), journal.expected.get(path))),
        "Created root files changed"
    );
    Ok(())
}
fn finish_creation(
    parent: &ProjectReadRoot,
    state: &Dir,
    journal: &mut CreationJournal,
    hook: &mut dyn FnMut(PublicationPoint) -> Result<()>,
) -> Result<PublicationReceipt> {
    validate_creation(parent, &journal.child, journal)?;
    parent.check_binding()?;
    let target = match parent.directory.open_dir_nofollow(&journal.child) {
        Ok(directory) => Some(directory),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    if let Some(target) = target {
        native::reject_reparse(&target.try_clone()?.into_std_file())?;
        ensure!(
            Binding::from(native::directory_identity(&target)?) == journal.binding,
            "Creation destination is occupied by another object"
        );
    } else {
        ensure!(!journal.committed, "Committed project root disappeared");
        verify_created_root(
            parent,
            &journal.candidate,
            journal,
            &Cancellation::default(),
        )?;
        let candidate = parent.directory.open_dir_nofollow(&journal.candidate)?;
        let identity = native::directory_identity(&candidate)?;
        ensure!(
            Binding::from(identity) == journal.binding,
            "Creation candidate changed before publication"
        );
        native::rename_new_directory(
            &parent.directory,
            &journal.candidate,
            &journal.child,
            identity,
        )?;
        hook(PublicationPoint::TargetChanged)?;
    }
    if !journal.committed {
        verify_created_root(parent, &journal.child, journal, &Cancellation::default())?;
        sync_directory(&parent.directory)?;
        hook(PublicationPoint::DirectorySynced)?;
        journal.committed = true;
        write_creation(state, journal)?;
        hook(PublicationPoint::Committed)?;
    }
    Ok(PublicationReceipt {
        disposition: PublicationDisposition::Published,
        operation: journal.operation.clone(),
        changed_files: journal.expected.len(),
        directory_synced: cfg!(unix),
        executable_bits_verified: cfg!(unix),
    })
}

#[cfg(test)]
mod tests;
