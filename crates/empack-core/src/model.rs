//! Normalized intent and exact resolution, independent of wire formats and I/O.
mod archive;
pub use archive::ProviderArchiveSource;

use crate::{
    digest::{ContentId, DigestSet},
    identity::{PinSelector, ProviderProjectId},
    path::{InstallDestination, PortableRelPath},
    requirements::{OptionalChoice, Requirement, Requirements},
};
use alloc::{
    collections::{BTreeMap, BTreeSet},
    string::String,
    vec::Vec,
};
use core::fmt;

/// A semantic document or resolution is internally inconsistent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelError(pub String);
impl fmt::Display for ModelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl core::error::Error for ModelError {}
fn invalid(message: &str) -> ModelError {
    ModelError(message.into())
}

macro_rules! label {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);
        impl $name {
            /// Preserve spelling; reject blank values and control characters.
            pub fn parse(value: &str) -> Result<Self, ModelError> {
                if value.trim().is_empty() || value.chars().any(char::is_control) {
                    return Err(invalid(concat!(
                        stringify!($name),
                        " must be nonempty and contain no control characters"
                    )));
                }
                Ok(Self(value.into()))
            }
            /// Original semantic spelling, not a native path or authority.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
    };
}
label!(
    DependencyKey,
    "A logical dependency label, independent of provider and filename identity."
);
label!(
    FileSlot,
    "A stable file role within one resolved dependency."
);
label!(
    GameVersion,
    "A game version in provider syntax, not necessarily SemVer."
);
label!(
    LoaderVersion,
    "A loader version in provider syntax, not necessarily SemVer."
);

/// A sequence that cannot be constructed empty or mutated into an empty sequence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NonEmpty<T>(Vec<T>);
impl<T> NonEmpty<T> {
    /// Check cardinality at the boundary.
    pub fn new(values: Vec<T>) -> Result<Self, ModelError> {
        if values.is_empty() {
            return Err(invalid("Expected at least one value"));
        }
        Ok(Self(values))
    }
    /// Consume the wrapper without cloning its elements.
    pub fn into_vec(self) -> Vec<T> {
        self.0
    }
    /// Append without weakening the nonempty invariant.
    pub fn push(&mut self, value: T) {
        self.0.push(value);
    }
    /// Borrow all elements in their declared order.
    pub fn as_slice(&self) -> &[T] {
        &self.0
    }
}

/// Canonical semantic document digest, computed by the codec with domain separation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SemanticRevision(pub [u8; 32]);
/// Exact raw document digest; comment-only edits change this revision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DocumentRevision(pub [u8; 32]);

