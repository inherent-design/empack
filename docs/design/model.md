# Semantic model and documents

Target contract for v0.5.0-alpha.1. Code blocks are design sketches unless the
[implementation ledger](implementation.md) identifies a compiled API.

## 4. Identity, paths, hashes, and versions

### 4.1 Use newtypes where confusion would change behavior

```rust
pub struct DependencyKey(String);          // User-facing logical label
pub struct BackendMetadataKey(String);     // Observed packwiz metadata key
pub struct ModrinthProjectId(String);       // Canonical validated provider ID
pub struct ModrinthVersionId(String);
pub struct CurseForgeProjectId(u64);
pub struct CurseForgeFileId(u64);
pub struct GameVersion(String);            // Provider/game syntax, not SemVer
pub struct LoaderVersion(String);
pub struct FileSlot(String);               // Stable role within a dependency

pub enum PinSelector {
    ModrinthVersion(ModrinthVersionId),
    CurseForgeFile(CurseForgeFileId),
}

pub enum ResolvedPin {
    ModrinthVersion { project: ModrinthProjectId, version: ModrinthVersionId },
    CurseForgeFile { project: CurseForgeProjectId, file: CurseForgeFileId },
}

pub enum ProviderProjectId {
    Modrinth(ModrinthProjectId),
    CurseForge(CurseForgeProjectId),
}

pub enum ProviderFileId {
    Modrinth {
        project: ModrinthProjectId,
        version: ModrinthVersionId,
        file: FileSlot,
    },
    CurseForge {
        project: CurseForgeProjectId,
        file: CurseForgeFileId,
    },
}

pub enum ProjectSelector {
    Query { text: String, providers: ProviderOrder },
    ModrinthId(ModrinthProjectId),
    ModrinthSlug(String),
    CurseForgeId(CurseForgeProjectId),
    ProviderUrl(ProviderProjectUrl),
}
```

Parsing a user string returns a selector. Provider resolution returns a canonical identity. A URL or slug is never assigned directly to a canonical ID field.

An input `PinSelector` does not require the caller to know a canonical project ID before resolving a slug. Resolution checks project ownership and content kind, then produces a project-bound `ResolvedPin` and exact selected `ProviderFileId` values. One pinned provider version may select multiple files. Provider strings have provider-specific parsers; generic trimming or case conversion must not silently change identity.

`DependencyKey` is not a path. Preserve a user's alias when updating an existing record. A command that deliberately renames a key produces a key change, not a reinstall.

### 4.2 Content identity versus provenance

```rust
pub struct ContentId([u8; 32]);             // Internal SHA-256 of actual bytes
pub struct RecipeId([u8; 32]);              // Domain-separated recipe digest
pub struct PlanId([u8; 32]);
pub struct SemanticRevision([u8; 32]);
pub struct DocumentRevision([u8; 32]);

pub enum ExpectedDigest {
    Sha512([u8; 64]),
    Sha256([u8; 32]),
    Sha1([u8; 20]),
    Md5([u8; 16]),
}

pub struct DigestSet {
    // Nonempty, one expected value per supported algorithm.
    values: Vec<ExpectedDigest>,
}

pub enum IntegrityEvidence {
    MatchedExpected { expected: DigestSet, actual: ContentId },
    ObservedOnly { actual: ContentId },
}

pub struct ProviderFingerprint {
    pub provider: ProviderKind,
    pub algorithm: FingerprintAlgorithm,    // For lookup, not strong byte identity
    pub value: Vec<u8>,
}
```

`DigestSet::parse` rejects malformed widths, duplicates with conflicting values, and unsupported algorithm declarations. Verify every supported declared digest, not just whichever one happens to match first. A policy may reject a source whose only digest is weak; compatibility mode must say what evidence it accepted. Internal SHA-256 addressing does not upgrade the trustworthiness of an upstream MD5 assertion.

