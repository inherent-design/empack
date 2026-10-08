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
project root or access to its recovery journal. Cache previews distinguish addressed blobs from abandoned publisher candidates.
Candidates are captured under store coordination and checked again before deletion;
active writers hold exclusive coordination. Unknown cache neighbors remain.
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
provider result or download association. Normal cache selection, execution-time missing-input continuation
and CLI composition remain completion work. Durable preparation save/resume and its
native host are implemented; see the current delivery ledger. The broader interface below
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

`BuildRequest.clean` captures the complete `dist/` namespace and includes obsolete
regular files in `BuildPreview.cleanup`. The grant must acknowledge that exact removal
summary. Every requested artifact is prepared and verified before one publication both
replaces outputs and removes obsolete files. Failure or cancellation before publication
retains previous distributions. New files, edits or symlinked artifacts invalidate the
captured authority. Source paths cannot overlap cleanup. The receipt names every removed
artifact; interrupted publication uses the same finish/restore recovery as other operations.
No recursive deletion or separate preliminary clean occurs in this Engine path.

`BuildRequest::with_content` attaches explicit `BuildAcquisitions` without putting
leases in a display-only preview. Supplied keys must match current acquisition
obligations. Preparation streams those bytes through the unchanged locked/backend
assertions and evidence policy; unrelated keys and mismatches fail before approval.

`BuildPreparationRequest::with_local_files` associates absolute host files with exact
locked or observed acquisition keys. Preparation derives assertions from captured
obligations, verifies regular files into private content under a shared byte/deadline
allowance, and preserves portable permissions. Missing, repeated or unrequested slots,
unsafe file kinds, digest mismatches and evidence downgrades fail before approval.
The host adapter `engine_host::build_with_local_files` resolves selected paths against
the invocation directory. Paths never enter project documents or authorize copy targets.

A build `Preparation::NeedsInput` retains an in-memory `PreparationContinuation`.
Its view contains only display data. `Engine::resume` accepts additional verified
files, revalidates the captured project, retains earlier supplied files and issues a
new plan. Another engine, changed inputs, a retargeted project selection or duplicate
supplied keys invalidate the resume. Dropping the continuation releases retained preparation and content resources.
`Engine::resume_with_local_files` performs the same recapture before reading newly
associated files. Earlier supplied bytes remain private and usable even when the host
source later changes or disappears. Every remaining obligation still needs verification.
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
project writer. Normalization retains credential-free HTTPS alternatives accepted by
the document policy. Signed, authenticated or otherwise nonpersistent locators stay
on execution-only provider records and can be refreshed by exact identity. Provider
import uses the same rule. Complete reference evidence permits mrpack export without
downloading payloads or embedding provider files merely because their URL was discarded.
When a build needs bytes, a provider obligation retains both its exact identity and saved
alternatives. Configured catalog access refreshes the locator and checks the original
assertions before transfer. A host without catalog access can still verify bytes from a
saved origin; a missing fresh locator also retains that original alternative. Failed
catalog lookup or changed provider assertions remain errors. Refresh does not rewrite
the project's lock or turn observed hashes into stronger source evidence.
Root requirements propagate through required edges; incompatible
optional choices require an explicit conversion. Default placement follows content
kind and configured layout; datapacks/worlds require a selected folder when none is
configured. `ProviderFiles::Placed` preserves per-file destinations and participation
for companion files, recording the explicit conversion. Required companions cannot
be omitted. Required dependencies reuse valid current exact selections and retain
their original assertions, aliases and placements. A changed pin, incompatible
runtime or insufficient participation needs an explicit change. Generated labels
reserve explicit roots and current records before choosing a disambiguated label.
CLI selection and local/URL hosts remain separate integration work.

`application::engine_host::add_providers` composes already selected provider requests,
required-closure resolution, exact reference recording and approved native publication.
It shares one resource governor across resolution and engine work. `AddRequest::source_revision`
can bind resolved choices to `WorkspaceSnapshot::revision()`: an opaque read precondition
containing the native root and both raw document revisions. The host always supplies it;
changed documents or a different native project require fresh resolution. The value grants
no write authority and cannot be deserialized as a saved approval.

