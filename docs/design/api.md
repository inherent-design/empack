# Engine API and operation traces

Target contract for v0.5.0-alpha.1. Code blocks are design sketches unless the
[implementation ledger](implementation.md) identifies a compiled API.

The compiled entry point is `empack_lib::engine::api::Engine`. Its `preview`
and `prepare` accept an absolute `ProjectTarget` and a typed request for build,
import, initialization, addition, update, adoption, removal, synchronization, artifact
cleanup or recovery.
Each captures and plans without live-project writes.
Host-only cache maintenance uses `preview_cache_cleanup(CacheCleanRequest::All)`
and `prepare_cache_cleanup` after explicit `with_content_store` wiring. Preparation
receives only a read-only lookup. The returned operation uses the same exact grant,
engine ownership and operation handle as project requests, without a fabricated
project root or access to its recovery journal. Unknown cache neighbors remain.
Changed native objects invalidate the selection before deletion. Newly inserted
objects are outside its authority. Active verified leases retain private copies.
Eviction reports removed and retained objects; a later failure returns
`PartiallyCompleted` with the cause, while a lost worker returns `ExecutionUncertain`.
This maintenance outcome does not implement independent dependency batches.
`OperationPreview` carries the operation-specific view. A build view includes exact artifact destinations,
runtime, missing content, network/tool requirements and the complete requested
options. It has no conversion into an executable operation. A ready preparation
can be consumed with an `ExecutionGrant` naming its opaque in-process `PlanId`;
the originating engine then admits `start`.

The owned driver acquires captured archives and remote content, prepares the exact
runtime, verifies every requested artifact and publishes their union. Missing
manual acquisitions remain `NeedsInput`. Attach `ProviderCatalog` with
`with_provider_catalog` to refresh exact provider file locators during authorized
execution. Preparation sees only provider availability and performs no API lookup.
The refresh retains every locked byte assertion and placement; changed declarations
fail, and a restricted file without a locator becomes explicit manual input. Missing
provider credentials remain preparation input. The build API does not invent a
provider result or download association. Normal cache selection, durable continuation
and CLI composition remain completion work. The broader interface below
remains the target where the implementation ledger identifies an outstanding API.

`engine::runtime_catalog::RuntimeCatalog` provides read-only official Minecraft and
loader choices through owned, bounded HTTP acquisition. `games` returns retained
`GameVersions`; `loaders` returns retained `LoaderVersions` for one exact game and
loader family. Their `resolve` methods check requested membership and return an
exact selection. The host decides whether that selection was explicitly pinned;
discovery never changes intent or publishes documents. Defaults select the official
Minecraft release and prefer stable loader releases. Explicit historical and
prerelease selections remain available. A catalog digest identifies the observed
response, not the executable bytes; build acquisition verifies those separately.

`BuildRequest::with_content` attaches explicit `BuildAcquisitions` without putting
leases in a display-only preview. Supplied keys must match current acquisition
obligations. Preparation streams those bytes through the unchanged locked/backend
assertions and evidence policy; unrelated keys and mismatches fail before approval.

A build `Preparation::NeedsInput` retains an in-memory `PreparationContinuation`.
Its view contains only display data. `Engine::resume` accepts additional verified
files, revalidates the captured project, retains earlier supplied files and issues a
new plan. Another engine, changed inputs, a retargeted project selection or duplicate
supplied keys invalidate the resume. Dropping the continuation releases retained preparation and content resources.
This does not persist a continuation or grant publication authority. Explicit grants
remain required after every obligation is satisfied.
Resume keeps its previous preparation reservation charged and admits only the extra
capacity needed for capture. The worker returns the replacement under the same retained
reservation; a host sized for one capture does not need to budget for two preparations.

Preparation, acquired content and retained receipts own explicit host admission
estimates. Those estimates are separate from enforced stream/snapshot byte limits.
Abandoned preparations retire before their registry entries disappear. Publication
receipts survive cancellation after commitment; a runtime-level worker failure or
`ExecutionUncertain` requires recovery assessment and must not be presented as proof
that no files changed. Native publication errors retain their recovery operation ID.
The compiled usage example is tested with Rust documentation tests in
[`api.rs`](../../crates/empack-lib/src/engine/api.rs).

The compiled `ProviderCatalog::resolve_pin` accepts a provider-qualified `PinSelector`
without a caller-supplied project. It returns a retained exact `ProviderResolution`
after provider ownership and file assertions are checked. This is distinct from
`resolve_exact`, which additionally requires the caller's asserted owner to match.

The compiled `ProviderCatalog::resolve_compatible` accepts a canonical project and a
`CompatibleRequest` containing ordered acceptable game versions, loader, content kind
and explicit `ReleasePolicy`. `SelectionLimits` bounds all pages, records and transferred
bytes under one deadline. Its retained result contains the exact provider resolution,
selected content kind, release channel, publication time and matched game version.
`CanonicalProject::kinds` describes the project-wide union;
`ProviderResolution::kinds` describes the exact selection. Neither substitutes for
individual file roles or placement choices. Pagination failure
or an incomplete bounded catalog returns an error, never a best-effort partial choice.
The resolver has no publisher and does not replace the exact lookup used by builds.

