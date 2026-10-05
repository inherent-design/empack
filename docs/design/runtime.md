# Runtime ownership and decisions

Target contract for v0.5.0-alpha.1. Code blocks are design sketches unless the
[implementation ledger](implementation.md) identifies a compiled API.

## 15. Runtime ownership, cancellation, and resource budgets

### 15.1 One host runtime; one owner per operation

The CLI starts Tokio and constructs `Engine`. Embedders supply their existing runtime. Do not create a private runtime per backend call. Provide an optional synchronous facade only at the outermost boundary, with documented restrictions against blocking an already-running async runtime.

Each started operation has an engine-owned driver. The driver owns its cancellation tree, admitted tasks, candidate work, result state, and publication eligibility. Workers receive owned immutable requests, retained content handles, and narrow services. They do not capture `&mut Engine`, a mutable project document, or terminal objects.

```rust
pub struct OperationScope {
    operation: OperationId,
    admission: AdmissionGate,
    cancellation: CancellationToken,
    tasks: OwnedTaskSet,
    current_attempt: AttemptId,
    results: ResultRegistry,
}

pub struct WorkToken { /* operation + attempt + weak validity state */ }

pub struct TaskResult<T> {
    pub operation: OperationId,
    pub attempt: AttemptId,
    pub value: T,
}
```

The owning driver accepts a result only when its operation and attempt are still current and the relevant preparation phase remains open. A token checked by a worker is advisory: validity can change immediately afterward. The owner's acceptance check is authoritative for result integration; publication additionally requires root/revision/approval checks.

This adapts Playground's activation-lifetime idea without introducing a render loop or assuming a token grants filesystem authority. [P1](https://github.com/mannie-exe/playground/blob/dc6846a/include/runtime/ActivationLifetime.hpp), [P9](https://github.com/mannie-exe/playground/blob/dc6846a/docs/platform/RUNTIME.md)

### 15.2 Admission, cancellation, and retirement

```rust
pub struct ResourceRequest {
    pub jobs: u32,
    pub estimated_memory_bytes: u64,
    pub scratch_disk_bytes: u64,
    pub open_files: u32,
}

pub enum Admission {
    Accepted(AdmissionPermit),
    Busy { limiting: ResourceKind },
    TooLarge { limiting: ResourceKind, maximum: u64 },
    Closed,
}

pub struct AdmissionPermit { /* owned by actual task until retirement */ }
```

Admission uses checked arithmetic and accounts for concurrent retained inputs and outputs, not only task count. Reservation estimates help scheduling but cannot guarantee physical RAM usage or allocator success. Actual downloads, decompression, captured output, and file count have separate enforced bounds.

Reject an oversized request immediately; do not leave it waiting forever for capacity that can never exist. A busy request may wait with cancellation or return a retryable outcome. Do not cancel a previous usable attempt merely because a replacement failed admission.

Calling `cancel` does not release the permit. Receiving a result also does not necessarily prove worker retirement. Release only when the task and its owned scratch resources have retired or their ownership has explicitly transferred to another charged object. This is the useful contract behind Playground's separate admission and retirement concepts. [P2](https://github.com/mannie-exe/playground/blob/dc6846a/include/runtime/Executor.hpp)

Closing a scope first closes the admission gate, then requests cancellation, then awaits retirement. Serialize admission and task registration against that close. Tokio's task tracker can support waiting, but its `close()` is not itself an admission gate. [R11](https://docs.rs/tokio-util/latest/tokio_util/task/task_tracker/struct.TaskTracker.html)

