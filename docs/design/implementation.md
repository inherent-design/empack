# v0.5 delivery ledger

Source baseline: native CLI cutover after `48bfc5d`, 2026-10-08. Target: **v0.5.0-alpha.1**.
This is the current delivery checklist, not a release claim or a completion percentage.
The [design](README.md) defines the target; the [feature requirements](parity.md)
define the capabilities to preserve. Historical implementation notes and test runs
remain in [the preceding ledger](https://github.com/inherent-design/empack/blob/e0d5083/docs/design/implementation.md).

**Ordinary project commands now dispatch to the v0.5 engine.** The old command
handlers and their outer mutation lock have been deleted. This establishes the
new route, not full feature completion: identification, worlds, complete adoption
frontends, fresh resolution, browser assistance and remaining library retirement
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
| Import local/remote packs | **CLI wired:** source classification, verified import and explicit conversion choices | Restricted-input continuation; live provider/archive matrix |
| Add | **CLI wired:** canonical provider selections, deliberate search and direct file publication | World members and full companion-file choices; explicit provider identification is wired |
| Update | **CLI wired:** exact logical selection, canonical provider refresh and declared direct sources | Broaden live update/companion-role tests; preserve changed-file refusal |
| Adopt observed content | **CLI wired for tracked local files:** verified document-only acceptance of changed bytes | Provider/URL observation selection and new untracked groups |
| Remove | **CLI wired:** shared exact ownership planner | Broaden executable alias/title/stem tests; explicit unknown-evidence policy and demotion are exposed |
| Sync | **CLI wired:** recorded selections, fresh resolution for missing/unsatisfied roots and explicit remote materialization | Broaden runtime, search and multi-file placement matrices; manual acquisition continuation |
| Build / continue | **CLI wired:** native build and saved recipe continuation | Browser assistance, execution-time missing-input retention, live target/runtime matrix |
| Clean | **CLI wired:** scoped artifact/cache cleanup | Remaining disposable stores and explicit stale/invalid-record cleanup |
| Recover | **CLI wired:** engine recovery | Retain interruption and restart tests through library retirement |
| Requirements / version | Host inspection; no managed-tool bootstrap | Capability-specific live prerequisites |

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
World interpretation and companion-file decisions remain open.

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
Native tests cover preview, runtime mismatch, auxiliary refusal, layer bytes and
re-export. Restricted-import continuation remains open.

## Open defects and delivery gates

| Priority | Item | Closure evidence |
| --- | --- | --- |
| Fixed locally | [Review 128: saved-record handle admission](https://github.com/inherent-design/empack/pull/82#discussion_r4215783750) | Reproduced eight-handle inspection failure; retained descriptors and subsequent read admission now share the allowance; stale-record inspection preserves bytes |
| Fixed locally | [Review 128: orphaned store candidates](https://github.com/inherent-design/empack/pull/82#discussion_r4215783765) | Reproduced ignored candidates; native cleanup now captures their identities under store coordination, rejects changed candidates and retains unknown/new entries |
| Open verification | Intermittent inherited-pipe warning | Combined cutover and template/runtime checks were clean; subsequent concurrent host subsets reported a retained output pipe despite passing assertions; investigate process retirement before final acceptance |
| Cutover | Search/identification/adoption frontends and world-member interpretation | Real CLI tests for every preserved input form, explicit choices and unsupported conversions |
| Cutover | Complete synchronization acquisition and resolution parity | Fresh root/pin/local-source resolution and optional remote materialization are wired; multi-file decisions and changed layout/compatibility-policy semantics need further coverage |
| Cutover | Continuation completion | Browser assistance, execution-time missing-input retention and explicit stale/invalid-state cleanup; previews remain read-only |
| Cutover | Ordinary cache integration and cleanup | Cache use does not change source evidence; cleanup preserves leases, recovery and saved requests |
| Cutover | Explicit `ContinueIndependent` batches | Successful independent groups publish with partial receipts; failed groups retain prior intent/content; AllRequested remains default |
| Implemented | Initialization scaffolding | Missing ignore files and native CI workflows join the approved file plan; existing files are retained, source changes and unsafe ancestors reject publication |
| Cutover | Runtime/CLI composition | Remove migrated handler bypasses and synchronous process bridges where superseded; isolate global display/error state for embedding |
| Final | Combined candidate validation | Offline CLI lifecycle, native platforms, strict live provider/import/runtime checks, measured coverage and Greptile against recorded revisions |

Both Review 128 findings reproduced before correction. The 38 affected tests and
all-target/all-feature Clippy pass after correction. These fixes have not been
re-reviewed. Per the user's instruction, further Greptile trigger cycles wait until
known implementation, CLI cutover, test rewrites and old-code removal are complete.

## Backend release

The retained compatibility backend is pinned to packwiz-tx **v0.2.1**, synced with upstream
main through `ef87d96`. Upstream publishes no release tag through GitHub Releases;
the fork integration preserves offline metadata and deferred refresh. Two upstream
Modrinth environment defects reproduced and were corrected before release. Fork
CI, race tests, vet, module installation and executable offline smoke passed. All six
published platform archives match their release checksums; the macOS ARM64 release
binary passes the same offline batch smoke. Ordinary native CLI commands no longer
require this executable. Native builds now own embedded-template rendering and pure Forge coordinate interpretation.
The unused legacy removal planner, slug-based dependency graph and obsolete mock-command test builder are deleted. Retirement of the remaining compatibility
modules remains open.

## Verification evidence

| Revision | Executed evidence | Qualification |
| --- | --- | --- |
| Remote synchronization materialization | 41 affected sync/native CLI/smoke cases and all-feature Clippy passed; rebuilt executable passed all 16 offline smoke cases | Bad remote bytes publish nothing; pins and lock bytes stay unchanged; previews do not download; supplied references cannot override local sources |
| Source lifetime correction | 87 affected add/remove/sync/verification cases and all-feature Clippy passed | Reproduced source deletion before fixing it; moved/removed placements retain shared sources, conflicting replacement fails before publication |
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
- Blocking work must cooperate with cancellation; the old synchronous observer bridge
  can still block its caller. Engine batch assembly remains sequential.
- Multi-file publication is recoverable, not simultaneously visible to external readers.
  Windows reports file synchronization rather than Unix directory synchronization.
- Unknown dependency evidence does not authorize automatic pruning. Explicit acknowledged
  removal remains distinct from inferred cleanup; known dependents are binding.
- Shared HTTP-cache snapshots remain disposable and last-writer-wins across processes.
- Untrusted downloaded 7z input is not supported by the current bounded ZIP import path;
  7z output verification reads privately generated candidates.
- No gameplay certification follows from successful archive or server-launch probes.

Update this ledger in place when behavior changes. Keep a single current routing table,
a small evidence table and unresolved gates; use Git history for the narrative.