The compiled `ProviderCatalog::search_projects` accepts `SearchQuery` with explicit
provider order, content kind, accepted games, loader and page offset. `SearchLimits`
bounds each window and the shared transport budget. `ProjectSearch` owns retained
`SearchPage` values and unsupported-provider evidence. A page contains
`ProjectCandidate` choices, its source total/ranks, `has_more` and `next_offset`.
Similarity is display guidance; there is no automatic exact-match outcome.

The compiled `ProviderCatalog::resolve_required_closure` accepts exact `ClosureRoot`
values plus explicit game, loader and release constraints. `ClosureLimits` bounds
projects/edges and carries a shared catalog budget. `ProviderClosure` retains selected
records, canonical required edges and `ClosureIssue` values. Its
`complete_for_required()` query applies only to the observed exact selections; it is
not a deletion grant or proof that another version assignment cannot work.

`ProviderCatalog::configure_acquisition` projects configured credentials into fixed
CDN origin rules without returning the key. `Engine::with_provider_catalog` applies
that configuration automatically. Neither method makes requests or grants network
authority.

`ProviderCatalog::resolve_addition` connects `ProviderAddInput` selectors to canonical
dependency groups. Slugs, IDs and project URLs use the same catalog boundary. Requested
pins remain exact intent; compatible selection remains unpinned. Selector lookup,
compatible selection and required-closure expansion share one byte/deadline budget.
`ProviderAdditionOutcome::NeedsInput` retains unresolved closure evidence without
producing a publishable subset. `Ready` retains provider records, the normalized
project and `AdditionGroup` for native preparation. It has no payload downloader or
project writer. Root requirements propagate through required edges; incompatible
optional choices require an explicit conversion. Default placement follows content
kind and configured layout; datapacks/worlds require a selected folder when none is
configured. `ProviderFiles::Placed` preserves per-file destinations and participation
for companion files, recording the explicit conversion. Required companions cannot
be omitted. Required dependencies reuse valid current exact selections and retain
their original assertions, aliases and placements. A changed pin, incompatible
runtime or insufficient participation needs an explicit change. Generated labels
reserve explicit roots and current records before choosing a disambiguated label.
CLI selection and local/URL hosts remain separate integration work.

`ProviderAddition::acquire_content` accepts one explicit `ProviderContentChoice`
per locked file: reference, acquisition, or supplied verified bytes. It validates the
whole decision set before downloading, reuses retained exact catalog records and
shares one transfer allowance across HTTP files. Restricted or unsupported
acquisition returns `ProviderContent::pending` with original assertions before
starting automatic transfers. `deferred_downloads` identifies those postponed slots
for a later request; a remote failure cannot hide the user-input requirement. Only a
complete inventory can pass native addition preparation; pending content is never
counted as a successful requested item. Supplied files are matched by exact logical
slot and every locked assertion, preserving their portable permissions. This is
in-memory preparation; durable manual continuation remains separate work.

`FileAddition::from_acquired` normalizes direct local and URL files into the same
`AdditionGroup` and per-slot content inventory. Explicit destinations, environment
requirements and source permissions are preserved. Local intent tracks the first
published placement; it never persists the source's absolute host path. URL input
retains ordered credential-free alternatives and source digests. `FileEvidence`
distinguishes declared expectations from explicitly accepted initial observations;
computed addresses do not manufacture independent source authenticity. Direct files
retain unknown dependency coverage until identification establishes more. Host file
selection, type identification and archive interpretation precede this boundary.

`acquire_local_file` reads an explicitly selected absolute host file into a private
verified lease. It captures only that regular file, rejects symbolic links and special
files, checks source identity and content across copying, and preserves portable
permissions. Scratch admission uses observed size rather than the configured maximum.
Source names remain native OS strings; UTF-8 and portable destination rules do not
apply to the selected host basename. Initial bytes without source evidence require explicit acceptance; their computed
address remains an observation. The source path is not durable project intent.

The compiled `ImportContentPlan::resolve` accepts retained import declarations and a
catalog, then resolves all exact provider references under one allowance.
`ImportContentPlan::acquire` accepts explicit supplied files keyed by
`ImportContentKey`; it returns `NeedsInput` with the retained plan/supplied files or
`Ready(VerifiedImportContent)` only after every requested byte obligation verifies.
Original declarations, provider records, source evidence and permissions remain
accessible. The result has no publisher, and does not substitute for a semantic
candidate or an approved replacement plan.

