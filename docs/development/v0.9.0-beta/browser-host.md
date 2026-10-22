# Browser host and persistence

`empack-wasm` hosts the [portable engine](drafts-and-ports.md) in one dedicated
worker. OPFS retains large content and scratch artifacts. IndexedDB stores revision
heads, logical references, operation receipts and retention records.

## Worker boundary

The main thread owns user-gesture dialogs and harness presentation. The worker owns
draft state, asset access, engine orchestration and storage. Thin JavaScript adapters
handle browser APIs; dependency, archive and recipe interpretation remains Rust.

| Envelope field | Contract |
| --- | --- |
| Protocol version | Independently versioned; reject unsupported required meaning |
| Session epoch | Invalidates handles and responses after worker restart |
| Request ID | Correlates one request and terminal response |
| Project ID / expected revision | Prevents applying edits to a different project or generation |
| Operation / attempt | Identifies long work and retained terminal outcomes |
| Idempotency key | Binds persistent operation to its input fingerprint; reuse with different inputs fails |

Large counters and offsets use decimal strings at the JSON/message boundary; Rust
checks their ranges. Do not pass WASM pointers as handles. Selected `File` objects
are structured-cloned for registration; bounded transferable buffers have explicit
ownership. Whole packs are not copied through JSON/base64. JS views into WASM memory
cannot survive growth/await without a safe copy or proven ownership contract.

Requests cover registration, analysis, edit proposals/application, save/status,
project-bundle read/write, export preparation/start, restricted input association,
cancel, capability inspection and handle release. Progress can be coalesced; final
outcomes remain queryable. Lost progress is not a lost operation receipt.

Worker restart establishes a new epoch and reopens committed state. Persisted
requests do not restore execution grants. No exclusive IndexedDB transaction or
codec handle stays open while waiting for an arbitrary user decision. Session
writer/retention leases may remain held to protect referenced work.

## Saving a captured revision

```text
retain captured inputs + register save ownership
    → write unique private OPFS backing
    → flush, close, seal and verify
    → register immutable ready objects
    → short IndexedDB transaction:
         compare expected saved head
         write revision + references + idempotent receipt
         advance head
    → await transaction completion
    → report saved revision
    → release provisional ownership
```

Do not hold an IndexedDB transaction across OPFS or network awaits. The expected
head comparison is part of the write transaction, not a separate load/check/write.
An explicit save requests strict durability where supported and reports the mode
actually used. A browser-committed receipt does not promise immunity from eviction
or device failure.

| Interruption | Reopen behavior |
| --- | --- |
| Before blob sealing | Previous saved head remains authoritative; incomplete owned scratch is recoverable |
| After sealing, before metadata commit | Previous head remains; recognized unreferenced objects can be reclaimed under collection ownership |
| During metadata transaction | Transaction outcome decides which head exists |
| After commit, before acknowledgement | Query the idempotency record; do not replay against a changed head |
| After acknowledged save, content later missing/corrupt | Preserve metadata and report missing/corrupt assets; never substitute bytes |

Saving revision 17 while editing revision 18 updates the saved-revision indicator
to 17 without clearing revision 18's dirty state. Partial-source saves identify
missing assets; invalid author text is stored unchanged.

## Ownership and collection

One writable owner holds a project Web Lock. If another context owns that project,
a caller can open read-only with shared storage retention and reader coordination,
or request explicit writer transfer. Commits still compare the saved head.
BroadcastChannel messages notify views but cannot grant ownership.

| Capability result | Permitted operations |
| --- | --- |
| Project writable | Protected reads and expected-head edits/save under project writer ownership |
| Project read-only, writer busy | Protected reads and snapshot export when all other capabilities exist; no project mutation |
| Storage coordination unavailable | No managed-content leases, project open or verified export through this store |

Read-only is a project permission, not a storage fallback. Failure to establish
shared storage ownership or backing-reader coordination reports storage unavailable.
A separately safe metadata-only diagnostic can explain the failure without reading
backing, acquiring ordinary content leases or claiming a usable project. If
coordination is lost mid-operation, stop affected reads and reconcile ownership;
do not downgrade silently and continue unprotected.

Blob users and in-progress saves hold a shared storage owner. Child operations
retain that owner's token; they never request the same Web Lock again. A session
saving while it already owns storage therefore cannot queue a nested shared request
behind a waiting collector. Retention keeps the original lock callback alive until
all child tokens retire.