/// Content categories remain independent of acquisition routes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ContentKind {
    /// Executable game mod.
    Mod,
    /// Game resources.
    ResourcePack,
    /// Rendering shaders.
    ShaderPack,
    /// World datapack.
    DataPack,
    /// World files.
    World,
    /// Configuration file.
    Config,
    /// Other explicitly placed content.
    OtherFile,
}
/// Layer precedence is resolved by a projection, never map insertion order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ContentLayer {
    /// Shared base.
    Common,
    /// Shared replacement applied after base content and before either side layer.
    CommonOverride,
    /// Client replacement or addition.
    Client,
    /// Server replacement or addition.
    Server,
}
/// Supported loader families, including a pack without a loader.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoaderKind {
    /// Unmodified game.
    Vanilla,
    /// Fabric.
    Fabric,
    /// Quilt.
    Quilt,
    /// Forge.
    Forge,
    /// NeoForge, including its historical version syntax.
    NeoForge,
}
/// Human pack metadata does not double as filesystem authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackMetadata {
    /// Stable pack identity, independent of display name and version.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Human release version.
    pub version: String,
    /// Ordered author attribution.
    pub authors: Vec<String>,
    /// Optional description.
    pub description: Option<String>,
}
/// Requested runtime compatibility.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeIntent {
    /// Declared minimum recipient Java major.
    pub java_major: u16,
    /// Primary game version.
    pub minecraft: GameVersion,
    /// Explicit additional compatible versions.
    pub acceptable_versions: Vec<GameVersion>,
    /// Selected loader family.
    pub loader: LoaderKind,
    /// Requested loader version; omission requires exact resolution before build.
    pub loader_version: Option<LoaderVersion>,
}
/// A source declaration is explicit; malformed provider records never become searches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceIntent {
    /// Canonical provider-owned project.
    Provider(ProviderProjectId),
    /// Exact readable lookup request, bound to canonical identity by the lock.
    ProviderSelector(ProviderSelector),
    /// Deliberately unresolved query and provider preference.
    Search {
        /// Search text.
        query: String,
        /// Provider priority, in order.
        providers: NonEmpty<ProviderKind>,
    },
    /// Direct content alternatives; credentials are disallowed by the wire adapter.
    Url(NonEmpty<String>),
    /// A tracked project-relative source file.
    Local(PortableRelPath),
    /// One logical dependency with an explicit source file for each stable member slot.
    LocalFiles(BTreeMap<FileSlot, PortableRelPath>),
}
/// Provider namespace for selection policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ProviderKind {
    /// Modrinth.
    Modrinth,
    /// CurseForge.
    CurseForge,
}
/// An exact provider lookup request, never a canonical ID by assignment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderSelector {
    provider: ProviderKind,
    value: String,
}
impl ProviderSelector {
    /// Validate portable lookup syntax without consulting a provider.
    pub fn new(provider: ProviderKind, value: &str) -> Result<Self, ModelError> {
        if value.is_empty()
            || value.len() > 256
            || matches!(value, "." | "..")
            || !value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.+".contains(&b))
        {
            return Err(invalid("Provider selector must be an exact ID or slug"));
        }
        Ok(Self {
            provider,
            value: value.into(),
        })
    }
    /// Provider namespace.
    pub fn provider(&self) -> ProviderKind {
        self.provider
    }
    /// Original exact lookup spelling.
    pub fn value(&self) -> &str {
        &self.value
    }
    /// Canonical identities may bind only within the requested namespace.
    pub fn accepts(&self, id: &ProviderProjectId) -> bool {
        matches!(
            (self.provider, id),
            (ProviderKind::Modrinth, ProviderProjectId::Modrinth(_))
                | (ProviderKind::CurseForge, ProviderProjectId::CurseForge(_))
        )
    }
}
impl SourceIntent {
    /// Authored exact lookup spelling, separate from its canonical binding.
    pub fn selector(&self) -> Option<&ProviderSelector> {
        match self {
            Self::ProviderSelector(value) => Some(value),
            _ => None,
        }
    }
    /// A selector can reuse identity only when the lock retains that exact request.
    pub fn canonical_project<'a>(
        &'a self,
        locked: Option<&'a LockedDependency>,
    ) -> Option<&'a ProviderProjectId> {
        match self {
            Self::Provider(id) => Some(id),
            Self::ProviderSelector(request) => locked.and_then(|locked| match &locked.identity {
                ResolvedIdentity::Provider(id)
                    if locked.source_selector.as_ref() == Some(request) && request.accepts(id) =>
                {
                    Some(id)
                }
                _ => None,
            }),
            _ => None,
        }
    }
}
/// Provider classifications used for automatic selection, independent of pack channels.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ReleasePolicy {
    /// Only provider-classified stable releases.
    StableOnly,
    /// Prefer stable releases, with eligible prereleases as fallback.
    #[default]
    PreferStable,
    /// Give stable and prerelease records equal preference.
    Any,
}
impl ReleasePolicy {
    /// Stable authoring and reporting spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::StableOnly => "stable-only",
            Self::PreferStable => "prefer-stable",
            Self::Any => "any",
        }
    }
}
/// Sync retains the exact lock; only update refreshes compatible selections.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DependencySelection {
    /// Resolve when missing or changed, then retain the exact lock selection.
    /// For URL sources this accepts first-observed bytes when no matching evidence exists;
    /// author documents must explicitly acknowledge that policy with `accept-observed`.
    Automatic,
    /// A provider-specific version or file request.
    ProviderPin(PinSelector),
    /// Explicit source-byte declarations.
    ContentAssertions(DigestSet),
}
/// Placement request, independent of source identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlacementIntent {
    /// Provider/type layout proposes placements during resolution.
    Automatic,
    /// Base directories for provider-world members, retaining roots across archive updates.
    ArchiveRoot(NonEmpty<Placement>),
    /// Explicit destinations, including multiple placements of the same bytes.
    Explicit(NonEmpty<Placement>),
    /// Exact named file roles, retaining each member-to-destination association.
    ByFile(BTreeMap<FileSlot, NonEmpty<Placement>>),
}
/// A requested dependency.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependencyIntent {
    /// Automatic selection override for this provider root.
    pub releases: Option<ReleasePolicy>,
    /// Independent acquisition intent.
    pub source: SourceIntent,
    /// Content kind.
    pub kind: ContentKind,
    /// Selection policy.
    pub selection: DependencySelection,
    /// Desired placements.
    pub placement: PlacementIntent,
    /// Required or optional participation per side.
    pub requirements: Requirements,
}
impl DependencyIntent {
    /// Check one exact selection against this root's source, pin, placement and participation.
    /// Runtime compatibility and native byte verification belong to their enclosing boundaries.
    pub fn validate_selection(
        &self,
        key: &DependencyKey,
        selected: &LockedDependency,
    ) -> Result<(), ModelError> {
        if selected.kind != self.kind {
            return Err(invalid("Lock changed requested content kind"));
        }
        match (&self.source, &selected.identity) {
            (
                SourceIntent::Provider(_) | SourceIntent::ProviderSelector(_),
                ResolvedIdentity::Provider(id),
            ) if self.source.canonical_project(Some(selected)) == Some(id) => {}
            (SourceIntent::Search { providers, .. }, ResolvedIdentity::Provider(project))
                if providers.as_slice().contains(&match project {
                    ProviderProjectId::Modrinth(_) => ProviderKind::Modrinth,
                    ProviderProjectId::CurseForge(_) => ProviderKind::CurseForge,
                }) => {}
            (SourceIntent::Url(_), ResolvedIdentity::Url(b))
            | (SourceIntent::Local(_) | SourceIntent::LocalFiles(_), ResolvedIdentity::Local(b))
                if key == b => {}
            _ => return Err(invalid("Locked identity differs from source intent")),
        }
        if let DependencySelection::ProviderPin(pin) = &self.selection
            && selected.selected.as_ref().map(|value| &value.selection) != Some(pin)
        {
            return Err(invalid("Lock does not satisfy requested pin"));
        }
        if let SourceIntent::LocalFiles(members) = &self.source
            && members.len() != selected.files.as_slice().len()
        {
            return Err(invalid("Lock changed tracked member coverage"));
        }
        for file in selected.files.as_slice() {
            match (&self.source, &file.acquisition) {
                (SourceIntent::Url(expected), AcquisitionSpec::Url(actual))
                    if expected == actual => {}
                (SourceIntent::Local(expected), AcquisitionSpec::Local(actual))
                    if expected == actual => {}
                (SourceIntent::LocalFiles(members), AcquisitionSpec::Local(actual))
                    if members.get(&file.slot) == Some(actual) => {}
                (
                    SourceIntent::Url(_) | SourceIntent::Local(_) | SourceIntent::LocalFiles(_),
                    _,
                ) => {
                    return Err(invalid("Lock changed declared acquisition source"));
                }
                _ => {}
            }
            if let DependencySelection::ContentAssertions(expected) = &self.selection
                && file.expected.digests.as_ref() != Some(expected)
            {
                return Err(invalid("Lock changed declared source digests"));
            }
        }
        if let PlacementIntent::ArchiveRoot(roots) = &self.placement {
            archive::validate_placements(self.kind, roots, selected)?;
        } else if let PlacementIntent::ByFile(expected) = &self.placement {
            if expected.len() != selected.files.as_slice().len()
                || selected.files.as_slice().iter().any(|file| {
                    expected.get(&file.slot).is_none_or(|places| {
                        places.as_slice().len() != file.placements.as_slice().len()
                            || places.as_slice().iter().any(|place| {
                                file.placements
                                    .as_slice()
                                    .iter()
                                    .filter(|actual| *actual == place)
                                    .count()
                                    != 1
                            })
                    })
                })
            {
                return Err(invalid("Lock changed explicit file roles or placements"));
            }
        } else if let PlacementIntent::Explicit(expected) = &self.placement {
            if selected.files.as_slice().len() != 1 {
                return Err(invalid(
                    "Multiple file roles require named placement intent",
                ));
            }
            let actual: Vec<_> = selected
                .files
                .as_slice()
                .iter()
                .flat_map(|file| file.placements.as_slice())
                .collect();
            if actual.len() != expected.as_slice().len()
                || expected
                    .as_slice()
                    .iter()
                    .any(|p| actual.iter().filter(|a| *a == &p).count() != 1)
            {
                return Err(invalid("Lock changed explicit placements or requirements"));
            }
        } else if selected
            .files
            .as_slice()
            .iter()
            .flat_map(|file| file.placements.as_slice())
            .any(|p| p.requirements != self.requirements)
        {
            return Err(invalid("Lock changed root requirements"));
        }
        Ok(())
    }
}