Hashing/compression/parsing use bounded blocking workers. Already-started `spawn_blocking` work cannot be aborted by Tokio; implement cooperative checkpoints and bounded reads, or isolate hard-to-cancel work in an owned process where justified. [R10](https://docs.rs/tokio/latest/tokio/task/fn.spawn_blocking.html)

### 15.3 Process and publication shutdown

Shutdown phases are explicit:

```text
Close new operation admission
  -> close task admission in active scopes
  -> cancel preparatory work
  -> retire tools and workers
  -> finish or journal a recoverable publication boundary
  -> persist final outcomes
  -> release retained resources
```

`Engine::shutdown` is async and must be awaited by an orderly host. Dropping the runtime or terminating the process is handled as a crash, not guaranteed clean shutdown. No destructor attempts unbounded blocking cleanup.

The engine is still a library running ordinary owned tasks, not a daemon or a generic distributed execution framework.

### 15.4 Status is not a queue of indispensable events

```rust
pub struct OperationStatus {
    pub phase: OperationPhase,
    pub progress: ProgressSnapshot,
    pub pending_input: Option<DecisionSummary>,
    pub terminal: Option<Arc<OperationOutcome>>,
}

pub enum ProgressEvent {
    PhaseChanged(OperationPhase),
    TransferProgress(TransferProgress),
    Diagnostic(Diagnostic),
    StatusAvailable,
}
```

Keep the authoritative latest status and terminal result in an operation-owned registry. Progress events are bounded hints and may coalesce. Critical diagnostics and terminal outcomes remain in authoritative status/result storage. Store a result first, then notify observers; a dropped or full queue must not lose completion.

A cache hit follows the same result-publication order as a worker completion. Subscriber callbacks do not run while holding internal mutation locks. The baseline Playground readiness change illustrates why ordering around cached results deserves an explicit test. [P10](https://github.com/mannie-exe/playground/commit/dc6846a)

A structured log may retain detailed diagnostics with its own size/rotation policy. Do not convert every progress line into permanent operation history. Renderers can be slow or disconnected without controlling backend deadlines.


## 16. Decisions, restricted downloads, and continuation

### 16.1 Decisions are typed and revision-bound

```rust
pub enum DecisionRequirement {
    ChooseProject { id: DecisionId, candidates: NonEmpty<ProjectCandidate> },
    ChooseContentKind { id: DecisionId, allowed: NonEmpty<ContentKind> },
    ChooseDatapackFolder { id: DecisionId, proposals: Vec<LayoutProposal> },
    AcceptObservedContent { id: DecisionId, content: ContentId, provenance: Provenance },
    AcceptConversion { id: DecisionId, conversion: SemanticConversion },
    ConfirmReplacement { id: DecisionId, summary: ReplacementSummary },
    AssociateManualFile { id: DecisionId, pending: PendingAcquisition },
}
```

Every decision references an operation/attempt and the digest of the facts shown to the user. A choice made for one content hash or replacement footprint cannot be replayed for another. Noninteractive policy may answer allowed categories automatically, but unresolved destructive or lossy choices fail explicitly.

Search candidates and direct-download classification use the same decision interface. This keeps interaction outside provider and import implementations.

### 16.2 Manual acquisition model

```rust
pub struct PendingAcquisition {
    pub id: AcquisitionId,
    pub dependency: Option<DependencyKey>,
    pub file: Option<ProviderFileId>,
    pub expected: ExpectedContent,
    pub suggested_names: Vec<String>,
    pub browser_target: Option<PublicProviderPage>,
    pub destination: InstallDestination,
}

pub struct ManualCandidate {
    pub selected: UserSelectedFile,
    pub observed: ContentId,
    pub size: u64,
}
```

Discovery scans configured download roots through read-only capabilities. Hash a candidate once per stable observation and reuse the result across pending items. An unreadable unrelated file is a diagnostic, not automatic failure of the whole scan. An explicitly selected unreadable file is a failed selection.

A name, extension, recent timestamp, or unique candidate count is not identity evidence. Automatic association requires sufficient expected digest/file evidence. Without it, require a typed explicit association and retain `ObservedOnly` provenance as appropriate. A single candidate cannot satisfy multiple different expected files unless verified content evidence actually permits that equivalence.

Browser opening is an explicit host capability and preserves provider restrictions; it is not permission to bypass access controls. Automatic continuation and timeout behavior remain host policy around the same pending acquisition.

### 16.3 Continuation records

```rust
pub struct ContinuationDto {
    pub schema: SchemaVersion,
    pub operation_hint: OperationId,
    pub project_hint: ProjectInstanceId,
    pub intent_revision: SemanticRevision,
    pub pending: Vec<PendingAcquisitionDto>,
    pub retained_content: Vec<ContentId>,
}

pub enum ResumeAssessment {
    Ready(PreparedOperation),
    NeedsInput(PreparationContinuation),
    Stale { changed: Vec<ChangedInput> },
    Invalid { diagnostics: Vec<Diagnostic> },
}
```

Continuation data stores logical destinations and content identities, never authoritative arbitrary filesystem paths or executable command strings. On resume, resolve the actual project root, validate the DTO, reconstruct the request, re-observe source revision, validate retained content, and re-plan.

Inspecting stale state is read-only. Removing a stale record is an explicit cleanup publication or host-state action, not a side effect of preview.

Manual input discovered during backend staging can suspend that attempt without live project changes. Persist only a validated resumable description and retained content; release active execution permits. Resuming creates a new attempt, revalidates, and reuses the normal acquisition/stage/verify/publish path. It is not a second build implementation.