`VerifiedImportContent::into_candidate` consumes the verified inventory and explicit
`ImportCandidateOptions`. Every file needs an `ImportFileDecision` preserving source
participation. Provider files share canonical identity and exact pins; declared files
retain their destination and layer. Optional defaults, provider placements and ambiguous
runtime choices are host decisions. URL persistence enforces the durable locator policy;
local retention is an explicit conversion. `ImportCandidate` exposes coherent documents
and file-slot bindings to retained bytes, with no publication authority.

`ProjectReader::capture_replacement` reads the bounded managed footprint of an existing
directory without requiring a valid old manifest. `prepare_project_replacement` consumes
that snapshot and an `ImportCandidate`, returning a `PreparedProjectReplacement` only
when its complete frozen file inventory verifies. Its plan names every replacement
and removal; publication revalidates the captured source. This lower-level composition
requires a trusted host to approve the exact plan.

`ImportRequest` consumes the candidate and replacement policy. Engine preparation
captures the existing destination and verifies privately staged content. `ProjectChangePreview`
exposes the complete file plan, metadata and runtime. Replacing existing managed files
requires an `ExecutionGrant` carrying the exact `ReplacementSummary` from that view;
missing or stale acknowledgements fail before publication. New-file-only plans need no
replacement acknowledgement. The engine also checks the originating instance and plan.

Build, import, initialization and removal share the owned operation registry and
`ExecutionOutcome`. Completed operations retain typed build, import, initialization
or removal receipts;
interruption and recovery outcomes are shared. Preparation reserves the candidate and
its staging copy, then retains the actual staged byte allowance until publication retires.
Import source resolution/acquisition remains an explicit earlier read-only composition.
`ProjectTarget::New(path)` selects one absent child of an existing parent for initialization or import.
Preparation retains the parent identity and absence without creating the destination
or host state. After approval, the publisher verifies a complete same-filesystem
candidate directory, records recovery intent and uses a no-replace rename. A racing
file, directory or link cannot become a replacement target. `Existing(path)` retains
the file-level replacement protocol. A `PathBuf` argument defaults to `Existing`.

`InitializeCandidate::new` checks empty-root intent against an exact runtime selection,
then encodes coherent intent and lock documents. Metadata, acceptable game versions,
layout, distribution settings and extensions survive. It does not claim remote runtime
availability; build acquisition verifies runtime declarations and bytes. Nonempty roots
must go through dependency resolution instead of receiving an empty lock.
`InitializeRequest` uses the shared replacement policy, preview and grant. Optional
common/client/server template seeds fill missing rendered destinations. Existing user
files, including their permissions, stay outside the publication footprint. A literal
file, a `.template` variant or a common-layer output prevents a default seed from
shadowing that destination; portable case and ancestor collisions count too. Capture selects only entries that can collide with seed outputs, before opening
unrelated contents or validating unrelated filenames. Selected entries and filtered
directory membership remain publication preconditions; unrelated templates remain
user-owned, even when large, unreadable, linked or nonportable. Traversal still has a
finite entry budget. Directory-valued destinations and link traversal fail. Default seeds
contain editable client configuration and server properties; selected build recipes
supply their exact installer and launch scripts. No user script feature is removed.

Creation journals bind the native candidate/root identity, with a parent/name index
for recovery before the root exists. Ordinary reads use the root identity to gate
unfinished creation, including after a move or through an alias. The lower-level
`Publisher::recover_new` completes retained creation without tools or downloads.
Publication and creation recovery are available through `Engine::inspect_recovery`
and `RecoverRequest`. `RecoveryStatus::kind` distinguishes the two journal types. Inspection reads only host-owned journals and does not require
valid project documents. Preparation verifies retained images and exposes the exact
remaining file changes for finishing or restoring the operation. The ordinary
plan-specific grant and engine ownership checks apply. Execution rechecks journal
revision and observed files under exclusive publication ownership; stale approval
cannot recover a different operation or overwrite a later edit. The retained
`RecoveryReceipt` distinguishes published from restored state. Failed preparation or
execution leaves the prior recovery record available for inspection. Creation recovery
finishes either an absent destination or an already visible retained root, including
a moved root. Its approval binds the parent, selected name, native identity and journal
revision. It refuses a new occupant or progress since preparation. Creation has no
restore action: deleting a project root requires separate authority. Recovery preview
preserves journal scratch; only approved execution cleans it. Durable continuation
and the remaining CLI composition remain unfinished.