The host shows canonical identity, existing-label bindings, replacement selections,
incomplete evidence and native file changes before approval. It records remote references
without claiming payload acquisition. A failed requested selection or unresolved required
edge publishes nothing. `engine_host::remove` similarly displays tracked and observed
selections, removal mode and missing evidence. `engine_host::synchronize` accepts an explicit
`SyncRequest`. Its `Recorded` mode derives local and archive-member content from captured
sources and preserves remote references; its `Supplied` mode accepts explicit content decisions.
Changed intent still needs a fresh resolution candidate. These
compiled entry points share preview, approval, cancellation, shutdown and receipt handling.
CLI search/selection and dispatcher cutover remain pending.

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

`FileAddition::acquire` composes bounded local/HTTP acquisition for explicit
`DirectFileInput` records. Every record declares its logical key, content kind,
requirements, placements and source-evidence policy. Download alternatives may be
transient; separately declared credential-free HTTPS origins become durable intent.
`DirectFileSource::DownloadAsLocal` explicitly publishes verified downloaded bytes
as a tracked local file at the first placement. No download locator enters the
documents; provenance records the conversion and retains original digest assertions.
Later synchronization and builds use the local bytes without contacting that source.
This representation requires an explicit choice, never a fallback after failed identification.
All HTTP files share one byte/deadline allowance. The overall byte allowance also
bounds the combined local/downloaded inventory. No successful subset is returned.

Typed ZIP/JAR inputs receive bounded archive structure, member CRC and layout checks.
Every member is decoded under the cumulative expanded-byte allowance before addition;
matching the outer archive digest cannot substitute for valid member data. Recognized
mod, resource-pack, datapack and shader layouts support the selected kind; unrecognized
layouts require `FileKindPolicy::AcceptUnrecognized`. These markers do not establish
game compatibility or provider ownership. Unsafe archive paths remain errors under
either policy. Configuration and explicitly placed other files remain opaque. World
archives require member interpretation rather than being installed as an opaque world.

`application::engine_host::add_files` connects these explicit choices to the same
native addition preview, approval and publication used by provider additions. Host
paths resolve against the invocation directory. Raw project documents and native root
identity bind acquisition to publication, so a concurrent edit requires replanning.
Provider identification/representation selection, world member interpretation and
CLI input selection remain frontend integration work; catalog failures do not implicitly
authorize unidentified content.

`application::engine_host::add` accepts a nonempty sequence of `AddHostInput::Provider`
and `AddHostInput::File`. Both use one captured project revision and one resource governor.
`ResolvedAdditionBatch::combine` consumes their resolved evidence into one inspected
addition group and complete content inventory. Provider entries remain exact references;
direct entries retain verified bytes. The evidence policy applies to direct acquisition;
provider references keep their original assertions for later acquisition policy checks.

Logical collisions and incompatible placements fail before publication. Required provider
closure remains attached to its roots. A failure in either source prevents publication of
both; there is one preview and approval, not one mutation per input class. Existing
`add_providers` and `add_files` entry points use the same composition. `AdditionGroup`
exposes its exact dependency records for pre-approval inspection without granting mutation.

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
`SyncRequest::Supplied` carries verified exact inputs through the shared Engine lifecycle.
`SyncRequest::Recorded` derives those decisions from exact recorded sources: local files
and embedded members must verify, while provider/URL/manual slots remain deferred references.
It neither downloads remote payloads nor selects newer versions. The source read set includes
only declared files plus managed placements and required metadata, including sources outside
`pack/`. Acquisition is admitted before creating private content; member readers share bounded
packed storage and process one source archive at a time. Captured archive readers avoid
duplicating compressed bytes; scratch admission accounts for selected member bytes and the
current member copy. Parser memory is reserved only when a captured archive is used.
Missing archives try captured installed placements until original assertions verify. Missing local sources, unsafe native
paths, corrupt members and stronger-evidence policy failures prevent the whole publication.
Captured sources remain publication preconditions, so an edit after approval invalidates it.
`SyncPreview` lists selected records, lock rebinding and the replacement plan;
`SyncReceipt` retains the coherent published project.
`SyncRequest::AcquiredReferences` combines captured local/member sources with externally
verified remote content. Unknown keys and local-source overrides are rejected. The host
binds the supplied content to the inspected resolution; native preparation rechecks
that resolution and every byte assertion. `sync --materialize` uses this route after
explicit acquisition approval, then displays the final native file plan before publication.
Remote acquisition failures publish no project subset. Dry-run reports obligations
without downloading their payloads. Any request variant
can carry a fresh `resolution` candidate for changed or missing intent. The planner preserves valid
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
`UpdateReceipt` contains the published project and the explicitly selected canonical keys.
`UpdateRequest.source_revision` can bind resolution to captured raw documents and native
root identity, using the same precondition as addition.