/// File destination and participation; this is not deletion authorization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placement {
    /// Relative destination beneath a content root.
    pub destination: InstallDestination,
    /// Shared or side-specific layer.
    pub layer: ContentLayer,
    /// Side requirements and optional choices.
    pub requirements: Requirements,
}
/// Supported extension values; maps have deterministic key order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtensionValue {
    /// Null.
    Null,
    /// Boolean.
    Bool(bool),
    /// Canonical number spelling, validated by the codec.
    Number(String),
    /// Text.
    Text(String),
    /// Ordered values.
    List(Vec<ExtensionValue>),
    /// Named values.
    Object(BTreeMap<String, ExtensionValue>),
}
/// A complete named consumer recipe, including its archive container.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildProfile {
    /// Consumer, delivery, environment and update authority.
    pub recipe: crate::distribution::Recipe,
    /// Container used for this profile.
    pub archive: DistributionArchive,
}
/// Authored profiles and their explicit default selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DistributionIntent {
    /// Complete named profiles, without implicit inheritance.
    pub profiles: BTreeMap<String, BuildProfile>,
    /// Explicit ordered profile names selected without CLI selectors.
    pub defaults: Vec<String>,
}
impl DistributionIntent {
    /// Seed named profiles when initializing or importing a project.
    pub fn from_recipes(
        recipes: NonEmpty<crate::distribution::Recipe>,
        archive: DistributionArchive,
    ) -> Self {
        use crate::distribution::Consumer;
        let mut profiles = BTreeMap::new();
        let mut defaults = Vec::new();
        for (index, recipe) in recipes.as_slice().iter().enumerate() {
            let consumer = match recipe.consumer() {
                Consumer::Modrinth => "modrinth",
                Consumer::CurseForge => "curseforge",
                Consumer::Prism => "prism",
                Consumer::Server => "server",
                Consumer::Empack => "empack",
            };
            let mut name = String::from(consumer);
            if profiles.contains_key(&name) {
                name = alloc::format!("{consumer}-{}", index + 1);
            }
            defaults.push(name.clone());
            profiles.insert(
                name,
                BuildProfile {
                    recipe: *recipe,
                    archive,
                },
            );
        }
        Self { profiles, defaults }
    }
    /// Validate portable names and the complete default selection.
    pub fn validate(&self) -> Result<(), ModelError> {
        if self.profiles.len() > 128 {
            return Err(ModelError("Select at most 128 build profiles".into()));
        }
        for name in self.profiles.keys() {
            if name.is_empty()
                || name.len() > 64
                || !name
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_'))
            {
                return Err(ModelError(
                    "Build profile names use 1–64 ASCII letters, digits, hyphens or underscores"
                        .into(),
                ));
            }
        }
        let mut selected = BTreeSet::new();
        for name in self.defaults.as_slice() {
            if !self.profiles.contains_key(name) || !selected.insert(name) {
                return Err(ModelError(
                    "Default build profiles must exist and be unique".into(),
                ));
            }
        }
        Ok(())
    }
}
/// All currently supported standalone archive formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DistributionArchive {
    /// ZIP.
    Zip,
    /// Gzip-compressed TAR.
    TarGz,
    /// 7z.
    SevenZip,
}
/// Authoring intent. It contains no installed state or native write authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectIntent {
    /// Default automatic selection policy for explicit and implicit provider content.
    pub dependency_policy: ReleasePolicy,
    /// Exact installation policy overrides, authored as integration bindings.
    pub installation_policies: BTreeMap<PortableRelPath, crate::instance::FilePolicy>,
    /// Layer-relative exclusion rules; locked sources remain explicit obligations.
    pub source_excludes: Vec<String>,
    /// Human metadata.
    pub metadata: PackMetadata,
    /// Runtime compatibility.
    pub runtime: RuntimeIntent,
    /// Explicit logical roots.
    pub roots: BTreeMap<DependencyKey, DependencyIntent>,
    /// Explicit content-kind directory overrides.
    pub layout: BTreeMap<ContentKind, PortableRelPath>,
    /// Default consumer recipes and archive format.
    pub distribution: DistributionIntent,
    /// Documented extension namespace, preserved during edits.
    pub extensions: BTreeMap<String, ExtensionValue>,
}
/// Exact provider selection binds a pin to its project.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ResolvedPin {
    /// Canonical owning project.
    pub project: ProviderProjectId,
    /// Exact version/file selection.
    pub selection: PinSelector,
}
impl ResolvedPin {
    /// Check provider namespace. Provider adapters separately prove ownership.
    pub fn validate(&self) -> Result<(), ModelError> {
        match (&self.project, &self.selection) {
            (ProviderProjectId::Modrinth(_), PinSelector::ModrinthVersion(_))
            | (ProviderProjectId::CurseForge(_), PinSelector::CurseForgeFile(_)) => Ok(()),
            _ => Err(invalid("Pin belongs to a different provider namespace")),
        }
    }
}
/// Exact source evidence required when acquiring a selected file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpectedContent {
    /// Independent source assertions, all of which must match.
    pub digests: Option<DigestSet>,
    /// Expected length when known.
    pub size: Option<u64>,
    /// An explicitly accepted prior observation, not independent authentication.
    pub accepted_observation: Option<ContentId>,
}
/// Readable locator without authority to publish its bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AcquisitionSpec {
    /// Refreshable provider selection plus stable alternatives, when available.
    Provider {
        /// Exact owning selection.
        pin: ResolvedPin,
        /// Exact provider filename or selector within this version, independent of the
        /// author-facing ResolvedFile::slot. Native resolution records the returned filename.
        slot: FileSlot,
        /// Stable, credential-free locators.
        alternatives: Vec<String>,
    },
    /// One interpreted world member whose archive retains its exact provider identity and evidence.
    ProviderArchiveMember {
        /// Original archive selection and source assertions, not member assertions.
        archive: ProviderArchiveSource,
        /// Exact regular-file path inside that archive.
        member: PortableRelPath,
    },
    /// Independent direct-download alternatives.
    Url(NonEmpty<String>),
    /// Tracked content under the project root.
    Local(PortableRelPath),
    /// Exact archive member under a retained archive source.
    Embedded {
        /// Project-relative archive source.
        archive: PortableRelPath,
        /// Archive-relative regular file.
        member: PortableRelPath,
    },
    /// User-supplied acquisition, preserving an exact expected selection.
    Manual {
        /// Provider selection if known.
        pin: Option<ResolvedPin>,
        /// Human instructions or provider page.
        instructions: String,
    },
}
/// Source attribution and accepted conversions, separate from acquisition authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provenance {
    /// Provider or source-format identifier.
    pub source: String,
    /// Original record location for diagnostics.
    pub location: Option<String>,
    /// Original source assertions, retained even when stronger hashes are observed later.
    pub declared_digests: Option<DigestSet>,
    /// Explicitly accepted conversions in source order.
    pub conversions: Vec<String>,
}
/// A selected file can have more than one placement, even when bytes are identical.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedFile {
    /// Stable role within this dependency.
    pub slot: FileSlot,
    /// How exact bytes can be acquired.
    pub acquisition: AcquisitionSpec,
    /// Source evidence.
    pub expected: ExpectedContent,
    /// Source attribution, not publication authority.
    pub provenance: Provenance,
    /// Destinations are not deduplicated by content hash.
    pub placements: NonEmpty<Placement>,
}
/// Canonical identity retained in the lock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedIdentity {
    /// Provider project.
    Provider(ProviderProjectId),
    /// URL-file logical root.
    Url(DependencyKey),
    /// Local-file logical root.
    Local(DependencyKey),
}
/// One resolved root or transitively retained dependency.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockedDependency {
    /// Exact authored selector whose canonical result is identity.
    pub source_selector: Option<ProviderSelector>,
    /// Resolved display title, independent of the logical key.
    pub title: String,
    /// Verified provider/file content kind.
    pub kind: ContentKind,
    /// Resolved identity, not a filename or title.
    pub identity: ResolvedIdentity,
    /// Exact selected provider pin where applicable.
    pub selected: Option<ResolvedPin>,
    /// Known negative provider assertions bound to this exact selection. An empty list
    /// does not establish complete compatibility evidence, especially for older locks.
    pub incompatibilities: Vec<ProviderIncompatibility>,
    /// Exact file inventory.
    pub files: NonEmpty<ResolvedFile>,
}
impl LockedDependency {
    /// Whether any resolved files can coexist on one side, including optional files.
    /// A disabled default does not prohibit a user from enabling that choice.
    pub fn can_share_environment(&self, other: &Self) -> bool {
        let participation = |dependency: &Self| {
            let mut sides = [false; 2];
            for file in dependency.files.as_slice() {
                for placement in file.placements.as_slice() {
                    sides[0] |= placement.layer != ContentLayer::Server
                        && placement.requirements.client != Requirement::Unsupported;
                    sides[1] |= placement.layer != ContentLayer::Client
                        && placement.requirements.server != Requirement::Unsupported;
                }
            }
            sides
        };
        let a = participation(self);
        let b = participation(other);
        (a[0] && b[0]) || (a[1] && b[1])
    }
}
/// A provider-declared conflict with a project, exact selection, or both.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderIncompatibility {
    /// Canonical target project when declared.
    pub project: Option<ProviderProjectId>,
    /// Exact target version/file when declared; absence means every project selection.
    pub selection: Option<PinSelector>,
}
impl ProviderIncompatibility {
    /// Match only the declared project and selection constraints.
    pub fn matches(&self, pin: &ResolvedPin) -> bool {
        (self.project.is_some() || self.selection.is_some())
            && self
                .project
                .as_ref()
                .is_none_or(|project| project == &pin.project)
            && self
                .selection
                .as_ref()
                .is_none_or(|selection| selection == &pin.selection)
    }
}
/// Whether missing dependency edges may be interpreted as a complete set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Coverage {
    /// Complete for the exact selected version and environments.
    CompleteForSelection,
    /// Some edges are known; absence proves nothing.
    Partial,
    /// No usable dependency evidence.
    Unknown,
}
/// Runtime selected for reproducible execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeResolution {
    /// Exact game version.
    pub minecraft: GameVersion,
    /// Exact loader family.
    pub loader: LoaderKind,
    /// Exact loader version, absent only for vanilla.
    pub loader_version: Option<LoaderVersion>,
}
/// Exact resolution is independent of raw source revisions and operation journals.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolutionLock {
    /// Additional accepted game versions used to resolve authoring roots, in preference order.
    /// This records selection policy, not a compatibility claim about untracked installations.
    pub acceptable_versions: Vec<GameVersion>,
    /// Semantic intent revision this lock resolves.
    pub intent_revision: SemanticRevision,
    /// Resolver implementation/version identity.
    pub resolver: String,
    /// Explicit and transitive selected dependencies.
    pub dependencies: BTreeMap<DependencyKey, LockedDependency>,
    /// Known required edges. Cycles are valid; references must exist.
    pub required_edges: BTreeMap<DependencyKey, BTreeSet<DependencyKey>>,
    /// Coverage for each exact selection.
    pub coverage: BTreeMap<DependencyKey, Coverage>,
    /// Exact runtime.
    pub runtime: RuntimeResolution,
}