`SynchronizationCandidate::prepare` rebinds authoring-only edits to the recorded
resolution without selecting newer files. It preserves all exact selections, original
source evidence and dependency edges, including installations no longer listed as
roots. A unique renamed provider label rebinds the same exact installation and graph
edges. Pin, source, placement or runtime edits that invalidate the old resolution
require a fresh candidate. `capture_synchronization` and `prepare_synchronization` bind
recorded placements and verified supplied bytes to a restoration plan. Modified files
are explicit replacements; unrelated content remains. Metadata discovery tolerates
opaque unrelated records and binds only interpretable records naming locked destinations.
No-op content remains a publication precondition without being staged again.
`SyncRequest` carries verified exact inputs through the shared Engine lifecycle.
`SyncPreview` lists selected records, lock rebinding and the replacement plan;
`SyncReceipt` retains the coherent published project. `SyncRequest.resolution` may
supply fresh resolution for changed or missing intent. The planner preserves valid
exact selections, rejects unrelated upgrades/deletions and requires new records to
belong to the changed roots' required closure. Known retained dependents remain binding.
A missing lock requires fresh resolution and cannot authorize adoption of existing
files. Changed placements capture both paths; obsolete files must still match their
old declarations before removal. Native publication preserves authoring bytes.
If an obsolete payload is absent, immutable acquired bytes may supply a missing
digest algorithm only when they satisfy every old digest, size and accepted
observation. Without that evidence, obsolete metadata remains untouched.
Resolution/acquisition hosts and CLI routing remain pending; a supplied candidate
alone has no write authority.

`UpdateRequest` supplies exact resolved selections and acquired content for requested
installed identities. It shares dependency capture, staging and publication with add,
but preserves current authoring intent byte-for-byte. Explicit pins remain binding.
Temporary request roots may select transitive installations without promoting them;
known retained dependents must be included when their required selection changes.
Uninstalled requested identities fail rather than becoming additions. New required
closure entries remain justified by the resolved group. `UpdatePreview` exposes
canonical bindings, selected records and the exact replacement footprint; the retained
`UpdateReceipt` contains the published project. Compatible selection by the host and
CLI wiring remain separate work.

`AdoptObservedRequest` supplies a resolved group describing selected files already
present in a project. Preparation verifies their original digest, size and
accepted-observation assertions, plus applicable backend identity and requirements.
It does not acquire missing files or rewrite payloads. Canonical aliases and retained
dependency constraints use the same addition planner. `AdoptObservedPreview` exposes
the exact document changes; execution requires their plan-specific grant and returns
`AdoptObservedReceipt`. Selected payloads remain publication preconditions even though
only intent, lock and affected index/pack documents may change. Repeated adoption is
a no-op. A missing lock can be created when the selected group resolves all retained
authoring roots and runtime requirements. Missing-lock adoption verifies every
selected placement; an unresolved retained root or a newly occupied lock destination
blocks publication. Existing authored roots remain unchanged: their source identity,
pin and placement constrain the proposed resolution. Only newly adopted root keys
are inserted. A present stale/invalid lock is not treated as absence. CLI
selection remains integration work.
Adoption intentionally accepts selected drift after review: existing bytes must
match the proposed complete assertions, not the superseded lock's byte assertions.
The old documents remain captured publication preconditions and recovery preimages;
retained dependency constraints still bind. Requiring the old bytes would turn this
operation into sync and prevent adoption of external edits. Indexed metadata keeps
its metadata role, aliases and extension fields when its digest is refreshed.

`AdditionGroup::from_resolved` extracts a validated, root-reachable dependency request.
`AdditionCandidate::prepare` binds it to coherent source documents, preserves canonical
aliases and rejects conflicts with retained selections. Its `AdditionPlan` exposes
requested-to-canonical key bindings and changed exact entries; neither grants file
mutation authority. `ProjectReader::capture_addition` captures old and new selected
placements. `prepare_addition` binds supplied request slots to canonical keys, verifies
original byte assertions, and prepares exact content/document changes. Untracked
collisions require a separate adoption decision. Changed selections retire owned stale derivative metadata; existing direct index
entries retain their fields and aliases while their byte digests are updated. `PreparedAddition::publish`
uses the shared recovery journal. The shared Engine accepts `AddRequest` with a resolved
group and immutable acquired content, defaults to rejecting existing requested roots,
and supports explicit same-identity updates or `ReplaceSelected(ReplacementSelection)`.
Replacement requires exact installed keys, each supplied as an explicit new root. It
composes removal safety and addition into one candidate; no intermediate deletion is
published. A replacement cannot redirect to another retained alias or modify an
existing root outside the selection. Known retained dependents block replacement;
unknown evidence requires explicit acknowledgement. `AddPreview` exposes prior exact
records and acknowledged incomplete evidence, and the receipt retains both.
`AddPreview` also exposes canonical bindings,
existing roots, file changes and the exact replacement summary; `AddReceipt` retains
the resulting project and publication result. Provider/local/URL request resolution,
and ContinueIndependent integration remain pending.

`AddRequest`, `UpdateRequest` and `SyncRequest` describe every file slot with
`DependencyContent::Materialized` or `DependencyContent::Reference`. Omitting a slot
is an error. A reference records an exact provider, URL or manual byte obligation;
it does not certify acquired bytes. Local and archive-member inputs require
materialization. Previews and receipts expose canonical reference slots. Matching
existing payloads remain unchanged; a reference-only update may retire verified old
payloads through its explicit replacement plan. Modified user files cannot be
discarded this way. The lock carries the obligation, and each build target decides
which bytes or generated backend references it needs. No new dependency metadata
must be generated in `pack/` merely to register an obligation.

