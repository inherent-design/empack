//! Provider-qualified identifiers. Parsing proves syntax, not provider existence.
use alloc::string::String;
use core::{fmt, num::NonZeroU64};

/// A provider identifier has invalid syntax.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentityError {
    /// Modrinth IDs contain exactly eight ASCII base62 characters.
    ModrinthId,
    /// CurseForge IDs are positive decimal integers without alternate spellings.
    CurseForgeId,
}
impl fmt::Display for IdentityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::ModrinthId => {
                "Modrinth identity must be an eight-character base62 ID, not a slug or URL"
            }
            Self::CurseForgeId => {
                "CurseForge identity must be a positive canonical decimal integer"
            }
        })
    }
}
impl core::error::Error for IdentityError {}

macro_rules! modrinth_id {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);
        impl $name {
            /// Validate provider ID syntax without normalizing case or whitespace.
            pub fn parse(value: &str) -> Result<Self, IdentityError> {
                if value.len() != 8 || !value.bytes().all(|b| b.is_ascii_alphanumeric()) {
                    return Err(IdentityError::ModrinthId);
                }
                Ok(Self(value.into()))
            }
            /// Return the original provider spelling.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}
modrinth_id!(
    ModrinthProjectId,
    "A canonical Modrinth project ID, distinct from a selector."
);
modrinth_id!(
    ModrinthVersionId,
    "A canonical Modrinth version ID, distinct from a project ID."
);

macro_rules! curseforge_id {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(NonZeroU64);
        impl $name {
            /// Reject signs, padding, zero and overflow rather than changing identity.
            pub fn parse(value: &str) -> Result<Self, IdentityError> {
                if value.starts_with('0') || !value.bytes().all(|b| b.is_ascii_digit()) {
                    return Err(IdentityError::CurseForgeId);
                }
                value
                    .parse::<u64>()
                    .ok()
                    .and_then(NonZeroU64::new)
                    .map(Self)
                    .ok_or(IdentityError::CurseForgeId)
            }
            /// Return the provider number.
            pub fn get(self) -> u64 {
                self.0.get()
            }
        }
        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }
    };
}
curseforge_id!(CurseForgeProjectId, "A canonical CurseForge project ID.");
curseforge_id!(
    CurseForgeFileId,
    "A canonical CurseForge file ID, distinct from its project ID."
);

/// Project identity includes its provider; labels and filenames are not identities.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ProviderProjectId {
    /// Modrinth project.
    Modrinth(ModrinthProjectId),
    /// CurseForge project.
    CurseForge(CurseForgeProjectId),
}
impl fmt::Display for ProviderProjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Modrinth(id) => id.fmt(f),
            Self::CurseForge(id) => id.fmt(f),
        }
    }
}

/// An input pin. Ownership must be checked against a provider response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PinSelector {
    /// Select a Modrinth version.
    ModrinthVersion(ModrinthVersionId),
    /// Select a CurseForge file.
    CurseForgeFile(CurseForgeFileId),
}

impl ProviderProjectId {
    /// Parse a pin in this provider's namespace. This does not prove ownership.
    pub fn parse_pin(&self, value: &str) -> Result<PinSelector, IdentityError> {
        match self {
            Self::Modrinth(_) => ModrinthVersionId::parse(value).map(PinSelector::ModrinthVersion),
            Self::CurseForge(_) => CurseForgeFileId::parse(value).map(PinSelector::CurseForgeFile),
        }
    }
}