The collector requests exclusive storage ownership with `ifAvailable: true`. If
unavailable, it defers with an ownership diagnostic; it does not queue an exclusive
request behind a long-lived session. It never takes a project lock, upgrades a
shared lock, steals a lock or treats elapsed time as proof that a tab is dead.
Collection cannot cross the private-write/commit-reference gap. Long sessions can
defer collection; quota diagnostics identify retained plans or sessions preventing
cleanup.

The acquisition order is:

```text
shared storage owner → project writer ownership, if needed
    → one backing-object I/O lock → short metadata transaction
```

A child inherits existing owners instead of reacquiring them. Never acquire an
earlier owner while holding a later one. Hold at most one backing I/O lock at a
time. Close its sync handle before registration; transactions never await backing
I/O, network, another lock or user input. A mapping lookup transaction completes
before acquiring an I/O lock; retained storage ownership protects the intervening
gap. Read-only contexts require storage ownership but no project writer lock.
The collector follows a separate exclusive-storage path, without project owners or
live readers. Database upgrade coordination does not acquire project locks.

Persistent roots include current saved revisions, retained history, pending work
and verified undelivered artifacts. Live roots include sessions, undo, active plans
and save ownership. Only recognized objects unreferenced by these roots can be
removed. Unknown files and timestamps do not establish deletion authority. Abandoned
save records are reconciled under exclusive ownership after proving no live owner
remains; a suspended tab is not inferred dead from elapsed time.

Database upgrades request old connections to close and surface a blocked upgrade.
Do not delete projects to force an upgrade. Store format, worker protocol, project
container and author schemas have independent version checks.

## Concurrent immutable objects

Shared storage ownership prevents collection; it does not serialize writers.
Each ingest creates a uniquely named private backing object and records its owner.
The logical content address is not its OPFS filename. A private writer closes and
releases every writable handle, then verifies the sealed bytes and length under
the original assertions before requesting registration.

A short IndexedDB transaction compares the ready mapping for the content identity.
If absent, it registers the verified backing and its retention record together. If
another ready object won, it retains that mapping and marks only the caller's
private duplicate for retirement. Transaction completion establishes registration;
no reader discovers an unregistered or partially written backing through the store.

Published backing is never reopened for mutation. Reuse verifies the selected
object against its assertions; a missing or corrupt winner produces a typed error,
not permission to overwrite it with a concurrent candidate. Replacement of damaged
storage requires a separate recovery decision and fresh verification. A lost
registration acknowledgement is resolved by querying the ingest ID and mapping.

Cleanup checks physical backing ownership and committed mapping/reference state.
An ingest failing before sealing releases only its private object. Failure after
registration cannot delete a winning object or invalidate existing leases. A
registered object without a saved project reference remains protected by live
ingest/save retention; later collection reconciles abandoned records under exclusive
storage ownership. Two logical assets sharing bytes retain distinct evidence.

Default OPFS synchronous handles exclude other handles. The baseline reader path
serializes access per physical backing across workers using its I/O lock, closes
the handle at each bounded work-unit boundary and releases the lock before yielding.
A tested read-only shared-handle mode may optimize this. Neither readers nor writers
use `readwrite-unsafe`; immutable publication never depends on concurrent writes.
The retained storage token survives between read units, so collection cannot remove
the backing while its handle is closed.

## Recoverable backing retirement

Exclusive collection ownership establishes eligibility; it does not make IndexedDB
and OPFS deletion transactional. Retire a registered backing through durable metadata:

```text
Ready(H → B), unreferenced under exclusive storage ownership
    → metadata transaction: compare ready mapping and retention roots,
      remove H → B, record Retiring(B, ownership, operation)
    → delete the exact owned backing B
    → metadata transaction: complete retirement of B
```

The retirement record retains the physical backing ID, unique storage name, original
ingest/ownership evidence and retirement operation ID until deletion is confirmed.
Names are never reused for a different backing. Remove the ready mapping and create
the record in the same transaction; never expose a ready object whose backing was
deleted by collection. No OPFS work occurs inside the transaction.

Deletion and recovery address B, not just its content hash H. A later ingest may
register H → B2 after collection releases ownership. Recovery of B must leave B2
and its references untouched. Under exclusive storage ownership, recheck that B is
not ready or retained, validate its recorded ownership, then delete or confirm its
absence. A conflicting ownership record blocks cleanup with a diagnostic. Unknown
files remain outside collection authority.

