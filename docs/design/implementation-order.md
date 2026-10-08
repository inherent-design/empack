# Implementation sequence and ownership

Target contract for v0.5.0-alpha.1. Code blocks are design sketches unless the
[implementation ledger](implementation.md) identifies a compiled API.

## 21. Implementation sequence

Implement the ideal state directly. Existing users do not require compatibility
adapters, dual schemas, deprecation cycles or automatic project migration.
Existing source is useful only where it satisfies the new contract; Git history
preserves removed behavior.

1. Establish pure semantic values, normalized documents and exact resolution.
2. Capture observations and read sets; make requests produce pure plans.
3. Implement native roots, private staging, verification and the publisher with
   durable journals and restart tests before exposing new live mutation paths.
4. Build exact projections and independently inspect every resulting artifact.
5. Wire engine-owned operations, bounded admission, decisions and retained outcomes.
6. Route commands through the engine, preserving useful provider and distribution
   capabilities. Delete each replaced orchestration path.
7. Implement continuation and scoped cleanup through the same contracts.

The [feature requirements](parity.md) describe intended capabilities. The
[implementation ledger](implementation.md) records delivered behavior, acceptance
evidence and any remaining work. It does not promise backward compatibility. New projects use the normalized schema; legacy
projects can be re-created or explicitly imported when an import adapter supports
them. Do not silently accept malformed old documents as new intent.

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
