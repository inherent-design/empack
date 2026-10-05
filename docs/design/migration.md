# Migration and ownership ledger

Target contract for v0.5.0-alpha.1. Code blocks are design sketches unless the
[implementation ledger](implementation.md) identifies a compiled API.

## 21. Feature parity and migration

### 21.1 Parity ledger

Before deleting old paths, enumerate CLI flags, environment variables, backend options, import branches, target variants, templates, caches, and recovery behavior. Classify each as **preserve**, **rename with compatibility adapter**, **explicitly unsupported today**, or **bug to remove**.

Use baseline implementation and fixtures to resolve contradictory or stale documentation. A bug is not a compatibility feature, but a specialized supported input must not disappear because the new common model forgot it.

| Feature family | Destination in this design | Required migration evidence |
|---|---|---|
| Provider search/order, exact IDs, slugs, provider URLs, pins | Selector and catalog ports | Equivalent identity and pin tests. |
| Direct downloads, recognized/unidentified JARs, typed ZIPs, local files | Acquisition/probe plus normalized add | Preview isolation and repeated sync. |
| Mods, resource packs, shaders, datapacks, worlds, arbitrary preserved overrides | Content kind, placement, provenance | Destination and type capability fixtures. |
| Modrinth mrpack and CurseForge ZIP, local/remote source | Import adapters | Semantic round trips and failure before replacement. |
| Recognized but currently unsupported formats | Explicit capability reporting | No accidental claim of implemented support. |
| Pack metadata/runtime overrides, loader families and accepted game versions | Project/runtime intent | Old valid configurations normalize without upgrades. |
| Datapack-folder inference and backend options | Pure layout proposals and backend recipe | Preserve precedence and explicit override behavior. |
| Common/client/server overrides and optional files | Requirements/layers/projectors | Collision, precedence, optionality, and unsupported-conversion tests. |
| `mrpack`, `client`, `server`, `client-full`, `server-full`, `all` | Build target planner | All targets share fresh input and inventory verification. |
| ZIP, TAR.GZ, 7z and binary templates | Archive adapters and renderer | Artifact content and format-specific metadata tests. |
| Bootstrap/full runtime installers and historical loader variants | Runtime adapters | Exact existing behavior represented or explicit compatibility diagnostic. |
| Browser assistance, download-directory scans, cache, manual association, continue | Pending acquisition and resume | Identity verification and stale-state read-only behavior. |
| Add/remove/sync/update/adopt, aliases, conservative retention | Shared identity planner | No wrong-target deletion, duplicate roots, or unsupported pruning. |
| Partial progress for independent work | Batch policy and partial receipt | Combined manifest/index remains consistent; non-complete outcome. |
| Workdir, CLI/env/dotenv precedence, interactive/headless, logs and exit codes | Host adapters | CLI smoke tests against compatibility flags. |
| Lazy managed/external tools and cross-platform process behavior | Tool resolver/process port | Bounded probes, tool provenance, descendant cleanup. |
| Clean builds/cache/all | Ownership-aware clean planner | No arbitrary tree deletion or active recovery/cache lease loss. |

