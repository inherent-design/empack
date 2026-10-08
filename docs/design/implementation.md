# v0.5 delivery ledger

Combined baseline: `b076760`, 2026-10-08. Target: **v0.5.0-alpha.1**.
This is the current delivery checklist, not a release claim or a completion percentage.
The [design](README.md) defines the target; the [feature requirements](parity.md)
define the capabilities to preserve. Historical implementation notes and test runs
remain in [the preceding ledger](https://github.com/inherent-design/empack/blob/e0d5083/docs/design/implementation.md).

**Ordinary project commands now dispatch to the v0.5 engine.** The old command
handlers and their outer mutation lock have been deleted. This establishes the
new route, not full release validation: remaining resolution conformance and combined live/platform acceptance still require work. The [dispatcher](../../crates/empack-lib/src/application/commands.rs)
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
| Add | **CLI wired:** canonical provider selections, deliberate search and direct file publication | Broader live provider/world parity; exact and supplied world archives are wired |
| Update | **CLI wired:** exact logical selection, canonical provider refresh and declared direct sources | Broaden live update/companion-role tests; preserve changed-file refusal |
| Adopt observed content | **CLI wired for tracked local/member files, URL files and providers across side layers, including first-lock adoption and explicit new source groups:** verified document-only acceptance of observed bytes and exact pins | Broaden live provider/member parity |
| Remove | **CLI wired:** shared exact ownership planner | Broaden executable alias/title/stem tests; explicit unknown-evidence policy and demotion are exposed |
| Sync | **CLI wired:** recorded selections, fresh resolution for missing/unsatisfied roots and explicit remote materialization | Native `sync --continue` and `clean sync` are wired; broaden runtime, search and multi-file placement matrices |
| Build / continue | **CLI wired:** native build, saved recipe continuation and bounded download waiting | Broaden live target/runtime and platform desktop matrix; execution-time missing inputs retain resumable state |
| Clean | **CLI wired:** scoped artifact/cache/retained-input cleanup | Explicit stale/invalid build, import and sync record cleanup; empty record categories permit bounded retained-payload reclamation |
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
Direct and provider world interpretation and explicit companion-file plans are wired.
Provider-world supplied inputs, changed-version updates and fresh-resolution sync now have
host tests. The companion-file fixture also edits authored placement decisions, rejects
an omitted required companion without mutation, verifies preview preservation and two
subsequent unchanged syncs. Broader live provider acceptance remains a final gate.

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
re-export. Restricted imports retain exact source bytes and verified associations through
`init --continue`; `clean import` explicitly discards saved state.

## Open defects and delivery gates

| Priority | Item | Closure evidence |
| --- | --- | --- |
| Fixed locally | [Review 128: saved-record handle admission](https://github.com/inherent-design/empack/pull/82#discussion_r4215783750) | Reproduced eight-handle inspection failure; retained descriptors and subsequent read admission now share the allowance; stale-record inspection preserves bytes |
| Fixed locally | [Review 128: orphaned store candidates](https://github.com/inherent-design/empack/pull/82#discussion_r4215783765) | Reproduced ignored candidates; native cleanup now captures their identities under store coordination, rejects changed candidates and retains unknown/new entries |
| Open verification | Intermittent inherited-pipe warning | Host Nextest upgraded from 0.9.124 to 0.9.148 after identifying its documented macOS capture-pipe fix; 226 affected tests pass without warnings; final combined acceptance remains required |
| Cutover | Broader live world/update/adoption parity | Real CLI tests for every preserved input form, explicit choices and unsupported conversions |
| Implemented | Synchronization resolution and manual acquisition | Authored per-file placements preserve companion roles and optional choices; missing required companions reject publication; exact manual-input continuation, layout and accepted-game-version revalidation are wired |
| Implemented | Continuation interfaces | Build browser/wait assistance, import and sync manual inputs, and explicit stale/invalid-state cleanup are wired; broaden combined live acceptance |
| Implemented | Acquisition cache integration | Build/sync/runtime and add/import consumers use verified lookup; approved mutations populate the cache; restricted inputs reuse exact asserted bytes |
| Implemented | Retained-input reclamation | `clean retained` reclaims empty record categories under save/cleanup coordination; saved and unknown records, active private leases and recovery journals remain protected |
| Implemented | Explicit `ContinueIndependent` batches | Add/update opt in with `--continue-independent`; resolved groups prepare independently and publish one combined candidate with partial receipts; unresolved identity/evidence still blocks the whole request |
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

## Latest combined evidence

Frozen `b076760` passed 928 default tests and eight doctests, 81 strict executable
cases, seven live provider probes, the live catalog probe and all eleven actual Java
runtime checks. Minimal-feature Clippy and Windows cross-compilation passed; Windows
retains two existing test-only warnings. The instrumented run passed all 1,009 selected
tests. Measured source-file line coverage was 92.77% after excluding standalone tests,
`.test.rs` files, the test crate and mock sessions; inline tests remain included.

Curated validation then exposed a historical CurseForge manifest without `required`
and an obsolete smoke driver. The parser regression reproduced before correction;
13 import tests and Clippy pass with omitted flags treated as required and explicit
null/type errors rejected. The driver now uses native import continuation, isolated
state/cache roots, exact supplied inputs, two no-op syncs and verified full-client
archives. Its five offline contracts pass. The new live curated run and final combined
acceptance remain open; the earlier frozen results do not cover these corrections.


At `a96d2bc`, the default combined run passed **912 tests and eight doctests**
in 171.45 seconds; 103 opt-in cases were excluded. This does not establish live/platform
acceptance or acceptance of subsequent changes. Synchronization continuation passed 91 affected CLI, executable smoke, import-record,
cleanup and recorded-sync tests, plus all-target/all-feature Clippy. The executable
was rebuilt before the final run. Tests cover repeated inputs across restart, wrong
bytes, symlinks, stale previews, exact cleanup and two unchanged subsequent syncs.
The cache-admission issue reported against `5fe32bc` reproduced with no spare inventory
memory. Optional inventory/copy/writer capacity exhaustion now skips caching without
consuming the prepared mutation. Cancellation, closed admission and byte-verification
failures still fail. The correction passed 49 affected runtime, mutation and store tests
and all-target/all-feature Clippy; it has not received final combined acceptance.

Restricted import/provider cache lookup reproduced the missing-hit refusal, then passed
50 affected acquisition/import/provider tests and all-feature Clippy. Compatibility hits
preserve MD5 evidence; strict-source policy, equal-length corruption and aggregate limits
remain enforced. Final same-head executable/live acceptance remains open.

## Storage ownership inventory

| Storage | Current owner and cleanup |
| --- | --- |
| Selected cache root / `content-v1` | The only active disposable cache; `clean cache` plans verified blobs, digest hints and abandoned candidates under store coordination |
| Host state / `pending-builds`, `pending-imports`, `pending-sync` | Durable operation records; explicit `clean continuation`, `clean import`, `clean sync`; `clean all` preserves them |
| Host state / `pending-content`, `pending-import-content`, `pending-sync-content` | Retained verified inputs; `clean retained` reclaims a category only when it has no saved records; unknown records preserve the category |
| Publication journal and preimages | Recovery authority; excluded from disposable-cache cleanup |
| Operation scratch and staging | Owned temporary lifetimes; no persistent path is selected for recursive cleanup |

Unused v0.4 bin/JAR/version/HTTP/packwiz cache-path APIs and their exclusive tests are
removed. They had no runtime callers. Installer JARs remain supported through verified
native acquisition and the shared content cache; distribution packwiz metadata remains
part of the build formats.

Retained-input cleanup: 48 affected cleanup/suspension tests, the rebuilt executable
manual-sync lifecycle test and all-target/all-feature Clippy passed. Tests exercise
preview preservation, pending and unknown records, active save exclusion, record and
blob changes after preparation, active private leases and unrelated recovery data.
This is targeted evidence after `e24dfc4`, not a new combined release-validation run.

Dependency batch API: 78 affected addition/cache tests passed, followed by six focused
component/API tests after tightening alias footprints and moving graph analysis into the
admitted worker. Coverage includes default refusal, partial addition/update receipts,
connected failures, declined preparation, stale approval and unchanged subsequent sync.
The CLI adapter subsequently passed 26 affected tests, two rebuilt executable partial-batch
cases and all-target/all-feature Clippy. Executable checks cover default refusal, preview,
preserved existing content and unowned directories, nonzero partial status, repeated sync
and an unchanged explicit batch update. Source resolution still precedes grouping.

Provider world document contracts now distinguish the exact provider archive, its original
source assertions, extracted member observations and stable destination roots. Private
extraction evidence binds a member to a verified archive without turning an observed
member hash into a provider assertion. Codec and extraction tests cover round trips,
foreign selections, invalid roots, altered member declarations, wrong members and weaker
archive evidence. This establishes the representation; later slices below wire acquisition and the initial
command lifecycle. The representation change passed 38 affected
model/document/archive tests and all-target/all-feature Clippy.

World-member build acquisition now refreshes the exact archive role, shares one transfer
among its members and extracts through a bounded reader. Strong-source builds require
verified archive extraction even when installed member bytes already match their recorded
observations. Cached member bytes alone do not supply archive permissions or source proof.
Thirty affected build/acquisition/API tests passed; focused tests also cover one catalog
lookup, changed source/member bytes, weaker evidence, expansion limits and actual mrpack
contents. Provider addition is covered by the next implementation slice.

Provider-world addition now returns an archive-interpretation phase before any publishable
group. The shared world reader verifies all members, retains exact provider ownership and
publishes them through the normal addition path. Recorded sync verifies installed members;
update and adoption select the archive rather than looking up individual member names.
The host lifecycle test covers unchanged preview, add, two no-op syncs, unchanged update,
adoption, actual client archive bytes, removal and subsequent sync. Removal preserves an
untracked neighbor. The slice passed 122 affected core/host/addition/sync tests and
all-target/all-feature Clippy. The subsequent slice below covers changed versions and
supplied/restricted inputs.

Provider-world input completion adds identified local archives and restricted-source cache
lookup, retaining original archive assertions. Tests reject mismatched input, missing
input, exhausted byte budgets and MD5-only strong verification. Host tests exercise
unpin → no-upgrade sync → changed-version update, renamed source archives, changed member
inventories, stable destination roots, supplied-file identification and fresh-resolution
sync previews. A seeded archive-cache fixture reproduced a missing cache adapter in CLI
selector preparation; that path now attaches verified cache lookup. This slice passed
60 affected tests and all-target/all-feature Clippy. These are deterministic fixtures,
not live-provider acceptance.

Provider-world pack imports now use the shared bounded member reader before returning
an import candidate. `init --world-folder` chooses the parent destination explicitly.
Native fixtures verify preview preservation, restricted supplied bytes, source MD5
retention, two unchanged syncs and actual mrpack member bytes; ambiguous world roots,
wrong supplied bytes and missing destinations leave the whole project untouched.
The affected import, initialization and publication suites passed 93 tests and
all-target/all-feature Clippy.

At `3987ba6`, default test binaries passed 927 tests (103 opt-in excluded). The strict
live run passed 75 of 81; six acquisition-deadline failures occurred with the inherited 30-second default.
All six passed a diagnostic rerun with an explicit 300-second allowance and two test
workers. The host default now matches the engine's 300-second phase budget; explicit
short-deadline tests remain unchanged. The 55 affected configuration/acquisition tests
and all-feature Clippy pass. A full default-concurrency rerun remains required.
Doctest compilation overlapped later source changes, so those results are not presented
as one frozen combined revision.

Combined `d8b790e` validation passed 919 tests and eight doctests, with 103 opt-in cases
excluded. Live/platform acceptance and fresh coverage remain open.

## Historical verification evidence

Earlier slice results, superseded counts and the work that remained at those revisions
are retained in [the prior ledger](https://github.com/inherent-design/empack/blob/3ecc13b/docs/design/implementation.md#historical-verification-evidence).
They are not outstanding items in this candidate and cannot be combined into a final-head
acceptance result.

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
