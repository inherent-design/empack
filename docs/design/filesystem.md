# Filesystem capabilities and staging

Target contract for v0.5.0-alpha.1. Code blocks are design sketches unless the
[implementation ledger](implementation.md) identifies a compiled API.

## 11. Filesystem capabilities and staging

### 11.1 Root-specific handles

```rust
pub struct ProjectReadRoot { /* native directory-relative handle */ }
pub struct StageWriteRoot { /* private candidate directory authority */ }
pub struct FrozenStageRoot { /* read authority only */ }
pub struct PublicationLease { /* project lock + bounded managed write authority */ }

pub enum ManagedPath {
    IntentDocument,
    LockDocument,
    BackendDocument(PortableRelPath),
    Content { layer: ContentLayer, path: PortableRelPath },
    UserTemplate(PortableRelPath),
    Artifact(PortableRelPath),
}

pub struct ManagedRemoval {
    pub path: ManagedPath,
    pub expected: FileObservation,
}
```

The root maps managed paths to native paths through one layout object. Relative syntax values are never joined to arbitrary caller-controlled roots inside a command handler.

A local dependency removal is a file removal with an expected observation. It never chooses `remove_dir_all` merely because the observed target happens to be a directory. Creation of directories and removal of empty owned directories are separate operations.

A cache cleanup has cache-root authority, not project-root authority. The engine journal lives outside the editable project and is not included in ordinary cache cleanup. If cleanup affects an active operation, it reports retained objects rather than breaking their leases.

### 11.2 Native filesystem policy

Wrap and test an existing directory-capability implementation where feasible instead of reimplementing path resolution from strings. `cap-std` is a candidate, not a substitute for defining the contract. [R9](https://docs.rs/cap-std/latest/cap_std/fs/struct.Dir.html)

Required behavior:

- Open/read/create/remove relative to retained roots and validate expected file kinds without a weaker lexical fallback on I/O failure.
- Reject unsafe ancestor traversal and reparse/link transitions for managed writes. Use native handle-relative and no-follow facilities where supported.
- Publish from a same-filesystem sibling temporary file; do not emulate failed cross-device replacement with delete-then-copy.
- Preserve appropriate permissions and support Windows in-use-file failures as structured errors.
- Avoid changing a source file through a hardlink alias during staging; immutable cache bytes must not be handed to a mutating backend as writable hardlinks.

Apply captured source-exclusion rules to native directory entries before requiring
portable names or opening payloads. An ignored backup remains unowned even if its
name cannot be used in a portable pack. Included files still require portable paths;
explicit locked inputs and backend control documents cannot disappear behind ignore
rules. Revalidation and recovery repeat the same captured traversal policy.

No normal user workflow should need arbitrary recursive project deletion. Root discovery can require ambient filesystem authority at the outer boundary; that authority should not leak to importers or planners.

### 11.3 Staging API

```rust
pub struct MutableStage { /* private writer owner and task/process registry */ }
pub struct FrozenStage { /* private immutable candidate and observations */ }

impl MutableStage {
    pub fn writer(&mut self) -> StageWriter<'_>;

    pub async fn freeze(
        self,
        context: &ExecutionContext,
    ) -> Result<FrozenStage, StageError>;
}

pub trait StageExecutor: Send + Sync {
    fn execute<'a>(
        &'a self,
        approved: &'a ApprovedOperation,
        stage: MutableStage,
        context: &'a ExecutionContext,
    ) -> PortFuture<'a, StagedExecution, StageError>;
}
```

`freeze` closes admission of new stage writers, waits for all authorized writers and tool descendants to retire, synchronizes required candidate files, and inventories actual content. A writable handle or live backend process must not survive into verification.

Type ownership helps, but an independently opened native handle can bypass a wrapper. The implementation must control all stage writers and enforce backend retirement, not just consume one Rust struct.

A frozen stage retains content handles and identities. The publisher rechecks candidate content or uses retained immutable handles during publication; verification followed by an unchecked pathname reopen would create a verification/use gap.

Only managed source content is staged. `.git`, unrelated root files, and user-owned directories are not recursively copied or replaced. Include opaque installed files in observed inventory where compatibility requires preserving them.

### 11.4 Why staging is not a sandbox

A trusted program launched with a stage working directory can still access other files, the network, and environment permissions. State that assumption. Supply a minimal environment, explicit arguments, and dedicated caches; do not pass live project paths unnecessarily.

A future OS sandbox can be a separate `ToolIsolation` policy. The minimum portable design does not claim it already exists. Source archives are data and cannot introduce executable hooks. User-authored templates may generate scripts as output, but importing a template does not execute it.