`engine_host::update` accepts the same typed provider/direct-file inputs as native addition.
It resolves compatible provider selections and required closure, verifies requested direct
bytes, and prepares one update under that captured revision. An explicit pin in current intent
cannot be changed by an update input. The host preserves raw authoring bytes, keeps transitive
records unpromoted, and shows selected identities plus native changes before approval.
The CLI exposes `update KEY...` for exact installed logical keys. It derives canonical
provider identity and authored pins from the captured project, retains explicit placements,
and refreshes direct content from its declared source. Unknown or repeated keys fail before
resolution. It does not authorize replacing externally changed installed bytes.
`adopt KEY...` accepts verified changes to tracked local files or common-layer provider
installations through document-only publication. Provider selection comes from the exact
observed metadata pin, never a latest-version query. Each selected destination must agree
about provider identity and pin; the catalog supplies the original byte assertions and
required closure. Native preparation verifies installed bytes and backend metadata before
approval. Existing authored pins, placements and root roles remain binding. URL observation,
side-layer provider evidence and untracked-file adoption still need frontend choices.

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
blocks publication. Existing authored roots and transitive roles remain unchanged: their source identity,
pin and placement constrain the proposed resolution. Only newly adopted root keys
are inserted. A present stale/invalid lock is not treated as absence. CLI
selection remains integration work.
Adoption intentionally accepts selected drift after review: existing bytes must
match the proposed complete assertions, not the superseded lock's byte assertions.
The old documents remain captured publication preconditions and recovery preimages;
retained dependency constraints still bind. Requiring the old bytes would turn this
operation into sync and prevent adoption of external edits. Indexed metadata keeps
its metadata role, aliases and extension fields when its digest is refreshed.

`engine_host::adopt_observed` accepts that explicit resolved group, displays selected
identities and exact document changes, and uses the shared preview/confirmation,
cancellation and publication flow. It can establish the first lock without a network
lookup or payload installation. Invalid or missing selected bytes prevent publication;
an invalid existing lock is never treated as an absent lock. Constructing the proposed
group from CLI identification and selection remains frontend work.
`AdoptObservedPreview.selections` carries captured before/after resolutions for changed
canonical keys, including pins, file assertions, placements, participation, required edges
and evidence coverage. First-lock entries have no prior resolution; unchanged entries are
omitted. The host streams these details before confirmation. Freeform manual instructions
and provenance locations are not expanded into automatic diagnostics.

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

`AddRequest`, `UpdateRequest` and `SyncRequest::Supplied` describe every file slot with
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
Family discovery shares one network deadline, preserving earlier successful
choices when the allowance expires. A requested loader pin excludes Vanilla and
other families that lack that exact version. The chosen family reuses its retained
catalog. No failed lookup is replaced with an invented version.

The host displays native file changes before confirmation. Preview, declined plans
and invalid options cannot create a project or durable host state. Forced replacement
requires the displayed footprint and preserves user templates and unrelated files.
Initialization and recovery share approval, cancellation, shutdown and outcome
classification. The initialization host currently accepts empty projects; source imports
have a separate normalization path. The CLI adapter selects that path for `--from`;
ordinary project commands now consume the native documents through engine hosts.

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

### Provider modpack archive selection

`ProviderCatalog::resolve_modpack_archive` accepts a `ModpackSelector`, release policy
and bounded `SelectionLimits`. Its retained `ModpackArchive` exposes the canonical
project, exact pin and original file evidence. Modpack projects are archive sources;
they do not acquire a fictitious dependency content kind.

Selectors accept project IDs, slugs and exact provider modpack pages. A numeric
CurseForge URL slug remains a slug. Modrinth pages may select a project-scoped version
number or ID; CurseForge pages may select a file ID. Conflicting selections fail.
Without an explicit version, release policy and publication timestamps determine the
choice across complete bounded pages. Truncated, overlapping or inconsistent pages
cannot return an earlier partial result. Server-pack files are excluded from client
modpack import sources. Automatic selection excludes unsupported archive formats;
an explicitly selected unsupported version fails. Unselected Modrinth attachments do
not supply acquisition evidence for the chosen primary archive. Ambiguous primary roles
and malformed assertions on the selected archive still fail.

