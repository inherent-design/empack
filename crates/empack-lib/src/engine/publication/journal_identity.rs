//! Saved recovery evidence is distinct from the live observations authorizing execution.
use super::ObjectIdentity;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct JournalBinding {
    persistent: [u8; 32],
}
impl JournalBinding {
    pub(super) fn capture(object: ObjectIdentity) -> Result<Self> {
        Ok(Self {
            persistent: object.persistent.context(
                "Filesystem lacks persistent object identity; publication recovery is unsupported",
            )?,
        })
    }
    pub(super) fn matches(&self, object: ObjectIdentity) -> bool {
        object.persistent.as_ref() == Some(&self.persistent)
    }
}

pub(super) fn matches_optional(
    saved: Option<&JournalBinding>,
    observed: Option<ObjectIdentity>,
) -> bool {
    match (saved, observed) {
        (None, None) => true,
        (Some(saved), Some(observed)) => saved.matches(observed),
        _ => false,
    }
}
