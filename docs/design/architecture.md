# Architecture and guarantees

Contract for v0.5.0-alpha.1. Linked Rust definitions and compiled examples
specify callable interfaces; this page describes behavior and ownership.

## Architectural decision and guarantees

Build a **modular monolith with a pure semantic core and one controlled mutation lifecycle**:

```text
CLI / embedding application
        |
        v
Request + policy + decisions
        |
        v
Normalize / resolve / inspect
        |
        v
Pure plan against a revision
        |
        v
Execute in private staging
        |
        v
Freeze and independently verify
        |
        v
Publish under ownership, with recovery journal
        |
        v
Receipt describing what actually became durable
```

The command layer describes an operation. It does not own installation, configuration publication, file deletion, or subprocess execution. A new input form must enter the same operation model as an existing input form.

The native engine owns provider resolution, metadata, acquisition, staging and
distribution assembly. Packwiz-compatible metadata and Java installer distributions
remain supported without the Go executable. Command hosts translate user choices;
they cannot replace verification or publish directly.

### Normative guarantees

| ID | Contract | Primary owner | Evidence required for success |
|---|---|---|---|
| C01 | A preview cannot mutate project files, artifacts, durable operation state, or persistent caches. | Preparation capabilities | Before/after filesystem and subprocess tests for every request variant. |
| C02 | A user selector, logical label, canonical identity, backend filename, and byte identity are never interchangeable. | Core identity model | Equivalent-selector and alias round-trip tests. |
| C03 | No live-project replacement begins before required semantic validation and acquisition succeed. | Preparation, staging, publisher | Forced-import failure preserves original files. |
| C04 | An effect is successful only when its domain postconditions hold. | Independent verifiers | Adversarial backend returning exit 0 with missing/wrong output fails. |
| C05 | Only explicitly authorized, managed destinations may be changed. | Change plan, root capability, publisher | Directory, symlink, path-collision, and wrong-root fixtures. |
| C06 | A plan is applied only to the project instance and revision it was prepared against. | Snapshotter, publisher | Conflict checks include relevant content and absent destinations. |
| C07 | Every required item in a build projection is represented correctly in the artifact. | Inventory verifier | Provider, URL, embedded, local, runtime, and unlisted-observed content tests. |
| C08 | A record claiming verification, approval, or completion does not grant authority merely because it deserializes. | Codec and resume boundary | Crafted journal/continuation input cannot authorize extra effects. |
| C09 | Cancellation requests, permission to accept results, and actual task retirement are different states. | Operation runtime | Late-result rejection and retained-budget tests. |
| C10 | Interrupted publication is detectable and recoverable; multi-file visibility is not falsely described as atomic. | Publisher and recovery | Crash after every durable step, then restart. |
| C11 | Partial success is explicit and non-complete; unknown dependency evidence does not authorize pruning. | Planner and outcomes | Partial batches preserve failed groups and required content. |
| C12 | A source digest is checked, not silently replaced by a digest computed from unexpected bytes. | Acquisition verifier | Wrong-body/matching-name test fails before publication. |
| C13 | Import and export preserve representable environment, optionality, destination, and layer semantics. | Normalized model and projections | Semantic round trips, not only parser assertions. |
| C14 | Resource admission is bounded, and estimates are not advertised as a hard physical-memory ceiling. | Runtime admission and stream limits | Oversized/chunked input and cancellation-retirement tests. |

The preview guarantee concerns application-controlled writes, not incidental operating-system access-time changes caused by reads. It does forbid deliberate cache-index/LRU updates, durable preparation records, tool installation, and project/artifact writes.

No architecture makes arbitrary external programs trustworthy or provides perfect isolation from an external editor that ignores locks. The guarantees here apply to empack's authorized operations on supported filesystems, with separately stated assumptions for tools and concurrent third-party mutation.

### Deliberate exclusions

Do not introduce a daemon, message bus, generic graph executor, event-sourced business framework, universal `Entity` hierarchy, or trait for every trivial helper. Do not make the public project layout opaque merely to simplify internals. Do not rewrite the provider ecosystem and storage protocol simultaneously.

## Crates, modules, and authority

| Module | Responsibility |
| --- | --- |
| `empack-core/src/` | Typed identity, paths, model, dependency planning, projections and inventories |
| `empack-lib/src/engine/api/` | Typed preparation, plan-bound approval, execution and outcomes |
| `engine/documents/`, `project/`, `snapshot/` | Strict documents, captured inputs and read-set validation |
| `engine/providers/`, `import/`, `acquisition/`, `content/` | Canonical selection, normalized imports, verified acquisition and retained bytes |
| `engine/addition/`, `removal/`, `synchronization/` | Dependency candidates and exact ownership |
| `engine/build/`, `artifacts/`, `server_runtime/` | Distribution assembly, independent artifact checks and runtime preparation |
| `engine/staging/`, `verification/`, `publication/` | Private candidates, proof construction and recoverable live changes |
| `engine/runtime.rs`, `resources.rs` | Operation ownership, admission and retained outcomes |
| `application/engine_host/` | CLI decisions and composition of engine capabilities |
| `application/process_runtime.rs` | Async child-tree supervision and bounded output |
| `empack/src/main.rs` | Host Tokio runtime, signal handling and process exit |
| `empack-tests/` | Executable fixtures, smoke and cross-command contracts |

Paths beginning with `engine/` are relative to `crates/empack-lib/src/`.

`empack-core` has no filesystem, process, networking, Tokio, terminal, or `Session` dependency. It may use value-only libraries for URLs, hashing descriptions, and validation where appropriate; it does not fetch or open anything.

Inside `empack-lib`, only the composition root can assemble all services. An importer receives an archive reader, not a publisher. A resolver receives catalog/acquisition readers, not a live project writer. A renderer sees status and receipts, not mutable engine state.

### Proof privacy

`ApprovedOperation` binds authorization to an engine and exact plan. `MutableStage`
and `FrozenStage` separate writer ownership from a quiescent candidate.
`VerifiedFileChange` has private proof state and is constructed by the verification
module only after comparing a frozen candidate with its plan. The publisher consumes
that proof and revalidates native ownership and expected-old state.

Wire DTOs cannot deserialize into any of these proofs. A saved phase label claiming
verification is recovery input, not a verified change or publication authority.

Rust privacy protects the intended implementation structure, not against malicious code already running with arbitrary native I/O in the same process. Architectural linting and code review must forbid ambient I/O outside the adapter modules.

## Operational limits

- Resource reservations are scheduling estimates, not a physical-memory sandbox.
- Blocking work cooperates with cancellation. Batch assembly is sequential;
  owned subprocesses run asynchronously on the host runtime.
- Multi-file publication is recoverable, not simultaneously visible to external
  readers. Windows reports file synchronization rather than Unix directory synchronization.
- Unknown dependency evidence does not authorize automatic pruning. Explicit
  acknowledged removal remains distinct from inferred cleanup; known dependents are binding.
- Verified caches are disposable. Missing or invalid cache evidence cannot replace
  a source assertion or authorize a project mutation.
- Untrusted input uses the bounded ZIP import path. 7z output verification reads
  privately generated candidates; downloaded 7z input is unsupported.
- Archive and server-launch probes do not establish gameplay correctness.
