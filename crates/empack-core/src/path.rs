//! Portable syntax values; filesystem authority belongs to native root adapters.
use alloc::string::String;
use core::fmt;

/// The namespace in which a portable file path will be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathSyntax {
    /// A file beneath a project content root.
    ProjectContent,
    /// A regular file member of an archive.
    ArchiveMember,
    /// One component used to derive an artifact filename.
    ArtifactName,
}

/// A nonempty portable relative file path, preserving original spelling.
///
/// This value cannot grant permission to read, write or delete native files.
///
/// ```compile_fail
/// use empack_core::path::PortableRelPath;
/// let path = PortableRelPath("../outside".into());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PortableRelPath(String);

/// Why an input cannot represent a portable file path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathError {
    /// A path or component was empty, current-directory or parent-directory syntax.
    EmptyOrTraversal,
    /// A component contains a separator, control or platform-invalid character.
    InvalidCharacter,
    /// A component ends with a space or dot.
    TrailingDotOrSpace,
    /// A component denotes a reserved Windows device name, with or without extension.
    ReservedName,
    /// An artifact name contains more than one component.
    MultipleComponents,
}

impl fmt::Display for PathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::EmptyOrTraversal => "file path contains an empty or traversal component",
            Self::InvalidCharacter => "file path contains a nonportable character",
            Self::TrailingDotOrSpace => "file path component ends with a dot or space",
            Self::ReservedName => "file path contains a reserved device name",
            Self::MultipleComponents => "artifact name must be one component",
        })
    }
}

impl core::error::Error for PathError {}

impl PortableRelPath {
    /// Parse forward-slash syntax without normalization or native filesystem access.
    pub fn parse(input: &str, policy: PathSyntax) -> Result<Self, PathError> {
        if policy == PathSyntax::ArtifactName && input.contains('/') {
            return Err(PathError::MultipleComponents);
        }
        for component in input.split('/') {
            validate_component(component)?;
        }
        Ok(Self(String::from(input)))
    }

    /// The original valid spelling, using forward slashes.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Validated path components in their original order.
    pub fn components(&self) -> impl Iterator<Item = &str> {
        self.0.split('/')
    }
}

/// A validated file destination beneath a content root, not a native capability.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct InstallDestination(PortableRelPath);

impl InstallDestination {
    /// Validate a project-relative file destination.
    pub fn parse(input: &str) -> Result<Self, PathError> {
        PortableRelPath::parse(input, PathSyntax::ProjectContent).map(Self)
    }

    /// Read the validated syntax value.
    pub fn relative(&self) -> &PortableRelPath {
        &self.0
    }
}

/// A single portable component from which an artifact filename can be derived.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ArtifactStem(PortableRelPath);

impl ArtifactStem {
    /// Validate an artifact component while preserving its display spelling.
    pub fn parse(input: &str) -> Result<Self, PathError> {
        PortableRelPath::parse(input, PathSyntax::ArtifactName).map(Self)
    }

    /// Read the validated component.
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

fn validate_component(component: &str) -> Result<(), PathError> {
    if matches!(component, "" | "." | "..") {
        return Err(PathError::EmptyOrTraversal);
    }
    if component
        .chars()
        .any(|c| c.is_control() || "\\:<>\"|?*".contains(c))
    {
        return Err(PathError::InvalidCharacter);
    }
    if component.ends_with(['.', ' ']) {
        return Err(PathError::TrailingDotOrSpace);
    }
    let base = component
        .split('.')
        .next()
        .unwrap()
        .trim_end_matches(' ')
        .to_ascii_uppercase();
    let device = matches!(
        base.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || ["COM", "LPT"].iter().any(|prefix| {
        base.strip_prefix(prefix).is_some_and(|number| {
            matches!(
                number,
                "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
            )
        })
    });
    if device {
        return Err(PathError::ReservedName);
    }
    Ok(())
}
