//! Explicit ownership survives interruption before publication intent exists.
use super::*;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Preparation {
    schema: u32,
    root: Binding,
    operation: String,
    files: BTreeSet<String>,
}

pub(super) fn record_preparation(
    state: &Dir,
    root: &ProjectReadRoot,
    operation: &str,
    files: BTreeSet<String>,
) -> Result<()> {
    write_record(
        state,
        "retention.json",
        &Preparation {
            schema: 1,
            root: root.binding.into(),
            operation: operation.into(),
            files,
        },
    )
}

// Call only with publication ownership. A matching journal supersedes this preparation
// descriptor: its copies may still be needed for recovery and must remain untouched.
pub(super) fn reclaim_preparation(state: &Dir, root: &ProjectReadRoot) -> Result<()> {
    let mut file = match native::open_file(state, "retention.json") {
        Ok(file) => file,
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) =>
        {
            return Ok(());
        }
        Err(error) => return Err(error),
    };
    let mut bytes = Vec::new();
    copy_bounded(
        &mut file,
        &mut bytes,
        JOURNAL_LIMIT,
        &Cancellation::default(),
    )?;
    drop(file);
    let record: Preparation = serde_json::from_slice(&bytes).context("Corrupt retention record")?;
    ensure!(
        record.schema == 1 && record.root == Binding::from(root.binding),
        "Retention root or schema mismatch"
    );
    let journal = load_journal(state)?;
    if let Some(journal) = &journal {
        validate_journal(journal, root)?;
    }
    if !journal.is_some_and(|journal| journal.operation == record.operation) {
        reclaim(state, &record.operation, &record.files)?;
    }
    state.remove_file("retention.json")?;
    sync_directory(state)
}

pub(super) fn reclaim(state: &Dir, operation: &str, files: &BTreeSet<String>) -> Result<u64> {
    PortableRelPath::parse(operation, PathSyntax::ArtifactName)?;
    ensure!(operation.starts_with("op-"), "Invalid retention operation");
    for name in files {
        PortableRelPath::parse(name, PathSyntax::ArtifactName)?;
        let (kind, index) = name
            .split_once('-')
            .context("Invalid retained object name")?;
        ensure!(
            matches!(kind, "before" | "after" | "restore") && index.parse::<usize>().is_ok(),
            "Invalid retained object name"
        );
    }
    let retained = match state.open_dir_nofollow(operation) {
        Ok(directory) => directory,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error.into()),
    };
    native::reject_reparse(&retained.try_clone()?.into_std_file())?;
    let mut bytes = 0u64;
    for name in files {
        match retained.symlink_metadata(name) {
            Ok(metadata) => {
                ensure!(
                    metadata.is_file() && !metadata.file_type().is_symlink(),
                    "Retained journal object is not a file"
                );
                retained.remove_file(name)?;
                bytes = bytes
                    .checked_add(metadata.len())
                    .context("Reclaimed byte count overflow")?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    sync_directory(&retained)?;
    drop(retained);
    // Unexpected objects block retirement; their presence never authorizes recursive deletion.
    state.remove_dir(operation)?;
    sync_directory(state)?;
    Ok(bytes)
}