`ProjectReader::capture_removal` resolves user selections from document/metadata
observations and captures their exact managed placements into a `MutationSnapshot`.
`capture_mutation` supports callers needing every locked placement. Both exclude
unrelated payloads; backend discovery retains finite traversal and metadata limits.
Excluded directory members cannot establish target absence for another request. `prepare_removal` consumes that snapshot and explicit logical keys
with `RemovalMode::ForgetRoots` or `RemoveContent`. Its read-only file plan precedes
any publication. `PreparedRemoval::publish` consumes verified documents and file
changes through the common publisher, returning the selected keys, actual removal
mode, resulting project and publication receipt. The shared `Engine` accepts `RemoveRequest` with exact logical keys or user queries and an explicit
mode. `RemovalEvidencePolicy` defaults to `RequireComplete`; explicit
`AcknowledgeUnknown` permits incomplete retained dependency evidence while recording
it in `RemovePreview::incomplete_evidence` and `RemoveReceipt::incomplete_evidence`.
Known required dependents still block removal. `RemovePreview` lists canonical
selections and the exact file footprint;
authorization requires its plan and replacement digest. `RemoveReceipt` distinguishes
demotion from physical removal. Source verification streams without retaining payload
copies, then staging reserves the actual candidate-document bytes. Publication reserves
its before-images separately. Missing selections, stale inputs or failed admission
cannot produce a successful subset. Exact logical keys win over colliding metadata stems. Queries otherwise combine
ASCII-case-insensitive titles and exact installed stems, bind metadata to an exact
locked file, and reject ambiguous or missing selections. Equivalent queries collapse
to one logical selection. An untracked installed stem remains an observed selection;
`RemovalSelector::Metadata` disambiguates exact pack-relative metadata paths. It
cannot be demoted as an authoring root. Native preparation verifies its regular file
against the captured declared digest and rejects document roles, directories and
links. Preview and receipt expose `observed` selections without download locators.
Untracked or conflicting retained metadata appears in `untracked_evidence` and
requires the same explicit uncertainty acknowledgement. Observed-only removal keeps
both logical documents byte-for-byte unchanged. CLI wiring remains integration work.

`CleanRequest::Artifacts` captures only the managed `dist` namespace. It exposes every
regular file and its removal size in `CleanPreview`; approval names that exact footprint.
Cleanup does not parse authoring files or read unrelated pack/template bytes. Changed
artifact bytes or directory membership invalidate execution. Links and special files
are refused, and interrupted removal uses the ordinary finish/restore journal. Empty
directories remain. `removed_bytes` reports logical bytes removed from `dist`, not
physical space reclaimed from recovery storage. Cache maintenance has separate host
authority and remains outside this project request.

## 17. Public engine API and application wiring

### 17.1 Public entry points

```rust
pub struct Engine { /* narrow services composed internally */ }

pub struct PrepareOptions {
    pub network: NetworkPermission,
    pub cache: CacheAccess,
    pub limits: PreparationLimits,
}

pub enum Preparation {
    Ready(PreparedOperation),
    NeedsInput(PreparationContinuation),
    Blocked(PreparationReport),
}

impl Engine {
    pub async fn preview(
        &self, target: ProjectTarget, request: Request, options: PreviewOptions,
    ) -> Result<PreviewReport, EngineError>;

    pub async fn prepare(
        &self, target: ProjectTarget, request: Request, options: PrepareOptions,
    ) -> Result<Preparation, EngineError>;

    pub fn start(&self, operation: ApprovedOperation)
        -> Result<OperationHandle, AdmissionError>;

    pub async fn resume(
        &self, target: ProjectTarget, input: ContinuationInput,
        decisions: DecisionSet, options: PrepareOptions,
    ) -> Result<ResumeAssessment, EngineError>;

    pub async fn inspect_recovery(
        &self, target: &ProjectTarget,
    ) -> Result<Option<RecoveryReport>, EngineError>;

    pub async fn recover(
        &self, target: ProjectTarget, decision: ApprovedRecovery,
    ) -> Result<RecoveryOutcome, EngineError>;

    pub async fn shutdown(&self, policy: ShutdownPolicy)
        -> Result<ShutdownReport, EngineError>;
}
```

`ProjectTarget` selects an existing or new root; the compiled variants are `Existing(PathBuf)` and `New(PathBuf)`. Both must resolve into host-bound root capabilities; creation is also a publication effect. For a nonexistent destination, bind and lock the existing parent plus validated child name and its expected absence until creation. Do not create the destination or a persistent project registration during preview; use a proposed in-memory identity until execution is authorized. Source/destination path selection belongs at the public boundary, not inside imported content.

`preview` uses the same resolver and pure planner as execution, but composes read-only cache/storage capabilities and no publisher or mutating backend capability. It returns plan confidence, required decisions, and unresolved blockers. Temporary downloads are bounded and removed. It does not serialize a durable operation as a side effect.

