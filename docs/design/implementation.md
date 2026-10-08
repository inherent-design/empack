# v0.5 delivery ledger

Tested source: `09c4135`, 2026-10-08. The accepted implementation is complete; verification evidence is recorded below. Target: **v0.5.0-alpha.1**.
This ledger records the accepted feature scope and its evidence. It does not authorize a release.
The [design](README.md) defines the target; the [feature requirements](parity.md)
define the capabilities to preserve. Historical implementation notes and test runs
remain in [the preceding ledger](https://github.com/inherent-design/empack/blob/e0d5083/docs/design/implementation.md).

**Ordinary project commands now dispatch to the v0.5 engine.** The old command
handlers and their outer mutation lock have been deleted. This establishes the
common route for ordinary operations. Curated import/build acceptance passed all seven packs. The [dispatcher](../../crates/empack-lib/src/application/commands.rs)
and executable tests are the source of truth.

## Delivery states

- **CLI wired:** the ordinary executable reaches the engine and has command-level evidence.
- **Host implemented:** the application adapter composes the engine; dispatcher integration remains.
- **Engine implemented:** compiled behavior has contract tests; frontend decisions may remain.
- **Open:** required behavior or acceptance evidence is missing; no accepted implementation item remains in this state.

A command is complete only after its preserved options, failures and recovery paths
pass executable tests and its replaced mutation path is deleted. Component tests,
a passing review and line coverage do not change that state by themselves.

## Command routing and cutover

| Workflow | Executable route | Acceptance evidence |
| --- | --- | --- |
| Initialize / forced replacement | **CLI wired:** native initialization and approved replacement | Native preview/replacement cases, explicit headless loader/latest choices and live catalog/runtime probes |
| Import local/remote packs | **CLI wired:** source classification, verified import and explicit conversion choices | Seven curated packs, URL/weak-digest contracts and verified continuation across restarts |
| Add | **CLI wired:** canonical provider selections, deliberate search and direct file publication | Canonical-selector/closure fixtures, live provider probes, exact and supplied world archives |
| Update | **CLI wired:** exact logical selection, canonical provider refresh and declared direct sources | Exact-selection, independent-batch, provider-world and companion-role fixtures; changed-file refusal |
| Adopt observed content | **CLI wired for tracked local/member files, URL files and providers across side layers, including first-lock adoption and explicit new source groups:** verified document-only acceptance of observed bytes and exact pins | Native provider/member, first-lock, side-layer and source-group fixtures |
| Remove | **CLI wired:** shared exact ownership planner | Alias/title/stem collisions, explicit unknown-evidence policy, demotion and wrong-kind/link refusals |
| Sync | **CLI wired:** recorded selections, fresh resolution for missing/unsatisfied roots and explicit remote materialization | Two no-op syncs per curated pack; runtime/layout/search/companion fixtures and exact manual continuation |
| Build / continue | **CLI wired:** native build, saved recipe continuation and bounded download waiting | Five target families, three archive formats, eleven actual Java runtime probes and executable continuation cases |
| Clean | **CLI wired:** scoped artifact/cache/retained-input cleanup | Explicit stale/invalid build, import and sync record cleanup; empty record categories permit bounded retained-payload reclamation |
| Recover | **CLI wired:** engine recovery | Native subprocess interruption and restart at durable publication boundaries |
| Requirements / version | Host inspection; no managed-tool bootstrap | Offline executable inspection and capability-specific live prerequisite checks |

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
parity includes the strict executable and curated pack runs recorded below.

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
subsequent unchanged syncs. Live provider probes and curated provider-world imports
complement these deterministic lifecycle cases; neither implies exhaustive provider coverage.

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

## Closed implementation and review items

| Status | Item | Closure evidence |
| --- | --- | --- |
| Verified | [Review 128: saved-record handle admission](https://github.com/inherent-design/empack/pull/82#discussion_r4215783750) | Reproduced eight-handle inspection failure; retained descriptors and subsequent read admission now share the allowance; stale-record inspection preserves bytes |
| Verified | [Review 128: orphaned store candidates](https://github.com/inherent-design/empack/pull/82#discussion_r4215783765) | Reproduced ignored candidates; native cleanup now captures their identities under store coordination, rejects changed candidates and retains unknown/new entries |
| Verified | Intermittent inherited-pipe warning | Host Nextest upgraded to 0.9.148; both subsequent combined default runs passed without inherited-pipe warnings |
| Implemented | Provider-world lifecycle | Native fixtures cover changed-version update, adoption, supplied archives and removal; Boosted FPS now imports its three real provider worlds, syncs twice and builds verified client world members |
| Implemented | Synchronization resolution and manual acquisition | Authored per-file placements preserve companion roles and optional choices; missing required companions reject publication; exact manual-input continuation, layout and accepted-game-version revalidation are wired |
| Implemented | Continuation interfaces | Build browser/wait assistance, import and sync manual inputs, and explicit stale/invalid-state cleanup are wired; strict and curated continuation acceptance passed |
| Implemented | Acquisition cache integration | Build/sync/runtime and add/import consumers use verified lookup; approved mutations populate the cache; restricted inputs reuse exact asserted bytes |
| Implemented | Retained-input reclamation | `clean retained` reclaims empty record categories under save/cleanup coordination; saved and unknown records, active private leases and recovery journals remain protected |
| Implemented | Explicit `ContinueIndependent` batches | Add/update opt in with `--continue-independent`; resolved groups prepare independently and publish one combined candidate with partial receipts; unresolved identity/evidence still blocks the whole request |
| Implemented | Initialization scaffolding | Missing ignore files and native CI workflows join the approved file plan; existing files are retained, source changes and unsafe ancestors reject publication |
| Implemented | Runtime/CLI composition | Legacy handlers, project services and private process bridge are deleted; display capabilities/palettes belong to sessions; executable errors have no global suppression flag |
| Verified | Review 129: continuation memory admission | Small sync save/load and three-restart import regression pass under 8 MiB; escaped record sizing and changed-size refusal retain bounds |
| Verified | Concurrent Windows storage creation | Native CI exposed error 183 when another process created the private root; creation now tolerates that outcome, then still checks native identity, kind, reparse points and DACL |
| Verified | Combined candidate validation | Local 1,017-test run; native platform/import matrix; recorded live probes and Greptile 131 |

Both Review 128 findings reproduced before correction. The 38 affected tests and
all-target/all-feature Clippy passed after correction. Review 129 repeated the already
corrected orphan-candidate finding; its native cleanup regressions still pass. It also
identified fixed continuation-memory reservations. Both sync and import reproduced
small-record failures under an 8 MiB allowance. Admission now scales with observed
record bytes and model data, and reads reject growth beyond their admitted size.
All 55 affected continuation/document/cleanup tests and all-feature Clippy pass
after correction. Review 131 is green at 5/5 on `09c4135`, including the later
Windows creation-race correction.

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

## Candidate evidence

Production/test candidate: `09c4135`. Later ledger-only changes do not alter this source snapshot.

| Check | Observed result |
| --- | --- |
| Combined instrumented tests | 1,017/1,017 passed, including 81 strict executable cases; 22 opt-in cases excluded |
| Documentation and smoke-driver contracts | Eight doctests and seven Python tests passed |
| Static checks | All-feature and minimal-feature Clippy, core isolation and Windows cross-compilation passed |
| Source-file coverage | 92.73% lines; standalone test files, test crate and mock sessions excluded; inline tests remain included |
| Greptile | Review 131: 5/5 on `09c4135`, no actionable new finding; all threads resolved |
| Native CI | [Linux, macOS and Windows default/strict suites and all three import-smoke jobs passed](https://github.com/inherent-design/empack/actions/runs/37845100325) |
| Frozen real-pack acceptance | Seven of seven curated import → sync twice → full-client build workflows passed at `c9a8159`; Boosted FPS includes three provider worlds and Fabulously Optimized exercises manual import continuation |
| Live provider/runtime probes | Seven provider and eleven actual Java runtime probes passed at `c9a8159`; loader catalog passed at `77ec34c` |

The curated/probe revisions precede the continuation admission correction; their
production build/provider code is unchanged. The combined candidate run exercises
the corrected continuation code. Earlier and targeted results are not summed into
a fictitious same-head run.

Review 129's continuation issue reproduced in both sync and import with an 8 MiB
allowance. Size-based encoding/decoding admission replaces fixed memory floors.
Fifty-five affected tests pass, including three import restarts, unchanged-project
assertions, escaped-record sizing and refusal when a record grows after admission.

Windows prompt failures at `77ec34c` traced to ConPTY 0.5's redirected-parent handles.
The fixture uses ConPTY 0.7's upstream correction, preserving real prompt/decline
assertions, inherited environment and quoted arguments. The server fixture now shares
the existing four-minute allowance; another Fabric server build took 184 seconds on
that runner. Both prompt cases and the local-content build passed in the next Windows run.
No production interactivity policy changed to accommodate the fixture.

That run also exposed a competing-creator race in private host storage. Windows
creation now accepts an already-existing path, then callers perform their normal
kind, reparse, owner and ACL checks. Existing shared directories remain rejected
and unchanged. Native regressions exercise repeated creation, shared ACL refusal,
file-kind refusal and concurrent openers of the same boundary. All 25 affected
publication/store tests and all-feature Clippy pass locally; Windows cross-compilation
passes. The Windows-only privacy cases and the subsequent strict CLI and CurseForge import/build smoke all passed natively.

The macOS linker emits a nonfatal unwind-table warning. Cross-compilation is not
native execution evidence. Coverage is measured, not a claim of exhaustive testing.

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
