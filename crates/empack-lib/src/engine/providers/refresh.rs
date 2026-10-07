//! Refresh a locator without changing the approved file's byte assertions or placement.
use super::*;
use empack_core::model::FileSlot;

impl ProviderResolution {
    /// Resolve a locked role by its declared filename/primary role, or uniquely matching
    /// source assertions. An arbitrary logical slot never becomes a guessed filename.
    /// Returned locators are transient. The original expectation must still verify the bytes.
    pub fn download_alternatives(
        &self,
        slot: &FileSlot,
        expected: &ExpectedContent,
    ) -> Result<Vec<String>> {
        Ok(self.file_for_slot(slot, expected)?.alternatives.clone())
    }
    pub(super) fn file_for_slot(
        &self,
        slot: &FileSlot,
        expected: &ExpectedContent,
    ) -> Result<&ProviderFile> {
        ensure!(
            expected.digests.is_some() || expected.accepted_observation.is_some(),
            "Locked file lacks content evidence"
        );
        let files = self.files.as_slice();
        let selected = if slot.as_str() == "primary" {
            // Modrinth defines the first file as primary when no primary flag is present.
            Some(files.iter().find(|file| file.primary).unwrap_or(&files[0]))
        } else {
            files.iter().find(|file| file.filename == slot.as_str())
        };
        if let Some(file) = selected {
            ensure!(
                compatible(file, expected),
                "Provider file assertions changed since resolution"
            );
            return Ok(file);
        }
        let candidates: Vec<_> = files
            .iter()
            .filter(|file| {
                compatible(file, expected)
                    && (files.len() == 1 || has_shared_digest(file, expected))
            })
            .collect();
        ensure!(
            candidates.len() == 1,
            "Locked file role has no unique provider match"
        );
        Ok(candidates[0])
    }
}
fn compatible(file: &ProviderFile, expected: &ExpectedContent) -> bool {
    if expected
        .size
        .zip(file.expected.size)
        .is_some_and(|(a, b)| a != b)
    {
        return false;
    }
    match (&expected.digests, &file.expected.digests) {
        (Some(locked), Some(current)) => locked.values().iter().all(|old| {
            current
                .values()
                .iter()
                .find(|new| new.algorithm() == old.algorithm())
                .is_none_or(|new| old == new)
        }),
        _ => true,
    }
}
fn has_shared_digest(file: &ProviderFile, expected: &ExpectedContent) -> bool {
    match (&expected.digests, &file.expected.digests) {
        (Some(locked), Some(current)) => locked
            .values()
            .iter()
            .any(|old| current.values().contains(old)),
        _ => false,
    }
}