| Interruption | Recovery |
| --- | --- |
| Before retirement transaction commits | Ready mapping remains; repeat eligibility checks |
| After retirement commit, before deletion | B is unavailable for reuse; retry owned deletion using its durable record |
| After deletion, before completion | Absence of the exact B completes the same retirement idempotently |
| After completion, before acknowledgement | Status lookup returns completion; no deletion by content hash |

A deletion failure retains the retirement record and reports unreclaimed bytes.
Metadata failure after deletion retains enough intent to confirm absence on retry.
Restart reconciliation attempts pending retirements when exclusive ownership is
available; a busy session defers cleanup explicitly instead of stranding a broken
ready entry or inferring that the session died.

Private abandoned writers and duplicate candidates use the same backing-identity
retirement record after proving their owner is no longer live. Registration losers
mark only their private backing for retirement; their cleanup never removes the
winning ready mapping. The ingest ownership record precedes creation, so failure
before sealing still leaves recognized, recoverable storage. This protocol adds no
second mutation engine and does not authorize reclaiming unrecognized OPFS files.

## Failure and resource behavior

Request persistent storage where available, report refusal without claiming it is
fatal to all authoring, and expose quota estimates as estimates. An out-of-quota
save leaves the last committed revision accessible. A failed backup or export keeps
the editable draft. Origin storage is not an external backup.

Account for OPFS objects, scratch, retained output, metadata, decoded bytes and
outstanding buffers. Do not carry the native 2 GiB admission default into the
browser unchanged. The [minimum profiles](feasibility.md) fix workload and timing
requirements before measurement. Adapters publish measured results and admission
limits, rejecting oversized operations before avoidable work. Quota can still fail
after a successful estimate.

A cancelled operation retires owned work and releases only its unreferenced content.
Forced worker termination produces restart/reconciliation, not a fabricated cancel
receipt. Background suspension resumes with fresh capability/ownership checks and
without silently accepting expired approval or stale edit proposals.

## Checkpoint acceptance

Use actual IndexedDB/OPFS through the harness. Exercise invalid/valid text saves,
reopen, permission/missing/corrupt assets, before/after-commit interruption and a lost
save acknowledgement. Retry the same idempotency key and reject changed inputs.
Terminate the worker during hashing and saving, then verify prior committed state.

Open two tabs, transfer writer ownership, submit a stale expected head, block a
database upgrade, and race collection against a save. Prove the collector cannot
delete content protected by active plans, undo or verified artifacts. Test quota
failure through deterministic fault injection and actual constrained storage where
available; report which mechanism supplied each result.

Run the workflow on all required desktop and physical mobile browsers. Background
and resume behavior is part of mobile acceptance. Node mocks and desktop mobile
emulation supplement these checks but do not replace them.

Hold a live session's shared storage owner, request collection and start a save.
Collection must defer and the save must progress without closing the workspace.
Assert that the save inherits ownership rather than submitting a nested lock request.

Concurrently ingest identical bytes from two different projects while a reader and
collector run. Inject one failure before sealing and another after registration,
including lost acknowledgement. Readers must never see partial replacement; cleanup
must never delete the winner. Test serialized reader handles and any optional
shared-read optimization separately on each supported browser.

Terminate collection before and after retirement metadata commit, backing deletion
and completion, including lost acknowledgement and injected deletion/metadata errors.
Reopen, ingest the same content as B2, and resume retirement of B. Verify the new
mapping remains readable, the retired ready mapping stays absent, and the exact
owned backing is eventually reclaimed. Repeat for a failed private writer and a
duplicate candidate; failure never deletes the registered winner.

Test writer-busy and unavailable-storage-coordinator cases separately. A read-only
tab retains storage and reader ownership while exporting a snapshot. An unavailable
coordinator issues no ordinary content lease and cannot take that export path;
metadata-only diagnostics are labeled separately.

Project-container operations reuse checkpoint 3's cooperative ZIP implementation.
Complete native bundle → browser edit/save/reopen → bundle → native reopen using
actual worker storage and the native project commands. Check exact raw documents,
asset evidence, unresolved decisions and discovery/coverage after reopening. Include
an invalid-text partial backup and a complete-source valid fixture. Storage-only
tests and the earlier temporary ZIP adapter do not satisfy this gate.

This checkpoint establishes durable browser project round-trips. Consumer artifact
construction and destination delivery belong to checkpoint 5. Checkpoint 6 repeats
storage, round-trip and delivery together against the packaged release candidate;
earlier device results remain evidence for their tested revisions only.
