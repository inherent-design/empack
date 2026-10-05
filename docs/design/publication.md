# Publication and recovery

Target contract for v0.5.0-alpha.1. Code blocks are design sketches unless the
[implementation ledger](implementation.md) identifies a compiled API.

## 14. Journaled publication and recovery

### 14.1 Location and trust boundary

Keep the **authoritative operation journal in host-private application state**, keyed by `ProjectInstanceId` and `OperationId`, rather than trusting a file supplied inside a downloaded project. The exact OS state directory is selected by the platform adapter. Project-local continuation files may be discoverable hints, but loading one does not establish approval or authorize deletion.

Candidate/preimage storage needed for recovery has explicit retention and is not ordinary evictable cache. Protect host-state permissions, bound parsing, and bind records to the actual root identity. A same-user malicious native program is outside the security boundary, but a malicious imported project should not be able to manufacture a previously approved operation.

### 14.2 Publication plan

```rust
pub struct PublicationPlan {
    pub operation: OperationId,
    pub base: SourceRevision,
    pub changes: Vec<FileChange>,
    pub expected_after: ExpectedProjectDigest,
}

pub enum FileChange {
    Replace {
        target: ManagedPath,
        before: ExpectedOldFile,
        after: CandidateFileRef,
        permissions: PermissionPolicy,
    },
    Remove {
        target: ManagedPath,
        before: FileObservation,
    },
    EnsureDirectory {
        target: ManagedDirectory,
    },
    RemoveEmptyOwnedDirectory {
        target: ManagedDirectory,
        expected_members: MembershipDigest,
    },
}

pub enum ExpectedOldFile { Absent, Exact(FileObservation) }
```

There is no arbitrary `DeleteTree(PathBuf)` action. A forced import computes an explicit managed change set. Removing obsolete managed directories happens only after file-level changes, when owned emptiness is verified.

A receipt-producing document update is subject to the same protocol. Success counters are derived from verified published changes, not incremented immediately after a backend invocation.

### 14.3 Publisher interface

```rust
pub struct Publisher { /* private filesystem and journal dependencies */ }

impl Publisher {
    pub async fn publish(
        &self,
        change: VerifiedChange,
        lease: PublicationLease,
        context: &PublicationContext,
    ) -> Result<PublicationResult, PublicationError>;
}

pub enum PublicationResult {
    Committed(PublicationReceipt),
    Incomplete(RecoveryRequired),
}
```

The engine reacquires the project lock, checks the root binding, hot-journal state, full relevant read set, and granted footprint, then obtains a `PublicationLease`. `publish` consumes the verified candidate. A persisted marker or caller boolean cannot substitute for `VerifiedChange`.

If the base changed, return a conflict before live mutation. Never silently publish against a new base. A new preparation may reuse retained verified content, but must re-plan and re-authorize any changed effects.

### 14.4 Durable protocol

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

### 14.5 Recovery policy

```rust
pub enum RecoveryDecision {
    RollForward,
    RestoreBeforeImages,
    InspectConflict,
}

pub struct RecoveryReport {
    pub operation: OperationId,
    pub recognized_after: Vec<ManagedPath>,
    pub recognized_before: Vec<ManagedPath>,
    pub conflicted: Vec<PathConflict>,
    pub available: Vec<RecoveryDecision>,
}
```

Prefer a deterministic roll-forward of a verified candidate when its source expectations and candidate data still hold. Restore before-images only where current files match the journal's own applied state; never overwrite a user's subsequent unrelated edit in the name of rollback.

Recovery reconciles file changes, not replay of arbitrary external installers. Staging subprocesses may be rerun in a new attempt before publication, but a journal should not re-execute unbounded external side effects because it lacks a completion marker.

Durability uncertainty is explicit. A file may have become visible even if directory synchronization or receipt persistence failed. Report `RecoveryRequired` with the operation ID; do not claim “nothing changed.” Filesystem/hardware assumptions and unsupported durability modes must be documented and exercised on supported platforms.

### 14.6 Cancellation during publication

Before publication starts, cancellation can discard candidate work. During a bounded publication step, defer cancellation until the step reaches a recoverable durable boundary. If termination is forced, restart recovery remains authoritative.

Dropping a caller future must not drop the sole owner of an in-progress publication protocol. The engine-owned operation driver survives handle cancellation long enough to retire safely or leave a durable recoverable journal. A hard process exit remains a crash path, not an orderly cancel.

Previous successful artifacts remain until replacement is verified and ready for publication. Predictable preflight failures must not delete them. Build cleanup operates on staging by default; explicit user cleanup is a separate planned change.

### Read budgets

Source and artifact read sets retain separate limits. Verification checks the
resulting files against every applicable capture group's budget before publication.
The journal records those groups so recovery applies the same limits to prior and
replacement bytes. A larger artifact allowance cannot widen source-file limits.
