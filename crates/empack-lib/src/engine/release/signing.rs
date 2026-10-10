//! Read explicit publisher secrets through a bounded native observation.
use super::*;
use ed25519_dalek::SigningKey;
use std::path::{Path, PathBuf};

pub(crate) fn read_keys(
    root: &Path,
    source: &Path,
    files: &[PathBuf],
    cancel: &crate::application::process_runtime::Cancellation,
) -> Result<Vec<SigningKey>> {
    use crate::engine::{
        project::read_document_limited,
        snapshot::{ProjectReadRoot, SnapshotLimits},
    };
    use empack_core::path::{PathSyntax, PortableRelPath};
    ensure!(
        !files.is_empty() && files.len() <= 16,
        "Select one to sixteen signing keys"
    );
    let root = std::fs::canonicalize(root)?;
    let source = std::fs::canonicalize(source)?;
    let mut keys = Vec::new();
    for file in files {
        cancel.check()?;
        let resolved = std::fs::canonicalize(file)?;
        ensure!(
            !resolved.starts_with(&root) && !resolved.starts_with(&source),
            "Signing keys must remain outside project and export roots"
        );
        let parent = ProjectReadRoot::open(file.parent().context("Signing key needs a parent")?)?;
        let name = file
            .file_name()
            .and_then(|name| name.to_str())
            .context("Signing key needs a portable filename")?;
        let path = PortableRelPath::parse(name, PathSyntax::ArtifactName)?;
        let snapshot = parent.capture(
            &[path],
            SnapshotLimits {
                file_bytes: 128,
                total_bytes: 128,
                entries: 1,
                depth: 1,
            },
            cancel,
        )?;
        let encoded = read_document_limited(&parent, &snapshot, name, 128, cancel)?
            .context("Signing key is missing")?;
        let seed = super::decode_hex::<32>(
            std::str::from_utf8(&encoded)
                .context("Signing key must be hexadecimal")?
                .trim(),
        )
        .context("Signing key must contain exactly 64 lowercase hexadecimal characters")?;
        keys.push(SigningKey::from_bytes(&seed));
        parent.revalidate(&snapshot, cancel)?;
    }
    Ok(keys)
}
