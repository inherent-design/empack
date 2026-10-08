# v0.5 delivery ledger

Source baseline: `e0d5083`, with the resource/cleanup corrections below, 2026-10-08. Target: **v0.5.0-alpha.1**.
This is the current delivery checklist, not a release claim or a completion percentage.
The [design](README.md) defines the target; the [feature requirements](parity.md)
define the capabilities to preserve. Historical implementation notes and test runs
remain in [the preceding ledger](https://github.com/inherent-design/empack/blob/e0d5083/docs/design/implementation.md).

**The engine is implemented substantially further than the CLI integration.**
The executable still routes ordinary project commands through repaired v0.4 handlers.
Only `recover` dispatches directly to the new engine host. Native hosts for the other
operations are callable and tested, but this does not establish executable parity.
The [dispatcher](../../crates/empack-lib/src/application/commands.rs) is the source of truth.

## Delivery states

- **CLI wired:** the ordinary executable reaches the engine and has command-level evidence.
- **Host implemented:** the application adapter composes the engine; dispatcher integration remains.
- **Engine implemented:** compiled behavior has contract tests; frontend decisions may remain.
- **Open:** required behavior or acceptance evidence is missing.

A command is complete only after its preserved options, failures and recovery paths
pass executable tests and its replaced mutation path is deleted. Component tests,
a passing review and line coverage do not change that state by themselves.

## Command routing and cutover

| Workflow | Current executable route | New implementation | Work before deleting the old route |
| --- | --- | --- | --- |
| Initialize / forced replacement | Existing `handle_init` | [Native initialization host](../../crates/empack-lib/src/application/engine_host/initialize.rs); [tests](../../crates/empack-lib/src/application/engine_host/initialize/tests.rs) | Route empty/source initialization, runtime choices and replacement approval; prove preview/decline/failure preserve the project |
| Import local/remote packs | Existing init/import orchestration | [Native import host](../../crates/empack-lib/src/application/engine_host/import.rs); [tests](../../crates/empack-lib/src/application/engine_host/import/tests.rs) | Connect CLI classification and representation decisions; test provider URLs, local archives, overrides and optional conversion |
| Add | Existing `handle_add` | [Mixed provider/file host](../../crates/empack-lib/src/application/engine_host/dependencies.rs); [tests](../../crates/empack-lib/src/application/engine_host/dependencies/tests.rs) | Connect search, exact selectors, identification, pins and explicit local acceptance; finish world-member interpretation |
| Update | No command | Selected-update host in the dependency module | Expose explicit selection and retain pins, aliases and root/transitive roles |
| Adopt observed content | No command | [Adoption host](../../crates/empack-lib/src/application/engine_host/dependencies/adoption.rs) and dependency host tests | Construct proposed groups from inspected content; show the precise intent/selection changes |
| Remove | Existing `handle_remove` | Exact identity/ownership removal host | Connect aliases, titles, installed names and explicit incomplete-evidence decisions; no automatic orphan inference |
| Sync | Existing `handle_sync` | Recorded/supplied synchronization host and engine | Add fresh-resolution decisions and optional remote materialization; wire lock-preserving sync |
| Build / continue | Existing `handle_build` | [Native build host](../../crates/empack-lib/src/application/engine_host/build.rs); [tests](../../crates/empack-lib/src/application/engine_host/build/tests.rs) | Wire all target/options, browser assistance, continuation and association; retain continuation for execution-time missing input |
| Clean | Existing `handle_clean` | [Scoped cleanup host](../../crates/empack-lib/src/application/engine_host/cleanup.rs); [tests](../../crates/empack-lib/src/application/engine_host/cleanup/tests.rs) | Cover remaining disposable stores and explicit stale-record cleanup; preserve recovery and unrelated files |
| Recover | **CLI wired** | [Engine host](../../crates/empack-lib/src/application/engine_host.rs); [tests](../../crates/empack-lib/src/application/engine_host/tests.rs) | Retain inspect/preview/finish/restore tests through the remaining cutover |
| Requirements / version | Existing handlers | Host concern | Make requirements capability-specific; inspection must not bootstrap unrelated tools |

Switch schema-producing and schema-consuming commands together once their missing
frontend choices are implemented. Do not introduce a long-lived dual-schema mode.
Tests may invoke hosts independently while that coordinated dispatcher change is prepared.

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
kinds, provider failures and original rank remain explicit. Selection still needs
canonical lookup and version/closure resolution. CLI search composition is open.
[Search tests](../../crates/empack-lib/src/engine/providers/search/tests.rs).

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

## Open defects and delivery gates

| Priority | Item | Closure evidence |
| --- | --- | --- |
| Fixed locally | [Review 128: saved-record handle admission](https://github.com/inherent-design/empack/pull/82#discussion_r4215783750) | Reproduced eight-handle inspection failure; retained descriptors and subsequent read admission now share the allowance; stale-record inspection preserves bytes |
| Fixed locally | [Review 128: orphaned store candidates](https://github.com/inherent-design/empack/pull/82#discussion_r4215783765) | Reproduced ignored candidates; native cleanup now captures their identities under store coordination, rejects changed candidates and retains unknown/new entries |
| Next | Full-suite inherited-pipe warning | Isolate `engine::templates::tests::template_failures_never_return_partial_outputs_or_modify_project`; determine cause or retain the unresolved warning in evidence |
| Cutover | Search/identification/adoption frontends and world-member interpretation | Real CLI tests for every preserved input form, explicit choices and unsupported conversions |
| Cutover | Fresh sync and optional materialization | Changed intent resolves correctly; ordinary sync retains exact selections; repeated sync is a no-op |
| Cutover | Continuation completion | Browser assistance, execution-time missing-input retention and explicit stale/invalid-state cleanup; previews remain read-only |
| Cutover | Ordinary cache integration and cleanup | Cache use does not change source evidence; cleanup preserves leases, recovery and saved requests |
| Cutover | Explicit `ContinueIndependent` batches | Successful independent groups publish with partial receipts; failed groups retain prior intent/content; AllRequested remains default |
| Cutover | Runtime/CLI composition | Remove migrated handler bypasses and synchronous process bridges where superseded; isolate global display/error state for embedding |
| Final | Combined candidate validation | Offline CLI lifecycle, native platforms, strict live provider/import/runtime checks, measured coverage and Greptile against recorded revisions |

Both Review 128 findings reproduced before correction. The 38 affected tests and
all-target/all-feature Clippy pass after correction. These fixes have not been
re-reviewed. Per the user's instruction, further Greptile trigger cycles wait until
known implementation, CLI cutover, test rewrites and old-code removal are complete.

## Backend release

The existing CLI backend is pinned to packwiz-tx **v0.2.1**, synced with upstream
main through `ef87d96`. Upstream publishes no release tag through GitHub Releases;
the fork integration preserves offline metadata and deferred refresh. Two upstream
Modrinth environment defects reproduced and were corrected before release. Fork
CI, race tests, vet, module installation and executable offline smoke passed. All six
published platform archives match their release checksums; the macOS ARM64 release
binary passes the same offline batch smoke. This does not replace CLI cutover.

## Verification evidence

| Revision | Executed evidence | Qualification |
| --- | --- | --- |
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