`prepare` can use the explicitly granted cache policy. It never changes live project/artifact content. Required verification that can be done before tools is completed here; required backend verification occurs on staging before publication.

`PreparationContinuation` is an in-memory continuation by default. Persisting it requires an explicit authorized operation or host action. A preview cannot obtain that writer by converting its report.

Host-only cache maintenance does not require a configured pack or a fabricated project snapshot. Give it a narrow `preview_cache_cleanup(CacheCleanRequest) -> CacheCleanupPlan` and `prepare_cache_cleanup(CacheCleanRequest) -> PreparedCacheCleanup` entry point. The latter is authorized and started through a maintenance handle; the store validates ownership, leases, expected identities, and verified eviction before mutation. It reuses admission, cancellation, read-only preview, receipts, and recovery primitives, but has no project writer. Its `CacheCleanupReceipt` reports reclaimed, retained, and failed objects. A combined `clean all` aggregates project and host-maintenance outcomes explicitly; it does not claim an atomic transaction across them. This is a distinct domain plan, not a second permissive deletion implementation.

### 17.2 Operation handles and outcomes

```rust
pub struct OperationHandle { /* observation + cancellation, not task resources */ }

impl OperationHandle {
    pub fn id(&self) -> OperationId;
    pub fn status(&self) -> Arc<OperationStatus>;
    pub fn subscribe(&self) -> ProgressSubscription;
    pub fn cancel(&self, reason: CancelReason);
    pub async fn wait(&mut self) -> Arc<OperationOutcome>;
}

pub enum OperationOutcome {
    Completed(OperationReceipt),
    PartiallyCompleted(PartialReceipt),
    NeedsInput(ContinuationReceipt),
    FailedBeforePublication(FailureReport),
    InterruptedBeforePublication(InterruptionReport),
    RecoveryRequired(RecoveryRequired),
}

pub struct OperationReceipt {
    pub operation: OperationId,
    pub plan: PlanId,
    pub before: SourceRevision,
    pub after: SourceRevision,
    pub applied_groups: Vec<GroupId>,
    pub artifacts: Vec<ArtifactReceipt>,
    pub verification: VerificationSummary,
    pub warnings: Vec<Diagnostic>,
}
```

The handle does not own task reservations or the only journal reference. Dropping it requests cancellation according to its configured policy, but driver retirement is engine-owned. `wait` returns a retained terminal outcome even if a notification was missed.

`PartiallyCompleted` includes a committed receipt for applied groups and separate blocked/failed groups. `NeedsInput` is not falsely labeled success or generic backend failure. When some groups were already published, use `PartiallyCompleted` with pending-input details rather than an outcome that implies no publication. `RecoveryRequired` distinguishes effects that may already be visible from failure before publication.

Errors use stable diagnostic codes and structured causes. A CLI adapter may preserve existing numeric exit classes, including cancellation, without making library users parse text. Include file/record locations, provider identity, operation ID, and a next safe action. Redact credentials by construction.

### 17.3 Composition root

This is illustrative wiring, not a proposed public service-locator interface:

```rust
pub fn assemble(config: HostConfig) -> Result<Engine, SetupError> {
    let limits = Arc::new(ResourceGovernor::new(config.resources)?);
    let runtime = Arc::new(OperationRuntime::new(limits.clone()));
    let roots = Arc::new(NativeRootFactory::new(config.filesystem_policy)?);
    let host_state = Arc::new(HostStateStore::open(config.state_root)?);
    let content = Arc::new(FileContentStore::open(config.cache_root, limits.clone())?);

    let transport = Arc::new(PolicyTransport::new(
        config.network, config.credentials, limits.clone(),
    )?);
    let acquisition = Arc::new(VerifiedAcquisition::new(
        transport.clone(), roots.scratch_factory(),
    ));
    let catalog = Arc::new(ProviderRegistry::new(vec![
        Arc::new(ModrinthCatalog::new(transport.clone())),
        Arc::new(CurseForgeCatalog::new(transport.clone())),
    ]));

    let processes = Arc::new(OwnedProcessRunner::new(runtime.clone()));
    let tools = Arc::new(LazyToolResolver::new(
        acquisition.clone(), processes.clone(), config.tools,
    ));
    let backend = Arc::new(PackwizAdapter::new(tools.clone(), processes.clone()));
    let codecs = Arc::new(ProjectCodecs::new());
    let journals = Arc::new(JournalStore::new(host_state.clone()));
    let reader = Arc::new(ProjectSnapshotter::new(
        roots.project_reader(), codecs.clone(), journals.reader(),
    ));

    let preparation = PreparationService::new(
        reader, catalog, acquisition.clone(), codecs.clone(),
        ImportRegistry::standard(),
    );
    let staging = StagingService::new(
        roots.stage_factory(), acquisition, backend, RuntimeAdapters::new(tools, processes),
        codecs,
    );
    let verification = VerificationService::new(ArtifactReaders::standard());
    let publisher = Publisher::new(roots.publisher_fs(), journals, host_state);
    let access = AccessScopes::new(content); // Only Engine can derive per-call capabilities.

    Ok(Engine::compose(runtime, access, preparation, staging, verification, publisher))
}
```

