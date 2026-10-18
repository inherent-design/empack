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

pub(crate) fn public_keys(values: &[String]) -> Result<Vec<ed25519_dalek::VerifyingKey>> {
    ensure!(
        values.len() <= 16,
        "Select at most sixteen previous public keys"
    );
    values
        .iter()
        .map(|value| {
            Ok(ed25519_dalek::VerifyingKey::from_bytes(
                &super::decode_hex::<32>(value)?,
            )?)
        })
        .collect()
}

/// External secret setup never enters a project mutation, export, or publication stage.
/// The directory handle fixes the selected parent; exclusive linking never replaces a key.
pub(crate) struct PublisherSetup {
    directory: cap_std::fs::Dir,
    name: String,
}
impl PublisherSetup {
    pub(crate) fn prepare(root: &Path, file: &Path) -> Result<Self> {
        let root = std::fs::canonicalize(root).context("Publisher workspace must already exist")?;
        ensure!(root.is_dir(), "Publisher workspace must be a directory");
        let parent = std::fs::canonicalize(file.parent().context("Key needs a parent directory")?)?;
        ensure!(
            !parent.starts_with(&root),
            "Signing keys must remain outside publisher and export roots"
        );
        let name = file
            .file_name()
            .and_then(|name| name.to_str())
            .context("Key needs a portable filename")?
            .to_owned();
        empack_core::path::PortableRelPath::parse(
            &name,
            empack_core::path::PathSyntax::ArtifactName,
        )?;
        let directory = crate::engine::publication::open_private_directory(&parent, false)
            .context("Signing key parent must be an existing private directory")?;
        match directory.symlink_metadata(&name) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
            Ok(_) => anyhow::bail!("Signing key path already exists; refusing to overwrite"),
        }
        Ok(Self { directory, name })
    }
    pub(crate) fn create(
        self,
        cancel: &crate::application::process_runtime::Cancellation,
    ) -> Result<(String, String)> {
        use std::io::Write;
        cancel.check()?;
        let mut seed = [0; 32];
        getrandom::fill(&mut seed)
            .map_err(|error| anyhow::anyhow!("Native key entropy unavailable: {error}"))?;
        let key = SigningKey::from_bytes(&seed);
        seed.fill(0);
        let (candidate, mut file) = crate::engine::staging::create_temporary(&self.directory)?;
        let result: Result<(String, String)> = (|| {
            writeln!(file, "{}", super::hex(key.as_bytes()))?;
            file.sync_all()?;
            cancel.check()?;
            self.directory
                .hard_link(&candidate, &self.directory, &self.name)
                .context(
                    "Cannot create signing key exclusively; existing paths are never replaced",
                )?;
            #[cfg(unix)]
            crate::engine::native::readable_directory(&self.directory)
                .and_then(|directory| directory.sync_all().map_err(Into::into))
                .context("Signing key was created, but its directory could not be synchronized")?;
            Ok((
                super::hex(key.verifying_key().as_bytes()),
                super::trust::key_id(&key.verifying_key()),
            ))
        })();
        drop(file);
        // Only the owned candidate is retired, never the destination (including on failure).
        let cleanup = self.directory.remove_file(candidate);
        match (result, cleanup) {
            (Ok(keys), Ok(())) => Ok(keys),
            (Err(error), Ok(())) => Err(error),
            (Ok(_), Err(error)) => Err(anyhow::Error::from(error).context(
                "Signing key was created, but its private temporary link could not be removed",
            )),
            (Err(error), Err(cleanup)) => Err(error.context(format!(
                "Private signing-key temporary link also could not be removed: {cleanup}",
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::process_runtime::Cancellation;
    use ed25519_dalek::{Signer, Verifier};

    #[test]
    fn publisher_setup_is_exclusive_private_and_signs_round_trip() {
        let workspace = tempfile::tempdir().unwrap();
        let secrets = tempfile::tempdir().unwrap();
        let private = secrets.path().join("private");
        crate::engine::publication::open_private_directory(&private, true).unwrap();
        let path = private.join("publisher.key");
        let prepared = PublisherSetup::prepare(workspace.path(), &path).unwrap();
        assert!(!path.exists());
        let (public, fingerprint) = prepared.create(&Cancellation::default()).unwrap();
        let keys = read_keys(
            workspace.path(),
            workspace.path(),
            std::slice::from_ref(&path),
            &Cancellation::default(),
        )
        .unwrap();
        assert_eq!(
            public,
            super::super::hex(keys[0].verifying_key().as_bytes())
        );
        assert_eq!(
            fingerprint,
            super::super::trust::key_id(&keys[0].verifying_key())
        );
        let signature = keys[0].sign(b"publisher setup round trip");
        keys[0]
            .verifying_key()
            .verify(b"publisher setup round trip", &signature)
            .unwrap();
        let before = std::fs::read(&path).unwrap();
        assert!(PublisherSetup::prepare(workspace.path(), &path).is_err());
        assert_eq!(before, std::fs::read(&path).unwrap());
        assert!(
            PublisherSetup::prepare(workspace.path(), &workspace.path().join("private.key"))
                .is_err()
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }
    #[test]
    fn publisher_setup_late_destination_is_never_removed() {
        let workspace = tempfile::tempdir().unwrap();
        let secrets = tempfile::tempdir().unwrap();
        let private = secrets.path().join("private");
        crate::engine::publication::open_private_directory(&private, true).unwrap();
        let path = private.join("publisher.key");
        let prepared = PublisherSetup::prepare(workspace.path(), &path).unwrap();
        std::fs::write(&path, b"preexisting key").unwrap();
        assert!(prepared.create(&Cancellation::default()).is_err());
        assert_eq!(std::fs::read(path).unwrap(), b"preexisting key");
        assert_eq!(std::fs::read_dir(&private).unwrap().count(), 1);
    }
}