impl ProjectIntent {
    /// Apply an operation override, then root preference, then project default.
    pub fn release_policy(
        &self,
        key: Option<&DependencyKey>,
        operation: Option<ReleasePolicy>,
    ) -> ReleasePolicy {
        operation
            .or_else(|| {
                key.and_then(|key| self.roots.get(key))
                    .and_then(|root| root.releases)
            })
            .unwrap_or(self.dependency_policy)
    }

    /// Resolve a type's automatic directory. Other kinds require an authored folder or placement.
    pub fn content_folder(&self, kind: ContentKind) -> Option<&str> {
        self.layout
            .get(&kind)
            .map(PortableRelPath::as_str)
            .or(match kind {
                ContentKind::Mod => Some("mods"),
                ContentKind::ResourcePack => Some("resourcepacks"),
                ContentKind::ShaderPack => Some("shaderpacks"),
                _ => None,
            })
    }

    /// Validate a root against both its explicit request and the project's automatic layout.
    pub fn validate_root_selection(
        &self,
        key: &DependencyKey,
        selected: &LockedDependency,
    ) -> Result<(), ModelError> {
        let root = self
            .roots
            .get(key)
            .ok_or_else(|| invalid("Unknown authoring root"))?;
        root.validate_selection(key, selected)?;
        if matches!(root.placement, PlacementIntent::Automatic) {
            let folder = self.content_folder(root.kind).ok_or_else(|| {
                invalid("Automatic placement requires a configured content directory")
            })?;
            if root.kind == ContentKind::World {
                return archive::validate_automatic(folder, selected);
            }
            if selected
                .files
                .as_slice()
                .iter()
                .flat_map(|file| file.placements.as_slice())
                .any(|placement| {
                    placement.layer != ContentLayer::Common
                        || placement
                            .destination
                            .relative()
                            .as_str()
                            .rsplit_once('/')
                            .map(|(parent, _)| parent)
                            != Some(folder)
                })
            {
                return Err(invalid(
                    "Lock does not satisfy the automatic content directory",
                ));
            }
        }
        Ok(())
    }

