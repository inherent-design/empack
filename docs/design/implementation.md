# Implementation status

Release target: v0.5.0-alpha.1. The [target contracts](README.md) and
[feature requirements](parity.md) define completion. This page records what the
source implements; it does not make the target API sketches available by implication.

The normalized engine can prepare and recoverably publish all five distribution
shapes from a captured workspace. Server recipes have verified vanilla,
Fabric, Quilt, Forge and NeoForge runtime preparation. HTTP acquisition and
explicit build-content obligations feed these paths. The compiled `Engine` now owns
build/import preview, preparation, exact-plan authorization and publication.
Build execution also owns acquisition and runtime assembly. The read-only provider catalog resolves canonical selectors and
exact and compatible Modrinth/CurseForge selections, offers bounded search choices,
and identifies acquired files by their content. Normalized mrpack and CurseForge inspection also runs over retained bounded archive sources. The remaining operation APIs,
continuation/cleanup and CLI cutover remain unfinished. Existing
commands retain their fixes and capabilities until their replacements pass parity
checks. There will be one implementation per operation, not a permanent legacy engine.

## Implemented boundaries

| Boundary | Current behavior | Contract evidence |
| --- | --- | --- |
| Build and import lifecycle | Read-only preview/preparation; engine-bound consumed grants; exact replacement acknowledgement; shared build acquisition; owned runtime/tool work; all-requested publication and typed retained receipts; abandoned preparation retirement | [Engine tests](../../crates/empack-lib/src/engine/api/tests.rs), [runtime tests](../../crates/empack-lib/src/engine/runtime/tests.rs) |
| Provider catalog | Canonical slug/ID/URL resolution, exact project-owned selections, bounded compatible-version selection and content identification; all file assertions, roles, environment facts and dependency relations retained; fixed-origin authenticated API requests, shared rate budgets, bounded bytes/deadline/retries and owned parsing | [Catalog tests](../../crates/empack-lib/src/engine/providers/tests.rs), [official API smoke](../../crates/empack-lib/tests/provider_catalog_smoke.rs) |
| Import inspection | Owned bounded mrpack/CurseForge parsing with original archive retention; exact provider references, URL declarations, embedded members, independent environment requirements, override layers and source locations; no project/backend authority | [Adapter tests](../../crates/empack-lib/src/engine/import/tests.rs), [archive smoke](../../crates/empack-lib/tests/import_archive_smoke.rs) |
| Semantic core | Dependency-free identities, pins, paths, requirements, digests, exact multi-file resolution, file plans, target prerequisites and inventory projection | [Core suites](../../crates/empack-core/tests/) |
| Documents | Intent schema 2 and lock schema 1; raw and semantic revisions; strict source/selection validation; original-byte no-op writes; stable credential-free persisted locators | [Codec tests](../../crates/empack-lib/src/engine/documents/tests.rs) |
| Project capture | Read-only recovery gate; retained native root; bounded bytes, identities, membership and absence; explicit local/archive sources; exact artifact destinations | [Reader tests](../../crates/empack-lib/src/engine/project/tests.rs), [snapshot tests](../../crates/empack-lib/src/engine/snapshot/tests.rs) |
| Source enumeration | Native traversal applies captured pack ignore rules before opening ignored bytes; explicit local/archive inputs remain captured; recovery retains the same filter; base/common-override/client/server layers remain separate | [Source matcher](../../crates/empack-lib/src/engine/source.rs), reader tests |
| Backend observations | Shared canonical provider/pin decoding; safe relative payload paths; requirements, optional defaults and digest declarations; captured metadata revisions | [Backend tests](../../crates/empack-lib/src/engine/backend/tests.rs) |
| Private content | Bounded quarantine, every source digest/size/observation checked, retained independent readers, original weak evidence separate from computed hashes | [Content tests](../../crates/empack-lib/src/engine/content/tests.rs) |
| Shared content storage | Downloaded build inputs, captured local/embedded content and downloaded runtime libraries share bounded private backing; source evidence remains per logical file; failed appends retain charges and cannot resume writing | [Pool tests](../../crates/empack-lib/src/engine/content/pool/tests.rs), [large build regression](../../crates/empack-lib/src/engine/api/tests.rs) |
| HTTP acquisition | HTTPS/redirect/status policy, bounded channel, cumulative mirror bytes, one deadline, scope-owned verifier, redacted locators, reservations retained by leases/readers | [Transfer tests](../../crates/empack-lib/src/engine/acquisition/tests.rs) |
| Build acquisition | Read-only target/side/optional selection before acquisition; bootstrap and mrpack retain their distinct reference requirements; selected local files verify while excluded files need no bytes; failed download batches return no successful subset; exact provider lookup can refresh execution-only locators without rewriting the lock; manual/missing-archive work remains explicit | [Build acquisition tests](../../crates/empack-lib/src/engine/build/acquisition/tests.rs) |
| Materialized game content | Shared captured identity/content checks; side and optional selection before missing-byte requirements; exact retained leases and source evidence; no unresolved entries in completed inventory | [Materialization tests](../../crates/empack-lib/src/engine/build/materialized/tests.rs), [core inventory tests](../../crates/empack-core/tests/inventory.rs) |
| Build batches | All five recipes prepare privately; duplicate outputs fail preflight; every candidate verifies before one journal publishes their union; original resolution and conversion evidence remain available | [Batch tests](../../crates/empack-lib/src/engine/build/batch/tests.rs) |
| Packwiz reference projection | Captured game selection feeds a generated pack/index/metadata tree; exact provider references, stable URLs, optional constraints and original declared-vs-acquired evidence remain explicit | [Projection tests](../../crates/empack-lib/src/engine/packwiz/tests.rs), game-content tests |
| Vanilla server runtime | Official catalog selection, per-version metadata digest, exact server bytes, bounded JAR main-section and launcher checks; owned parsing and verification | [Runtime tests](../../crates/empack-lib/src/engine/server_runtime/tests.rs) |
| Fabric and Quilt server runtimes | Exact catalog/library coordinates, declared digest sidecars where needed, verified Minecraft base, generated classpath or historical shaded launcher, service merging and retained source/tool evidence | [Library runtime tests](../../crates/empack-lib/src/engine/server_runtime/library/tests.rs), [live runtime smoke](../../crates/empack-lib/tests/runtime_server_smoke.rs) |
| Forge-family server runtimes | Exact official installer evidence, owned private execution, independently checked libraries/generated outputs, historical executable and modern host-specific argument launch contracts | [Installer tests](../../crates/empack-lib/src/engine/server_runtime/installer/tests.rs), live profile smoke |
| Installer assets | Reviewed bootstrap/main-installer versions, bounded governed acquisition, exact size/digest validation and retained tool identities; pins are maintained by empack | [Tool tests](../../crates/empack-lib/src/engine/bootstrap_tools.rs) |
| Lightweight client distribution | Generated packwiz tree, exact bundled tools, local byte projection, pinned launcher command, all archive formats and batch publication | [Client tests](../../crates/empack-lib/src/engine/build/client/tests.rs) |
| Server distributions | Full and reference game views, exact prepared runtime, preserved templates, executable launch scripts, pinned bootstrap tools and all archive formats; runtime selection must match the lock | [Server tests](../../crates/empack-lib/src/engine/build/server/tests.rs), batch tests |
| Full client distribution | `.minecraft` game view, captured templates, exact launcher components, generated defaults, all three archive formats and verified publication | [Client build tests](../../crates/empack-lib/src/engine/build/client/tests.rs) |
| Templates | Captured common/side projection, nested paths, binary and explicit literal copying, strict expressions, format helpers, bounded outputs and preserved portable attributes | [Template tests](../../crates/empack-lib/src/engine/templates/tests.rs) |
| Archive sources | Retained bounded ZIP reader; raw directory preflight; every member path/kind/collision checked; selected bytes, CRC and portable attributes verified; failed attempts consume extraction allowance | [Archive source tests](../../crates/empack-lib/src/engine/archive_source/tests.rs) |
| Staging | Private native storage, copied inputs, no project/cache hardlinks, closed writers before freeze, packed private backing for multi-file trees, bounded member reads and safe cleanup order | [Staging tests](../../crates/empack-lib/src/engine/staging/tests.rs) |
| File verification | Exact candidate inventory and portable attributes, explicit managed roles, collision checks, complete resulting read budgets | [Verification tests](../../crates/empack-lib/src/engine/verification/tests.rs) |
| Publication | Project lock, synchronized before/after images, durable intent, recorded sibling replacement, final inventory, retained receipt; recovery without replaying tools | [Publication and crash tests](../../crates/empack-lib/src/engine/publication/tests.rs) |
| Runtime ownership | Coordinated resource admission, owned async/blocking workers, stale-result rejection, retirement before completion, retained outcomes and explicit output reservations | [Resource tests](../../crates/empack-lib/src/engine/resources/tests.rs), [runtime tests](../../crates/empack-lib/src/engine/runtime/tests.rs) |
| Process ownership | Async supervision on the host runtime, deadline across exit and pipe lifetime, Unix groups/Windows jobs, bounded nonblocking progress | [Process runtime](../../crates/empack-lib/src/application/process_runtime.rs) |
| Containers | ZIP, TAR.GZ and 7z from frozen inputs; binary streaming, portable modes, empty directories, duplicate-name and encoded/expanded limits; independent exact inventory verification | [Artifact tests](../../crates/empack-lib/src/engine/artifacts/tests.rs) |
| Mrpack semantics | Every locked file/placement accounted for, exact reference evidence, layered replacements, retained observed content, optional conversion rules and inspected output archives | [Mrpack tests](../../crates/empack-lib/src/engine/mrpack/tests.rs), [observed-content tests](../../crates/empack-lib/src/engine/mrpack/observed.rs) |

File-level verification is not a semantic proof. Container integrity is not evidence
that every required dependency was included. The composed recipes connect normalized obligations, verified acquisitions,
independently checked output and the journal publisher. Runtime adapters enter the approved build Engine. CLI composition remains pending;
adapter and Engine tests do not establish CLI parity.