Only `assemble` sees all dependencies. `PreparationService` does not receive `Publisher` or a capability factory that can grant itself cache writes. `Engine::preview` passes read-only per-call acquisition authority; `Engine::prepare` derives only the cache authority the caller allowed. Root factories are projected into scratch, staging, reader, and publisher interfaces, and snapshotting receives only the journal reader. A snapshotter therefore cannot delete stale recovery state. `PackwizAdapter` does not receive project-document publication. `VerificationService` does not trust a backend-provided expected inventory.

The constructor names are proposed concrete adapters. In implementation, prefer grouped typed configuration objects when a constructor grows too large, but do not introduce `services: Arc<Everything>` to shorten the signature.

### Implemented initialization host

`application::engine_host::initialize` composes parsed `InitArgs`, session interaction,
read-only runtime discovery and approved Engine initialization. It retains metadata,
explicit loader pins, accepted game versions and datapack-folder choices. Exact
explicit runtime coordinates can initialize offline; build preparation still verifies
their executable availability and bytes. Missing runtime choices use bounded official
catalogs. Without an explicit loader family, the host selects Minecraft first and
offers only families with observed compatible versions, alongside Vanilla. Failed
family lookups are disclosed and do not become selectable compatibility claims.
The chosen family reuses its retained catalog. No failed lookup is replaced with
an invented version.

The host displays native file changes before confirmation. Preview, declined plans
and invalid options cannot create a project or durable host state. Forced replacement
requires the displayed footprint and preserves user templates and unrelated files.
Initialization and recovery share approval, cancellation, shutdown and outcome
classification. The initialization host currently accepts empty projects; source imports
have a separate normalization path. CLI dispatch cutover remains pending until the
other command hosts can consume these documents together.

### Implemented recovery host

The CLI `recover [inspect|finish|restore]` composes the existing inspection and approved
recovery APIs. Inspection is the default. `--operation` can bind the selected journal;
preview and declined confirmation remain read-only. Execution uses the normal owned
handle and drains workers on cancellation or error. `--state-dir` / `EMPACK_STATE_DIR`
select durable host state independently of disposable caches. Existing and absent
project selections support publication and creation recovery respectively. This host
does not parse project intent or bootstrap packwiz.

### 17.4 CLI usage

```rust
async fn run_change(
    engine: &Engine,
    args: ChangeArgs,
    ui: &mut dyn UserInterface,
) -> Result<ExitClass, CliError> {
    let (target, request) = translate_args(args.clone())?;

    if args.dry_run {
        let report = engine.preview(target, request, args.preview_options()).await?;
        ui.show_preview(&report)?;
        return Ok(ExitClass::from_preview(&report));
    }

    let prepared = match engine.prepare(target, request, args.prepare_options()).await? {
        Preparation::Ready(value) => value,
        Preparation::NeedsInput(value) => {
            return ui.handle_preparation_input(engine, value).await;
        }
        Preparation::Blocked(report) => {
            ui.show_preparation_failure(&report)?;
            return Ok(ExitClass::from_preparation(&report));
        }
    };

    let grant = ui.authorize(prepared.view(), args.interaction_policy()).await?;
    let approved = prepared.authorize(grant)?;
    let mut operation = engine.start(approved)?;
    ui.observe(operation.subscribe())?;
    let outcome = operation.wait().await;
    ui.show_outcome(&outcome)?;
    Ok(ExitClass::from_outcome(&outcome))
}
```

This `UserInterface` is an illustrative host interface; an object-safe implementation uses the same boxed-future convention for its async operations. It receives no live filesystem or backend capability.

The only CLI-specific dry-run branch chooses the preview API. There are no dry-run booleans threaded through destructive helpers. Preview cannot accidentally call a downloader that also installs a JAR because acquisition and installation are different interfaces.

### 17.5 Embedding usage and configuration

An embedding application creates one engine, prepares typed requests, answers structured decisions, starts operations, and consumes status/receipts. It may run preparation for several projects concurrently; publication is serialized per host-bound project instance.

