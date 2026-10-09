# Runtime ownership and decisions

Contract for v0.5.0-alpha.1. Callable types and signatures are defined in the
[operation runtime](../../crates/empack-lib/src/engine/runtime.rs), [resource admission](../../crates/empack-lib/src/engine/resources.rs) and [process supervision](../../crates/empack-lib/src/application/process_runtime.rs). This page specifies their behavior and ownership.

## Runtime ownership, cancellation, and resource budgets

### One host runtime; one owner per operation

The CLI starts Tokio and constructs `Engine`. Embedders supply their existing runtime. Do not create a private runtime per backend call. Library entry points are asynchronous; embedders retain ownership of runtime startup and shutdown.

Each started operation has an engine-owned driver. The driver owns its cancellation tree, admitted tasks, candidate work, result state, and publication eligibility. Workers receive owned immutable requests, retained content handles, and narrow services. They do not capture `&mut Engine`, a mutable project document, or terminal objects.

The owning driver accepts a result only when its operation and attempt are still current and the relevant preparation phase remains open. A token checked by a worker is advisory: validity can change immediately afterward. The owner's acceptance check is authoritative for result integration; publication additionally requires root/revision/approval checks.

### Admission, cancellation, and retirement

Admission uses checked arithmetic and accounts for concurrent retained inputs and outputs, not only task count. Reservation estimates help scheduling but cannot guarantee physical RAM usage or allocator success. Actual downloads, decompression, captured output, and file count have separate enforced bounds.

After a parser retires, its retained result may release unused allowance based on the
observed manifest size and entry count. This transfer can only shrink the reservation.
Import inspection keeps its configured parsing bounds, then charges the retained model
for the inspected input so extraction does not also carry unused parser capacity.

Continuation admission follows the observed record and encoded model size, independently
of the maximum permitted document size. Loading first measures the native record,
reserves its decoding allowance, and bounds the read to that observation; growth
requires a fresh attempt. Encoding estimates include variable metadata, file roles,
placements, locators and extensions before allocating wire representations. Small
records must remain usable under small budgets. Limits and identity checks still
apply, and retained decoded records keep their reservations until released.

Reject an oversized request immediately; do not leave it waiting forever for capacity that can never exist. A busy request may wait with cancellation or return a retryable outcome. Do not cancel a previous usable attempt merely because a replacement failed admission.

Calling `cancel` does not release the permit. Receiving a result also does not necessarily prove worker retirement. Release only when the task and its owned scratch resources have retired or their ownership has explicitly transferred to another charged object.

Closing a scope first closes the admission gate, then requests cancellation, then awaits retirement. Serialize admission and task registration against that close. Tokio's task tracker can support waiting, but its `close()` is not itself an admission gate. [R11](https://docs.rs/tokio-util/latest/tokio_util/task/task_tracker/struct.TaskTracker.html)

Hashing/compression/parsing use bounded blocking workers. Already-started `spawn_blocking` work cannot be aborted by Tokio; implement cooperative checkpoints and bounded reads, or isolate hard-to-cancel work in an owned process where justified. [R10](https://docs.rs/tokio/latest/tokio/task/fn.spawn_blocking.html)

### Process and publication shutdown

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

### Status is not a queue of indispensable events

Keep the authoritative latest status and terminal result in an operation-owned registry. Progress events are bounded hints and may coalesce. Critical diagnostics and terminal outcomes remain in authoritative status/result storage. Store a result first, then notify observers; a dropped or full queue must not lose completion.

A cache hit follows the same result-publication order as a worker completion. Subscriber callbacks do not run while holding internal mutation locks. Test cached and worker-produced results against the same ordering contract.

A structured log may retain detailed diagnostics with its own size/rotation policy. Do not convert every progress line into permanent operation history. Renderers can be slow or disconnected without controlling backend deadlines.

## Decisions, restricted downloads, and continuation

### Decisions are typed and revision-bound

Every decision references an operation/attempt and the digest of the facts shown to the user. A choice made for one content hash or replacement footprint cannot be replayed for another. Noninteractive policy may answer allowed categories automatically, but unresolved destructive or lossy choices fail explicitly.

Search candidates and direct-download classification use the same decision interface. This keeps interaction outside provider and import implementations.

### Manual acquisition model

Discovery scans configured download roots through read-only capabilities. Hash a candidate once per stable observation and reuse the result across pending items. An unreadable unrelated file is a diagnostic, not automatic failure of the whole scan. An explicitly selected unreadable file is a failed selection.

A name, extension, recent timestamp, or unique candidate count is not identity evidence. Automatic association requires sufficient expected digest/file evidence. Without it, require a typed explicit association and retain `ObservedOnly` provenance as appropriate. A single candidate cannot satisfy multiple different expected files unless verified content evidence actually permits that equivalence.

Browser opening is an explicit host capability and preserves provider restrictions; it is not permission to bypass access controls. Automatic continuation and timeout behavior remain host policy around the same pending acquisition.

### Continuation records

Continuation data stores logical destinations and content identities, never authoritative arbitrary filesystem paths or executable command strings. On resume, resolve the actual project root, validate the DTO, reconstruct the request, re-observe source revision, validate retained content, and re-plan.

Inspecting stale state is read-only. Removing a stale record is an explicit cleanup publication or host-state action, not a side effect of preview.

Manual input discovered during backend staging can suspend that attempt without live project changes. Persist only a validated resumable description and retained content; release active execution permits. Resuming creates a new attempt, revalidates, and reuses the normal acquisition/stage/verify/publish path. It is not a second build implementation.
