//! Bounded digest-to-address hints accelerate discovery; original assertions still verify bytes.
use super::*;
use std::io::{Read, Write};

impl FileContentLookup {
    /// Resolve one non-authoritative hint, then reverify all original assertions and the address.
    pub async fn retain_expected(
        &self,
        scope: &mut WorkScope,
        expected: ExpectedContent,
        maximum: u64,
        evidence: SourceEvidencePolicy,
        initial: InitialObservation,
    ) -> Result<Option<AcquiredContent>> {
        let direct = expected.accepted_observation.clone().or_else(|| {
            expected
                .digests
                .as_ref()?
                .values()
                .iter()
                .find_map(|digest| match digest {
                    ExpectedDigest::Sha256(bytes) => Some(ContentId::from_sha256(*bytes)),
                    _ => None,
                })
        });
        let id = if let Some(id) = direct {
            Some(id)
        } else if let Some(digest) = expected
            .digests
            .as_ref()
            .and_then(|set| set.values().iter().max_by_key(|digest| digest.algorithm()))
        {
            let name = hint_name(digest);
            let lookup = self.clone();
            let work = scope.spawn_blocking(
                ResourceRequest {
                    jobs: 1,
                    open_files: 3,
                    memory_bytes: 1024,
                    ..Default::default()
                },
                ResourceRequest::default(),
                move |cancel| {
                    cancel.check()?;
                    let _lock = lookup.0.lock(false)?;
                    let mut file = match native::open_file(&lookup.0.root, &name) {
                        Ok(file) => file,
                        Err(error) if missing(&error) => return Ok(None),
                        Err(error) => return Err(error),
                    };
                    ensure!(file.metadata()?.len() == 32, "Invalid content hint length");
                    let mut bytes = [0; 32];
                    file.read_exact(&mut bytes)?;
                    Ok(Some(ContentId::from_sha256(bytes)))
                },
            )?;
            scope
                .accept(work.wait().await?)?
                .transpose()?
                .into_parts()
                .0
        } else {
            None
        };
        let Some(id) = id else { return Ok(None) };
        self.retain(
            scope,
            CachedFileRequest {
                id,
                expected,
                maximum,
                evidence,
                initial,
            },
        )
        .await
    }
}
impl FileContentStore {
    /// Publish verified bytes and discoverable declarations. Hints never upgrade their assurance.
    /// A crash between blob and hint publication loses an optimization, not source evidence.
    pub async fn publish_expected(
        &self,
        scope: &mut WorkScope,
        content: AcquiredContent,
        expected: ExpectedContent,
    ) -> Result<ContentStored> {
        if let Some(digests) = &expected.digests {
            digests.check(content.observed_digests().values())?;
        }
        ensure!(
            expected
                .size
                .is_none_or(|size| size == content.lease().len())
                && expected
                    .accepted_observation
                    .as_ref()
                    .is_none_or(|id| *id == content.lease().id()),
            "Content does not satisfy cache source assertions"
        );
        let receipt = self.publish_verified(scope, content).await?;
        let hints: Vec<_> = expected
            .digests
            .iter()
            .flat_map(|set| set.values())
            .filter(|digest| !matches!(digest, ExpectedDigest::Sha256(_)))
            .map(hint_name)
            .collect();
        if !hints.is_empty() {
            let store = self.clone();
            let id = receipt.id.clone();
            let work = scope.spawn_blocking(
                ResourceRequest {
                    jobs: 1,
                    memory_bytes: 4096,
                    open_files: 4,
                    ..Default::default()
                },
                ResourceRequest::default(),
                move |cancel| store.0.publish_hints(&hints, &id, &cancel),
            )?;
            scope.accept(work.wait().await?)?.transpose()?;
        }
        Ok(receipt)
    }
}
impl Store {
    fn publish_hints(&self, hints: &[String], id: &ContentId, cancel: &Cancellation) -> Result<()> {
        let _lock = self.lock(true)?;
        for name in hints {
            cancel.check()?;
            match native::open_file(&self.root, name) {
                Ok(mut prior) => {
                    // Conflicting or malformed disposable hints are not overwritten implicitly.
                    if prior.metadata()?.len() == 32 {
                        let mut bytes = [0; 32];
                        prior.read_exact(&mut bytes)?;
                        if bytes == *id.bytes() {
                            continue;
                        }
                    }
                    anyhow::bail!("Existing source hint differs; clean the disposable cache");
                }
                Err(error) if missing(&error) => {}
                Err(error) => return Err(error),
            }
            let (count, used) = self.usage(cancel)?;
            ensure!(
                count < self.limits.entries
                    && used <= self.limits.total_bytes
                    && 32 <= self.limits.total_bytes - used,
                "Content store capacity exceeded"
            );
            let (temporary, mut file) = create_temporary(&self.root)?;
            let result = (|| {
                file.write_all(id.bytes())?;
                file.sync_all()?;
                drop(file);
                cancel.check()?;
                self.root.rename(&temporary, &self.root, name)?;
                sync(&self.root)
            })();
            if result.is_err() {
                let _ = self.root.remove_file(&temporary);
            }
            result?;
        }
        Ok(())
    }
}
fn hint_name(digest: &ExpectedDigest) -> String {
    format!("{}-{}.hint", digest.algorithm().name(), digest.hex())
}
pub(super) fn is_hint(name: &str) -> bool {
    let Some((algorithm, hex)) = name
        .strip_suffix(".hint")
        .and_then(|name| name.split_once('-'))
    else {
        return false;
    };
    matches!(algorithm, "md5" | "sha1" | "sha512")
        && hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        && ExpectedDigest::parse(algorithm, hex).is_ok()
}

#[cfg(test)]
mod tests;
