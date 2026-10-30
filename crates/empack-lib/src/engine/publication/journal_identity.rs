//! Saved recovery evidence is distinct from the live observations authorizing execution.
use super::{Binding, ObjectIdentity};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
pub(super) enum JournalBinding {
    Persistent { persistent: [u8; 32] },
    Legacy(Binding),
}
impl JournalBinding {
    pub(super) fn capture(schema: u32, object: ObjectIdentity) -> Result<Self> {
        match schema {
            4 => Ok(Self::Legacy(object.into())),
            5 => Ok(Self::Persistent {
                persistent: object.persistent.context(
                    "Filesystem lacks persistent object identity; publication recovery is unsupported",
                )?,
            }),
            _ => bail!("Unsupported publication journal schema"),
        }
    }
    pub(super) fn supports_schema(&self, schema: u32) -> bool {
        matches!(
            (schema, self),
            (4, Self::Legacy(_)) | (5, Self::Persistent { .. })
        )
    }
    pub(super) fn matches(&self, object: ObjectIdentity) -> bool {
        match self {
            Self::Legacy(binding) => *binding == Binding::from(object),
            Self::Persistent { persistent } => object.persistent.as_ref() == Some(persistent),
        }
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