    /// Check source/policy combinations without imposing target-format restrictions.
    pub fn validate(&self) -> Result<(), ModelError> {
        self.distribution.validate()?;
        if self
            .source_excludes
            .iter()
            .any(|rule| rule.len() > 4096 || rule.chars().any(char::is_control))
        {
            return Err(invalid("Source exclusion must be one bounded pattern"));
        }
        if self.metadata.id.is_empty()
            || self.metadata.id.len() > 128
            || !self
                .metadata
                .id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"-_.+".contains(&c))
            || self.runtime.java_major < 8
        {
            return Err(invalid(
                "Pack identity and a Java minimum of at least 8 are required",
            ));
        }
        if self
            .metadata
            .authors
            .iter()
            .any(|author| author.trim().is_empty() || author.chars().any(char::is_control))
        {
            return Err(invalid("Authors must be nonempty attribution strings"));
        }
        if self.metadata.name.trim().is_empty() || self.metadata.version.trim().is_empty() {
            return Err(invalid("Pack name and version are required"));
        }
        if self.runtime.loader == LoaderKind::Vanilla && self.runtime.loader_version.is_some() {
            return Err(invalid("Vanilla cannot select a loader version"));
        }
        let mut choices = BTreeMap::new();
        for dep in self.roots.values() {
            validate_shared_choices(&mut choices, &dep.requirements)?;
            if dep.releases.is_some()
                && (!matches!(
                    dep.source,
                    SourceIntent::Provider(_)
                        | SourceIntent::ProviderSelector(_)
                        | SourceIntent::Search { .. }
                ) || matches!(dep.selection, DependencySelection::ProviderPin(_)))
            {
                return Err(invalid(
                    "Release preference requires an unpinned provider request",
                ));
            }
            validate_requirements(&dep.requirements)?;
            match (&dep.source, &dep.selection) {
                (
                    SourceIntent::Provider(_)
                    | SourceIntent::ProviderSelector(_)
                    | SourceIntent::Search { .. },
                    DependencySelection::ContentAssertions(_),
                ) => {
                    return Err(invalid(
                        "Provider content assertions belong to exact resolution",
                    ));
                }
                (SourceIntent::Search { .. }, DependencySelection::ProviderPin(_)) => {
                    return Err(invalid("An exact pin requires an explicit provider source"));
                }
                (SourceIntent::Provider(project), DependencySelection::ProviderPin(selection)) => {
                    ResolvedPin {
                        project: project.clone(),
                        selection: selection.clone(),
                    }
                    .validate()?
                }
                (
                    SourceIntent::ProviderSelector(request),
                    DependencySelection::ProviderPin(selection),
                ) => {
                    if !matches!(
                        (request.provider(), selection),
                        (ProviderKind::Modrinth, PinSelector::ModrinthVersion(_))
                            | (ProviderKind::CurseForge, PinSelector::CurseForgeFile(_))
                    ) {
                        return Err(invalid("Pin belongs to a different provider namespace"));
                    }
                }
                (
                    SourceIntent::Url(_) | SourceIntent::Local(_) | SourceIntent::LocalFiles(_),
                    DependencySelection::ProviderPin(_),
                ) => {
                    return Err(invalid("A file source cannot carry a provider pin"));
                }
                (SourceIntent::Search { query, providers }, _) => {
                    if query.trim().is_empty() {
                        return Err(invalid("Search text is empty"));
                    }
                    let unique: BTreeSet<_> = providers.as_slice().iter().collect();
                    if unique.len() != providers.as_slice().len() {
                        return Err(invalid("Provider order contains duplicates"));
                    }
                }
                _ => {}
            }
            if let SourceIntent::LocalFiles(members) = &dep.source {
                if members.is_empty() {
                    return Err(invalid("Tracked member inventory is empty"));
                }
                if !matches!(dep.selection, DependencySelection::Automatic) {
                    return Err(invalid(
                        "Tracked members retain individual content assertions",
                    ));
                }
                if !matches!(dep.placement, PlacementIntent::ByFile(_)) {
                    return Err(invalid("Tracked members require explicit placements"));
                }
            }
            if let PlacementIntent::ByFile(files) = &dep.placement {
                if files.is_empty() {
                    return Err(invalid("Explicit file placement map is empty"));
                }
                if let SourceIntent::LocalFiles(members) = &dep.source
                    && !members.keys().eq(files.keys())
                {
                    return Err(invalid("Member sources and placement roles disagree"));
                }
                for placements in files.values() {
                    for placement in placements.as_slice() {
                        validate_shared_choices(&mut choices, &placement.requirements)?;
                        validate_placement(placement)?;
                    }
                }
            }
            if let PlacementIntent::ArchiveRoot(placements) = &dep.placement {
                if dep.kind != ContentKind::World
                    || !matches!(
                        dep.source,
                        SourceIntent::Provider(_)
                            | SourceIntent::ProviderSelector(_)
                            | SourceIntent::Search { .. }
                    )
                {
                    return Err(invalid("Archive-root placement requires a provider world"));
                }
                for placement in placements.as_slice() {
                    validate_shared_choices(&mut choices, &placement.requirements)?;
                    validate_placement(placement)?;
                }
            }
            if let PlacementIntent::Explicit(placements) = &dep.placement {
                for placement in placements.as_slice() {
                    validate_shared_choices(&mut choices, &placement.requirements)?;
                    validate_placement(placement)?;
                }
            }
        }
        Ok(())
    }
}
fn validate_requirements(value: &Requirements) -> Result<(), ModelError> {
    if value.client == Requirement::Unsupported && value.server == Requirement::Unsupported {
        return Err(invalid("Content is unsupported in both environments"));
    }
    Ok(())
}
fn validate_placement(value: &Placement) -> Result<(), ModelError> {
    validate_requirements(&value.requirements)?;
    if (value.layer == ContentLayer::Client
        && value.requirements.server != Requirement::Unsupported)
        || (value.layer == ContentLayer::Server
            && value.requirements.client != Requirement::Unsupported)
    {
        return Err(invalid(
            "A side layer cannot place content in the opposite environment",
        ));
    }
    Ok(())
}

