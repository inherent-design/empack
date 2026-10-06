# Implementation status

Release target: v0.5.0-alpha.1. The [target contracts](README.md) and
[feature requirements](parity.md) define completion. This page records what the
source implements; it does not make the target API sketches available by implication.

The normalized engine can prepare and recoverably publish all five distribution
shapes from a captured workspace. Server recipes have verified vanilla,
Fabric, Quilt, Forge and NeoForge runtime preparation. HTTP acquisition and
explicit build-content obligations feed these paths. The compiled `Engine` now owns
build preview, preparation, exact-plan authorization, acquisition, runtime assembly
and publication. The read-only provider catalog resolves canonical selectors and
exact and compatible Modrinth/CurseForge selections, offers bounded search choices,
and identifies acquired files by their content. Normalized mrpack and CurseForge inspection also runs over retained bounded archive sources. Import/operation
composition, the remaining operation APIs,
continuation/cleanup and CLI cutover remain unfinished. Existing
commands retain their fixes and capabilities until their replacements pass parity
checks. There will be one implementation per operation, not a permanent legacy engine.

## Implemented boundaries

| Boundary | Current behavior | Contract evidence |
| --- | --- | --- |
| Build lifecycle | Read-only preview/preparation; engine-bound consumed grants; shared target acquisition; owned runtime/tool work; all-requested publication and retained receipts; abandoned preparation retirement | [Engine tests](../../crates/empack-lib/src/engine/api/tests.rs), [runtime tests](../../crates/empack-lib/src/engine/runtime/tests.rs) |
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
Internal catalog composition can carry one request budget through selector, pin and
compatible lookups; it does not restart the deadline between dependencies.
Transient signed URLs are execution data; the document codec still rejects them in
persistent alternatives. Import composition and command cutover remain completion gates. The build Engine can
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
content. Import and mutation preparation still need to assign roles/placements, resolve
requirements and compose this evidence with verified publication.

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
semantic interpretation, conversion decisions and project publication still need
composition before the CLI import path can be replaced. The existing importer remains
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

These values grant no project write authority. Import still needs semantic candidate
assembly, explicit optional/default and layout choices, replacement planning and
composition with the Engine's approval/publication lifecycle.

## Composed build behavior

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
loader runtime contracts must join this boundary before CLI `all` can cut over.

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

## Remaining integration and limits

- Extend the compiled build `Engine` lifecycle to semantic project mutations,
  provider catalogs and import normalization. All five build recipes and all loader
  runtime families now enter the same approved build driver.
- Replace command orchestration with the shared lifecycle. Wire manual acquisition,
  provider-locator refresh, continuation and scoped
  clean through the same verified obligations.
- Add persistent content lookup/store capabilities with read-only preview authority.
  Coordinate provider authentication, retry and rate policy; the new content HTTP
  port does not replace the existing catalog clients yet.
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
