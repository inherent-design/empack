# Architecture and guarantees

Target contract for v0.5.0-alpha.1. Code blocks are design sketches unless the
[implementation ledger](implementation.md) identifies a compiled API.

## 1. Architectural decision and guarantees

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

Keep packwiz as an adapter. Keep the existing useful infrastructure: canonical resolution, provider integrations, process-tree ownership, verified acquisition, individual-document atomic replacement, and project locks. Replace the arrangements that let one caller bypass those mechanisms. The baseline already contains many of these contracts, but they are distributed across workflows. [E1](https://github.com/inherent-design/empack/blob/50c121f/docs/specs/workflow-contracts.md), [E2](https://github.com/inherent-design/empack/blob/50c121f/docs/specs/session-providers.md)

### 1.1 Normative guarantees

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

### 1.2 Deliberate exclusions

Do not introduce a daemon, message bus, generic graph executor, event-sourced business framework, universal `Entity` hierarchy, or trait for every trivial helper. Do not make the public project layout opaque merely to simplify internals. Do not rewrite the provider ecosystem and storage protocol simultaneously.

The minimum substantial implementation is a domain model, narrow ports, staged execution, verification, and a recoverable publisher. Content-addressed caching, sophisticated scheduling, and additional sandboxing can grow behind those boundaries.


## 2. Reference designs: what to adopt and what not to copy

### 2.1 Playground

Playground is particularly relevant because it separates ownership from references, preparation from installation, and cancellation from eligibility to accept a result. The inspected `ActivationLifetime` and `Executor` headers expose actual primitives: weak activation state, explicit task admission, and retirement distinct from cancellation. Its completion queue has many producers and a single draining owner. These are implementation references, not just architectural labels. [P1](https://github.com/mannie-exe/playground/blob/dc6846a/include/runtime/ActivationLifetime.hpp), [P2](https://github.com/mannie-exe/playground/blob/dc6846a/include/runtime/Executor.hpp), [P3](https://github.com/mannie-exe/playground/blob/dc6846a/include/runtime/CompletionQueue.hpp)

| Playground idea | Empack adaptation | Do not transplant |
|---|---|---|
| Activation generation and lifetime | An `AttemptId` and operation-owned acceptance gate reject results from superseded preparation. | A UI-thread queue or a claim that a token check is a filesystem lock. |
| Owned preparation inputs and immutable results | Workers receive `AcquisitionRequest`/`StageRecipe` values and return immutable observations. | Workers holding borrowed mutable project/session facades. |
| Admission versus retirement | `AdmissionPermit` is held until a task really stops using its reserved resources. | Releasing capacity merely because cancellation was requested. |
| Frozen catalog versus cached realization | `ResolvedProject` is semantic data; `ContentLease` is retained cached content; `FrozenStage` is one candidate realization. | Treating the cache index as authoritative project intent. |
| Separate package/source/resource graphs | Distinct dependency evidence, execution prerequisites, and content-retention graphs. | Assuming every graph is acyclic or using one graph to justify every deletion. |
| Store result before signaling readiness | Terminal outcomes remain retrievable even if progress notifications are dropped. | Letting a bounded progress queue be the sole record of completion. |

The assets, packages, applications, and manifests documents provide useful target contracts around exact resolution, replacement after validation, retained mounts, and inventory ownership. Some describe intended architecture rather than demonstrated end-to-end implementation. This document adapts those ideas; it does not assert that all of Playground's package system is already implemented. [P4](https://github.com/mannie-exe/playground/blob/dc6846a/docs/platform/ASSETS.md), [P5](https://github.com/mannie-exe/playground/blob/dc6846a/docs/platform/PACKAGES.md), [P6](https://github.com/mannie-exe/playground/blob/dc6846a/docs/platform/APPLICATIONS.md), [P7](https://github.com/mannie-exe/playground/blob/dc6846a/docs/platform/MANIFESTS.md)

Playground's resource accounting also makes a useful distinction between managed estimates and total physical memory. Empack should charge retained input, output, and scratch ownership consistently, while separately enforcing actual streamed-byte limits. A canceled task still owns its reservation until retirement. [P8](https://github.com/mannie-exe/playground/blob/dc6846a/docs/render/RESOURCES.md)

### 2.2 Established programs and libraries

| Reference | Concrete borrowing | Limit or rejection |
|---|---|---|
| Cargo manifest and lockfile; `PackageId` implementation | Separate user requirements from exact selected versions; keep source identity in resolved identity. [R1](https://doc.rust-lang.org/cargo/guide/cargo-toml-vs-cargo-lock.html), [R2](https://github.com/rust-lang/cargo/blob/0.89.0/src/cargo/core/package_id.rs) | Do not force provider IDs or Minecraft versions into SemVer, or depend on Cargo internals as a general package library. |
| Nix derivations | Build recipes name explicit inputs, outputs, options, and tool/runtime identities. [R3](https://nix.dev/manual/nix/2.35/store/derivation/) | No Nix language, daemon, store replacement, or claim that every installer is reproducible. |
| Git expected-old reference updates and lockfile publication | Bind writes to expected prior state; prepare a replacement before making it visible. [R4](https://git-scm.com/docs/git-update-ref), [R5](https://github.com/git/git/blob/v2.50.1/lockfile.c) | Git reference transactions do not make an arbitrary editable project tree atomic. Do not copy symlink-following defaults blindly. |
| OSTree deployment preparation | Prepare and verify the candidate while retaining the previous usable result. [R6](https://ostreedev.github.io/ostree/atomic-upgrades/) | Do not transplant Linux deployment switching into cross-platform editable project directories. OSTree's repository code itself distinguishes non-atomic multi-ref updates. [R7](https://github.com/ostreedev/ostree/blob/main/src/libostree/ostree-repo-commit.c) |
| SQLite atomic-commit reasoning and crash tests | Persist recovery information before changes; define restart behavior at every durable boundary. [R8](https://sqlite.org/atomiccommit.html) | A database transaction alone cannot roll back external subprocess effects or unrelated files. |
| `cap-std::fs::Dir` | Directory-relative filesystem authority as a useful implementation candidate. [R9](https://docs.rs/cap-std/latest/cap_std/fs/struct.Dir.html) | A validated `PathBuf`, working directory, or capability wrapper is not a subprocess sandbox. |
| Tokio task primitives | One host runtime, owned task tracking, bounded blocking work. [R10](https://docs.rs/tokio/latest/tokio/task/fn.spawn_blocking.html), [R11](https://docs.rs/tokio-util/latest/tokio_util/task/task_tracker/struct.TaskTracker.html) | `spawn_blocking` does not abort already-running work; `TaskTracker::close()` does not itself forbid further spawns. |

The combination is intentionally smaller than any complete reference system: Cargo-like resolution, Nix-like recipe description, Git-like expected-old checking, SQLite-like recovery discipline, and Playground-like result ownership.


## 3. Crates, modules, and authority

Use Rust structs, enums, modules, and traits rather than an inheritance hierarchy. “Class” responsibilities below are represented by structs with controlled constructors and implementations.

```text
crates/
  empack-core/
    identity.rs         # Keys, provider identities, versions, digests
    path.rs             # Portable relative path values, not I/O authority
    project.rs          # Intent, lock, resolved files, environments
    observation.rs      # Immutable observed facts and read sets
    request.rs          # Public operation requests and policies
    plan/               # Pure planners, expected outcomes, conflicts
    projection/         # Pure build inventory and format capability checks
    diagnostic.rs       # Domain errors with stable codes and locations

  empack-lib/
    codec/              # YAML/TOML/JSON DTOs, loss-aware patches, migrations
    engine/
      prepare.rs        # Read-only orchestration
      driver.rs         # Owns one admitted operation
      stage.rs          # Candidate workspace construction and freeze
      verify.rs         # Only constructor of VerifiedChange
      publish.rs        # Live managed changes and durable receipt
      recovery.rs       # Interrupted operation reconciliation
    ports/              # Narrow object-safe effect boundaries
    adapters/
      provider/         # Modrinth, CurseForge
      import/           # mrpack, CurseForge ZIP, future formats
      packwiz/          # Pinned managed or explicit external backend
      distribution/     # Targets, templates, installers, archive writers
      fs/               # Native root handles and atomic replacement
      network/          # Credentials, redirects, retries, quotas, streaming
    runtime/            # Scope, admission, process ownership, progress
    api.rs              # Engine, requests, results, embedding entry points

  empack/               # CLI parsing, presentation, host runtime, exit codes
  empack-testkit/       # Contract suites, controlled HTTP/process/FS fixtures
```

Allowed dependencies:

```text
empack -> empack-lib -> empack-core
                 \-> adapter libraries / Tokio / operating-system APIs
empack-testkit -> public contracts + privileged test fixtures
```

`empack-core` has no filesystem, process, networking, Tokio, terminal, or `Session` dependency. It may use value-only libraries for URLs, hashing descriptions, and validation where appropriate; it does not fetch or open anything.

Inside `empack-lib`, only the composition root can assemble all services. An importer receives an archive reader, not a publisher. A resolver receives catalog/acquisition readers, not a live project writer. A renderer sees status and receipts, not mutable engine state.

### 3.1 Proof privacy

The following types have private fields and no public `new`, `Default`, `Clone`, or `Deserialize`: `ApprovedOperation`, `MutableStage`, `FrozenStage`, `VerifiedChange`, and `PublicationLease`.

Each is constructed by the module that establishes its invariant. Other modules can consume it through the relevant method but cannot manufacture it. `VerifiedChange` lives in `engine::verify`; its constructor is private to that module. Making an unchecked constructor `pub(crate)` would undermine the boundary.

DTOs are deliberately different types. `SavedOperationDto { phase: "verified" }` is untrusted data, not a `VerifiedChange`.

Rust privacy protects the intended implementation structure, not against malicious code already running with arbitrary native I/O in the same process. Architectural linting and code review must forbid ambient I/O outside the adapter modules.