Murmur2-style provider lookup fingerprints are not interchangeable with collision-resistant content identities. The packwiz format supports several hash and optional-file representations; retain compatible metadata without treating every algorithm as equally strong verification evidence. [F2](https://packwiz.infra.link/reference/pack-format/mod-toml/)

`ObservedOnly` is permitted for a newly added, explicitly accepted unknown local/direct file. It means “these are the bytes obtained,” not “these bytes matched an independent source declaration.” Reusing that observation later can detect change; it cannot retroactively authenticate the initial content.

### 4.3 Paths are values; roots are authority

```rust
pub struct PortableRelPath { components: Vec<String> }
pub struct InstallDestination(PortableRelPath);
pub struct ArtifactStem(String);
pub struct PathCollisionKey(String);

impl PortableRelPath {
    pub fn parse(input: &str, policy: PathSyntax) -> Result<Self, PathError>;
    pub fn components(&self) -> impl Iterator<Item = &str>;
}

pub enum PathSyntax { ProjectContent, ArchiveMember, ArtifactName }
```

Reject absolute paths, `..`, empty file paths, NUL, drive/prefix syntax, and target-platform-invalid components. Define separator handling once; do not accidentally interpret a backslash as a harmless character during validation and as a separator during extraction.

Do **not** copy a lowercase-only package path policy into Minecraft content. Valid pack filenames may contain spaces, Unicode, or brackets. Preserve spelling; compute a separate conservative collision key for portability checks. Case-folding or Unicode normalization for collision detection must not silently rename a user's content. [F2](https://packwiz.infra.link/reference/pack-format/mod-toml/)

A `PortableRelPath` proves syntax only. Resolving or deleting it requires a root-relative filesystem capability and an operation-specific expected file kind. Symbolic links, junctions/reparse points, hardlink aliasing, and filesystem races require native implementation tests.

### 4.4 Portable identity versus host identity

```rust
pub struct ProjectInstanceId([u8; 16]);     // Host registry identity, not pack name
pub struct OperationId([u8; 16]);           // Fresh admitted operation
pub struct AttemptId(u64);                 // Monotonic within an operation
pub struct HostRootBinding { /* private native identity */ }
```

Reproducible recipe hashes never include an absolute home directory. Publication authorization always includes the actual local project instance/root binding. A copied manifest does not copy the authority to write the original project.


## 5. The semantic project model

### 5.1 Four independent state objects

```rust
pub struct ProjectIntent {
    pub schema: SchemaVersion,
    pub metadata: PackMetadata,
    pub runtime: RuntimeIntent,
    pub roots: BTreeMap<DependencyKey, DependencyIntent>,
    pub layout: ContentLayout,
    pub distribution: DistributionIntent,
    pub extensions: ExtensionMap,
}

pub struct ResolutionLock {
    pub schema: SchemaVersion,
    pub intent_revision: SemanticRevision,
    pub resolver: ResolverIdentity,
    pub dependencies: BTreeMap<DependencyKey, LockedDependency>,
    pub dependency_evidence: DependencyEvidence,
    pub runtime: RuntimeResolution,
}

pub struct WorkspaceSnapshot { /* immutable, validated by snapshotter */ }
pub struct OperationJournal { /* private host-state representation */ }
```

Intent answers what is wanted. The lock records exact selections. Observation records what exists. The journal records unfinished or completed effects. None substitutes for the others.

### 5.2 Dependency sources and version policy

```rust
pub struct DependencyIntent {
    pub source: SourceIntent,
    pub kind: ContentKind,
    pub version: VersionIntent,
    pub placement: PlacementIntent,
    pub requirements: Requirements,
}

pub enum SourceIntent {
    Provider(ProviderProjectId),
    Search(SearchIntent),                  // Explicitly unresolved authoring form
    Url(UrlFileIntent),
    Local(TrackedFileIntent),
}

pub enum VersionIntent {
    FollowCompatible,                      // Lock still records an exact selection
    Exact(PinSelector),
    ContentPinned(DigestSet),
}

pub enum ContentKind {
    Mod, ResourcePack, ShaderPack, DataPack, World, Config, OtherFile,
}
```

A typed ZIP, a direct JAR, a provider project, and an archive entry are input routes, not four independent add implementations. A provider may not support every `ContentKind`; the provider adapter returns a capability error instead of coercing a world into a mod.

`Search` is explicit and cannot be a fallback after a malformed `Provider` or `Url` record fails to deserialize. Unresolved authoring intent is allowed; a strict build refuses an unresolved lock.

`sync` retains a valid exact locked selection unless intent changed or required resolution is absent. `update` deliberately refreshes eligible selections. `build` does not silently install, upgrade, or rewrite intent.

### 5.3 Exact files and placements

```rust
pub struct LockedDependency {
    pub identity: ResolvedIdentity,
    pub files: NonEmpty<ResolvedFile>,
    pub requested_pin: Option<ResolvedPin>,
    pub evidence: ResolutionEvidence,
}

pub enum ResolvedIdentity {
    Provider(ProviderProjectId),
    Url { logical_origin: UrlOriginId },
    Local { tracked: TrackedFileId },
}

pub struct ResolvedFile {
    pub slot: FileSlot,
    pub provider_file: Option<ProviderFileId>,
    pub acquisition: AcquisitionSpec,
    pub expected: ExpectedContent,
    pub placements: NonEmpty<Placement>,
    pub provenance: Provenance,
}

pub struct Placement {
    pub destination: InstallDestination,
    pub layer: ContentLayer,
    pub requirements: Requirements,
    pub owner: ContentOwner,
}

pub enum AcquisitionSpec {
    ProviderDownload(ProviderDownloadSpec),
    UrlAlternatives(NonEmpty<DownloadAlternative>),
    TrackedLocal(TrackedFileRef),
    Embedded(ArchiveEntryRef),
    Manual(ManualAcquisitionSpec),
}

pub struct ExpectedContent {
    pub digests: Option<DigestSet>,
    pub size: Option<u64>,
    pub accepted_observation: Option<ContentId>,
}
```

One dependency can have multiple files; identical bytes can have multiple placements. Do not deduplicate placements by `ContentId`. Cache bytes by content identity, but validate ownership and destination separately.

`DownloadAlternative` describes an allowed origin and locator without serializing credentials. Ephemeral signed URLs belong in the execution context and redacted diagnostics, not a reproducible lock fingerprint. A provider locator can be refreshed while preserving expected file identity.

`Provenance` includes source format, source record location, provider/file IDs where applicable, imported digest evidence, acquisition origin, and accepted conversion decisions. It is useful diagnosis and policy data, not a legal assertion about redistribution rights.

```rust
pub enum ContentOwner {
    Dependency { key: DependencyKey, slot: FileSlot },
    Override { layer: ContentLayer, source: PortableRelPath },
    Template { source: TemplateId },
    Runtime { step: RuntimeStepId },
    PreservedObserved { observation: ObservationId },
}

pub struct ResolvedProject {
    intent: ProjectIntent,
    lock: ResolutionLock,
    files: Vec<ResolvedFile>,
    identities: CanonicalIdentityIndex,
}

impl ResolvedProject {
    pub fn validate(
        intent: ProjectIntent, lock: ResolutionLock,
    ) -> Result<Self, ModelError>;
}
```

`ResolvedProject::validate` checks lock/intent correspondence, canonical-identity uniqueness under the chosen multi-file model, pin ownership, file placement requirements, and resolution completeness. It establishes a coherent resolved model, **not** that installed files already satisfy it. Snapshot/preflight verification establishes that separate fact.

`ContentOwner` is semantic provenance, not deletion authority. A planner derives managed ownership from trusted layout rules, prior receipts, and explicit user decisions; importing a record labeled as a template or runtime file cannot grant permission to overwrite user-owned content.

### 5.4 Requirements and override precedence

```rust
pub struct PerSide<T> { pub client: T, pub server: T }
pub type Requirements = PerSide<Requirement>;

pub enum Requirement {
    Unsupported,
    Required,
    Optional(OptionalChoice),
}

pub struct OptionalChoice {
    pub key: ChoiceKey,
    pub default_enabled: bool,
    pub description: Option<String>,
}

pub enum ContentLayer { Common, CommonOverride, Client, Server }
```

For a selected environment, common content is the base, common overrides replace that
base, and the matching side layer takes precedence over both. Common overrides live
under `overrides/common/`; this retains a declared download and an overriding file
without flattening either into the other's source identity. Same-layer destination collisions fail unless the source format explicitly defines an ordering that is represented in the model. Case-collision checks run on the final projected namespace as well as each layer.

A file being client-only does not say whether it is optional. The internal model retains both dimensions. Mrpack and packwiz encode these dimensions differently; adapters must establish representability before execution. [F1](https://support.modrinth.com/en/articles/8802351-modrinth-modpack-format-mrpack), [F2](https://packwiz.infra.link/reference/pack-format/mod-toml/)

A common file and an environment-specific replacement are two placements with documented precedence, not an accidental “last copy wins.” Optional omission follows the target's explicit choice policy. Optional metadata that a target can preserve stays in the output rather than being silently resolved away.

### 5.5 Dependency evidence is not execution scheduling

```rust
pub struct DependencyEdge {
    pub from: ResolvedDependencyRef,
    pub to: DependencyConstraint,
    pub relation: DependencyRelation,
    pub environment: EnvironmentPredicate,
    pub evidence: EvidenceSource,
}

pub enum Coverage { CompleteForSelection, Partial, Unknown }
pub struct DependencyEvidence {
    pub edges: Vec<DependencyEdge>,
    pub coverage: BTreeMap<ResolvedDependencyRef, Coverage>,
}
```

Provider dependencies are version- and environment-specific. Missing edges do not establish an empty dependency set. Retain unlisted installed content when closure evidence is incomplete.

Dependency graphs may contain legitimate mutually dependent groups. Analyze strongly connected components when necessary; do not impose Playground's asset-definition DAG rule on every package graph. Execution prerequisites, by contrast, must admit a valid execution order. Cache reachability is a third graph with different roots.

Removing an explicit root that remains required by another root may demote its explicit-root status while retaining installation. A request to uninstall it physically must identify dependent removals or report a conflict. Never silently expand one removal into removal of unrelated roots.


## 6. Documents and lockfiles

### 6.1 DTOs are not domain values

```rust
pub struct DocumentCodec;

impl DocumentCodec {
    pub fn decode_intent(
        &self, bytes: &[u8], origin: DocumentOrigin,
    ) -> Result<DecodedIntent, DocumentError>;

    pub fn decode_lock(
        &self, bytes: &[u8], intent: &ProjectIntent,
    ) -> Result<ResolutionLock, DocumentError>;

    pub fn patch_intent(
        &self, source: &DecodedIntent, delta: &IntentDelta,
    ) -> Result<PreparedDocument, DocumentError>;
}

pub struct DecodedIntent {
    pub intent: ProjectIntent,
    pub raw_revision: DocumentRevision,
    pub syntax: PreservedSyntax,
    pub field_origins: FieldOrigins,
}
```

Decode with an explicit schema/tag. Validate required fields before constructing domain types. Unknown intent fields fail unless contained in a documented extension namespace. A newer unsupported schema is read-only or rejected, never rewritten through an older partial model.

Retain user comments and unrelated supported fields where the editing library can do so. If lossless editing is unavailable, make full reformatting an explicit document-edit outcome. Backend-owned TOML fields outside empack's semantic subset remain preserved opaque document nodes with their original revision; do not drop them during normalization.

`create_project_plan`-style parsing errors must stay parsing errors. Only an actual missing file can trigger an explicitly supported absent-project flow. An invalid existing manifest must never be treated as permission to install with defaults.

### 6.2 Illustrative authoring document

Intent schema 2 and lock schema 1 are versioned separately from the program and APIs. The compiled codec accepts this authoring form:

```yaml
schema: 2
pack:
  name: Example Pack
  version: 1.0.0
runtime:
  minecraft: "1.21.1"
  loader:
    kind: fabric
    version: "0.16.0"
distribution:
  targets: [mrpack, client, server]
  archive: zip
layout:
  data-pack: world/datapacks
dependencies:
  renderer-alias:
    source:
      kind: provider
      identity:
        provider: modrinth
        project: AANobbMI
    content: mod
    version:
      mode: follow-compatible
    placement: automatic
    environment:
      client: required
      server: unsupported
extensions: {}
```

This is a schema illustration, not a recommendation that the example runtime versions are current. The lock records the selected provider release/file IDs, destination, source hashes, expected size, and resolution evidence. Literal IDs are strings even where they look numeric in another provider.

Human versions, schema versions, resolver versions, recipe versions, and artifact content hashes are separate fields. Do not make an archive contain a required hash of its own final bytes; put that digest in an external receipt or index.

### 6.3 Document replacement and observed drift

Use one normalized, explicitly versioned authoring schema and one lock schema.
Legacy schema conversion is not required. Unsupported schemas fail with a clear
message; they are never partially rewritten. A future explicit import or migration
can be added as a separate request if it becomes useful.

Preserve comments and unrelated valid fields during ordinary edits when the codec
can do so. If an edit must reformat a document, report that fact as part of the
planned document replacement. Unknown semantic fields remain errors outside the
extension namespace.

External packwiz edits are observed drift. Adoption incorporates selected drift
into intent and lock after review; sync restores recorded resolution. Neither
operation deletes unrelated files. Every document replacement binds to its raw
revision, including comment-only edits.