The baseline usage and build documents establish the current command/target surface; this table describes how to retain it, not a claim that the refactor is already implemented. [E3](https://github.com/inherent-design/empack/blob/50c121f/docs/usage.md), [E4](https://github.com/inherent-design/empack/blob/50c121f/docs/specs/build-and-distribution.md)

### 21.2 Incremental vertical migration

**Phase A: characterize and stop bypasses.** Freeze representative fixtures and add regression tests for known destructive ordering. Introduce shared canonical model and document codec without changing user-facing formats immediately.

**Phase B: establish observation and pure planning.** Build `WorkspaceSnapshot`, read sets, identity indexes, and pure request planners. Compare old/new plans in test fixtures, not by performing duplicate live effects. Seed locks from installed state where safe.

**Phase C: implement publisher independently.** Build root capabilities, staged file changes, durable journals, receipts, and recovery tests before migrating every command. All writers, old and new, must honor the same project lock and hot-journal gate.

**Phase D: migrate builds.** They exercise exact input, staging, inventory verification, and artifact publication without needing to mutate intent. Preserve targets and runtime adapters; retire filename-existence freshness checks.

**Phase E: migrate add/sync/remove together.** They must share logical-key/identity handling, partial-batch policy, and document publication. A compatibility CLI can keep old spellings while all effects use the new engine.

**Phase F: migrate initialization/import.** Prepare a complete candidate before replacement. Integrate direct files, URL content, optional requirements, layout inference, and manual acquisition into the same model.

**Phase G: migrate continuation and cleanup, then delete bypasses.** Saved state is reinterpreted through the normal preparation path. Privatize/remove old helpers that write live files or run packwiz from commands.

Do not keep a legacy path indefinitely “for edge cases.” During transition, route a feature explicitly to one implementation and test its declared guarantees. Never let two engines independently update one live project.

### 21.3 Avoid unnecessary simultaneous changes

Retain the existing manifest syntax through DTO compatibility where feasible. Introduce the lockfile and new internal types before requiring schema migration. Keep the pinned backend while restructuring the adapter. Change CLI names only where improved semantics require it, with deprecation mappings.

Optimize copying, multi-target caching, and provider concurrency after correctness boundaries work. Parallel execution is not the first deliverable. The simplest correct build executor is often sequential with bounded acquisition and shared fresh prerequisites.


## 22. Implementation order and review checklist

### 22.1 Smallest useful landing sequence

| Landing unit | Concrete types/modules | Acceptance gate |
|---|---|---|
| 1. Semantic vocabulary | IDs, requirements, placements, explicit DTO dispatch | Parse/normalize and alias/pin tests. |
| 2. Observations and plans | Snapshot/read set, identity index, requests, pure planner | Convergence and unsupported-operation tests. |
| 3. Safe acquisition | Verified quarantine, content leases, byte limits | Digest mismatch and preview isolation. |
| 4. Native staging/publication | Root handles, frozen candidate, journal, receipt | Real crash-recovery and confinement suite. |
| 5. Build projection/verification | BuildInput, inventory, artifact readers | Success-with-omission rejected; prior artifact retained. |
| 6. Engine lifecycle | Prepared/approved/verified states, driver, outcomes | Public API usage examples compile and run. |
| 7. Feature adapters | Provider/import/backend/runtime/CLI | Parity ledger fixtures and cross-platform E2E. |

Some units develop in parallel, but live mutation should not route through incomplete verifiers or stub publication guarantees.

### 22.2 Questions for every feature change

1. What new intent or representation does this feature introduce, and can the common model preserve it without lossy defaults?
2. Which pure plan describes its effects, and what exact postconditions establish completion?
3. Which narrow capability performs each effect, and can preview reach that capability?
4. Which identity and root/revision bind the operation? What happens after a concurrent edit or late worker result?
5. Can failure preserve the previous usable project/artifact? If publication began, how does restart determine reality?
6. Which existing feature combinations and reusable adapter contract suites exercise this change?

These are review questions, not another framework to implement. Keep each normative rule in one contract location and link its tests. Generate reference CLI/config/capability tables when practical instead of maintaining several conflicting prose descriptions.

### 22.3 Acceptance criteria for the redesign

The redesign is successful when adding another supported input requires normalization/resolution plus its adapter tests, **not another command-specific mutation path**. Adding a target requires projection, writing, and verification, not another interpretation of dependency identity. A caller cannot bypass staged verification merely by using a lower-level helper that happens to be public.

The goal is fewer independent correctness decisions per feature. The hard guarantees live in shared components and their tests; the adapters contribute domain-specific evidence.


## 23. Type and ownership index

| Type or family | Owner/lifetime | What it proves or represents |
|---|---|---|
| `DependencyKey` | Intent document | Logical label only; not provider or path identity. |
| `ProjectSelector` | Request/preparation | Unresolved selection syntax. |
| `ProviderProjectId`, `ProviderFileId` | Resolver/core | Canonical provider identity and project-owned file selection. |
| `ContentId`, `IntegrityEvidence` | Acquisition/core | Actual bytes and the separate evidence they matched expectations. |
| `PortableRelPath`, `InstallDestination` | Core values | Validated syntax; not native filesystem authority. |
| `ProjectIntent`, `ResolutionLock` | Immutable snapshot | User requirements and exact selected resolution. |
| `ResolvedFile`, `Placement`, `Requirements` | Resolved model | Acquisition, placement, optionality, and environment semantics. |
| `WorkspaceSnapshot`, `ReadSet` | Snapshot/preparation | Observed facts and expected source revision. |
| `Plan`, `ActionGroup` | Pure planner | Intended effects, dependencies, decisions, and postconditions. |
| `PreparedOperation` | Preparation result | Resolved plan plus retained preparation inputs; not authorization. |
| `ApprovedOperation` | Engine driver | Plan-specific authorization; consumed once. |
| `ContentLease` | Retaining operation/cache | Readable retained bytes; eviction cannot invalidate use. |
| `MutableStage` | Stage executor | Private candidate write ownership. |
| `FrozenStage` | Verifier | Quiescent candidate after writer/tool retirement. |
| `VerifiedChange` | Verification module | Candidate satisfies declared checks; only publisher consumes it. |
| `PublicationLease` | Publisher | Current project/root ownership, lock, and bounded change authority. |
| `OperationJournal` | Host-private state | Durable recovery facts, not imported permission. |
| `OperationReceipt` | Host/library consumer | What was verified and published, with source/artifact identities. |
| `OperationScope`, `WorkToken` | Driver/task | Task ownership and eligibility to integrate a result. |
| `AdmissionPermit` | Actual executing/retained work | Resource reservation until retirement or explicit transfer. |
| `OperationHandle` | Caller | Observation/cancellation only; not sole owner of active publication. |
| `PendingAcquisition` | Preparation/continuation | A missing content obligation, not an arbitrary copy destination. |

Auxiliary leaf types in signatures are deliberately small values: `SchemaVersion` is a validated schema number; `NonEmpty<T>` enforces at least one element; `GroupId`, `DecisionId`, `AcquisitionId`, `InventoryId`, and `RuntimeStepId` are operation/model-scoped identifiers; `Diagnostic` is a stable code plus structured context and source location. Native handles, credentials, and authority-bearing wrappers are not serializable by default.
