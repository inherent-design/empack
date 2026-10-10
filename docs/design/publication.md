# Publication and recovery

Contract for v0.6.0-beta. Callable types and signatures are defined in the
[publication](../../crates/empack-lib/src/engine/publication.rs), [verification](../../crates/empack-lib/src/engine/verification.rs) and [recovery API](../../crates/empack-lib/src/engine/api/recovery.rs). This page specifies their behavior and ownership.

## Journaled publication and recovery

### Location and trust boundary

Keep the **authoritative operation journal in host-private application state**, bound to native project identity and publication operation ID, rather than trusting a file supplied inside a downloaded project. The exact OS state directory is selected by the platform adapter. Project-local continuation files may be discoverable hints, but loading one does not establish approval or authorize deletion.

Candidate/preimage storage needed for recovery has explicit retention and is not ordinary evictable cache. Protect host-state permissions, bound parsing, and bind records to the actual root identity. A same-user malicious native program is outside the security boundary, but a malicious imported project should not be able to manufacture a previously approved operation.

### Publication plan

There is no arbitrary `DeleteTree(PathBuf)` action. A forced import computes an explicit managed change set. Removing obsolete managed directories happens only after file-level changes, when owned emptiness is verified.

A receipt-producing document update is subject to the same protocol. Success counters are derived from verified published changes, not incremented immediately after a backend invocation.

### Retained recovery data

Under the project publication lock, a new publication validates the preceding
journal and reclaims its committed operation's explicitly named copies before
superseding that journal. Uncommitted journals block ordinary publication. The
latest committed receipt and its recovery copies remain available until the next
publication; successful repetitions do not accumulate historical copies.

Before writing preimages or candidates, publication persists a separate bounded,
root-bound preparation descriptor naming its owned operation and files. A retry
can reclaim copies abandoned before journal intent. If the current journal names
the same operation, it takes precedence and its recovery data remains protected.
Unknown neighboring directories are ignored. Unexpected files inside an owned
operation prevent retirement and descriptor replacement; cleanup never recursively
deletes them or infers ownership from an `op-` prefix alone.

### Publisher interface

The engine reacquires the project lock, checks the root binding, hot-journal state, full relevant read set, and granted footprint, then admits publication of the verified candidate. A persisted marker or caller
boolean cannot substitute for `VerifiedFileChange`. Native publication and staging
are internal to the library; external callers use `Engine::prepare`, authorization,
and `Engine::start`. Public receipts and recovery inspection expose evidence, not
an alternate publication capability.

If the base changed, return a conflict before live mutation. Never silently publish against a new base. A new preparation may reuse retained verified content, but must re-plan and re-authorize any changed effects.

### Durable protocol

Use an explicitly versioned state machine:

```text
Prepared
  -> RecoveryDataDurable
  -> Applying(step)
  -> Applied
  -> VerifiedAfterPublication
  -> Committed
  -> RecoveryDataReclaimable
```

1. Under ownership, revalidate the base and freeze the final change list.
2. Create same-filesystem sibling candidate files for replacements. Retain originals or durable preimages required by the chosen recovery policy. Synchronize them and relevant parent directories as supported.
3. Persist the complete intended change list, before/after identities, root binding, operation identity, and recovery locations. Confirm that recovery data is durable **before** the first live change.
4. Apply each file change with expected-old checks. Synchronize the affected file/directory as required. Record completed progress durably.
5. Observe the resulting project and verify the planned postconditions. Mark the journal committed and persist a structured receipt.
6. Reclaim backups only after the committed receipt and retention policy permit it.

Crash between a file replacement and recording its progress is expected. On restart, compare the target with its before and after identities. A match to after means that step was applied; a match to before means it is pending. Neither match means external change or corruption and requires conflict handling, not blind overwrite.

A same-filesystem single-file replacement is a useful primitive. Replacing an arbitrary nonempty project directory is not a portable substitute: Rust's `rename` documentation includes cross-filesystem and destination-directory restrictions. [R13](https://doc.rust-lang.org/std/fs/fn.rename.html)

The journal gives recoverability, not all-at-once visibility across multiple files. All empack entry points check for an unfinished journal. Other tools may observe intermediate files unless they participate in coordination. Git's expected-old update discipline and SQLite's preimage-before-change ordering are references for these separate concerns, not evidence that empack inherits their transactions automatically. [R4](https://git-scm.com/docs/git-update-ref), [R8](https://sqlite.org/atomiccommit.html)

### Recovery policy

Prefer a deterministic roll-forward of a verified candidate when its source expectations and candidate data still hold. Restore before-images only where current files match the journal's own applied state; never overwrite a user's subsequent unrelated edit in the name of rollback.

Recovery reconciles file changes, not replay of arbitrary external installers. Staging subprocesses may be rerun in a new attempt before publication, but a journal should not re-execute unbounded external side effects because it lacks a completion marker.

Durability uncertainty is explicit. A file may have become visible even if directory synchronization or receipt persistence failed. Report `RecoveryRequired` with the operation ID; do not claim “nothing changed.” Filesystem/hardware assumptions and unsupported durability modes must be documented and exercised on supported platforms.

### Cancellation during publication

Before publication starts, cancellation can discard candidate work. During a bounded publication step, defer cancellation until the step reaches a recoverable durable boundary. If termination is forced, restart recovery remains authoritative.

Dropping a caller future must not drop the sole owner of an in-progress publication protocol. The engine-owned operation driver survives handle cancellation long enough to retire safely or leave a durable recoverable journal. A hard process exit remains a crash path, not an orderly cancel.

Previous successful artifacts remain until replacement is verified and ready for publication. Predictable preflight failures must not delete them. Build cleanup operates on staging by default; explicit user cleanup is a separate planned change.

### Read budgets

Source and artifact read sets retain separate limits. Verification checks the
resulting files against every applicable capture group's budget before publication.
The journal records those groups so recovery applies the same limits to prior and
replacement bytes. A larger artifact allowance cannot widen source-file limits.

## Instance activation and rollback

The instance record participates in the verified publication footprint. A release
is not activated until its selected inventory and instance state agree. Launch
checks pending recovery before trusting a completed instance record. Preserve
root identity and expected-old checks for client and server installations.

Committed publication copies are short-lived recovery storage. Retained release
records and content needed for operator rollback have separate ownership and leases;
reclaiming a journal must not evict them. Rollback is a new verified operation with
current-file conflicts, not blind restoration of every historical byte.