File identity, digest and download-origin validation are shared with dependency
resolution. Modrinth's primary-file/first-file rule follows its
[version API](https://docs.modrinth.com/api/operations/getprojectversions/).
CurseForge file assertions and pagination follow its
[REST contract](https://docs.curseforge.com/rest-api/). Restricted files retain their
exact identity and expected bytes with no invented download origin.

### Native build host

`application::engine_host::build` accepts parsed build arguments, `BuildDecisions`
and verified `BuildAcquisitions`. It resolves the invocation-relative project without
changing process cwd, reads coherent documents through the native reader, and prepares
all requested outputs through the engine. `BuildPreparationRequest::require_intent` binds
derived output choices to that semantic intent revision. Changed metadata or distribution
preferences require fresh planning; comment-only edits before preparation preserve the
choices and remain part of the new captured read set. Empty target selection uses project defaults;
explicit targets are validated and deduplicated. An omitted archive override uses the
project preference. Portable name/version components determine artifact filenames.

The host displays runtime, destinations, optional-content policy, evidence policy,
network/tool requirements and obsolete-artifact removals before shared approval.
`--yes` does not resolve optional choices or accept conversion loss. Those decisions
remain explicit typed input. Supplied manual files retain exact logical slots and
source assertions. Retained locked and observed slots count toward the preparation's
cumulative content allowance across resumes; distinct slots count separately even when
their bytes match. Later HTTP acquisition receives only the remaining allowance.
Per-file limits apply to supplied leases as well as host file reads. Archive extraction
and runtime/tool downloads also retain their own bounds. Missing obligations appear in
read-only preview and prevent execution.
Every completed artifact reports its verified byte length. Failed preparation leaves old
artifacts intact, including when cleanup was requested.

A selected `downloads_dir` is scanned read-only for unresolved obligations. The native
host matches declared content evidence, collapses duplicate bytes and re-acquires unique
matches through `Engine::resume_with_local_files`. Different matching contents remain an
explicit association decision. A filename or extension never establishes identity.

Fresh requests enter `build`; saved requests enter the separate `continue_build` host.
The latter accepts `--continue`, a download root and explicit associations, preserving the
saved recipe. Recipe overrides require a fresh build. Both entry points are wired to
the CLI. `--optional CHOICE=true|false` and `--optional-defaults` supply materialization
choices; `--allow-optional-metadata-loss` separately authorizes the mrpack conversion.
Execution confirmation never supplies those semantic decisions implicitly.

### Native import host

`application::engine_host::import` accepts an `ImportHostRequest` and a decision
callback over `VerifiedImportContent`. Sources are provider modpack selections, explicit
native paths or download alternatives. File source variants carry original digest and size assertions; selecting a
local file does not downgrade a strong-source policy to an accepted observation.
A provider source can include an explicit `supplied_archive` native path. Its bytes must
satisfy that exact provider archive's original digest and size, including when no download
URL is available. This association is separate from supplied files inside the archive.
Native paths resolve from the invocation;
destination paths resolve from the selected workdir. Remote acquisition enforces the
compressed archive limit while streaming, before parsing or project preparation.

Inspection, exact provider resolution, content verification and native publication share
the host resource budget. Restricted inputs need explicit verified associations. Missing
input, invalid conversion or any acquisition failure leaves the project unpublished.
The callback chooses metadata, loader declaration, file representation, placement and
optional defaults without receiving project write authority. It cannot change declared
participation or silently redirect source destinations.

The host displays the exact managed replacement plan before the common approval step.
Preview and declined approval leave project and durable host state unchanged. Provider
pages use the archive catalog and verify its assertions before inspection. Runtime
conversion choices and durable import pending-input storage remain integration work.
The ordinary CLI import dispatcher uses this host.

### Native cleanup host

`application::engine_host::clean` accepts `builds`, `cache`, `continuation` and `all`; omission selects
artifacts. It validates every target and prepares all selected scopes before one approval.
Artifacts use native publication without needing valid intent/lock documents. The content
cache lives under the selected cache root's `content-v1` directory. Opening an absent cache
for inspection does not create it; cache-only cleanup does not require a project.

The host displays artifact paths, logical bytes, cache object IDs and maximum eviction
bytes. Artifact recovery data remains separate. Combined cleanup is explicitly sequential:
if cache eviction fails after artifact publication, the error identifies the completed
artifact scope and retains the cache receipt's partial result. The scopes cannot overlap.
Unknown neighbors and unselected storage remain untouched. The ordinary CLI dispatches
to this host for artifact, content-store and saved-recipe cleanup.


`clean continuation` explicitly discards only the selected project's saved build recipe.
`all` continues to select artifacts and disposable cache, so it does not implicitly erase
pending work. Cleanup can be combined with other explicit scopes and shares their preview
and approval. Inspection hashes bounded opaque record bytes without decoding a recipe or
requiring valid project documents. Malformed and stale recipes remain removable. The
engine-owned observation binds conditional deletion to that exact record; replacement
between inspection and execution is refused. Cached payloads and recovery journals remain.

### Read-only download discovery

`engine::acquisition::discovery::discover_downloads` accepts explicit absolute host roots,
logical acquisition keys with original expectations, source-evidence policy and bounded
scan limits. It returns admitted candidate observations, not content leases or publication
authority. Only direct regular children are considered. Root aliases and hard-linked files
are read once per scan; every inspected file is hashed once against all requirements.
Unreadable unrelated files, links, special files and oversized candidates are skipped.

All declared digests, size and any accepted observation must agree. Compatibility mode
can match MD5 evidence; strong-source mode cannot upgrade it. Evidence-free requests
remain unresolved. Different content IDs matching weaker evidence remain ambiguous;
identical bytes at several paths yield one suggestion. Entry, total-byte, association and
deadline exhaustion fail the scan without returning a partial set. Failed and unrelated
reads consume the byte allowance. Candidate paths never enter project intent, and the
normal local acquisition step verifies their current bytes again before accepting them.


### Durable build suspension

A provider lookup during approved execution can reveal missing manual content. The
`ExecutionOutcome::NeedsInput` value retains an `ExecutionInput`: reported obligations
plus a single-consumer continuation with the original recipe, captured project and verified
leases. `take_continuation` transfers that state without transferring execution approval.
The caller can use the ordinary resume or suspension APIs. Repreparation revalidates input
and requires a fresh plan grant. Retained native acquisition reservations survive repeated
input decisions and are reused rather than accumulated. The CLI offers the same explicit
saved-build decision as preparation-time missing inputs; no artifact is published.

`Engine::suspend_build` consumes a pending build owned by that engine. Calling it is
an explicit host action authorizing saved state and verified-content publication;
preview never calls it. The method revalidates captured input, publishes verified leases
to the host's content-addressed `pending-content` store, and atomically replaces the
project's record in `pending-builds`. These directories live below the configured private
state root. A failed save can leave reusable verified cache objects, but cannot publish
project documents or distributions.

The bounded, versioned record stores the build recipe, source-comparison fingerprint,
logical acquisition keys, content IDs and portable permissions. It contains no execution
grant, process command or authoritative native source/destination locator. Every build
choice survives encoding, including optional policy, template modes/values/limits, archive
format, source-evidence policy and installer interaction. A record is limited to 4 MiB;
unknown fields, versions and invalid portable paths fail validation.

`Engine::resume_saved_build` accepts a host-selected absolute project path. It returns
`Missing`, `Stale`, or a fresh `Preparation`. Raw intent/lock changes are classified before
parsing changed documents. Other captured input changes are checked against a fresh native
snapshot. Invalid current inputs or malformed records return errors. None of these outcomes
deletes saved state. Cached bytes are checked against the current original expectations;
a missing cache object leaves the corresponding obligation unresolved, while corrupted
bytes fail. Resumed inputs still share the ordinary cumulative acquisition allowance.

A fresh execution grant is required after resume. `ResumedBuild` contains the preparation
and an opaque `SavedBuildRecord` observation. `Engine::observe_saved_build` provides a read-only opaque observation for explicit cleanup, even when a recipe cannot decode or resume. `Engine::discard_saved_build` is a separate
host action; it deletes only if that exact record still exists. Changed or missing records
return false. The native continue host invokes cleanup after a verified build receipt;
preview, decline and failed execution retain the record. No serialized field can reconstruct a native snapshot or approve
effects. Records and project recovery journals are separate from disposable cached bytes.


The native build host offers to save unresolved preparation and returns an incomplete-build
error after a successful save. `--yes` accepts that host action; dry-run and decline do not
create state. The continue host resolves `FILENAME=PATH` against captured destination names.
Ambiguous names require the displayed `locked:KEY:SLOT` or `observed:METADATA` selector;
components are percent-encoded so arbitrary logical labels remain distinct. Duplicate,
unknown and mismatched associations fail before publication. Associations are verified
before automatic discovery fills other pending slots.

Suspension metadata admission scales with an encoded-size estimate; inspection probes a
record's size before reserving decode memory and bounds the subsequent read to that size.
The persistent content store accounts for publication candidates under its exclusive
capacity lock, independently of the retained source's private-scratch reservation. Known
orphaned publication candidates consume store capacity as well as canonical content objects.

### Supplied-file provider identification

`add --platform PROVIDER FILE` acquires and inspects the supplied bytes, verifies their
provider identity, then resolves that exact provider selection and its required closure.
Identification precedes kind selection: an unambiguous provider kind supplies the default,
including for renamed ZIP files. An explicit `--type` must agree with that exact selection.
Both direct and identified archives use the same bounded member and layout validation.
The supplied payload remains materialized in the publication batch; it is not replaced by
a remote reference. Original provider digest and size assertions are checked again before
publication. Unknown identities, ambiguous matches, multiple matching file roles and
changed provider assertions fail the complete request. Omitting `--platform` explicitly
retains the direct-file representation. Multi-role and companion-file choices remain
separate placement decisions.

### Unattended execution and failure classes

A prepared CLI mutation requires `--yes` when no interactive choice is available.
Without it, the command exits with usage status 2 and leaves the plan unapplied.
`--dry-run` remains read-only and successful; an interactive user may explicitly decline.
Malformed intent and lock documents also retain usage status 2 through contextual errors.
Provider network failure remains status 3, and interruption retires the native request
before returning status 130. The executable tests exercise these boundaries with native
projects, a blocked HTTP proxy and no external packwiz process.

### Initialization authoring files

Initialization seeds `.gitignore`, `pack/.packwizignore` and editable validation/release
workflows alongside the data templates. These files participate in the same approved,
verified publication as the intent and lock. Existing regular files remain user-owned,
even during forced replacement. Captured edits invalidate the prepared operation;
symlinked ancestors and directory-valued seed destinations fail before publication.
Only the three named root scaffolds are addressable through the scaffold role.

The generated workflows install the tagged `v0.5.0-alpha.1` source and run
`empack build --yes mrpack`, preserving recorded selections. Validation has read-only
repository permissions; tagged releases publish the verified archive. These defaults
require that empack release tag to exist and remain editable project files. They do not
run packwiz, infer a version update or repair a stale project implicitly.

### CLI resolution for synchronization

`sync` first checks whether current intent can bind to recorded selections. A valid
lock takes the recorded restoration path without provider discovery. Missing locks
and unsatisfied roots use bounded provider/direct-file resolution before the same
native synchronization planner. Search intent still requires an explicit selection.
No project file changes during discovery or preview.

Fresh resolution preserves authored YAML and validates the complete proposed lock
against retained installations. Placement-only provider edits retain the exact pin
and original content assertions; conflicting refreshed assertions fail. An explicit
pin or source edit can request new content. Unrequested local-byte drift still needs
adoption or an authored content pin. Local authoring sources retain their declared
project-relative paths, independently of installation destinations.

The CLI currently records remote references and materializes local/member sources.
It does not yet expose optional remote materialization. World/member choices and
ambiguous companion placements remain explicit implementation gates.

### Explicit provider file plans

`DocumentCodec::decode_provider_files` reads schema-1 file plans using the same strict
placement and environment representation as intent and lock documents. It returns
`ProviderFileSelection`, containing root requirements and a nonempty map from exact
provider filenames to nonempty placement lists. Input is limited to 1 MiB and 128 files,
with at most 128 placements per file. Unknown fields and malformed requirements fail.

The CLI's `add --file-plan PATH` reads one explicitly selected regular file through
bounded native acquisition and retains decode admission during resolution. The plan
applies to exactly one provider project or identified supplied file. It becomes
`ProviderFiles::Placed`; catalog resolution checks role membership, required companions
and participation before the shared addition planner checks ownership and collisions.
File-plan paths are invocation-relative. A file plan is source intent for this request,
not approval or filesystem mutation authority. Preview remains read-only.
