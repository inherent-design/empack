# Build projections and verification

Target contract for v0.5.0-alpha.1. Code blocks are design sketches unless the
[implementation ledger](implementation.md) identifies a compiled API.

## 13. Build inputs, projections, and artifact verification

### 13.1 One exact build input

```rust
pub struct BuildInput {
    pub project: ResolvedProject,
    pub source: FrozenSourceTree,
    pub templates: TemplateSet,
    pub runtime: RuntimeResolution,
    pub tools: ToolchainResolution,
    pub options: BuildOptions,
}

pub enum BuildTarget { Mrpack, Client, Server, ClientFull, ServerFull }
pub enum ArchiveFormat { Zip, TarGz, SevenZip }

pub struct BuildRequest {
    pub targets: NonEmpty<BuildTarget>,
    pub archive: ArchiveFormat,
    pub input_policy: BuildInputPolicy,
    pub optional: OptionalSelectionPolicy,
}

pub enum BuildInputPolicy {
    RequireSatisfiedIntent,
    ObservedSnapshot { acknowledged: SnapshotBuildAcknowledgment },
}
```

Default build preflight requires every explicit provider identity/pin, URL expectation, local file, and environment requirement to be satisfied. It rejects unresolved or inconsistent inputs before modifying distribution outputs. An explicit observed-snapshot mode can preserve workflows that intentionally package current installed state, but the receipt labels that mode and records divergence from intent.

Do not silently call sync from build. A combined user command may submit sync then build as an explicit request sequence with defined failure behavior.

### 13.2 Pure projection and target capability checks

```rust
pub fn project_build(
    input: &BuildInput,
    target: BuildTarget,
    policy: &ProjectionPolicy,
) -> Result<BuildProjection, ProjectionError>;

pub struct BuildProjection {
    pub recipe: RecipeId,
    pub target: BuildTarget,
    pub inventory: ExpectedInventory,
    pub runtime_steps: Vec<RuntimePreparation>,
    pub format_constraints: FormatConstraints,
}

pub struct InventoryEntry {
    pub owner: ContentOwner,
    pub destination: InstallDestination,
    pub requirement: ProjectedRequirement,
    pub representation: ExpectedRepresentation,
    pub precedence: PrecedenceEvidence,
}

pub enum ExpectedRepresentation {
    Embedded { content: ContentId },
    DownloadReference { expected: ExpectedContent, allowed: DownloadOrigins },
    RuntimeGenerated { producer: RuntimeStepId, verification: RuntimeContract },
}
```

Every included item has an expected representation. Required provider files, unknown URL content, local files, embedded overrides, side-specific replacements, templates, runtime assets, and deliberately preserved observed content are accounted for. The inventory, not dependency source kind, decides verification.

If exact hashes are unavailable for a required downloadable reference, acquire and verify sufficient evidence or stop for input; do not mark an unknown expectation satisfied. `RuntimeGenerated` obligations are discharged after the responsible runtime step, recording observed content and its semantic checks before final inventory verification.

A representation check is target-specific. Reference-based packs need correct destinations, hashes, requirement metadata, and valid allowed locators; they need not download every remote byte again when adequate verified metadata already exists. Full distributions require the materialized bytes. A known failed download cannot be ignored just because another representation verified successfully.

### 13.3 Build prerequisites without a generic workflow framework

A small deterministic planner expands target prerequisites and deduplicates shared work. Lightweight client/server generation can share a newly produced intermediate mrpack within the same invocation. It cannot reuse an old file merely because name/version match.

`RecipeId` is a canonical digest over schema version, semantic resolution, relevant source/template content, selected runtime/tool identities, target, and meaningful options. Exclude machine-specific absolute paths and secret tokens. Include intentional output-affecting platform distinctions.

Initially, run every build from fresh staging. Add reuse only behind a verified recipe/content cache. Byte reproducibility additionally requires deterministic archive ordering, timestamps, permissions, and installer behavior; a correct recipe key alone does not guarantee it.

### 13.4 Templates and runtime preparation

```rust
pub trait TemplateRenderer: Send + Sync {
    fn render(
        &self, template: &TemplateSource, values: &TemplateValues,
    ) -> Result<RenderedFile, TemplateError>;
}

pub trait RuntimePreparer: Send + Sync {
    fn prepare<'a>(
        &'a self,
        step: &'a RuntimePreparation,
        stage: &'a StageWriteRoot,
        context: &'a ExecutionContext,
    ) -> PortFuture<'a, RuntimeObservation, RuntimeError>;
}
```

Preserve common/client/server template precedence, user-owned templates, binary-file copying, build-time metadata interpolation, loader-specific bootstrap/full behavior, and accepted historical runtime variants. Renderer selection is based on intended output language, not filename guesses alone.

Embedded default templates remain templates until build time. User-authored scripts are explicit inputs; do not silently rewrite them. Runtime preparation uses exact resolved requirements and bounded tools, records its outputs, and never returns “complete” just because an installer process exited.

### 13.5 Independent verification

```rust
// engine/verify.rs: fields and constructors remain private to this module.
pub(crate) struct VerifiedChange {
    stage: FrozenStage,
    publication: PublicationPlan,
    evidence: VerificationEvidence,
}

pub(crate) fn verify_change(
    expected: &ExpectedProject,
    stage: FrozenStage,
    readers: &VerificationReaders,
) -> Result<VerifiedChange, VerificationError>;

pub trait ArtifactReader: Send + Sync {
    fn inspect(&self, source: &mut dyn ContentRead)
        -> Result<ArtifactObservation, ArtifactError>;
}
```

Parse the actual candidate archive/index and compare it against the expected inventory. Do not trust the exporter to enumerate what it should have produced. Check missing and unexpected destinations, correct side/optional metadata, content identity, duplicate/collision rules, and referenced-content requirements.

Archive integrity is an additional check, not a completeness proof. A successful ZIP CRC says nothing about a required item that was never written.

Store a verification summary in the receipt, including inventory digest and artifact byte length from metadata. Distinguish pack semantics, byte integrity, and distribution-permission policy; successful byte verification alone is not evidence that redistribution is authorized.
