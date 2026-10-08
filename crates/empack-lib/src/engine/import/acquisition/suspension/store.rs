//! Import schema over the shared native continuation store.
use super::*;
use crate::engine::continuation_store::{self as native, BoundRecord, Kind};
pub(super) use native::{Binding, MAX_RECORD, bind};
use serde::{Deserialize, Serialize};
use std::path::Path;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Record {
    pub schema: u32,
    pub binding: Binding,
    pub archive: [u8; 32],
    pub archive_bytes: u64,
    pub archive_digests: Vec<(String, String)>,
    pub revision: [u8; 32],
    pub strong: bool,
    pub files: BTreeMap<String, [u8; 32]>,
}

impl BoundRecord for Record {
    fn schema(&self) -> u32 {
        self.schema
    }
    fn binding(&self) -> &Binding {
        &self.binding
    }
}
pub(super) fn read(
    state: &Path,
    target: &Path,
    cancel: &Cancellation,
) -> Result<Option<(SavedImportRecord, Record)>> {
    Ok(
        native::read(state, Kind::Import, target, MAX_RECORD, cancel)?
            .map(|(saved, record)| (SavedImportRecord(saved), record)),
    )
}
pub(super) fn observe(
    state: &Path,
    target: &Path,
    cancel: &Cancellation,
) -> Result<Option<PendingImportCleanup>> {
    Ok(native::observe(state, Kind::Import, target, cancel)?.map(PendingImportCleanup))
}
pub(super) fn save(
    state: &Path,
    target: &Path,
    record: &Record,
    prior: Option<&SavedImportRecord>,
    cancel: &Cancellation,
) -> Result<SavedImportRecord> {
    native::save(
        state,
        Kind::Import,
        target,
        record,
        prior.map(|prior| &prior.0),
        cancel,
    )
    .map(SavedImportRecord)
}
pub(super) fn discard(saved: &PendingImportCleanup, cancel: &Cancellation) -> Result<bool> {
    native::discard(&saved.0, cancel)
}
#[cfg(test)]
mod tests;
