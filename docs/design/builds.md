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

Selection may carry an `Unacquired` expectation while applying side, override and
optional choices. `BuildSelection` is not a completed inventory: finishing it rejects
any surviving unacquired item, and full targets also reject download references.
This lets a disabled optional replacement retain its common fallback without first
requiring the replacement's bytes. Missing work remains identified by the exact
locked file slot or observed backend metadata path.

Download origins distinguish stable URL alternatives from an exact provider pin and
file slot. Unknown size remains unknown; it never becomes a zero-byte assertion.
Completed references require real digest or accepted-observation evidence. Each
format then applies its own constraints: mrpack needs direct URLs, exact length,
SHA-1 and SHA-512, while packwiz can express an exact CurseForge metadata reference.
Full targets still require acquired bytes.

Every included item in the completed inventory has an expected representation. Required provider files, unknown URL content, local files, embedded overrides, side-specific replacements, templates, runtime assets, and deliberately preserved observed content are accounted for. The inventory, not dependency source kind, decides verification.

If exact hashes are unavailable for a required downloadable reference, acquire and verify sufficient evidence or stop for input; do not mark an unknown expectation satisfied. `RuntimeGenerated` obligations are discharged after the responsible runtime step, recording observed content and its semantic checks before final inventory verification.

A representation check is target-specific. Reference-based packs need correct destinations, hashes, requirement metadata, and valid allowed locators; they need not download every remote byte again when adequate verified metadata already exists. Full distributions require the materialized bytes. A known failed download cannot be ignored just because another representation verified successfully.

### 13.3 Build prerequisites without a generic workflow framework

A small deterministic planner expands target prerequisites and deduplicates shared work. Lightweight client/server generation can share a newly produced intermediate mrpack within the same invocation. It cannot reuse an old file merely because name/version match.

`RecipeId` is a canonical digest over schema version, semantic resolution, relevant source/template content, selected runtime/tool identities, target, and meaningful options. Exclude machine-specific absolute paths and secret tokens. Include intentional output-affecting platform distinctions.

Requested artifacts form one publication group by default. Prepare and verify all
candidates before publishing their combined file plan. Duplicate or portable-alias
output paths fail before recipes run. If a later recipe fails, retain every previous
artifact. Carry each target's source assurance, conversion choices and expected
member inventory into the result; concatenating archives is not a completion proof.

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

Template expressions choose their value syntax explicitly: `shell_quote` emits a
POSIX shell literal, `ini_quote` emits a scalar QSettings string, and
`properties_value` emits a Java properties value or value fragment. Raw text
substitution remains available for user-authored text. Do not infer shell quoting
from a filename or use HTML escaping for configuration files. The generated launcher
command must retain its argument quotes after INI parsing.

The launcher defaults follow [Prism's QSettings reader](https://github.com/PrismLauncher/PrismLauncher/blob/develop/launcher/settings/INIFile.cpp)
and [Qt's scalar encoding](https://github.com/qt/qtbase/blob/dev/src/corelib/io/qsettings.cpp).
Server values follow [Java Properties parsing](https://docs.oracle.com/en/java/javase/25/docs/api/java.base/java/util/Properties.html#load(java.io.Reader)),
including UTF-16 escapes for non-ASCII characters. Dynamic metadata belongs in
encoded values, not unescaped generated comments.

Embedded default templates remain templates until build time. User-authored scripts are explicit inputs; do not silently rewrite them. Runtime preparation uses exact resolved requirements and bounded tools, records its outputs, and never returns “complete” just because an installer process exited.

Full client output is a launcher instance: materialized game files under
`.minecraft`, root-level templates/configuration and `mmc-pack.json` with the exact
Minecraft and loader components. The launcher obtains its normal game binaries,
libraries and assets; “full” describes pack content, not an offline Minecraft
installation. ZIP is directly importable; TAR.GZ and 7z preserve the same tree for
explicit extraction. The component format follows [Prism's reader](https://github.com/PrismLauncher/PrismLauncher/blob/develop/launcher/minecraft/PackProfile.cpp).
A user component manifest may add exact components but must retain the locked
runtime without duplicate or disabled required entries. Captured custom launcher
configuration remains user input, including its commands. Template/game path
collisions require resolution before publication.

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

### Optional replacement with a fallback

An optional client file replacing common bytes at the same path needs conditional
fallback semantics: declining the optional file must retain common bytes, while
accepting it must install the replacement. Mrpack has optional download entries
and unconditional override directories, but no conditional override/fallback
relationship in its [format](https://support.modrinth.com/en/articles/8802351-modrinth-modpack-format-mrpack).
Do not emit conflicting paths and assume installer ordering implements that choice.
Require an explicit representable conversion or selected variant. Acknowledging
loss of optional descriptions/defaults alone does not authorize making an optional
file mandatory or deleting its fallback. Ordinary optional references remain
supported.

### Bootstrap reference trees

Prepare the packwiz tree from selected game obligations. Its index hashes every
metadata or embedded file; the pack document hashes the index. References preserve
exact destinations, approved origins, side and representable optional settings.
Keep original source evidence beside computed output hashes. Metadata-only references
must not claim an acquired content address.

Packwiz has one download locator per entry; select the first approved alternative
and retain the complete origin list in the expected inventory. CurseForge metadata
mode retains the canonical project and file ID. Paths for generated metadata must
be deterministic and collision checked against both source files and installed
payload destinations.

The [pinned installer's headless UI](https://github.com/packwiz/packwiz-installer/tree/v0.5.14/src/main/kotlin/link/infra/packwiz/installer/ui)
enables optional entries instead of honoring their defaults. Headless recipes must
resolve optional choices before encoding the tree. Interactive recipes may retain
independent optional references; grouped choices and optional embedded payloads
require explicit selection. Do not infer these behaviors from `side = "server"`.

Bundle reviewed installer versions and their checked digests. The bootstrap's
[`--bootstrap-no-update` and `--bootstrap-main-jar` options](https://github.com/packwiz/packwiz-installer-bootstrap/blob/v0.0.3/src/main/java/link/infra/packwiz/installer/bootstrap/Main.java)
keep launch behavior tied to those exact assets. An empack-maintained digest pin
must be attributed as such when upstream supplies no published checksum.
