//! Shared serialization of native file effects; expected bytes and attributes stay intact.
use anyhow::Result;
use empack_core::files::{FileChange, FileContent, FilePlan, ObservedPath};
use serde::Serialize;

#[derive(Serialize)]
struct Content {
    sha256: String,
    bytes: u64,
    readonly: bool,
    executable: bool,
}
impl From<&FileContent> for Content {
    fn from(value: &FileContent) -> Self {
        Self {
            sha256: empack_core::digest::ExpectedDigest::Sha256(*value.content.bytes()).hex(),
            bytes: value.bytes,
            readonly: value.permissions.readonly,
            executable: value.permissions.executable,
        }
    }
}
#[derive(Serialize)]
pub(in crate::application::engine_host) struct Change {
    path: String,
    action: &'static str,
    before: Option<Content>,
    after: Option<Content>,
}
pub(super) fn changes(files: &FilePlan) -> Result<Vec<Change>> {
    files
        .changes()
        .iter()
        .map(|change| {
            let path = crate::engine::layout::ProjectLayout::path(change.target())?
                .as_str()
                .to_owned();
            Ok(match change {
                FileChange::Replace { before, after, .. } => Change {
                    path,
                    action: if matches!(before, ObservedPath::Absent) {
                        "create"
                    } else {
                        "replace"
                    },
                    before: match before {
                        ObservedPath::File(value) => Some(value.into()),
                        _ => None,
                    },
                    after: Some(after.into()),
                },
                FileChange::Remove { before, .. } => Change {
                    path,
                    action: "remove",
                    before: Some(before.into()),
                    after: None,
                },
            })
        })
        .collect::<Result<_>>()
}