impl ResolutionLock {
    /// Validate an exact lock independently of current authoring intent.
    /// A valid prior lock is evidence for reconciliation, not proof of current satisfaction.
    pub fn validate_structure(&self) -> Result<(), ModelError> {
        if self.resolver.trim().is_empty() {
            return Err(invalid("Lock requires a resolver identity"));
        }
        if (self.runtime.loader == LoaderKind::Vanilla) != self.runtime.loader_version.is_none() {
            return Err(invalid(
                "Non-vanilla runtime requires an exact loader version",
            ));
        }
        let mut identities = BTreeSet::new();
        let mut destinations = BTreeSet::new();
        for (key, dependency) in &self.dependencies {
            if let Some(request) = &dependency.source_selector
                && !matches!(&dependency.identity, ResolvedIdentity::Provider(id) if request.accepts(id))
            {
                return Err(invalid("Source selector binds another provider namespace"));
            }
            if let ResolvedIdentity::Provider(project) = &dependency.identity {
                if !identities.insert(project) {
                    return Err(invalid(
                        "Multiple logical dependencies share one provider identity",
                    ));
                }
                let selected = dependency
                    .selected
                    .as_ref()
                    .ok_or_else(|| invalid("Provider dependency lacks exact selection"))?;
                selected.validate()?;
                if &selected.project != project {
                    return Err(invalid("Selected pin belongs to another project"));
                }
            } else if dependency.selected.is_some() {
                return Err(invalid(
                    "A URL or local identity cannot claim a provider selection",
                ));
            }
            for assertion in &dependency.incompatibilities {
                if dependency.selected.is_none()
                    || (assertion.project.is_none() && assertion.selection.is_none())
                {
                    return Err(invalid(
                        "Incompatibility requires a provider source and target",
                    ));
                }
                if let (Some(project), Some(selection)) = (&assertion.project, &assertion.selection)
                {
                    ResolvedPin {
                        project: project.clone(),
                        selection: selection.clone(),
                    }
                    .validate()?;
                }
                if let Some((target, _)) = self.dependencies.iter().find(|(_, target)| {
                    target
                        .selected
                        .as_ref()
                        .is_some_and(|pin| assertion.matches(pin))
                        && dependency.can_share_environment(target)
                }) {
                    return Err(ModelError(alloc::format!(
                        "{} declares incompatibility with {} in the candidate inventory",
                        key.as_str(),
                        target.as_str()
                    )));
                }
            }
            archive::validate_members(dependency)?;
            let mut slots = BTreeSet::new();
            for file in dependency.files.as_slice() {
                if !slots.insert(&file.slot) {
                    return Err(invalid("Duplicate file slot"));
                }
                if file.provenance.source.trim().is_empty() {
                    return Err(invalid("File provenance source is empty"));
                }
                if let Some(declared) = &file.provenance.declared_digests {
                    let expected =
                        file.expected.digests.as_ref().ok_or_else(|| {
                            invalid("Original source declarations were discarded")
                        })?;
                    declared.check(expected.values()).map_err(|_| {
                        invalid("Expected content changed original source declarations")
                    })?;
                }
                if file.expected.digests.is_none() && file.expected.accepted_observation.is_none() {
                    return Err(invalid("Selected file lacks expected content evidence"));
                }
                if let AcquisitionSpec::Provider { pin, .. } = &file.acquisition {
                    pin.validate()?;
                    if dependency.selected.as_ref() != Some(pin) {
                        return Err(invalid("File selection differs from owning dependency"));
                    }
                }
                if let AcquisitionSpec::Manual { pin: Some(pin), .. } = &file.acquisition {
                    pin.validate()?;
                    if dependency.selected.as_ref() != Some(pin) {
                        return Err(invalid(
                            "Manual file selection differs from owning dependency",
                        ));
                    }
                }
                for placement in file.placements.as_slice() {
                    validate_placement(placement)?;
                    if !destinations
                        .insert((placement.layer, placement.destination.relative().clone()))
                    {
                        return Err(invalid("Two placements occupy one layer destination"));
                    }
                }
            }
            if !self.coverage.contains_key(key) {
                return Err(invalid("Dependency coverage must be explicit"));
            }
        }
        if self
            .coverage
            .keys()
            .any(|key| !self.dependencies.contains_key(key))
        {
            return Err(invalid("Coverage references an unknown dependency"));
        }
        for (from, edges) in &self.required_edges {
            if !self.dependencies.contains_key(from)
                || edges.iter().any(|to| !self.dependencies.contains_key(to))
            {
                return Err(invalid("Dependency edge references an unknown selection"));
            }
        }
        Ok(())
    }
}