Configuration parsing produces `HostConfig` plus field provenance. Define CLI/environment/dotenv precedence through one adapter. Help/version can use a minimal parsing path that does not bootstrap tools or unnecessarily read malformed project configuration. Workdir resolution has one invocation-root rule. [E3](https://github.com/inherent-design/empack/blob/50c121f/docs/usage.md)

No operation reads global CLI flags. Providers and planners receive explicit per-operation policy. Display/logging state is instance-owned or explicitly shared by the embedding host, not hidden mutable globals.


## 18. End-to-end usage traces

### 18.1 Direct ZIP/JAR add, including preview

1. CLI creates `AddInput::DirectDownload` with optional content kind.
2. Preparation acquires into bounded scratch, using read-only cache under preview. It computes content evidence and probes archive/JAR type.
3. Provider identification produces a canonical record, a choice, or `Unknown`. Unknown content can become tracked local/URL intent only through the applicable acceptance policy.
4. The planner selects the logical key and exact placements. Existing same-identity aliases are updated, not duplicated.
5. Preview renders that plan and drops scratch leases. No backend or live project writer was available.
6. Execution materializes a stage, verifies installed identity/content/requirements, freezes it, and publishes a verified change.

**Regression prevented:** a helper cannot download-and-install before a later dry-run check because its acquisition port has no installation authority.

### 18.2 Forced import into an existing pack

1. Read and snapshot the current project without resetting it.
2. Parse the archive into `ImportedProject`; preserve provenance, optionality, and layers.
3. Resolve provider references and verify required URL/embedded bytes. Unsupported conversion or wrong digest blocks preparation while the old pack remains intact.
4. Construct the replacement in staging, including metadata overrides, datapack-folder choices, backend options, and generated default templates where authorized.
5. The approval names the managed replacement footprint. User-owned unrelated content is excluded.
6. Verify the complete replacement, then publish with durable recovery data.

**Regression prevented:** the importer and its caller cannot disagree about whether preflight already happened; the publisher consumes only a verified candidate.

### 18.3 Remove by alias, title, or installed stem

1. Resolve user selection against intent and observations. An ambiguous title/stem returns a choice.
2. Produce `RemovalSelection` binding one logical key to its canonical identity, installed metadata key, and expected content.
3. Check dependency evidence. Demote a root still needed by another root or report the explicit dependent-removal requirement.
4. Modify the stage and exact logical record, then verify absence/retention postconditions.
5. Publish only the planned managed file changes. A local record pointing to a directory fails before any removal.

**Regression prevented:** strings from different identity namespaces never reach a raw backend remove operation interchangeably.

### 18.4 Build all targets and reject silent omission

1. Capture satisfied exact build input and all preserved observed content.
2. Project inventories for selected targets and expand shared prerequisites.
3. Build an intermediate once in this invocation, in staging. Do not reuse a name/version-matched old archive.
4. An exporter returns exit 0 but omits a required provider file. `ArtifactReader` detects a missing inventory item, and verification fails.
5. Discard failed candidate artifacts; keep the prior published distribution.

**Regression prevented:** every source kind contributes to inventory verification; no URL-only verification exception exists.

### 18.5 Restricted acquisition and resume

1. Preparation or staged backend execution produces `PendingAcquisition` with expected identity/evidence.
2. Host optionally opens the provider page and watches configured read-only download roots.
3. Verified candidate bytes satisfy the pending item; ambiguous or evidence-poor matches require explicit association.
4. If suspended, retain validated content and a bounded continuation description, releasing active execution permits.
5. Resume assesses the current source revision. Stale state is reported without deletion. Valid state re-enters the normal plan/stage/verify/publish flow.

**Regression prevented:** continuation is not a path-authoritative second build pipeline or a filename-guessing cache insertion path.

### 18.6 Concurrent edit during a long acquisition

1. Prepare plan A against revision R and release the project lock while acquiring bytes.
2. A user edits the manifest or changes relevant source content.
3. A stages and verifies its candidate, then requests publication.
4. Publisher detects read-set mismatch and returns conflict before live changes.
5. Reprepare against the new revision, reusing validated content if applicable. Any changed destructive footprint requires a new grant.

**Regression prevented:** a long-running plan cannot overwrite a later user edit solely because it still holds an old manifest in memory.

## Compiled import inspection API

`engine::import::inspect_import(&mut WorkScope, AcquiredContent, ImportLimits)`
returns a `RetainedOutput<ImportedProject>`. `ImportFormat`, `ImportLocation`,
`ImportedProvider`, `ImportedFile`, `ImportedRuntime` and `ImportedRequirements`
represent input evidence. They are not a resolved lock, an approved mutation or a
publication receipt. `ImportedProject::archive()` retains the original verified
source for later bounded member acquisition. `datapack_layout_proposals()` returns
read-only suggestions and their source locations.

Unknown optional defaults and multiple loader declarations require resolution.
Signed HTTPS and HTTP alternatives remain source declarations with diagnostics;
preparation must resolve transport and durable-provenance decisions.
Inspection can recognize an unsupported packwiz archive; recognition does not make
packwiz-directory import available. See the implementation ledger for the remaining
composition work.

## Compiled content identification API

`ProviderCatalog::identify_file(&mut WorkScope, AcquiredContent,
NonEmpty<ProviderKind>, IdentificationLimits)` returns `Identification::Unknown`,
`Exact` or `Ambiguous`. Exact/ambiguous results contain retained
`IdentifiedSelection` values with the observed content ID, complete provider
resolution and all matching filenames. Names identify roles within that exact
selection; they are not installation destinations. Provider failures remain errors.
