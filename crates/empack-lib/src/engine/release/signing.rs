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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum KeyCreationPoint {
    Write,
    FileSync,
    Link,
    #[cfg(unix)]
    DirectorySync,
    Cleanup,
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
        self.create_with_hook(cancel, &mut |_| Ok(()))
    }

    // Private checkpoints mirror the native operations. The ordinary caller
    // always supplies a no-op; no product configuration enables fault injection.
    fn create_with_hook(
        self,
        cancel: &crate::application::process_runtime::Cancellation,
        hook: &mut dyn FnMut(KeyCreationPoint) -> Result<()>,
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
            hook(KeyCreationPoint::Write)?;
            writeln!(file, "{}", super::hex(key.as_bytes()))?;
            hook(KeyCreationPoint::FileSync)?;
            file.sync_all()?;
            cancel.check()?;
            hook(KeyCreationPoint::Link)
                .and_then(|()| {
                    self.directory
                        .hard_link(&candidate, &self.directory, &self.name)
                        .map_err(Into::into)
                })
                .context(
                    "Cannot create signing key exclusively; existing paths are never replaced",
                )?;
            #[cfg(unix)]
            hook(KeyCreationPoint::DirectorySync)
                .and_then(|()| crate::engine::native::readable_directory(&self.directory))
                .and_then(|directory| directory.sync_all().map_err(Into::into))
                .context("Signing key was created, but its directory could not be synchronized")?;
            Ok((
                super::hex(key.verifying_key().as_bytes()),
                super::trust::key_id(&key.verifying_key()),
            ))
        })();
        drop(file);
        // Only the owned candidate is retired, never the destination (including on failure).
        let cleanup = hook(KeyCreationPoint::Cleanup)
            .and_then(|()| self.directory.remove_file(candidate).map_err(Into::into));
        match (result, cleanup) {
            (Ok(keys), Ok(())) => Ok(keys),
            (Err(error), Ok(())) => Err(error),
            (Ok(_), Err(error)) => Err(error.context(
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
    use std::fs;

    fn private_failure_fixture() -> (tempfile::TempDir, tempfile::TempDir, PathBuf, PathBuf) {
        let workspace = tempfile::tempdir().unwrap();
        let secrets = tempfile::tempdir().unwrap();
        let private = secrets.path().join("private");
        crate::engine::publication::open_private_directory(&private, true).unwrap();
        fs::write(private.join("unrelated"), b"keep other private work").unwrap();
        let key = private.join("publisher.key");
        (workspace, secrets, private, key)
    }

    fn assert_private_work_preserved(workspace: &Path, private: &Path) {
        assert_eq!(fs::read_dir(workspace).unwrap().count(), 0);
        assert_eq!(
            fs::read(private.join("unrelated")).unwrap(),
            b"keep other private work"
        );
    }

    #[test]
    fn publisher_setup_write_sync_and_link_failures_retire_only_the_private_candidate() {
        for fault in [
            KeyCreationPoint::Write,
            KeyCreationPoint::FileSync,
            KeyCreationPoint::Link,
        ] {
            let (workspace, _secrets, private, path) = private_failure_fixture();
            let prepared = PublisherSetup::prepare(workspace.path(), &path).unwrap();
            let mut reached = false;
            let error = prepared
                .create_with_hook(&Cancellation::default(), &mut |point| {
                    if point == fault {
                        reached = true;
                        anyhow::bail!("controlled private key operation failure");
                    }
                    Ok(())
                })
                .unwrap_err();
            assert!(reached);
            assert!(format!("{error:#}").contains("controlled private key operation failure"));
            assert!(!path.exists());
            assert_eq!(fs::read_dir(&private).unwrap().count(), 1);
            assert_private_work_preserved(workspace.path(), &private);
        }
    }

    #[cfg(unix)]
    #[test]
    fn publisher_setup_directory_sync_failure_preserves_created_key_and_reports_durability() {
        let (workspace, _secrets, private, path) = private_failure_fixture();
        let error = PublisherSetup::prepare(workspace.path(), &path)
            .unwrap()
            .create_with_hook(&Cancellation::default(), &mut |point| {
                if point == KeyCreationPoint::DirectorySync {
                    anyhow::bail!("controlled directory synchronization failure");
                }
                Ok(())
            })
            .unwrap_err();
        let message = format!("{error:#}");
        assert!(
            message
                .contains("Signing key was created, but its directory could not be synchronized")
        );
        assert!(message.contains("controlled directory synchronization failure"));
        let encoded = fs::read_to_string(&path).unwrap();
        assert_eq!(encoded.len(), 65);
        assert!(!message.contains(encoded.trim()));
        assert_eq!(
            read_keys(
                workspace.path(),
                workspace.path(),
                &[path],
                &Cancellation::default()
            )
            .unwrap()
            .len(),
            1
        );
        assert_eq!(fs::read_dir(&private).unwrap().count(), 2);
        assert_private_work_preserved(workspace.path(), &private);
    }

    #[test]
    fn publisher_setup_cleanup_failure_preserves_key_state_and_primary_error() {
        for primary_failure in [false, true] {
            let (workspace, _secrets, private, path) = private_failure_fixture();
            let error = PublisherSetup::prepare(workspace.path(), &path)
                .unwrap()
                .create_with_hook(&Cancellation::default(), &mut |point| {
                    if primary_failure && point == KeyCreationPoint::FileSync {
                        anyhow::bail!("controlled primary synchronization failure");
                    }
                    if point == KeyCreationPoint::Cleanup {
                        anyhow::bail!("controlled temporary cleanup failure");
                    }
                    Ok(())
                })
                .unwrap_err();
            let message = format!("{error:#}");
            assert!(message.contains("controlled temporary cleanup failure"));
            assert_eq!(path.exists(), !primary_failure);
            let candidates: Vec<_> = fs::read_dir(&private)
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .filter(|entry| {
                    entry.file_name().unwrap() != "publisher.key"
                        && entry.file_name().unwrap() != "unrelated"
                })
                .collect();
            assert_eq!(candidates.len(), 1);
            let encoded = fs::read_to_string(&candidates[0]).unwrap();
            assert_eq!(encoded.len(), 65);
            assert!(!message.contains(encoded.trim()));
            if primary_failure {
                assert!(message.contains("controlled primary synchronization failure"));
                assert!(
                    message
                        .contains("Private signing-key temporary link also could not be removed")
                );
            } else {
                assert!(message.contains(
                    "Signing key was created, but its private temporary link could not be removed"
                ));
                assert!(fs::read(&path).unwrap() == encoded.as_bytes());
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                assert_eq!(
                    fs::metadata(&candidates[0]).unwrap().permissions().mode() & 0o777,
                    0o600
                );
            }
            assert_private_work_preserved(workspace.path(), &private);
        }
    }

    #[test]
    fn publisher_setup_cancellation_preserves_unpublished_and_completed_outcomes() {
        for late in [false, true] {
            let (workspace, _secrets, private, path) = private_failure_fixture();
            let cancel = Cancellation::default();
            let point = if late {
                KeyCreationPoint::Cleanup
            } else {
                KeyCreationPoint::FileSync
            };
            let outcome = PublisherSetup::prepare(workspace.path(), &path)
                .unwrap()
                .create_with_hook(&cancel, &mut |actual| {
                    if actual == point {
                        cancel.cancel();
                    }
                    Ok(())
                });
            assert!(cancel.is_cancelled());
            if late {
                let (public, _) = outcome.unwrap();
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
                assert_eq!(fs::read_dir(&private).unwrap().count(), 2);
            } else {
                let error = outcome.unwrap_err();
                assert_eq!(
                    crate::application::exit::classify_error(&error),
                    crate::application::exit::EmpackExitCode::Interrupted
                );
                assert!(!path.exists());
                assert_eq!(fs::read_dir(&private).unwrap().count(), 1);
            }
            assert_private_work_preserved(workspace.path(), &private);
        }
    }

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

    #[test]
    fn publisher_setup_cancellation_and_replaced_entry_preserve_private_directory() {
        let workspace = tempfile::tempdir().unwrap();
        let secrets = tempfile::tempdir().unwrap();
        let private = secrets.path().join("private");
        crate::engine::publication::open_private_directory(&private, true).unwrap();
        let path = private.join("publisher.key");
        let prepared = PublisherSetup::prepare(workspace.path(), &path).unwrap();
        let cancel = Cancellation::default();
        cancel.cancel();
        assert!(prepared.create(&cancel).is_err());
        assert!(!path.exists());
        assert_eq!(std::fs::read_dir(&private).unwrap().count(), 0);

        let prepared = PublisherSetup::prepare(workspace.path(), &path).unwrap();
        std::fs::create_dir(&path).unwrap();
        std::fs::write(path.join("retained"), b"unrelated private work").unwrap();
        assert!(prepared.create(&Cancellation::default()).is_err());
        assert_eq!(
            std::fs::read(path.join("retained")).unwrap(),
            b"unrelated private work"
        );
        assert_eq!(std::fs::read_dir(&private).unwrap().count(), 1);
    }
}