The compiled `engine::providers::ProviderCatalog` is concrete and read-only. Its
`resolve_selector`, `resolve_pin`, `resolve_exact` and `resolve_compatible` methods return admitted
retained data. They cannot publish a manifest or claim a resolved dependency closure. Provider environment facts remain separate from user requirements. A
restricted file keeps its identity, size and original hashes even without a locator.
`resolve_pin` accepts a provider-qualified version/file selector without an asserted
project. It verifies the exact response ID, resolves the declared owner, then validates
all file evidence. CurseForge lookup rejects multiple or foreign-game records.
This supports version-only dependency references without guessing their owner.
The provider addition adapter now composes selector lookup, explicit or compatible
selection and required-closure resolution into normalized dependency groups. Its
requests share one transport budget. Canonical labels, exact requested pins, content
kind, file slots, placements, original assertions and required edges reach the native
addition path. An incomplete closure returns input requirements, not a successful
subset. Optional participation is combined independently of traversal order; a
required root takes precedence over optional roots for their shared dependency.
Companion file roles require explicit placements and participation, with conversions
retained in provenance. The primary-file fallback follows the [Modrinth schema](https://github.com/modrinth/docs/blob/master/static/openapi.yaml).
The 62-test affected provider suite and full 1,811-test suite plus eleven doctests
pass, including a catalog-resolution → reference-addition → repeated-sync workflow.
All-feature Clippy and Windows cross-compilation pass. Final review also made
non-primary file choices retain explicit placement intent; its regression and the
seven-test adapter suite pass separately from that full-suite run. This service does not yet replace CLI selection or provide
local/URL input hosts. Required-closure composition now prefers compatible locked
selections without querying a newer version. It preserves original byte assertions,
aliases, placements and participation, rejecting incompatible or changed obligations.
Generated labels reserve explicit and retained records before disambiguation.
The prior shared-selection conflict and review 87 label collision both reproduced;
the combined 1,819-test suite and eleven doctests pass, with all-feature Clippy
and Windows cross-compilation. The native-filesystem and CLI smoke cases are part
of that suite; live provider/CDN checks are separate. Explicit root environment conflicts remain
a refusal: the host must submit the broadened participation as a new request.
The catalog now configures the acquisition transport with the fixed CurseForge
CDN credential rule; Engine attachment applies both together. Origin, port,
redirect, mirror and redaction regressions pass in the 74-test affected suite,
alongside provider resolution and approved build refresh. All-feature Clippy
passes. The first fixture run retained its completed handle during the reservation
assertion; dropping that final owner corrected the test. No production resource
release behavior was changed. Live authenticated CDN verification remains a
combined-candidate gate.
Provider additions now connect retained exact catalog records to the shared verified
HTTP acquisition port. Every slot has an explicit reference, acquire or supplied-byte
decision. Validation precedes payload requests; HTTP files share one transfer budget.
Restricted files retain pending obligations and original MD5 evidence. Supplied
bytes must satisfy the selected slot's complete assertions before native preparation.
The composed regression publishes acquired root and required files, then synchronizes
twice without changes. Negative cases cover mismatched bytes, cumulative limits and
missing/extra decisions. This does not yet establish durable continuation or CLI parity.
Review 88's retained companion case reproduced: main content now establishes required
participation while companion files retain their separate sides. A negative case
ensures a companion cannot satisfy an absent main-content requirement. The 106-test
affected provider, acquisition, addition and build-refresh suite passes, with
all-feature Clippy. This is targeted evidence after the 1,819-test full run.
Review 89's pending-input loss also reproduced. Provider content preflight now
returns required input before starting automatic downloads; postponed slots remain
visible separately. A failing ordinary endpoint is not contacted while a restricted
slot still needs a decision.
Internal catalog composition can carry one request budget through selector, pin and
compatible lookups; it does not restart the deadline between dependencies.
Transient signed URLs are execution data; the document codec still rejects them in
persistent alternatives. Command cutover remains a completion gate. The build Engine can
use this catalog after authorization to refresh a locked file's locator. It selects
a declared role or unique matching source evidence, rejects changed assertions and
retains all original expectations during acquisition. Missing credentials and
restricted downloads remain explicit input; they cannot produce a partial artifact.
Slots sharing an exact provider pin reuse one bounded resolution. If locator refresh
finds manual input, the build reports it before downloading other payloads; an
unrelated transport failure cannot hide that requirement.

Compatible selection requires explicit game versions, loader, content kind and release
policy. Its provisional default prefers stable releases, falling back to prereleases
only when no compatible stable selection exists. It also supports stable-only and
any-channel selection, compares
publication timestamps as instants, and uses canonical pin identity to break ties.
Stable preference applies before game-version preference; explicitly accepted game
alternatives follow the primary version. Mods must advertise the selected loader;
resource packs and other non-mod content do not inherit that mod-loader filter.
Project facts retain all advertised content kinds. Exact selections retain their own
kinds: a project offering both mods and datapacks can have mod-only or datapack-only
versions. Compatible resolution checks the selected version against the requested kind;
it cannot substitute a newer mod release for a requested datapack. File roles and
placement decisions remain separate from these selection-wide facts.

Modrinth's filtered version list and CurseForge's paginated per-game queries share one
transfer budget and deadline per resolution. Page/record limits, invalid pagination,
conflicting repeated pins and malformed selectable identity/evidence fail without a partial
selection. Unavailable records retain ownership and duplicate checks without requiring
downloadable payload evidence. Each page retains its best candidate and compact identity evidence under
admission. Only the final selection survives. This capability is for new or explicitly
updated resolution; it does not upgrade a valid sync lock or rewrite project intent.

## Runtime discovery

`RuntimeCatalog` uses the official Minecraft, Fabric, Quilt, Forge and NeoForge
catalogs without a project writer, persistent cache writer or invented fallback.
Every response has transfer, deadline and entry limits; parsing and retained choices
remain admitted to the operation. Fabric and Quilt records must name the requested
game. Forge retains historical coordinate normalization. NeoForge filters exact game
components and snapshot suffixes while retaining explicit beta choices. Default
loader selection prefers stable versions; selection order is deterministic.

The [catalog tests](../../crates/empack-lib/src/engine/runtime_catalog/tests.rs)
exercise response limits, rejected provider responses, coherent defaults, historical
choices and initialization document roundtrips. All five pass. The explicit
`mise run smoke:catalog` probe passes against the live official endpoints for all four
loader families, including Forge 1.7.10 and NeoForge 1.20.1. All-feature Clippy and
Windows cross-compilation also pass for this worktree. These checks establish the
service boundary; CLI initialization composition remains unfinished.

Sources: [Minecraft manifest](https://piston-meta.mojang.com/mc/game/version_manifest_v2.json),
[Fabric Meta](https://github.com/FabricMC/fabric-meta),
[Quilt Meta](https://meta.quiltmc.org/v3/versions/loader/1.21.1),
[Forge metadata](https://files.minecraftforge.net/net/minecraftforge/forge/maven-metadata.json),
and [NeoForge metadata](https://maven.neoforged.net/api/maven/versions/releases/net/neoforged/neoforge).

## Required dependency expansion

`ProviderCatalog::resolve_required_closure` resolves explicit pins first, then follows
required provider edges. Project-only references select a compatible file; version-only
references establish their owner before entering the graph. Repeated selections are
retained once, and mutually required groups remain valid. Known required edges use
canonical exact pins, independent of manifest labels or installation filenames.

One transport budget and deadline cover all roots, dependencies and retries. Node and
edge limits bound graph expansion. Defaults allow 256 MiB of catalog traffic over five
minutes, with a separate 4 MiB ceiling for each response. Graph bookkeeping grows with admitted nodes and relations; retained provider records
remain charged to the operation. A network failure or exhausted allowance returns no
partial graph. Optional and embedded metadata stay available without automatically
installing those dependencies.

Missing coverage, filename-only requirements, ambiguous content kinds, uninterpreted
tool/include relations, incompatible required pins and observed pin conflicts remain explicit issues. Incompatible
relations are checked after expansion, including references discovered later. The
expander does not backtrack across alternative versions or claim that a conflict proves
no solution exists. The host must resolve choices before publication. Explicit roots
cannot be displaced by a transitive selection.

This is dependency evidence, not an installation plan or authority to delete unlisted
content. Mutation preparation still needs to assign roles/placements, resolve requirements
and compose this evidence with verified publication.

## Provider search

`ProviderCatalog::search_projects` returns choices grouped by provider preference
and explicitly accepted game version. Each window retains its original provider
rank, total, offset, continuation offset and truncation flag. Reaching a provider's
paging ceiling does not claim the catalog is exhausted. Hosts follow pages explicitly;
search does not fetch an unbounded catalog or silently choose a winner.

Modrinth uses all advertised project types so mixed mod/datapack projects remain
findable. CurseForge uses Minecraft class IDs and game-specific loader filters.
Both adapters distinguish an empty successful query from transport/authentication
failure. Unsupported provider kinds remain explicit capability evidence. A later
provider failure discards earlier windows. All requests and retries share one
byte/deadline budget; retained pages keep their memory reservations.

Fuzzy title/slug similarity helps order a window, using bounded strings and rolling
edit-distance rows. It does not prove identity, file ownership or compatibility.
Chosen candidates still go through canonical lookup and exact or compatible
selection before preparation. These capabilities do not yet replace CLI search.

## Content identification

`ProviderCatalog::identify_file` starts from retained acquired bytes and the exact
providers requested by its caller. Modrinth lookup uses the observed SHA-512.
CurseForge lookup uses a whitespace-normalized Murmur2 fingerprint computed in two
bounded streaming passes. Its fingerprint nominates candidates; it never establishes
content integrity. Fingerprint nominees are compared with the acquired bytes before
project lookup or content-class interpretation, so an unrelated collision cannot hide
a valid match. Malformed evidence and inconsistent owner records still fail. Every accepted file must also match the provider's original
hash assertions and size. Matching roles remain explicit, including several files
with identical bytes in one version.

Results distinguish unknown, one exact selection and several possible selections.
Provider preference does not silently discard a second match. Missing credentials,
authorization failures, incomplete fingerprint indexes, malformed identity and
transport failures remain errors. A later provider failure returns no earlier
successful subset. The lookup, project metadata and retries share one cumulative
byte allowance and deadline. Response parsing and fingerprint work belong to the
operation; retained results keep their resource charges.

This catalog capability does not adopt a file, rewrite its source provenance or
publish a project. Original MD5/SHA-1 assertions remain weaker evidence even when
the acquired bytes have an internal SHA-256 address.

## Normalized import inspection

`inspect_import` consumes an acquired archive and returns declarations with source
locations. It validates all archive members before reading the bounded manifest.
The original archive stays available for later digest-checked extraction. Parsing
has no project root, provider client, process launcher or publisher.

Mrpack files retain their destination, every supported digest, size, download
alternatives and independent client/server requirements. Read-only inspection retains
signed HTTPS and HTTP declarations with redacted, record-specific diagnostics.
Preparation must resolve transient-locator persistence or HTTP transport policy;
inspection neither authorizes the transfer nor writes a durable URL. A CDN-shaped URL does not
become a canonical provider identity. Empty download lists refer to a required
embedded member; missing members and size conflicts fail inspection. Compatibility
archives missing the specified SHA-1/SHA-512 pair retain their actual assertions and
a diagnostic. No computed hash upgrades that evidence.

CurseForge records retain exact project/file IDs and required/optional participation;
they do not acquire invented mod filenames or content kinds. Loader declarations
remain available for selection, including a declared primary. Vanilla is represented
by no loader. Unknown runtime requirements fail explicitly.

Common, client and server overrides retain separate layers and portable attributes.
Exact layer replacements are distinct from case aliases and file/ancestor conflicts.
Manifest downloads and common overrides stay separate so preparation can preserve
the format's replacement order. Optional defaults/descriptions absent from an import
remain absent; resolution must supply an explicit choice. Datapack-folder inference
returns competing suggestions with evidence, without changing backend options.
Auxiliary archive members remain recorded rather than silently disappearing.

This is the inspection boundary. The acquisition composition below adds verified bytes;
semantic candidate assembly follows below. Replacement planning and project publication
still need composition before the CLI import path can be replaced. The existing importer remains
available until that complete replacement passes the feature requirements.

## Shared override precedence

The compiled model now distinguishes base content from a common override.
`ContentLayer::CommonOverride` occupies `overrides/common/` and uses
`common-override` in both normalized documents. Archive adapters map their shared
`overrides/` tree to this layer; declared files remain base content. The project
reader captures both roots independently.

Projection follows base, shared override, then selected side. Optional replacements
retain fallback bytes and require a selection when a flat format cannot preserve
the choice. Same-layer collisions still fail. Mrpack re-export materializes an
otherwise ambiguous overlap into the effective client/server views, retaining the
source inventory rather than relying on archive insertion order. This fills the
explicit source-ordering provision in the target model.

This wave passed 1,667 tests and eleven doctests, 283 core/engine checks and
all-feature Clippy. Both real Fabulously Optimized 1.20.1 archives passed content
verification: 78 files per format, with two CurseForge files explicitly supplied
and reverified. This does not establish import publication or CLI parity.
Greptile review 64's mixed-mirror failure reproduced before correction: acquisition
now selects permitted alternatives while retaining every original declaration.

## Verified import content

`ImportContentPlan::resolve` composes archive declarations with exact provider records.
Provider references share one catalog byte/deadline allowance. Repeated pins reuse
records; conflicting versions of one project fail before lookup. Provider identity,
all selected file assertions and format record locations remain available.

Acquisition checks the complete inventory before returning `VerifiedImportContent`.
Declared destinations, optional requirements and common/client/server layers remain
unchanged. Restricted files and unsupported transports return exact `NeedsInput`
obligations before unrelated extraction or downloads. Supplied bytes bind to an
obligation and are reverified against its original digest and size. MD5 remains
weaker source evidence. Supplied files survive an additional pending input decision.

Embedded files share one admitted archive reader. Verified bytes use packed private
backing, retain their resource reservations and remain readable independently.
Downloads share cumulative bytes and a deadline across files and mirrors. A later
provider, integrity, resource or transport failure returns no successful subset.
Record and content-size limits are separate from network allowances; bookkeeping
is charged for actual records rather than the configured maximum.

These values grant no project write authority. The interpretation boundary below
builds candidate documents; native replacement preparation follows below. Composition
with the Engine's operation-bound approval lifecycle remains separate.

## Import candidate interpretation

`VerifiedImportContent::into_candidate` requires a decision for every verified file.
It preserves declared destinations, layers and required/optional/unsupported
participation. The host supplies absent optional defaults and descriptions, provider
placements, logical keys and content-kind choices. Provider choices must match the
exact catalog selection. Multiple files retain one provider identity and exact pin.
Competing runtime declarations require a selection unless there is a sole primary.

URL files remain URL roots with their original digest assertions. Transient or unsafe
locators cannot enter durable documents. An explicit local-retention choice keeps the
verified bytes and records the conversion. Embedded files use observed content identity
without presenting it as independent source authentication. Shared and side overrides
remain separate roots. Unknown auxiliary members require an explicit decision. The
`empack.import` extension records the source archive SHA-256 and exact exclusions,
including for an otherwise empty import.

The candidate binds every lock file slot to retained verified content, checks portable
collisions and reserved backend paths, and round-trips the durable document boundary.
Known required edges between included provider selections are retained; partial
coverage cannot authorize removal. Source declarations and provider facts remain
available through the retained import. The candidate holds no project writer and
cannot authorize replacement on its own.

## Shared project initialization and replacement

The native application initialization host now consumes existing CLI options and session
interaction, resolves missing runtime choices, and publishes through `InitializeRequest`.
It preserves explicit pins, accepted versions, datapack layout, default targets and editable
scaffolding. The test sequence initializes a real directory, synchronizes twice without
changes, and inspects both mrpack and full-client builds. Separate fixtures verify forced
preview/decline, approved replacement, invalid choices, stable/prerelease discovery and
provider failure without publication. This host is compiled and callable; replacing the
legacy CLI dispatcher remains a coordinated integration gate with the other command hosts.

All 14 affected host/catalog tests pass, including deterministic HTTP discovery and
native initialization → repeated synchronization → inspected distributions. All-feature
Clippy and Windows cross-compilation pass. An intermediate combined run reported
Nextest pipe-leak warnings in two pure catalog tests; the final combined run is clean.


`ProjectReader::capture_replacement` binds managed documents and content roots even
when the prior documents are missing or malformed. It gates recovery, rejects unsafe
ancestors and captures membership and absence. Captured ignore rules exclude unowned
pack files before opening them; the existing rule file remains unchanged. Incoming
paths omitted by that traversal need independent unfiltered absence evidence, so an
ignored backup cannot be overwritten merely because it was missing from the managed
inventory. Distributions and unrelated root files remain outside this footprint. Initialization
may explicitly seed selected templates; existing user versions are retained.

`prepare_project_replacement` consumes a complete semantic candidate. `RejectExisting`
refuses occupied managed content. `ReplaceManagedContent` produces an explicit
file-level replacement/removal plan, with no recursive directory deletion. Every
candidate document and payload is staged and checked against that plan before
publication. Portable file attributes survive staging. Source leases retire before
the frozen publication tree is packed.

The trusted host can publish the verified plan through the journaled publisher.
Concurrent source edits, added files and cancellation before publication prevent the
whole replacement. Tests publish into an empty existing directory and replace a
broken project, preserve unrelated files, and re-export the correct layered bytes.
The shared Engine accepts build, import, initialization, resolved addition, recorded synchronization and removal requests. Import preparation stages
the full candidate under admission, exposes its file plan and requires an exact
replacement-summary acknowledgement before existing managed files can change.
All three operation kinds use the same engine-bound approval, owned runtime, cancellation
and retained terminal outcomes. Import receipts retain the published semantic project.
New-root import now uses `ProjectReader::capture_new` and `ProjectTarget::New`.
The parent must exist, the child must be a valid portable directory name, and the
captured destination must remain absent. Preparation does not create it. Approved
execution stages and verifies the whole root beside its destination, writes durable
recovery intent and publishes with a no-replace directory rename. File permissions
and the expected inventory are checked independently before and after the rename.

The immutable parent/name record can recover a crash before the root exists; an
authoritative journal belongs to the created native root identity. Ordinary project
reads therefore detect unfinished creation without depending on the spelling or
current parent of the project path. A completed project can move and its old name can
be reused. Recovery verifies retained candidates and refuses unrelated occupants or
changed bytes. Cancellation is deferred after durable intent. Pre-intent ordinary
failures discard private scratch; abrupt process death before intent can leave private
scratch that still needs the planned retention/cleanup catalog.

Windows creation uses `NtSetInformationFile` with a retained destination directory
and `ReplaceIfExists = false`. Native CI at `8b1790b` exposed error 87 in the Win32
wrapper when given a relative destination and root handle; the native call follows the
[relative-name contract](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/ns-ntifs-_file_rename_information)
directly. Cross-compilation verifies the bindings; native CI must verify execution.

Existing-root recovery now enters the shared Engine lifecycle. Read-only inspection
returns the interrupted operation without reading potentially damaged authoring
files. `RecoverRequest` selects finish or restore; preparation binds the journal
revision, current native objects and exact remaining effects. Execution requires the
normal plan-specific grant, validates that binding under publication ownership and
retains a typed recovery receipt. Restoration reserves space for both retained inverse
images and publication siblings. Native tests verify read-only plans, both recovery
directions, stale journal progress and late user edits. All 18 publication/recovery
tests pass, including process-crash boundaries and the public approval lifecycle;
all-feature Clippy and Windows cross-compilation pass at `8bfcbe1`.

Creation recovery now uses the same inspection, approval and receipt surface. It
finishes an absent destination or an already visible retained root, including a moved
root. Preparation verifies the complete retained inventory and binds the selected
parent/name, native identity and journal revision. A new occupant or intervening
recovery invalidates approval. Restore is refused because creation approval does not
authorize root deletion. Durable continuation and CLI composition remain required.

Review 94 reproduced a recovery-preview mutation after a real child-process crash:
preflight removed the recorded publication sibling. Read-only recovery capture now
omits only exact validated journal scratch paths; approved finish or restore performs
cleanup after preflight. Final inventory verification still observes the full tree.
The regression covers both recovery directions, root and nested siblings, preserved
bytes and modification times, and conflicts from unrelated additions.
The combined run passed 1,844 tests and exposed one missing-directory error in the
new test fixture. After correcting that fixture, all 38 affected tests and eleven
doctests pass. All-feature Clippy and Windows cross-compilation also pass; native CI
will verify the final combined revision.

`InitializeCandidate` validates empty intent and an exact runtime across all loader
families. It preserves metadata, compatibility alternatives, layout, distribution
preferences and extension values. The shared project-change module stages its documents
and optional template seeds without an artificial archive or backend call. Captured
existing user templates remain outside the write set, retaining bytes and permissions.
Seeds compare rendered destinations across their own and common layers, so a different
filename or a common-layer template cannot be shadowed by a default. A persisted traversal policy selects only entries capable of colliding with seed
outputs. Unrelated template bytes, links and names are excluded before content reads
and portable validation. Relevant additions and edits block publication; unrelated
files stay outside the read and write sets. Directory enumeration remains bounded. Missing seeds retain expressions until build time. Conflicting edits, directories and links block publication. A published
empty project feeds the existing mrpack build path directly.

This is semantic Engine initialization, not CLI cutover. Interactive runtime selection,
root-level repository scaffolding and command wiring still need integration. Current
CLI capabilities remain available until that parity work verifies. Initialization's
host-supplied exact runtime is checked for internal coherence, not online availability.

## Composed build behavior

Build HTTP acquisition now uses the same cumulative batch transfer as provider
additions and imports. All declarations validate before the first request; multiple
files cannot reset byte or time allowances. The per-file reset reproduced; 22
affected acquisition/build-refresh tests and all-feature Clippy pass after correction.

`prepare_mrpack_build` retains one workspace read set through publication. Preparation
writes private candidates only. Source changes block publication before the previous
artifact is replaced. Unrelated distributions are outside the read set. Existing and
new selected outputs use artifact budgets; those allowances cannot widen source-file
limits. Recovery persists and reuses each capture group's limits. Publication
preflights the complete candidate against those captured allowances before copying;
private staging uses the exact planned sizes rather than an unrelated default ceiling.

`plan_build_acquisitions` distinguishes sufficient reference evidence from required
materialization. It accounts for layered replacements and unlisted backend files.
`acquire_http` verifies every requested download before returning a batch; unresolved
manual, provider-locator and missing-archive work remains pending. Captured ZIP
members are acquired through one retained reader per archive, and the source archive
is not silently included as game content. Outputs cannot overlap source scopes,
including portable case aliases. Neither step can
publish. Final build verification still checks supplied content against its exact
logical file and captured installation.

Unlisted installed content remains an observed obligation instead of becoming invented
manifest intent. Its bytes must match its declaration. Locked backend comparisons
record whether they matched acquired bytes, a same-algorithm declaration, or an
independent locked reference with no comparable backend algorithm. Mismatches block
preparation. Backend URLs never replace locked download assertions.

Verified observed bytes with a transient/non-persistable URL are embedded without
exporting the locator. Ordinary optional references retain optional participation.
Mrpack cannot represent optional embedded content or a selectable replacement with a
common fallback losslessly. Those cases require an explicit representable choice;
acknowledging missing description/default fields does not make optional content mandatory.

`prepare_game_content` projects complete client/server game views from the same
captured obligations as mrpack. It preserves source assurance per logical owner,
exact locked resolution and observed-backend evidence. Unacquired content on the
other side or behind a disabled choice does not become a required download. This
is game-content completeness, not launcher/server runtime completeness.
`prepare_bootstrap_game_content` uses the same captured obligations for client/server
references. `PreparedGameContent::packwiz` checks generated metadata through the shared
reader; unlisted metadata-only references retain `actual: None` rather than a fabricated
byte observation. Full targets still require actual selected bytes.
Vanilla, Fabric, Quilt, Forge and NeoForge runtime resolution and byte/launcher
verification are implemented; server recipes bind that preparation to the captured lock. Server output records original runtime evidence; computed
SHA-256 does not turn a SHA-1 declaration into strong source assurance.

Template preparation selects common and target-side inputs before rendering. Exact
side replacements retain their replaced source identity; same-layer duplicates,
portable aliases and file/ancestor conflicts fail. `.template` files require UTF-8;
other files preserve the text-or-binary convention, with an explicit `Copy` mode
for literal UTF-8. Expressions use current bound metadata and exact runtime versions.
Missing values and rendering/size failures return no partial set. Project templates
remain untouched. `BOOTSTRAP` distinguishes lightweight from full-target defaults.
Full client preparation adds a format-1 launcher component manifest tied to the
locked Minecraft/loader versions and maps selected game content under `.minecraft`.
Default full-client settings do not run an absent bootstrap installer. A captured
user configuration remains an explicit input; arbitrary user commands are not
certified by artifact verification. User component manifests must preserve the
locked game/loader. Template/game collisions fail before publication.

Full client archives materialize pack content, not Minecraft binaries and assets:
the launcher still resolves its normal game/runtime components. ZIP supports direct
launcher import; TAR.GZ and 7z contain the same instance tree for explicit extraction.

`prepare_build_batch` composes mrpack, client, server and both full recipes under the
AllRequested rule. A later failed recipe drops earlier private candidates. Source
changes block the whole publication, and output collisions fail preflight. One
recoverable journal owns the combined artifact changes; this does not claim an
atomic filesystem-wide visibility switch. Each result retains exact resolution,
original backend evidence, conversions and expected archive members. Remaining
CLI composition must route every supported target through this boundary.

## Canonical addition planning

Native local-file acquisition now feeds direct-file normalization and publication.
The reader captures only the selected regular file, preserves portable attributes,
rejects changed or replaced input and charges actual retained bytes. Four reader
tests plus three direct-file tests pass, including real file acquisition through
publication and two no-op syncs. All 48 affected acquisition/addition/API tests,
all-feature Clippy and Windows cross-compilation pass. CLI selection, content
identification and archive interpretation remain integration work.

Direct local and URL files now normalize through `FileAddition::from_acquired`.
Declared digests or explicitly accepted observations remain distinct; optional and
side-specific placements, URL alternatives and portable source permissions reach
native publication. Local source intent points at the published first placement.
A direct file retains unknown dependency coverage rather than claiming no required
content. Three focused tests pass, including local/URL publication followed by two
no-op syncs and rejection of mismatched evidence, duplicate keys, persistent secrets
and attempted promotion of observations to strong source authenticity. The host
still supplies acquired bytes and explicit type/placement choices.
The combined snapshot passes all 1,827 default tests and eleven doctests, all-feature
Clippy and Windows cross-compilation. One legacy Forge test reported an output-pipe
leak in the full run; its isolated rerun passed without that warning.

`AdditionGroup` contains a resolved request whose selections are all reachable from
explicit roots. `AdditionPlan` binds provider identities to existing logical keys,
retains unrequested installations and settings, and carries explicit pin intent into
the next project. An existing alias wins over a proposed new label. Occupied labels,
changed shared selections, mismatched runtimes and unjustified installations fail the
whole request before native preparation.

Repeated exact additions preserve the logical state. Known required edges are not
lost when a weaker observation arrives for the same exact selection. Compatible edge
sets merge; contradictory complete evidence requires explicit resolution. A changed
pin cannot silently invalidate retained dependents. `AdditionCandidate` binds raw
source revisions and computes coherent next documents without publication authority.
Host request composition remains implementation work; this planner alone does not
replace a CLI command. Eight focused
contract tests and all-feature Clippy pass for this planning boundary.

Explicit foreign-identity replacement now composes the existing removal safety rules
and addition planner into one native publication. Every selected installed key needs
an explicit replacement root. Retained aliases cannot redirect the selected target;
changes to unselected existing roots fail. Known incoming dependencies remain binding,
and acknowledged incomplete evidence is visible with the old exact records in the
preview and receipt. Native verification still requires original bytes, a complete
new inventory, and unoccupied untracked destinations. Composed tests replace the
identity, retain unrelated content and synchronize twice without changes. All 76
affected addition, adoption, capture and sync tests pass, with all-feature Clippy and
Windows cross-compilation. This is targeted evidence after the full `dec0d33` run.

Observed adoption now uses the same resolved group, canonical binding and selected
capture. `AdoptObservedRequest` verifies present payloads against the proposed source
assertions and validates relevant backend records against exact selections and
requirements. It publishes descriptions and refreshed index digests without rewriting
payload bytes. Missing or mismatched content fails before publication; late edits fail
the captured read set. The public lifecycle requires exact plan authorization and
retains a typed adoption receipt. Native and public tests cover explicit adoption,
wrong/missing bytes, changed observations, provider pin mismatch, repeated adoption,
unchanged modification times and a subsequent no-op sync. Adoption can now establish
a first lock from a group resolving all retained authoring roots and runtime
requirements. The missing-lock failure reproduced through the public API before the
change. Public regressions verify read-only preview, unchanged payload times, two
no-op syncs, missing/wrong bytes and late payload/lock collisions. Present stale locks
still require reconciliation. The combined snapshot passes all 1,834 default tests
and eleven doctests without output-pipe leak warnings, plus all-feature Clippy and
Windows cross-compilation. Host/CLI resolution remains integration work.
Review 92 reproduced an existing authored root being overwritten during first-lock
adoption. Existing roots now remain intact and constrain validation; regressions
reject mismatched identity, pin and placement, while a matching authored pin remains
unchanged. Review 91 also identified unnecessary UTF-8 source-name validation. Native
file acquisition now retains OS strings. Its non-UTF-8 filesystem regression runs on
Linux; the local macOS filesystem itself rejects such names. The 91-test affected
suite passes, followed by five focused tests after the native helper refinement.
All-feature Clippy and Windows cross-compilation pass. Linux filename execution
remains a CI check rather than locally executed evidence.
Adoption deliberately verifies the proposed description of selected observed drift,
rather than requiring the previous lock's bytes. Its old documents still participate
in concurrency checks and recovery. A native regression reproduced rejection of valid
indexed metadata; the shared index adapter now separates payload and metadata updates
and preserves their roles, aliases and extension fields.

Dependency requests now distinguish materialized payloads from explicit references.
Every selected slot must be supplied, including deferred provider/manual obligations.
References retain original assertions and expose their canonical slots in previews
and receipts. Local and archive-member content cannot be deferred. Native planning
preserves matching existing bytes, refuses modified user content, and explicitly
retires verified old materializations when their requested reference changes. The
lock supplies build obligations; backend metadata remains a derived representation.
Cross-command tests exercise reference addition, repeated sync, reference export,
later materialization, restricted CurseForge evidence and scoped index retirement.
Validation: 75 affected dependency/removal/API tests pass, followed by the expanded
indexed-metadata regression including rejection of a wrong index role. All-feature
Clippy and Windows cross-compilation pass. The most recent full run is `f0cdfd2`:
1,799 default tests and eleven doctests; that run predates reference inputs and the
indexed-metadata correction. Fresh native CI and final combined live checks remain
separate release gates.

A further native regression reproduced stale direct index entries when a referenced
payload was already absent. Add/update and sync now retire those owned entries even
when there is no payload left to delete. Both focused regressions pass, including
retention of a neighboring present file and a second no-op synchronization.

The native addition adapter captures old and new selected placements, binds supplied
request slots to canonical aliases, and verifies original digest/size/observation
assertions. Present old bytes must still match the prior lock. Untracked files,
directories, selected links and extra input slots fail the whole preparation. Missing
tracked files can be restored without changing their logical selection. No-op re-adds
preserve raw intent and lock documents, including cosmetic edits.

Updates retire exact owned derivative metadata and rebind affected direct index
entries while preserving unrelated entries, aliases and fields. The shared index adapter
rebinds the pack digest. Staged content, documents and explicit removals publish through
the existing journal; changed inputs after preparation prevent publication. Native
regressions cover first addition, repeated addition, alias binding, restoration,
wrong bytes, collisions, links, update/index publication and post-preview changes.

Resolved additions now enter the shared Engine lifecycle through `AddRequest`. Existing
requested identities are refused by default; same-identity updates require an explicit
policy. Preparation returns a read-only preview with exact canonical bindings and file
changes. Plan identity, replacement acknowledgement and engine ownership remain required
at execution. Resource reservations and terminal outcomes follow the same rules as
other mutations. A composed removal/addition/re-add/build test checks durable intent,
bytes, no-op behavior and release of retained resources. This API currently takes
resolved, acquired inputs; it does not yet replace the CLI's selector/acquisition host.

The combined addition revision passes 1,770 default tests and eleven doctests, plus
52 focused engine tests, all-feature Clippy and Windows cross-compilation. Native
cross-platform CI and review remain separate gates; these counts do not establish
completion of the remaining request hosts or lifecycle services.

## Explicit dependency updates

`AdditionPlan::prepare_update` reuses canonical identity and dependency-closure checks
while preserving current authoring intent. Requested identities must already be
installed. Resolved group roots select work; they do not promote transitive content,
change source declarations or relax explicit pins. Retained required dependents remain
binding, and unrelated installations are preserved.

`UpdateRequest` runs this contract through shared capture, private staging, exact-plan
approval and publication. Its preview lists requested canonical records and replacements;
its typed receipt retains the coherent published project. Raw authoring comments remain
unchanged. Addition and update now stage only changed files, with unchanged observations
retained as publication preconditions. Provider selection and acquisition hosts, batch
continuation and CLI routing remain unfinished.

The affected update/addition tests pass, including aliases, unknown identities, pins,
transitive root preservation and update followed by two no-op sync previews. All-feature
Clippy and Windows cross-compilation pass. The combined revision passes 1,786 default
tests and eleven doctests. One pure legacy-coordinate test reported a nextest pipe-leak
warning; the check contains no subprocess or I/O operation. Its isolated repeat passed
cleanly. Native CI and Greptile for this update revision remain separate gates.

Add and update now share synchronization's bounded selected-metadata discovery. The
unrelated malformed/directory-valued metadata regression reproduced before the change;
91 affected tests and all-feature Clippy pass afterward. The update-to-sync composition
also preserves an unrelated malformed record. This targeted result follows the full
1,786-test update revision above.

## Recorded synchronization candidate

`SynchronizationCandidate` validates current intent against the prior exact resolution.
Authoring-only changes can rebind the lock without new acquisition or version choice.
Raw intent remains unchanged; an already bound lock requests raw-byte preservation.
Unlisted selections and original source assertions remain. Changed pins, sources,
placements or runtime requirements that the old lock cannot satisfy produce a typed
`ResolutionRequired` outcome. Changed-intent resolution and acquisition hosts remain pending. Four focused contracts and all-feature Clippy pass.

The shared Engine accepts `SyncRequest` with exact acquired slots, publishes an
engine-bound restoration plan, and retains a typed receipt. The composed sequence now
exercises removal, addition, re-addition, synchronization twice and distribution builds.
A changed runtime fails before publication with `ResolutionRequired`.

Native synchronization captures recorded placements and restores missing or modified
files from verified exact bytes. It retains unrelated content, checks directory/link
conflicts, preserves raw no-op documents, and rebinds authoring-only edits. Stale
metadata at selected destinations is removed explicitly; existing pack metadata and
runtime fields follow the coherent project. Addition, removal and synchronization
share frozen mutation staging and independent final inventory verification.

The review 79 index-loss regression reproduced before correction. The shared index
adapter now separates deletion from direct-file updates, preserves aliased entries and
extension fields, and honors disabled internal hashes. Per-entry digest algorithms
follow the [pinned backend wire contract](https://github.com/mannie-exe/packwiz-tx/blob/v0.2.0/core/indexfiles.go), so changing one entry does not reinterpret retained hashes.

The combined native synchronization and index correction passes 107 affected tests and
all-feature Clippy. One pure addition test reported a nextest pipe-leak warning; its
isolated repeat passed cleanly. The broader final-head suite remains a later gate.

The shared synchronization API revision passes 1,779 default tests and eleven doctests,
47 focused tests, all-feature Clippy and Windows cross-compilation. The full run has no
pipe-leak warnings. Review 80's two regressions reproduced independently of that suite.
Synchronization now discovers backend records through bounded, non-following reads and
captures only interpretable records naming locked common destinations. Malformed,
nonregular and unrelated metadata remains untouched. Selected metadata and payloads
still bind publication to their observed identities and bytes.

Synchronization stages only changed files and documents. Unchanged inputs remain in
the native read set and durable postconditions without being copied into the candidate.
The advanced API still accepts acquired bytes for every locked slot; this change does
not claim to eliminate acquisition by a future host. The corrections pass 106 affected
tests and all-feature Clippy, including empty no-op staging, late selected-file edits,
malformed and symlinked metadata, bounded discovery and publication recovery.

## Changed-intent synchronization

`SynchronizationResolution` checks fresh resolution against the previous lock and current
authoring intent. Valid unpinned selections and runtime versions remain exact; new or
changed roots may introduce only their required closure. Unlisted records remain, and
known retained dependents cannot be invalidated. One root-conformance check now serves
both project validation and reconciliation, including allowed search providers.
A changed unresolved search requires fresh resolution rather than silently reusing a
selection for an unknown previous query.

Unique provider-label changes preserve the same installation and rebind required edges.
`SyncRequest.resolution` carries a coherent fresh candidate through the existing engine
lifecycle. The native plan captures old and new placements, refuses untracked occupied
destinations, and verifies obsolete bytes before removal. A first lock can be created
from fresh resolution when its destinations are absent; matching existing bytes do not
authorize adoption. Raw authoring documents remain unchanged.

Tests cover changed pins, fresh required dependencies, retained selections/dependents,
aliases, search policy, placement publication/conflicts and first-lock creation. The
composed engine sequence now exercises explicit update, no-op sync, an author-edited
pin, fresh synchronization and another no-op. The combined revision passes 1,795 default
tests and eleven doctests without pipe-leak warnings, all-feature Clippy and Windows
cross-compilation. Earlier targeted runs passed 103 affected tests and all 23 core tests;
the final 23 sync/API tests passed cleanly. Native CI and review remain separate gates.

## Semantic removal planning

`RemovalPlan` in the pure core selects exact logical keys from a coherent resolution.
`RemoveContent` rejects retained required dependents. Incomplete retained dependency
evidence is refused by default and requires explicit acknowledgement to proceed. Selecting an entire required cycle is valid; no unrequested selection is
collected. `ForgetRoots` removes explicit authoring roots while retaining exact files,
pins and graph evidence. It is a distinct outcome, not successful content deletion.

`RemovalCandidate` binds the source documents, computes the next semantic revision and
encodes coherent intent/lock documents. Unknown or repeated selections fail the entire
request. A stale lock cannot establish removability, and the original raw intent
revision remains a publication precondition. These values establish semantic coherence;
native ownership and publication require the separate preparation described below.
Seven regressions cover batch errors, cycles, evidence gaps, demotion, unrequested
retention and stale documents.

`ProjectReader::capture_removal` first observes bounded backend metadata and resolves
queries, then captures only the selected managed placements into a `MutationSnapshot`.
The lower-level `capture_mutation` captures all locked placements for callers that
have already selected exact keys. Locked placements remain visible through ignore
rules; unrelated payload bytes, templates, artifacts and external acquisition paths
grant no deletion authority. The traversal policy survives journal persistence and
revalidation. Backend discovery still has finite directory-entry and metadata limits. `prepare_removal` checks
existing payloads against original lock evidence, rejects directory targets, and
binds derivative metadata by exact selection, destination, requirements and digest.
Manifest labels never become backend filenames. Unlisted files remain untouched.

The candidate contains updated intent/lock documents, exact file removals and, when
present, an updated backend index and its pack-document digest. Existing index
content must match its digest when one is declared; portable aliases cannot leave a stale reference
to removed content. An omitted empty file list and disabled internal index hashes
remain valid, following the pinned [index reader](https://raw.githubusercontent.com/mannie-exe/packwiz-tx/v0.2.0/core/index.go)
and [pack options](https://raw.githubusercontent.com/mannie-exe/packwiz-tx/v0.2.0/core/pack.go).
This does not relax source-content assertions. Private staging and scoped file verification feed the shared
recoverable publisher. Root demotion publishes documents only. Concurrent document
or payload edits invalidate the entire plan. This lower-level composition is tested
on real filesystems. Shared Engine removal carries logical keys or user queries and an
explicit mode through preparation, footprint acknowledgement, execution and retained
receipts. Failed admission and stale inputs return failure before publication.

Removal observations share digest validation with acquired content but retain no
payload copy. A bounded planning worker determines exact document staging bytes;
separate admission covers staging and publication before-images. Root demotion and
physical removal feed subsequent builds with distinct preserved inventories. The
selector resolves exact keys first, then ASCII-case-insensitive titles and exact
installed stems. Metadata ownership includes the exact pin, placement and requirements.
Ambiguous matches return candidate keys; equivalent aliases select one dependency.
A different installation from the same provider project does not claim another
locked destination. Untracked stems and exact metadata paths select observed files
without inventing logical roots or provider identity. Their declared hashes must
match regular managed content before removal; directories, links and backend-control
roles remain forbidden. Preview and receipt retain observed selections and untracked
dependency evidence without acquisition locators. Ambiguous stems require an exact
metadata choice. Observed-only removal preserves raw intent/lock bytes. CLI integration
remains implementation work.

## Existing command guarantees

Earlier fixes remain in the live commands: canonical add/sync/remove identities and
pins, conservative retention of unlisted dependencies, failure on required manifest
publication errors, import preflight before forced reset, bounded archive downloads,
preserved environment/optional semantics, fresh build prerequisites, confined local
removal and process-tree ownership. Core values and the shared archive writer already
serve these paths. Their continued presence does not give them the new engine's full
multi-file publication contract.

Use the [feature requirements](parity.md) when replacing each path. Retain provider,
loader, import, archive, template, continuation, cleanup and host-configuration
capabilities. Old experimental schemas do not need migration machinery; useful
features are not optional migration work.

## Verification snapshots

Results describe the stated revision, not every later edit.

| Revision | Executed checks |
| --- | --- |
| Shared Engine removal | 1,735 tests and eleven doctests pass before the final backend-option case. The final 37 affected tests pass, covering streamed evidence, native removal, grants, resource failure and removal/demotion followed by builds. The empty-index failure reproduced before correction; omitted file lists and disabled internal hashes now verify. Final all-feature Clippy and Windows cross-compilation pass; real Modrinth and CurseForge imports each publish and reverify 78 roots |
| Creation review 73 | The unrelated-template byte-limit regression reproduced before correction. All 45 affected initialization, snapshot, source and publication checks pass, including real process-crash recovery. Filtering is retained in recovery observations. Native CI at `bcbd8e5` passed Linux/macOS/Windows default and strict E2E suites, all three import-smoke jobs, lint and coverage |
| Creation review 72 | 1,712 tests and eleven doctests pass; all-feature Clippy and Windows cross-compilation pass. The template collision reproduced before correction. New cases cover literal/template aliases, common-layer precedence, portable case collisions and additions after preparation. Native Windows execution remains required for the relative rename correction |
| Semantic initialization | 1,709 tests and eleven doctests pass. Both real import formats publish and reverify 78 roots through the shared project-change implementation. All-feature Clippy and Windows cross-compilation pass. Seven initialization regressions cover loader intent, approval, user templates, conflicts, directory/link rejection and a subsequent mrpack build. Native CI remains required |
| Creation review 70 | Both moved-root and first-index-failure regressions reproduced before correction. All 39 affected publication/native/API tests and all-feature Clippy pass. Windows handle lifetime is corrected; native Windows CI remains required |
| New-root publication | 1,700 tests and eleven doctests pass. Both real import formats publish 78 roots into previously absent destinations through Engine approval, with every placed file checked. Forty-three focused publication/project/API checks cover real process exits and recovery; Windows GNU cross-compilation passes. The final Windows-only shared-parent ACL guard and regression compile but still require native CI execution |
| Shared build/import Engine | 1,685 tests and eleven doctests passed before the final control-root guard. The final 302 core/engine checks and all-feature Clippy pass; one nextest pipe-leak warning passed cleanly in isolation. Both real import formats published 78 roots through Engine approval and verified every placed file. All seven live provider probes and all eleven Java runtime/distribution probes pass |
| Native-name review 68 | The original ignored `backup?.zip` failure reproduced. Exclusion now precedes portable validation; included invalid names still fail. A broad-ignore control-root regression also reproduced and is covered by the final core/engine checks. Linux-only invalid-Unicode coverage is compiled for native CI; APFS rejects those names at creation |
| Replacement review 67 | Ignored-file deletion and ignored-link traversal both reproduced. The ownership fix passes 26 replacement/project/publication checks; all-feature Clippy passed on the combined Engine working tree. Explicit incoming destinations need real absence evidence; existing unowned files and source rules remain untouched |
| Native import replacement | 1,678 tests and eleven doctests passed, along with 58 affected import/project/staging/publication checks and all-feature Clippy. Both real Fabulously Optimized formats published 78 roots into temporary projects; every placed file was reread against original evidence. This remains lower-level publication evidence, not Engine approval or CLI parity |
| Import candidate interpretation | 1,673 tests and eleven doctests passed before the final provenance/index refinement. The final 50 core/import checks and all-feature Clippy cover that refinement. Both real formats produced 78 coherent roots, including explicit restricted-file association and exclusion of the known generated `modlist.html` report. The first CurseForge candidate probe correctly refused that unacknowledged auxiliary member. Publication and CLI parity remain unfinished |
| Verified import content | 1,661 tests and eleven doctests passed without a nextest leak warning; 83 affected catalog/import/acquisition tests and all-feature Clippy passed. Both real Fabulously Optimized 1.20.1 archives passed full content acquisition with explicit prior-byte association enabled. The initial CurseForge run correctly stopped for two restricted files; the Modrinth pass verified 78 files / 27,902,856 bytes. This is acquisition evidence, not project publication parity |
| Closure review 62 | Both the 16 MiB small-graph admission failure and incompatible required-pin abort reproduced. Incremental bookkeeping and explicit incompatible-requirement evidence pass the combined 80 catalog/import/acquisition regressions |
| Required dependency expansion | 1,651 tests and eleven doctests, 54 affected provider tests, all-feature Clippy and all seven live provider probes passed. Cycles, conflicts, incomplete evidence and shared budgets are covered. One staging test emitted a nextest pipe-leak warning; its isolated repeat passed cleanly. Three graph-capacity edge cases and Greptile review 61's 8 MiB pin-lookup admission failure reproduced before their fixes |
| Exact pin ownership | 44 affected provider tests, all-feature Clippy and six live provider probes passed. The three exact-provider cases compare owner-free lookup with project-qualified identity and every original file assertion. The preceding full-suite snapshot is the bounded-search revision below |
| Bounded provider search | 1,638 tests and eleven doctests; 41 affected provider tests and all-feature Clippy passed. All six live provider probes passed, with search exercised for Sodium, JEI and Terralith. An initial focused run emitted a leak warning; the expanded provider run and full suite passed without it. Greptile review 59 is green for the preceding kind/fingerprint commit `380d9a0` |
| Provider kinds and fingerprint filtering | 1,630 tests and eleven doctests, all-feature Clippy and six live provider probes passed. Terralith resolves as a datapack despite also advertising mod versions. Review 58 fingerprint-collision failure reproduced before the fix; unrelated candidates no longer trigger project lookup |
| Content identification | Combined suite: 1,626 tests and eleven doctests; 40 affected import/catalog tests and all-feature Clippy. All five live provider probes passed, including content identification of downloaded mod/resource/CurseForge bytes. Greptile review 57 is green for the accompanying transient-import fix at `d1f69c6` |
| Normalized import inspection | Combined suite: 1,618 tests and eleven doctests; 36 affected adapter/catalog/archive tests; all-feature Clippy. Real Fabulously Optimized 1.20.1 archives passed in both formats, including every embedded member. Greptile review 55 is green for the accompanying provider fix at `057d838` |
| Compatible selection | 1,607 tests and eleven doctests, all-feature Clippy and all five live provider probes passed. Both compatible probes acquired bytes against the selected provider digest/size. The first full-suite attempt failed compilation when the disk filled; the retry passed after removing reproducible incremental artifacts |
| `c7b1c64` | [Native PR CI 37387301198](https://github.com/inherent-design/empack/actions/runs/37387301198) passed Linux/macOS/Windows tests and import-smoke, lint and coverage |
| `180c4c3` | `mise run test`: 1,498 tests and ten doctests passed |
| `efb7799` | `mise run e2e:strict`: 101 tests passed; [native CI 37388992838](https://github.com/inherent-design/empack/actions/runs/37388992838) passed |
| `1901434` | 17 template tests and all-feature Clippy passed; six generated configurations each round-tripped through Qt QSettings and Java Properties |
| `3733d71` | 1,514 default tests passed; the subsequent doctest compile overlapped a source edit and required a repeat |
| `c5cd762` | 1,517 default tests and ten doctests passed; Greptile review 35 reported no new blocking findings |
| `713fcc0` | 1,524 default tests and ten doctests passed; strict E2E passed 101 tests |
| `7eaba64` | 46 affected core/build/mrpack tests and all-feature Clippy passed |
| `bbe4ee1` | 1,530 default tests and ten doctests passed; native CI 37396023965 passed Linux/macOS/Windows tests and import-smoke, lint and coverage; Greptile review 38 reported no actionable findings |
| `719735a` | 101 strict E2E tests passed; Greptile review 39 reported no actionable findings |
| Vanilla runtime integration | Live official-catalog resolution and exact Minecraft 1.20.1 server acquisition passed; the actual server completed `--help` under process supervision without accepting the EULA |
| Lightweight client integration | 46 affected tests passed across templates, tools and builds. Governed acquisition fetched both pinned assets; the published archive installed with its bundled Java tools; Qt read the exact command |
| Bootstrap projection integration | 29 affected build/mrpack/packwiz tests passed; the actual pinned Java installer consumed a generated selected tree and produced the expected local bytes |
| Publication budget integration | 32 affected tests passed; a passing acquisition test reported a pipe-leak warning and its isolated repeat passed without one. Large-size accounting uses synthetic metadata, not a 65 GiB allocation |
| Build batch integration | 13 composed build tests and all-feature Clippy passed |
| Full client assembly | 138 affected build/template/core tests passed; the final rebuild regression and all-feature Clippy passed. Archives were published and independently read in ZIP, TAR.GZ and 7z |
| Materialization integration | 30 affected core inventory, build, mrpack and reader tests passed |
| Captured template integration | 20 affected template tests passed, including all four standalone target selections, malformed inputs, output budgets, cancellation and source changes |
| Archive and budget integration | 21 affected acquisition/archive/build/reader tests passed; the final five archive tests passed after tightening per-member reads. Three focused source-ownership/scratch tests and all-feature Clippy passed |

The HTTP and build tests use deterministic local fixtures. They do not establish
live provider authorization or catalog behavior. Windows cross-compilation checks
types and configuration; native CI establishes execution on Windows.

An earlier full run at `4be1023` had four tool-probe deadline failures; isolated
repeats and the quiet full run at `0455614` passed without weakening assertions.
A build-acquisition test run reported one nextest pipe-leak warning; its targeted
repeat passed without a leak. These observations do not establish a production fix.

The provider-catalog landing passed 1,589 tests plus eleven doctests before the
final adjustment to reserve parsing memory from actual response lengths. Its final
fourteen focused regressions and all-feature Clippy passed. Three official API
probes verified canonical identity, exact ownership and downloaded bytes for a
Modrinth mod, a Modrinth resource pack and a CurseForge mod. The earlier default
library configuration passed 1,431 tests, followed by all 38 search tests after
enabling their ordinary unit-test configuration.

The provider-to-build integration passes 31 affected catalog, acquisition and Engine
tests plus all-feature Clippy. All three live provider probes passed again; the
resource-pack probe now publishes a full-client ZIP from a lock without stored
locators, verifies the member against its original source hash and confirms that
both project documents remain byte-for-byte unchanged. Deterministic cases cover
changed provider assertions, failed acquisition, missing credentials and restricted
files before publication.

## Supplied build input and in-memory continuation

The public build API accepts `BuildRequest::with_content`. Every supplied locked or
observed key must satisfy a current acquisition obligation, including every original
digest, expected size, accepted observation and source-evidence policy. Preparation
reads the retained bytes rather than trusting their label. Missing inputs retain a
`PreparationContinuation` with prior supplied leases and captured project evidence.
`Engine::resume` revalidates that evidence and combines new inputs before issuing a
new plan. It refuses another engine, changed inputs and duplicate selections. Views
contain no leases or conversion into approval; preview drops its temporary continuation.

The composed test supplies two manual files across separate decisions, builds mrpack
and full-client archives offline, checks every declared placement and verifies that
intent/lock bytes remain unchanged. Durable suspension after process exit and the
execution-time restricted-provider path still need continuation integration.
The full combined snapshot passed 1,853 tests and eleven doctests. Subsequent native
regressions reproduced and fixed continuation through a retargeted project alias and
review 96's duplicate-cache scratch admission. All 33 affected checks then passed,
including the formerly leaky reference-input test; Clippy and Windows cross-compilation
pass. Resume checks the selected native root before and after fresh capture.

## Scoped artifact cleanup

`CleanRequest::Artifacts` uses captured native inputs, exact replacement approval and
the common publisher. Its read set covers only `dist`, so damaged authoring or unrelated
pack/template content cannot block artifact cleanup. The preview names every regular
file to remove; changed bytes or new entries invalidate execution. Symlinked roots
and members are rejected. Recovery can finish or restore an interrupted cleanup.
Directories and unrelated namespaces remain, and recovery preimages retain their
separate storage policy. Capture uses the archive-container size limit rather than the
smaller source-member limit. A scaled regression reproduces Greptile 98's oversized
artifact failure and verifies deletion after the correction. CLI routing remains
integration work.

Review 97's resume admission failure reproduced with a governor sized for one capture.
Resume now keeps the prior preparation reservation charged and admits only additional
capacity; the resulting preparation retains that same reservation. All 25 affected
cleanup, continuation, runtime and resource tests pass, including tight-budget resume
and interruption after a cleanup target changes. All-feature Clippy and Windows
cross-compilation pass.

## Persistent content storage

`FileContentStore` publishes verified content-addressed objects into private host
storage. Its separate lookup capability is read-only. Both operations use admitted
workers; lookup charges the observed byte size and retains that reservation on the
returned lease. Every cache hit checks the request's unchanged source declarations
and the stored address. MD5 evidence stays weaker, and strong-source policy still
refuses it. Copies survive cache eviction without persistent pin writes. Existing-object verification
runs before requesting space for another copy; a cache hit works even when the source
lease occupies the remaining scratch budget. A cache miss still requires full copy
admission and exclusive revalidation before insertion.

Native tests cover reopen/read-only behavior, corruption, conflicting assertions,
limits, cancellation, coordination, links/special files, privacy and retained reader
lifetimes. A composed file-addition test reopens cached content, publishes its declared
placement and requirements, then synchronizes twice. Durable continuation and normal
Engine/CLI cache selection remain separate integration work.

## Host cache maintenance

`CacheCleanRequest::All` inspects only canonical content-object names in the selected
private store. Preparation binds its native root and every selected regular file;
execution revalidates all selections under the exclusive store lock before deletion.
It never recursively removes storage, unknown neighbors or recovery data. A new
object inserted after preparation is retained. Corrupt regular cache content can be
removed without reading it as authenticated data. Symlinks and changed objects fail.

`Engine::prepare_cache_cleanup` and `preview_cache_cleanup` require explicit store
wiring, not a project. Exact approval, engine ownership, resource admission,
cancellation and owned operation handles reuse the shared lifecycle. The opaque plan
and receipt retain their metadata reservation. Inspection first counts objects under
a retained shared lock, then admits memory for that observed selection. Unknown
neighbors have a separate bounded traversal allowance and do not consume content
object capacity. Uncoordinated membership changes invalidate the second pass. Review 99's full-store
neighbor and empty-store admission failures reproduced before this correction.
Both are now covered, along with uncoordinated growth/shrink and insertion capacity. If deletion fails or is cancelled
partway through, the receipt records removed and retained objects with the failure;
only complete eviction returns `Completed`. Independent verified leases remain
readable after their cache object is removed.

All 18 focused native-store, artifact-cleanup and public-maintenance tests pass,
including review 98's reproduced archive-size mismatch. The combined snapshot passes all 1,868 tests and eleven doctests. All-feature
Clippy and Windows cross-compilation also pass. Combined CLI cleanup and persistent acquisition
integration remain unfinished; this is not a transaction across project and cache.

## Recovery CLI

`recover` routes the real CLI dispatcher through the engine. The default `inspect`
action is read-only; `finish` and `restore` prepare the exact journal-bound footprint
and require confirmation or `--yes`. `--dry-run` returns before authorization.
`--operation` rejects a different or no-longer-pending journal. Recovery works without
valid project documents, supports absent creation destinations, and never acquires
the old project-mutation lock. The host drains engine workers before returning.

Durable state uses platform application data, with explicit `--state-dir` /
`EMPACK_STATE_DIR` selection. Relative paths use the invocation root. Construction and
inspection do not create it. The host prints each affected path, action and byte-size change before either preview
returns or confirmation is requested. Review 100 reproduced the earlier count-only
output; captured subprocess output now verifies create, replace and remove entries.
Native dispatcher tests inject interrupted publication,
compare project and journal bytes through inspect/preview/decline, and verify both
finish and restore. Configuration parsing and merging preserve state-directory
selection. All 27 focused CLI/configuration/host tests and 24 offline executable smoke
tests pass, along with all-feature Clippy and Windows cross-compilation. Existing
command migration and global-display removal remain separate work.

## Remaining integration and limits

- Complete command-host composition for the implemented project operations. Build,
  import, initialization, add/update/adopt, sync, removal, cleanup and recovery have
  compiled Engine routes. All five build recipes and all loader runtime families
  enter the approved build driver; shared components alone do not establish CLI parity.
- Replace command orchestration with the shared lifecycle. Wire manual acquisition,
  provider-locator refresh, continuation and scoped
  clean through the same verified obligations.
- Connect persistent content lookup/store to normal Engine/CLI acquisition and durable
  continuation. The separate read-only and insertion ports are implemented.
  Coordinate provider authentication, retry and rate policy; the content HTTP port
  does not replace existing catalog clients yet.
- Native build capture filters ignored pack content before opening its bytes. Directory
  enumeration remains bounded; captured rules and explicit input exceptions survive
  revalidation and journal recovery.
- Windows publication reports file synchronization, not Unix directory synchronization.
  Filesystems without durable root creation identity are refused. Portable permission
  checks cover read-only/executable intent, not arbitrary ACL equivalence.
- Restoration can leave newly created empty managed directories. Retained committed
  data has explicit reclamation, but a complete retention catalog remains pending.
- The 7z decoder exposes no configurable allocation ceiling. Current packaging
  verification reads privately generated candidates, not untrusted downloaded 7z.
- Existing disposable HTTP-cache snapshots have last-writer-wins cross-process
  behavior. They are not the consistency model for intent or recovery state.
- The synchronous process bridge and global display/error state still constrain
  independent embedded sessions. Their removal belongs to command cutover.

The upstream installer v0.5.14 enables all optional entries in headless mode,
regardless of their declared default. This was observed in a disposable installer
run; projection now requires resolved choices for headless execution. Interactive
reference packages retain supported optional metadata. Lightweight client defaults
bundle both exact tools and disable bootstrap updates. Captured user configuration
remains explicit input; its arbitrary commands are not certified by these checks. Grouped optional files and
optional embedded bytes need explicit selections because the format lacks those
relationships. These checks precede any publication.

No alpha release is ready while these feature and lifecycle gates remain incomplete.

### Removal review validation

The unrelated-content regression failed before correction because a nonportable
filename entered the removal snapshot. Seventy focused tests pass after correction,
including oversized unrelated and unselected locked payloads, symlinks, colliding
names, source edits and durable publication recovery. All-feature Clippy and Windows
cross-compilation pass. The full suite passes 1,746 tests and eleven doctests. Native CI at `c5b6e06`
also passed on Linux, macOS and Windows; these new changes still require their own
pushed-revision CI and Greptile review.

The final portable-alias regression also reproduced: a differently cased selected
filename was treated as absence. Mutation capture now includes portable aliases and
checks their spelling against selected destinations before planning. After this
correction, 42 affected tests pass. The 1,746-test full run above predates this final
regression; native CI must exercise the combined pushed revision.

### Explicit removal uncertainty

`RemovalEvidencePolicy` distinguishes the default `RequireComplete` policy from
`AcknowledgeUnknown`. Known retained required edges always block deletion. An
acknowledged plan, Engine preview and terminal receipt retain the incomplete
dependency keys, and the resulting lock keeps their original coverage. No source,
path or publication check is relaxed. Root demotion retains content and requires
no uncertainty acknowledgement. Untracked metadata has no complete locked graph;
its presence also requires explicit acknowledgement for content removal and remains
listed in preview/receipt evidence. Selected ownership checks still run strictly.

Validation: 32 focused removal tests pass, including default refusal, explicit
acknowledgement, preserved uncertainty in the lock/receipt and known-edge rejection.

Observed-removal validation: 1,753 default tests and eleven doctests pass on the
combined tree, along with 53 affected tests, all-feature Clippy and Windows
cross-compilation. The full run includes backend-index publication and preserves
raw logical documents. Native CI and Greptile at the next pushed revision remain
separate release evidence.

Filtered directory membership proves absence only for destinations included in its
captured scopes and traversal policy. An excluded file is neither absent nor an
implicit overwrite target. A regression reproduced that incorrect inference; 78
affected capture, verification and public Engine tests pass after correction.

## Review 99–100 validation

All 21 affected cache and recovery-host tests pass, including the three reproduced
review findings. The earlier cache-only run reported one Nextest pipe-leak warning
in an existing reader test; that test passed cleanly in isolation and both subsequent
combined runs are clean. All-feature Clippy and Windows cross-compilation pass.
The latest full-suite evidence remains the 1,868 tests and eleven doctests at
`9ccb32b`; final combined validation remains a release gate.

## Initialization host validation and review

At `fab68fb`, all 1,888 tests, eleven doctests and 24 executable smoke tests pass.
Review 103 then exposed loader selection before game selection: the menu could offer
NeoForge for Minecraft 1.7.10. A deterministic catalog fixture reproduced the failed
initialization. The host now selects the game first, retains compatible family catalogs
under one discovery governor and offers only observed choices plus Vanilla. Failed
family lookups are disclosed without claiming compatibility. Selected versions reuse
the same retained observation. All 15 affected tests pass; the final historical fixture
also checks that an unavailable Quilt service does not prevent an evidenced Forge
selection. All-feature Clippy and Windows cross-compilation pass separately from the
recorded full-suite snapshot.

## Combined build cleanup

The compiled `BuildRequest.clean` closes the preparation gap for the preserved
clean/build option. It captures all existing artifact files and directory membership,
previews only obsolete removals and requires their exact acknowledgment. Requested
outputs remain build replacements. All candidates must verify before cleanup and
replacement enter one journaled publication. Failed recipes, cancellation, late
artifacts and edits retain the previous distributions. Cleanup cannot follow links or
consume a declared source. Old output capture uses the configured archive allowance.
Recovery before-images receive additional admitted scratch capacity.

Native tests inspect replacement archives and removal receipts, exercise unchanged
failure outcomes, and interrupt the combined publication before finishing or restoring
it. The CLI build host still needs to route its existing clean option through this
request; this capability does not make the old command dispatcher transactional.

All 126 affected build, project, API and host tests pass. A separate combined-publication
interruption test passes for both finish and restore. All-feature Clippy and Windows
cross-compilation pass on the combined worktree.

## Loader-menu limits and explicit pins

Review 104's two findings reproduced: each family restarted the timeout, and a
loader pin still allowed an invalid Vanilla selection. Family discovery now carries
one deadline across requests. A stalled provider cannot restart the allowance for
later families; earlier verified choices remain available. With a requested pin,
the menu contains only families whose retained catalog includes that version.
Missing pins fail without publication. All 17 affected host/catalog tests pass,
including a stalled-response fixture that verifies no later request starts after
the shared deadline expires.
All-feature Clippy and Windows cross-compilation also pass for these menu changes.

## Loader discovery fairness

Greptile 105 identified that a stalled early provider could consume the shared discovery
deadline before a later family was checked. Each lookup now receives a fair share of the
remaining allowance, reserving time for every uninspected family. The overall deadline
and retained earlier choices remain binding. An explicit pin filters the evidenced menu.

The regression fails before this correction when NeoForge stalls and Forge has the
requested historical pin. Independent loopback origins model independent services; a
single blocking mock server would incorrectly stall every provider. Both deadline tests
pass after the correction, including all requested origins receiving a lookup. All-feature
Clippy and Windows cross-compilation also pass on the combined host-work tree.

## Native build host

`application::engine_host::build` now composes native document reads, recipe selection,
provider capability wiring, engine preparation, effect display, approval and owned
execution. It preserves requested target order, validates every explicit target, uses
project defaults when none are selected and distinguishes an absent archive override
from an explicit ZIP selection. Build decisions carry optional choices, accepted format
conversions, template options, source-evidence policy and installer interaction explicitly.

The host displays missing content without publishing. Exact supplied manual bytes can
complete the same requested distributions. Preview and declined approval retain project
and host files. Clean builds publish verified replacements and obsolete removals together;
a failing recipe leaves every previous artifact intact. Native fixtures inspect mrpack
and full-client payloads after changing source bytes without a version bump.

All 138 affected host, CLI, build and engine API tests pass, together with all-feature
Clippy and Windows cross-compilation. The initial manual-content fixture incorrectly
paired URL intent with manual provider acquisition; the corrected fixture carries its
canonical provider identity and exact selection. No production validation was weakened.
At `486808c`, the combined revision passed all 1,903 default tests and eleven doctests,
with 123 opt-in tests skipped. All 24 executable smoke tests also passed.

This is a compiled host, not yet the CLI build dispatcher. Durable continuation,
download scanning/association and the remaining command hosts are still required for
coherent cutover. Unsupported continuation arguments fail explicitly at this boundary;
the existing dispatcher retains those features while their replacement is completed.


## Build-selection binding and concurrent catalogs

Greptile 106 identified a gap between the host's preliminary document read and engine
capture. Build requests derived from project defaults now carry the expected semantic
intent revision. Preparation rejects changed intent before producing an approvable plan.
Comment-only changes before capture remain valid and their exact bytes are retained.

The review also demonstrated that dividing a network deadline into serial shares rejects
a compatible catalog that responds within the full deadline. The family menu now starts
four owned catalog operations concurrently, sharing one resource governor and absolute
network deadline. Each provider receives the remaining full allowance. One stalled service
neither blocks another request's start nor shortens its deadline. Shutdown drains every
owned operation before results reach the menu. This replaces the earlier serial fair-share
implementation recorded above.

Both reported failures reproduced before correction. The corrected regressions cover a
changed build intent, a valid catalog responding after its former time slice, a stalled
early provider and retained earlier results. All 94 affected host, engine API and catalog
tests pass, including comment-only intent edits, with all-feature Clippy and Windows
cross-compilation. The full-suite result above remains pinned to `486808c`.

## Native dependency hosts

Selected provider addition now connects catalog resolution to native engine publication.
Removal and explicit synchronization use the same approval and execution lifecycle. All
three display their exact changes before mutation. Resolution and publication share resource
admission; cancellation drains their owned workers. The provider host records references,
leaving payload verification to an operation that actually needs the bytes.

Addition binds its resolved choices to the native root and original intent/lock documents.
A concurrent document edit rejects the stale request and preserves the independent edit.
Identical documents in a different project cannot satisfy that binding. An unresolved
required edge or any failed requested selection prevents the whole addition from publishing.

The native composed regression initializes a project, adds a pinned root with a required
dependency, re-adds by equivalent selectors under the existing alias, synchronizes twice,
builds a full client with verified supplied content, removes the root by title and
synchronizes again. It inspects actual packaged bytes and retained dependency intent.
Preview and declined execution leave project and host files unchanged.

These hosts are compiled library entry points. They do not complete CLI search/selection,
local/URL input composition, automatic sync resolution or dispatcher replacement.

All 65 affected host, addition, removal and provider tests pass, with all-feature Clippy
and Windows cross-compilation. Two tests reported inherited output pipes in the parallel
run; both passed without that warning in an isolated serial rerun. The full-suite result
remains pinned to `486808c`; these checks do not establish final CLI parity.

## Provider reference origins

Provider addition and import now retain download alternatives that satisfy the durable
document policy. Previously they discarded every locator, including stable public URLs,
forcing a provider requery and payload acquisition for ordinary reference exports.
Signed URLs and credential-bearing alternatives remain execution-only; their exact
provider identities remain available for later acquisition.

The missing-origin regression failed before correction. The composed host workflow now
also exports an mrpack using only the original reference evidence and inspects its paths,
download URLs, sizes and hashes. Neither provider payload is embedded. Negative cases
exclude signed query parameters, user information, HTTP and fragments from durable origins.

All 104 affected provider, import and dependency-host tests pass, with all-feature Clippy
and Windows cross-compilation. The native mrpack test makes no payload requests.

## Native cleanup host

Artifact publication and content-cache eviction now compose through one displayed cleanup
request and one confirmation. Both scopes are prepared before either starts. Invalid
targets, unsafe artifact links, malformed cache objects and overlapping storage fail before
mutation. Artifact cleanup reads no project documents; cache-only cleanup needs no project.
Absent stores remain absent during preview and execution.

The combined operation preserves the distinction between recoverable artifact publication
and disposable cache eviction. A later failure reports which earlier scope completed. The
native regression changes a selected cache object after preparation, verifies that artifact
cleanup completed, and checks that the independent cache edit remains intact. Other cases
cover preview/decline, one confirmation, unknown neighbors and outside sentinels.

This compiled host covers the new content store. CLI routing and the remaining disposable
storage categories stay on the integration ledger; this change does not claim their cutover.

All 54 affected host, cleanup and content-store tests pass, together with all-feature
Clippy and Windows cross-compilation. Two parallel tests reported inherited output pipes;
the serial rerun of all 29 cleanup/store tests passed without those warnings.

At `ae63534`, the combined revision passed all 1,918 default tests and eleven doctests,
including executable smoke cases, with no pipe warnings. The 123 opt-in tests were skipped.

## Exact refresh with saved provider origins

Greptile 109 identified that saved origins caused materialized builds to discard their
provider lookup obligation. The native regression reproduced a failed build when the old
URL was unavailable and the same exact provider selection had a working replacement.

Materialization now retains the pin, file role and saved origins together. Available
catalog access refreshes the exact selection before transfer; changed original assertions
still fail. Hosts without catalog access can use existing download evidence, and a missing
fresh locator does not remove a previously declared alternative. Complete reference exports
remain offline. No refreshed locator or changed expectation is written into project intent.

All 41 affected API/acquisition/host tests pass, with all-feature Clippy and Windows
cross-compilation. The six focused refresh/fallback tests pass serially. One parallel
host test reported an inherited output pipe; its isolated rerun passed without that warning.

## Saved origin fallback after refresh

Greptile 110's failed-fresh/working-saved URL scenario reproduced before correction.
Exact lookup now keeps distinct saved origins after fresh ones. Downloads still verify
the original hash and size; successful lookup alone cannot revoke a working origin.
The regression inspects the acquired bytes and both HTTP requests.

The combined fallback and import-admission changes pass all 57 affected tests,
all-feature Clippy and Windows cross-compilation. This is targeted evidence; the
last complete suite remains the 1,918 tests and eleven doctests at `ae63534`.

## Import inspection retirement

A small archive reproduced an admission failure under the native host's 512 MiB
budget with default import limits. Inspection retained its maximum parsing allowance
while extraction requested its own bounded archive index.

Inspection still admits the configured worst case before parsing. After the worker
retires, its result retains an estimate based on actual manifest bytes and inspected
names. The transfer cannot grow any resource dimension. The native-budget regression
verifies extracted bytes and complete reservation retirement; a separate test rejects
an attempted reservation increase. All 57 affected import/runtime/acquisition tests,
all-feature Clippy and Windows cross-compilation pass on the combined change.

## Native selected-archive import host

The compiled import host connects selected native files and bounded remote downloads
to inspection, exact provider resolution, verified content, explicit conversion choices
and approved project publication. All phases share the normal host resource budget.
Missing restricted content stops before conversion or publication; supplied associations
are reverified against the original assertions. Provider CDN credentials follow the same
fixed-origin policy as engine builds.

Six native host regressions pass. They cover preview/decline, refused and approved forced
replacement, retained unrelated notes, source/destination symlinks, HTTP byte limits,
wrong archive digests and a stalled chunked response rejected before EOF. Restricted
CurseForge content requires explicit supplied bytes and retains MD5 evidence and optional
participation. The composed import → full-client → mrpack case inspects original common
bytes, effective client/server layers and optional URL references without rewriting intent.
Fully shadowed common bytes remain in the project; exports contain the effective side views.

This host accepts explicit archive and representation choices. Runtime conversion,
durable pending-input storage and CLI selection/cutover remain open.
The existing CLI is unchanged; this does not claim complete product parity.

At `43a8a3c`, the frozen combined revision passed all 1,929 default tests and eleven
doctests without pipe warnings; 123 opt-in tests were skipped. The six native host cases,
all-feature Clippy and Windows cross-compilation also pass.

## Local import source assertions

Greptile 112 identified that local archive selections could not carry independently
known digests. Strong-source acquisition therefore rejected them before inspection.
The failure reproduced with a caller-known archive digest. Local and remote source
variants now both carry `ExpectedContent`; acquisition enforces these assertions
without changing the selected evidence policy. The regression checks missing, weak,
wrong-digest and wrong-size evidence against an unchanged native project tree before
accepting the correct SHA-512 declaration.

All 39 affected import/local-acquisition tests, all-feature Clippy and Windows
cross-compilation pass. The complete-suite result remains pinned to `43a8a3c`.

## Provider modpack archive selection

Provider modpack pages now resolve through a bounded archive catalog before native import.
Selectors are distinct from dependency content kinds. Modrinth project/version selectors
and CurseForge project/file selectors resolve to canonical ownership, original digest and
size assertions, and transient download alternatives. Latest selection applies the selected
release policy; an explicit version remains exact. CurseForge server packs are excluded.
Missing restricted archive URLs require explicit acquisition instead of a guessed endpoint.

One request budget covers project lookup and all pages. Malformed records, duplicate or
changing pages, exhausted limits and mismatched owners fail without accepting an earlier
partial result. Native imports verify the selected archive before replacement. The tests
exercise both providers, same-size changed archive bytes, numeric URL slugs, file selection,
restricted URLs and resource retirement.

All 114 affected provider/import tests pass. After final selector and record-validation
refinements, all 15 catalog/host tests pass. The live Fabulously Optimized Modrinth catalog
probe passes using default limits; it resolves metadata and does not download the archive.
Final all-feature Clippy and Windows cross-compilation pass. The latest complete suite
remains the 1,929 tests and eleven doctests at `43a8a3c`.

## Selected archive availability and manual input

Greptile 114's unselected-file and unsupported-format cases both reproduced. Modrinth
archive selection now validates acquisition evidence for the selected primary role;
an unrelated attachment cannot block it. Automatic selection skips unsupported archive
formats while exact unsupported selections fail. Ownership, primary-role ambiguity and
selected-file digest checks remain mandatory.

Provider imports also accept an explicit native `supplied_archive`. Its bytes are checked
against the exact catalog digest and size before parsing. Native regressions cover a
restricted CurseForge archive, missing/wrong supplied bytes, read-only preview and successful
replacement. This association is separate from manual files inside the archive.

The combined review corrections and direct-file host pass all 145 affected tests and
all-feature Clippy. Two parallel tests reported inherited output pipes; isolated evidence
is recorded with the direct-file host below. Final combined-suite validation remains open.

## Native direct-file addition host

Explicit local and URL files now acquire, normalize and publish through the shared
addition lifecycle. Original assertions, source permissions, placements and environment
requirements survive. Transient download locators stay out of intent. ZIP/JAR structure
and layout checks precede typed addition; unknown layouts require explicit acceptance.
Every requested file must verify before publication, including mixed local/URL batches.

Six native regressions cover preview/decline, repeated addition, full-client output bytes,
wrong hashes, aggregate limits, unsafe archive members, unknown JAR acceptance, destination
collisions, selected source symlinks and concurrent document edits. Two early fixture
assumptions were corrected: full clients use `.minecraft/`, and explicit local selection
rejects a linked leaf while canonicalizing the selected parent. No production validation
was weakened. All 145 affected tests, all-feature Clippy and Windows cross-compilation
pass. Both parallel inherited-pipe warnings pass cleanly in isolated serial reruns.

The CLI remains on its existing path until coordinated parity verifies. Provider
identification/representation selection, world archive interpretation, mixed provider/direct
batches and automatic synchronization still need host composition. This change does not
claim those features are complete or remove their existing CLI implementations.

## Direct archive member verification

At `7857098`, the frozen combined revision passed all 1,947 default tests and eleven
doctests without pipe warnings; 124 opt-in tests were skipped. PR CI also passed lint,
coverage and native Linux tests at that revision; the other native jobs were still running
when recorded. These results predate the following correction.

Greptile 115's corrupt-member fixture reproduced despite a matching declared archive hash.
Typed direct additions now decode every ZIP member and enforce CRC, actual size and the
cumulative expanded-byte allowance before normalization. Explicit unidentified-kind acceptance
does not bypass these checks. Verification writes no extracted files, checks cancellation
between bounded reads and charges failed reads so retries cannot reset their allowance.

All 40 affected archive/import/direct-file tests pass. The final twelve focused tests pass
serially without the earlier parallel pipe warning. Final all-feature Clippy and Windows
cross-compilation also pass.
The last complete suite remains pinned to `7857098`; this correction has targeted evidence.

## Mixed native addition batches

The native addition host now accepts provider and direct-file requests together. It
captures the project once, resolves required provider closure, verifies direct files,
combines compatible logical records and prepares one approved publication. The existing
provider-only and direct-only entry points use this path. Source reservations survive
composition; provider references remain distinct from acquired direct bytes.

The composed native regression verifies two explicit roots plus a required dependency,
preview/decline, exact repeated addition, two no-op synchronizations and mrpack export
containing both provider references and local bytes. A failure matrix covers unavailable
providers/files and collisions with explicit roots, required dependencies or placements.
Every failure preserves the entire project tree. All 59 affected tests, all-feature
Clippy and Windows cross-compilation pass. CLI input classification and identification decisions remain open; this
completes native batch composition, not the CLI cutover.


## Shared file-add entry point

At `e317773`, the frozen combined revision passed all 1,951 default tests and eleven
doctests without pipe warnings; 124 opt-in tests were skipped. Greptile 117 reported
no blocking findings and noted duplicate catalog initialization in the file-only host.
That entry point now delegates directly to the mixed addition host. Its native regression
verifies an exact no-op through the public entry point. The focused regression, all-feature
Clippy and Windows cross-compilation pass after the correction.


## Recorded synchronization inputs

`SyncRequest::Recorded` now captures and verifies local sources and archive members,
retaining provider/URL/manual slots as exact references. It shares the existing sync planner,
approval and publisher with `SyncRequest::Supplied`. Changed semantic intent still requires
fresh resolution; ordinary synchronization does not upgrade selections or download payloads.

Native tests restore missing and modified placements, repeat synchronization without rewriting
documents, reuse verified installed members when their archive is absent, reject unsafe paths
and corrupt/missing/weak sources, and invalidate publication after a captured source changes.
The mixed-add → sync twice → export fixture now uses recorded synchronization without manually
constructing its file decisions. All 52 affected tests, all-feature Clippy and Windows cross-compilation pass.
The preceding `75eb4be` revision passed Greptile 118 with no outstanding findings.
The last completed full suite remains pinned to `e317773` (1,951 tests and eleven doctests).


## Combined synchronization and coverage evidence

The frozen `05ac71f` revision passed all 1,955 default tests and eleven doctests.
Greptile 119 identified fallback-placement selection and scratch over-reservation cases;
those require targeted corrections despite the passing suite.

At `75eb4be`, CI coverage executed all 2,047 selected tests successfully, then failed
merging one corrupt raw profile. A separate instrumented Rust fixture reproduced strict
merge failure, verified that valid profiles survive mixed input, and verified failure when
all input is invalid. Coverage now uses that supported merge policy with warnings retained.
The test command still fails on any failing test. The CI table also reads actual line
counts and prints the report used for the job summary.