/// Internally coherent resolution. It does not prove installed bytes or authorize publication.
#[derive(Debug, Clone)]
pub struct ResolvedProject {
    intent: ProjectIntent,
    lock: ResolutionLock,
}
impl ResolvedProject {
    /// Validate exact selections, root completeness, pin ownership, slots and placements.
    /// The codec supplies the canonical intent revision; native observations remain separate.
    pub fn validate(
        intent: ProjectIntent,
        lock: ResolutionLock,
        revision: SemanticRevision,
    ) -> Result<Self, ModelError> {
        intent.validate()?;
        if lock.intent_revision != revision {
            return Err(invalid("Lock resolves a different intent revision"));
        }
        if lock.runtime.minecraft != intent.runtime.minecraft
            || lock.runtime.loader != intent.runtime.loader
            || (intent.runtime.loader_version.is_some()
                && lock.runtime.loader_version != intent.runtime.loader_version)
        {
            return Err(invalid("Locked runtime differs from intent"));
        }
        if lock.acceptable_versions != intent.runtime.acceptable_versions {
            return Err(invalid(
                "Lock resolves another accepted game-version policy",
            ));
        }
        for key in intent.roots.keys() {
            let selected = lock
                .dependencies
                .get(key)
                .ok_or_else(|| invalid("Lock omits an explicit dependency root"))?;
            intent.validate_root_selection(key, selected)?;
        }
        lock.validate_structure()?;
        Ok(Self { intent, lock })
    }
    /// Validated user intent.
    pub fn intent(&self) -> &ProjectIntent {
        &self.intent
    }
    /// Validated exact resolution.
    pub fn lock(&self) -> &ResolutionLock {
        &self.lock
    }
}

/// Choice keys are shared installation intent, so conflicting declarations fail before resolution.
fn validate_shared_choices<'a>(
    declared: &mut BTreeMap<&'a str, &'a OptionalChoice>,
    requirements: &'a Requirements,
) -> Result<(), ModelError> {
    for requirement in [&requirements.client, &requirements.server] {
        if let Requirement::Optional(choice) = requirement
            && let Some(previous) = declared.insert(choice.key.as_str(), choice)
            && previous != choice
        {
            return Err(ModelError(alloc::format!(
                "Conflicting optional choice: {}",
                choice.key.as_str()
            )));
        }
    }
    Ok(())
}
