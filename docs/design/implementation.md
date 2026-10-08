# v0.5 delivery ledger

Source baseline: native CLI cutover after `48bfc5d`, 2026-10-08. Target: **v0.5.0-alpha.1**.
This is the current delivery checklist, not a release claim or a completion percentage.
The [design](README.md) defines the target; the [feature requirements](parity.md)
define the capabilities to preserve. Historical implementation notes and test runs
remain in [the preceding ledger](https://github.com/inherent-design/empack/blob/e0d5083/docs/design/implementation.md).

**Ordinary project commands now dispatch to the v0.5 engine.** The old command
handlers and their outer mutation lock have been deleted. This establishes the
new route, not full feature completion: provider-owned worlds, independent batches and remaining host-service retirement
still require work. The [dispatcher](../../crates/empack-lib/src/application/commands.rs)
and executable tests are the source of truth.

## Delivery states

- **CLI wired:** the ordinary executable reaches the engine and has command-level evidence.
- **Host implemented:** the application adapter composes the engine; dispatcher integration remains.
- **Engine implemented:** compiled behavior has contract tests; frontend decisions may remain.
- **Open:** required behavior or acceptance evidence is missing.

A command is complete only after its preserved options, failures and recovery paths
pass executable tests and its replaced mutation path is deleted. Component tests,
a passing review and line coverage do not change that state by themselves.

## Command routing and cutover

| Workflow | Executable route | Remaining acceptance work |
| --- | --- | --- |
| Initialize / forced replacement | **CLI wired:** native initialization and approved replacement | Live runtime matrix; headless loader/latest choices are now explicit |
| Import local/remote packs | **CLI wired:** source classification, verified import and explicit conversion choices | Native `init --continue` and `clean import` are wired; broaden live provider/archive matrix |
| Add | **CLI wired:** canonical provider selections, deliberate search and direct file publication | Provider-owned world members and broader live provider parity; direct world groups, explicit file plans and provider identification are wired |
| Update | **CLI wired:** exact logical selection, canonical provider refresh and declared direct sources | Broaden live update/companion-role tests; preserve changed-file refusal |
| Adopt observed content | **CLI wired for tracked local/member files, URL files and providers across side layers, including first-lock adoption and explicit new source groups:** verified document-only acceptance of observed bytes and exact pins | Broaden live provider/member parity |
| Remove | **CLI wired:** shared exact ownership planner | Broaden executable alias/title/stem tests; explicit unknown-evidence policy and demotion are exposed |
| Sync | **CLI wired:** recorded selections, fresh resolution for missing/unsatisfied roots and explicit remote materialization | Broaden runtime, search and multi-file placement matrices; manual acquisition continuation |
| Build / continue | **CLI wired:** native build, saved recipe continuation and bounded download waiting | Broaden live target/runtime and platform desktop matrix; execution-time missing inputs retain resumable state |
| Clean | **CLI wired:** scoped artifact/cache cleanup | Remaining disposable stores; explicit stale/invalid build and import cleanup is wired |
| Recover | **CLI wired:** engine recovery | Retain interruption and restart tests through library retirement |
| Requirements / version | Host inspection; no managed-tool bootstrap | Capability-specific live prerequisites |

The legacy command handler file and the old `empack/` library tree are removed.
The previous synchronization planner, project-state/config/build/import machinery,
HTTP cache/manager and mock filesystem/network/backend implementations are deleted.
`Session` now exposes invocation, configuration, display, interaction and process services.
Project effects use native engine capabilities in both production and tests.
The managed Go-tool bootstrap and synchronous process bridge are also removed.
Git author discovery awaits the host process runtime; test prerequisites no longer
resolve or install a packwiz executable.

The legacy command handler file is replaced. Its tests asserting subprocess arguments,
legacy YAML/state flags, simulated backend responses and cache-seeded searches are
retired with it. Their safety and semantic obligations continue in native engine,
provider, import, process and publication suites. New dispatcher tests exercise the
initialize → add → sync twice → rebuild current bytes → remove → sync → clean
sequence. Offline executable smoke now uses real native projects and intentionally
unavailable tooling/network, including whole-tree previews, failed batches, deletion
confinement, invalid forced imports, configuration precedence and recovery inspection.
The eleven mock-command integration files are consolidated into `native_commands.rs`,
which exercises loader families, content kinds, format writers, previews, demotion,
unknown-evidence acknowledgment and failure preservation. Provider alias/pin/closure
checks remain in the native adapter fixtures; live executable
parity is still a final gate, not inferred from those fixtures.

## Implemented engine contracts

| Boundary | Current behavior | Evidence location |
| --- | --- | --- |
| Semantic model | Provider-qualified identity, exact pins, independent labels, file slots, placements, side/optional requirements and dependency evidence | [Core tests](../../crates/empack-core/tests/) |
| Documents | Intent schema 2, lock schema 1, strict decoding, raw/semantic revisions, credential-free durable locators and byte-preserving no-op writes | [Document tests](../../crates/empack-lib/src/engine/documents/tests.rs) |
| Capture and ownership | Native root/object binding, bounded relevant inputs, ignore rules, membership/absence and expected-old checks | [Project tests](../../crates/empack-lib/src/engine/project/tests.rs), [snapshot tests](../../crates/empack-lib/src/engine/snapshot/tests.rs) |
| Acquisition | Bounded streaming, original digest/size verification, redirect-scoped credentials, cumulative limits and owned content leases | [Acquisition tests](../../crates/empack-lib/src/engine/acquisition/tests.rs) |
| Dependency changes | Canonical addition and required closure; selected updates; observed adoption; conservative removal; recorded/supplied sync | [Dependency hosts and tests](../../crates/empack-lib/src/application/engine_host/dependencies.rs), [core tests](../../crates/empack-core/tests/) |
| Distributions | Mrpack, client, server, full-client, full-server; ZIP/TAR.GZ/7z; current captured content, layers, optional decisions and templates | [Build modules and tests](../../crates/empack-lib/src/engine/build/), [artifact tests](../../crates/empack-lib/src/engine/artifacts/tests.rs) |
| Runtime preparation | Vanilla, Fabric, Quilt, historical/current Forge and both NeoForge families; exact assets and launcher checks | [Runtime modules and tests](../../crates/empack-lib/src/engine/server_runtime/), [live runtime suite](../../crates/empack-lib/tests/runtime_server_smoke.rs) |
| Publication | Private staging, frozen output, independent inventory checks, project ownership, journaled publication and restart recovery | [Publication tests](../../crates/empack-lib/src/engine/publication/tests.rs), [verification tests](../../crates/empack-lib/src/engine/verification/tests.rs) |
| Runtime ownership | Host Tokio runtime, admitted async/blocking work, stale-result rejection, cancellation separate from retirement, retained outcomes | [Runtime tests](../../crates/empack-lib/src/engine/runtime/tests.rs), [resource tests](../../crates/empack-lib/src/engine/resources/tests.rs) |
| Process ownership | Deadline includes pipe lifetime; Unix groups/Windows jobs; bounded nonblocking progress in the async API | [Process runtime and tests](../../crates/empack-lib/src/application/process_runtime.rs) |
| Manual build inputs | Exact slot/local-file association, bounded content-based download discovery, durable saved recipe, fresh resume approval and conditional record deletion | [Suspension tests](../../crates/empack-lib/src/engine/api/suspension/tests.rs), [build host tests](../../crates/empack-lib/src/application/engine_host/build/tests.rs) |
| Content storage | Verified addressed insertion/read-only lookup and scoped cleanup; pending builds use persistent content storage | [Store tests](../../crates/empack-lib/src/engine/content/store/tests.rs), [cleanup](../../crates/empack-lib/src/engine/content/store/cleanup.rs) |

### Provider search

`ProviderCatalog::search_projects` offers bounded windows by provider preference and
accepted game version. Ranking does not authorize a choice. Pagination, unsupported
kinds, provider failures and original rank remain explicit. The new CLI add adapter
composes pagination and deliberate selection with canonical lookup and closure resolution.
Headless search requires an explicit project URL or provider selector; `--yes` never
selects the first search hit. The dispatcher now uses this adapter.
[Search tests](../../crates/empack-lib/src/engine/providers/search/tests.rs).

The [CLI adapter tests](../../crates/empack-lib/src/application/engine_host/cli/tests.rs)
exercise equivalent slug/ID/URL additions followed by two unchanged syncs, preview,
conflicting pins/providers, deliberate search selection and all-requested direct-file
failure against native temporary projects. Provider network fixtures exercise the adapter; offline lifecycle tests also
exercise the executable. Explicit supplied-file provider identification now retains verified bytes through publication.
Direct world interpretation and explicit companion-file plans are wired; provider-owned
world interpretation and broader live companion-file parity remain open.

### Content identification

`ProviderCatalog::identify_file` compares acquired bytes with original provider
assertions. CurseForge fingerprints nominate candidates; they do not verify content.
Unknown, exact and ambiguous outcomes are distinct from provider failures. The host
must decide whether to retain provider ownership or explicitly accept direct content.
[Catalog tests](../../crates/empack-lib/src/engine/providers/tests.rs).

### Normalized import inspection

`inspect_import` reads bounded retained archives and preserves destinations, original
hashes, independent environment requirements, embedded members and override layers.
Inspection has no publisher. Import preparation requires explicit decisions for
representations that cannot be preserved automatically.
[Import tests](../../crates/empack-lib/src/engine/import/tests.rs).

The CLI import adapter now maps local archives, HTTPS downloads and provider pack
pages into that pipeline. It preserves paths, environment layers and runtime pins;
additional accepted game versions remain explicit intent. Optional defaults,
auxiliary-member exclusion and download-to-local conversion have explicit flags.
`--import-file SELECTOR=PATH` supplies an exact download obligation without changing
its source identity or original integrity assertions. Native executable tests cover
SHA-256 and MD5 imports through two syncs, export, materialized build and removal.
Native tests cover preview, runtime mismatch, auxiliary refusal, layer bytes and
re-export. Restricted-import continuation remains open.

## Open defects and delivery gates

| Priority | Item | Closure evidence |
| --- | --- | --- |
| Fixed locally | [Review 128: saved-record handle admission](https://github.com/inherent-design/empack/pull/82#discussion_r4215783750) | Reproduced eight-handle inspection failure; retained descriptors and subsequent read admission now share the allowance; stale-record inspection preserves bytes |
| Fixed locally | [Review 128: orphaned store candidates](https://github.com/inherent-design/empack/pull/82#discussion_r4215783765) | Reproduced ignored candidates; native cleanup now captures their identities under store coordination, rejects changed candidates and retains unknown/new entries |
| Open verification | Intermittent inherited-pipe warning | Host Nextest upgraded from 0.9.124 to 0.9.148 after identifying its documented macOS capture-pipe fix; 226 affected tests pass without warnings; final combined acceptance remains required |
| Cutover | Provider-owned world interpretation and live adoption parity | Real CLI tests for every preserved input form, explicit choices and unsupported conversions |
| Cutover | Complete synchronization acquisition and resolution parity | Fresh root/pin/local-source resolution and optional remote materialization are wired; multi-file decisions and manual-input continuation remain open; automatic layout and accepted-game-version policy changes use explicit revalidation |
| Cutover | Continuation completion | Browser assistance, execution-time missing-input retention and explicit stale/invalid-state cleanup; previews remain read-only |
| Cutover | Remaining acquisition cache integration and cleanup | Builds and synchronization reuse verified content; runtime assets populate the cache during approved execution; add/import acquisition has read-only lookup, with publication-time insertion and remaining disposable cleanup still open |
| Cutover | Explicit `ContinueIndependent` batches | Successful independent groups publish with partial receipts; failed groups retain prior intent/content; AllRequested remains default |
| Implemented | Initialization scaffolding | Missing ignore files and native CI workflows join the approved file plan; existing files are retained, source changes and unsafe ancestors reject publication |
| Implemented | Runtime/CLI composition | Legacy handlers, project services and private process bridge are deleted; display capabilities/palettes belong to sessions; executable errors have no global suppression flag |
| Final | Combined candidate validation | Offline CLI lifecycle, native platforms, strict live provider/import/runtime checks, measured coverage and Greptile against recorded revisions |

Both Review 128 findings reproduced before correction. The 38 affected tests and
all-target/all-feature Clippy pass after correction. These fixes have not been
re-reviewed. Per the user's instruction, further Greptile trigger cycles wait until
known implementation, CLI cutover, test rewrites and old-code removal are complete.

## Backend release

The separate packwiz-tx fork was released as **v0.2.1**, synced with upstream
main through `ef87d96`. Upstream publishes no release tag through GitHub Releases;
the fork integration preserves offline metadata and deferred refresh. Two upstream
Modrinth environment defects reproduced and were corrected before release. Fork
CI, race tests, vet, module installation and executable offline smoke passed. All six
published platform archives match their release checksums; the macOS ARM64 release
binary passes the same offline batch smoke. Ordinary native CLI commands no longer
require this executable. Native builds now own embedded-template rendering and pure Forge coordinate interpretation.
The fork remains a separately released tool. The native engine owns project metadata,
acquisition, templates and distribution assembly; the executable bootstrap and the
old library implementations are deleted.

## Verification evidence

Browser assistance: nine affected provider/build/launcher tests, ten native executable
interactive/restricted cases and all-feature Clippy passed. The live Modrinth case
resolves the exact Faithful resource-pack selection, invokes a fixture desktop opener,
verifies the renamed supplied bytes and completes the saved build. Preview and decline
have no desktop effects. Native PTY initialization now checks actual text/confirmation
prompts and resulting documents; the old fake-packwiz/browser/Java scaffolds and an
obsolete permanently ignored interactive fixture are deleted. The first PTY run exposed
a mismatched test prompt label; the corrected native prompt checks both pass.


Bounded download waiting: 41 affected CLI/build/continuation tests, six executable
restricted-build cases and all-feature Clippy passed. Real subprocess cases cover
preview, argument limits, renamed verified downloads, timeout, competing recipes and
Unix Ctrl-C status 130. Suspension now returns the exact published record observation;
retaining partial inputs checks that observation under the record lock. An unchanged
poll does not rewrite state. Browser opening remains a separate unfinished feature.


Combined `f22c46c`: `mise run test` passed 1,671 tests and eleven doctests;
102 opt-in tests were excluded. The upgraded runner reported no inherited-pipe warning.
A follow-up saved-build regression reproduced rejection of a slot already satisfied
by ordinary cache lookup. Sixteen continuation/cache tests passed after checking its
saved address and permissions against the verified current bytes and restoring only
still-pending slots. Bounded download assistance is the next integration step.


Ordinary verified build cache: 226 affected engine/host/configuration/native-command
cases and all-feature Clippy passed. A stale missing-content fixture first failed
because its preceding build now populated the cache; it now clears that disposable
fixture cache before exercising unresolved input. Separate tests prove unchanged
preview cache snapshots, MD5 evidence retention, corrupt-hint refusal and offline
Modrinth/CurseForge builds without catalog access. CLI `--cache-dir` and cache cleanup
share the same selected root. This does not claim other acquisition workflows use it.
The host runner was upgraded to Nextest 0.9.148; the affected run reported no leak
warning. The upstream macOS pipe-inheritance fix is a plausible explanation for the
prior intermittent warnings, not proof that every process path is leak-free.


Source-digest cache discovery: 21 store/cleanup tests passed, then three focused
index tests passed with an equal-length impostor blob added to the negative cases.
All-feature Clippy passed. Lookup retains original weak/strong assertions; bad hints
cannot satisfy requests, limits include hints, cleanup preserves unowned files and
retained leases. Add/import/sync and runtime asset reuse remain open.

Explicit new-source adoption: 44 affected CLI/adoption/file-plan tests, eight native
and executable cases, and all-feature Clippy passed. Real CLI coverage rejects missing
payloads, preserves previews, creates/restores locks and requires two unchanged syncs.
Provider fixtures cover exact selectors and supplied-file identification, named side-layer
copies, missing-copy refusal and no latest query or companion-payload download. The
parser regression exposed and fixed ignored source flags combined with tracked keys.

First-lock adoption: the CLI refusal reproduced before correction. Then 44 affected
adoption, observation and synchronization tests passed; the extended invalid-lock and
symlink regression passed separately, and all-feature Clippy passed. Local members,
URL content and provider metadata/side-layer identification preserve intent and bytes,
reject partial selections, and converge through two subsequent syncs. No latest-version
query is permitted by those provider fixtures. Those checks cover declared roots.

Named file roles and tracked sources: 669 affected tests and all-feature Clippy
passed. A subsequent normalization optimization passed 16 affected tests and
all-feature Clippy. Regression coverage rejects swapped role destinations, preserves
independent authoring sources, retains imported URL roles, and refuses symlinked
sources. Both targeted runs reported one intermittent inherited-output-pipe warning;
the download-budget case passed clean on rerun, while a provider-world case reported
it. This remains an open verification gate, not a clean combined-head claim.

Combined `bb2d3ee`: `mise run test` passed 1,666 tests and eleven doctests;
102 opt-in tests were excluded. No inherited-pipe warning occurred. This includes
direct worlds, first-lock and new-source adoption, not final live parity.

Demoted local groups: reproduced the multi-file update refusal, then passed the extended
world lifecycle and two related native update tests. Update and adoption retain the
non-root role and exact member set; two subsequent syncs preserve the project.

Provider side-layer adoption: 34 affected adoption, identification and native-command
tests plus all-feature Clippy passed. The reproduced refusal is replaced by exact
byte identification; wrong-project bytes and inconsistent copies preserve the project.
The fixture verifies read-only preview, the observed pin, unchanged intent and two syncs.

Tracked URL adoption: 59 affected tests and all-feature Clippy passed. The regression
first reproduced the CLI refusal, then verified preserved URL identity, unchanged
payloads, side placements, read-only preview, two unchanged syncs, binding authored
digests and rejection of a selected symlink.

Direct world groups: nine focused tests and all-feature Clippy pass after reproducing
and correcting retained metadata over-reservation across two archives. Coverage includes
131-member refresh, add/sync/build/adopt/remove, read-only preview, malformed archives,
explicit remote conversion and missing-lock ownership refusal. The final affected run passed 86 tests across host, addition, document and native command
contracts; this is not final release validation.

| Revision | Executed evidence | Qualification |
| --- | --- | --- |
| Durable import continuation | 86 affected import/cleanup/CLI tests, three strict live CurseForge cases and all-feature Clippy passed | Source retention, repeated associations, stale facts/targets, missing/corrupt archive bytes, original MD5 evidence, preview and explicit opaque cleanup covered; final combined suite remains open |
| Native live fixture cutover | Six strict executable cases and all-feature Clippy passed | Three build targets inspect exported bytes; live Modrinth import preserves documents across two syncs and re-export verifies hashes and sizes |
| `773c4ad` strict baseline | 81 executable cases: 73 passed, eight failed; 31 nonmatching tests excluded | Five stale-fixture failures corrected in the next row; three restricted CurseForge import cases still require durable continuation; no missing-prerequisite skips accepted |
| Display ownership | Reproduced first-session terminal policy leakage; 26 affected tests, concurrent in-process display suite, seven library doctests and all-feature Clippy passed | Removed global display/palette initialization and error-suppression flags; executable boundary renders returned errors |
| `2ea6a3e` combined candidate | `mise run test`: 898 tests and eight doctests passed; 103 opt-in cases excluded | No inherited-pipe warnings; 151.72 seconds; strict live matrix and coverage remain open |
| Acquisition cache capabilities | 69 affected acquisition/runtime/build/sync tests, expanded negative regression and all-feature Clippy passed | Read-only preparation does not create storage; approved runtime assets retain original assertions; unknown catalogs remain fresh; corrupt bytes, strict MD5 refusal, failed batches and aggregate/per-request limits covered |
| Synchronization content reuse | 48 affected sync/build-cache/continuation/store tests and all-feature Clippy passed | Reproduced offline restoration failure before wiring shared cache verification; exact pins, two no-op syncs, corrupt equal-length bytes and whole-tree previews covered |
| Async host and bootstrap retirement | `mise run test`: 897 tests and eight doctests passed; 103 opt-in cases excluded; 30 affected tests, all-feature Clippy and Windows cross-compilation passed | Git lookup uses the host runtime; process cancellation/deadlines remain covered; two Windows test-only warnings remain; strict live matrix and coverage are still open |
| Legacy library retirement candidate | `mise run test`: 921 tests and eight doctests passed; 103 opt-in cases excluded; 15 selected executable cases, all-feature Clippy and Windows cross-compilation passed | Windows retains test-only warnings; removed old implementations and their exclusive suites; native filesystem, identity, publication, cancellation and smoke checks remain; live browser and Modrinth import/build pass; two restricted CurseForge cases remain open |
| `f322477` combined candidate | `mise run test`: 1,678 tests and eleven doctests passed; 103 opt-in cases excluded | No inherited-pipe warnings; 118.96 seconds; strict live matrix and final coverage remain open |
| CurseForge optional metadata | Twelve import inspection tests, two native executable import/build cases and all-feature Clippy passed | Reproduced blank display-version rejection with a live archive; controls and required runtime identifiers stay invalid; local and live Modrinth import/build pass; restricted CurseForge continuation remains open |
| Provider import test retirement | 45 affected native import/CLI/executable cases and all-feature Clippy passed | Replaced old import API fixtures with four-kind provider import, exact file associations, MD5 evidence, optionality, two syncs and mrpack re-export; opaque world ZIP placement is not counted as implemented world support |
| Explicit import file associations | 58 affected import/CLI tests, three rewritten executable lifecycle cases and all-feature Clippy passed | Original SHA-256/MD5 evidence, optional export, full-client bytes, two syncs, duplicate/ambiguous/provider-qualified selectors and symlink refusal; two old provider-import fixtures remain to port |
| `9ce7616` combined candidate | `mise run test`: 1,675 tests and eleven doctests passed; 105 opt-in cases excluded | No inherited-pipe warnings; 142.41 seconds; final strict live matrix and coverage remain open |
| Legacy fixture retirement | 24 fixture/smoke tests and test-crate all-feature Clippy passed | Deleted unused fake packwiz and old continuation helpers; ZIP fixtures use explicit members independent of application archive code |
| Accepted game-version policy, combined candidate | `mise run test`: 1,645 tests and eleven doctests passed; 101 opt-in cases skipped; all-feature Clippy passed | Reproduced incompatible-pin retention; explicit pins fail safely, still-compatible selections remain fixed; 177 affected tests passed; no inherited-pipe warnings |
| Automatic layout reconciliation, combined candidate | `mise run test`: 1,644 tests and eleven doctests passed; 101 opt-in cases skipped; all-feature Clippy passed | No inherited-pipe warnings; custom-directory and side-layer fixtures now declare explicit placements; strict live matrix and fresh coverage remain open |
| Native exit classification | 15 affected host/classifier tests, 12 executable exit tests and all-feature Clippy passed | Reproduced incorrect transfer status; removed v0.4 classifiers and substring heuristics; native download and invalid-target diagnostics retain typed status |
| Remote synchronization materialization | 41 affected sync/native CLI/smoke cases and all-feature Clippy passed; rebuilt executable passed all 16 offline smoke cases | Bad remote bytes publish nothing; pins and lock bytes stay unchanged; previews do not download; supplied references cannot override local sources |
| Source lifetime correction | 87 affected add/remove/sync/verification cases and all-feature Clippy passed | Reproduced source deletion before fixing it; moved/removed placements retain shared sources, conflicting replacement fails before publication |
| Native telemetry and provider-add E2E | Seven strict telemetry/version cases, three strict add cases and all-feature Clippy passed | Missing command spans reproduced; Chrome/OTLP and failed-export shutdown verified; selector privacy, live canonical identity, replacement approval and repeated sync |
| `acf576f` combined candidate | `mise run test`: 1,652 tests and eleven doctests passed; 101 opt-in cases skipped | No inherited-pipe warnings; full run completed in 103.64 seconds; strict live matrix and final coverage remain open |
| Explicit provider file plans | 62 affected CLI/document/provider/native-command tests and all-feature Clippy passed | Required companions, per-role destinations, optional participation, strict bounded input, preview preservation and two unchanged syncs |
| Supplied ZIP identification | 86 affected CLI/addition/acquisition/provider tests and all-feature Clippy passed | Reproduced premature type refusal; provider kind now precedes placement, shared archive validation and aggregate remote transfer limits retained; renamed resource pack and two unchanged syncs |
| Native build/restricted fixture retirement | Six strict build E2E cases, two strict restricted-build cases, 22 CLI/archive tests and all-feature Clippy passed | Removed legacy config/archive fixture dependencies and unused CLI conversions; initial broad filter also exposed three unported browser cases and two restricted cases now replaced |
| Execution-time continuation | 107 affected API/build cases and all-feature Clippy passed | Repeated input decisions retain exact recipe and verified files; durable reload completes offline after supplied bytes; reservations release; legacy pipe-warning case passed clean in this run |
| `99b9a88` combined candidate | `mise run test`: 1,649 tests and eleven doctests passed; 101 opt-in cases skipped | One inherited-pipe warning in legacy `test_resolve_manifest_recovers_from_panics_and_passthrough_warnings`; strict live and final coverage remain open |
| Explicit continuation cleanup | 19 affected suspension/cleanup cases and all-feature Clippy passed | Opaque stale/malformed records, read-only preview, changed-record refusal, foreign engine ownership and symlink rejection |
| Observed provider adoption | 73 affected cases plus a root-role regression passed; all-feature Clippy passed | Reproduced and fixed accidental pinning; negative identity/pin/byte checks and two unchanged syncs; macOS test-linker unwind-size warning |
| `3060149` combined candidate | `mise run test`: 1,646 tests and eleven doctests passed; 99 opt-in cases skipped | No inherited-pipe warnings; strict live matrix and final coverage remain open; obsolete graph/mock tests account for the lower count |
| `f30e6e1` combined candidate | 1,677 default tests and eleven doctests passed; no inherited-pipe warnings | Strict E2E baseline: 25 passed, 50 failed; failures include stale v0.4 fixtures and known workflow gaps |
| Fresh synchronization frontend | 64 affected resolver/sync/CLI/smoke cases passed, including 16 offline executable smoke tests | Missing locks, exact pin changes, retained assertions, source confinement, preview and failed-batch preservation; remote materialization remains open |
| Native initialization scaffolding | 40 affected initialization/layout/replacement/publication cases and nine executable initialization cases passed | Includes user-file preservation, changed-source rejection, symlink confinement and live runtime discovery; all-feature Clippy passed |
| Native exit/cancellation rewrite | 36 affected document/host/exit cases passed, including ten executable contracts | Native proxy cancellation returns 130; malformed documents and missing unattended approval return 2 |
| Native live fixture rewrite | 17 fixture/runtime/build/clean cases passed against current intent and lock documents | Includes live Fabric, Forge, Quilt and NeoForge discovery; remaining E2E families still need conversion |
| Selected update/local adoption | 44 affected host/CLI/native tests plus three focused lifecycle cases passed; all-feature Clippy passed | One intermittent inherited-pipe warning in the affected run; no final combined rerun |
| Native dispatcher cutover | 1,676 default tests and eleven doctests passed; workspace all-feature Clippy, formatting and Windows cross-compilation passed | 98 opt-in tests skipped; no inherited-pipe warnings; Windows retains seven existing test-only warnings; live parity and new coverage remain open |
| `48bfc5d` | 94 affected host/import tests and all-feature Clippy passed | One inherited-pipe warning; isolated affected cleanup test passed clean |
| `9893353` | Five CLI adapter regressions and all-feature Clippy passed | Canonical selectors, deliberate search, direct files and repeated sync |
| This resource/cleanup correction | 38 affected tests and all-target/all-feature Clippy passed | Includes both pre-fix reproductions and changed-candidate refusal; no combined full-suite rerun |
| `e0d5083` | `mise run test`: 1,989 tests and eleven doctests passed | 124 opt-in tests skipped; one inherited-pipe warning in the template test named above; no final live/CLI parity claim |
| `e0d5083` implementation snapshot | 43 affected tests; all-target/all-feature Clippy; Windows cross-compilation passed | Windows retains seven existing test-only configuration warnings; targeted run was clean |
| `84f5583` | 1,978 default tests, eleven doctests and 24 offline CLI smoke tests passed | Earlier source revision; old CLI routing; cannot substitute for cutover tests |
| `e394d4d` | CI coverage: 2,069 tests, 93.70% workspace line coverage | Includes inline tests; not production-only coverage or a completion percentage |

Commands and opt-in prerequisites are maintained in [testing](../testing.md).
Live provider, archive and runtime suites already exist; their earlier successes do
not establish behavior at an untested final head. Record the tested revision and any
warnings each time the combined candidate changes. Do not lower assertions to keep
an obsolete implementation green.

## Cutover and test retirement rules

1. Finish missing input decisions for the native hosts. Keep decisions outside the pure core.
2. Add executable tests for new-schema initialize → add → sync twice → update → build
   → continue/recover → remove → sync, including negative and interrupted paths.
3. Switch related dispatcher routes together. Audit every flag, exit class, workdir,
   headless choice, environment override and preserved capability.
4. Delete each replaced handler and its exclusive helpers. Port assertions about user
   outcomes; remove assertions that only require obsolete subprocess arguments.
5. Retain real-filesystem confinement, ownership, crash, cancellation and adversarial
   HTTP tests. Mocks alone do not establish those guarantees.
6. Repeat combined validation and review. No merge or release follows merely from a
   green incremental review.

## Explicit limits

- Resource reservations are scheduling estimates, not a physical-memory sandbox.
- Blocking work must cooperate with cancellation. Engine batch assembly remains sequential;
  owned subprocesses run asynchronously on the host runtime.
- Multi-file publication is recoverable, not simultaneously visible to external readers.
  Windows reports file synchronization rather than Unix directory synchronization.
- Unknown dependency evidence does not authorize automatic pruning. Explicit acknowledged
  removal remains distinct from inferred cleanup; known dependents are binding.
- Verified content caches are disposable; missing or invalid cache evidence does not
  replace the source assertion or authorize a project mutation.
- Untrusted downloaded 7z input is not supported by the current bounded ZIP import path;
  7z output verification reads privately generated candidates.
- No gameplay certification follows from successful archive or server-launch probes.

Update this ledger in place when behavior changes. Keep a single current routing table,
a small evidence table and unresolved gates; use Git history for the narrative.
