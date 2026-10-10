# Filesystem capabilities and staging

Contract for v0.6.0-beta. Callable types and signatures are defined in the
[native roots](../../crates/empack-lib/src/engine/native.rs), [staging](../../crates/empack-lib/src/engine/staging.rs) and [managed layout](../../crates/empack-lib/src/engine/layout.rs). This page specifies their behavior and ownership.

## Filesystem capabilities and staging

### Root-specific handles

The root maps managed paths to native paths through one layout object. Relative syntax values are never joined to arbitrary caller-controlled roots inside a command handler.

A local dependency removal is a file removal with an expected observation. It never chooses `remove_dir_all` merely because the observed target happens to be a directory. Creation of directories and removal of empty owned directories are separate operations.

An installed placement and an acquisition source have separate lifetimes. Moving or
removing a placement must preserve its file while another surviving lock record
still uses that path as a local source or archive. Retained sources remain captured
read inputs, not new writes. Replacing a local source must satisfy every surviving
record's original content assertions. Replacing an archive still used for embedded
members requires verified member resolution; a placement change alone cannot
authorize it. Unreferenced user sources do not acquire deletion authority merely
because a dependency was removed.

A cache cleanup has cache-root authority, not project-root authority. The engine journal lives outside the editable project and is not included in ordinary cache cleanup. If cleanup affects an active operation, it reports retained objects rather than breaking their leases.

### Native filesystem policy

Native roots use directory capabilities with platform-specific identity, no-follow
and reparse checks. `cap-std` alone does not establish confinement. [R9](https://docs.rs/cap-std/latest/cap_std/fs/struct.Dir.html)

Required behavior:

- Open/read/create/remove relative to retained roots and validate expected file kinds without a weaker lexical fallback on I/O failure.
- Reject unsafe ancestor traversal and reparse/link transitions for managed writes. Use native handle-relative and no-follow facilities where supported.
- Publish from a same-filesystem sibling temporary file; do not emulate failed cross-device replacement with delete-then-copy.
- Preserve appropriate permissions and support Windows in-use-file failures as structured errors.
- Avoid changing a source file through a hardlink alias during staging; immutable cache bytes must not be handed to a mutating backend as writable hardlinks.

Apply captured source-exclusion rules to native directory entries before requiring
portable names or opening payloads. An ignored backup remains unowned even if its
name cannot be used in a portable pack. Included files still require portable paths;
explicit locked inputs cannot disappear behind exclusion rules. Author exclusions
come from `sources.exclude` in `empack.yml` and apply within each source layer.
No foreign index, metadata filename or ignore file changes traversal or ownership. Revalidation and recovery repeat the same captured traversal policy.

No normal user workflow should need arbitrary recursive project deletion. Root discovery can require ambient filesystem authority at the outer boundary; that authority should not leak to importers or planners.

A new project is a different publication footprint from replacing existing files.
Bind the existing parent and expected child absence during preparation. Publish a
complete verified sibling directory using a native no-replace operation, with no
fallback that can replace an occupied directory. Linux uses `RENAME_NOREPLACE`, Apple
uses `RENAME_EXCL`, and Windows uses a handle-relative rename with `ReplaceIfExists`
false. The Windows flag explicitly requires an error when the destination exists.
[Microsoft's rename contract](https://learn.microsoft.com/en-us/windows/win32/api/winbase/ns-winbase-file_rename_info)
Unsupported native/filesystem behavior is an error, not permission to delete and retry.
Windows directory capabilities that deny delete sharing must retire before the
identity-checked rename handle opens. Failed first-index writes discard private
staging only when intent never became visible. A failed sync after index publication
retains recovery bytes. Recovery of an already published root uses its native
identity journal, including when the root moved before the commit record was written.

### Staging API

The executor retires authorized workers and tool descendants before consuming
`MutableStage::freeze`. Freeze inventories candidate bytes; verification compares
that quiescent inventory with the planned changes. A writable handle or live backend process must not survive into verification.

Type ownership helps, but an independently opened native handle can bypass a wrapper. The implementation must control all stage writers and enforce backend retirement, not just consume one Rust struct.

A frozen stage retains content handles and identities. The publisher rechecks candidate content or uses retained immutable handles during publication; verification followed by an unchecked pathname reopen would create a verification/use gap.

Only managed source content is staged. `.git`, unrelated root files, and user-owned directories are not recursively copied or replaced. Include opaque installed files in observed inventory where compatibility requires preserving them.

### Why staging is not a sandbox

A trusted program launched with a stage working directory can still access other files, the network, and environment permissions. State that assumption. Supply a minimal environment, explicit arguments, and dedicated caches; do not pass live project paths unnecessarily.

The engine does not provide an OS sandbox. Source archives are data and cannot introduce executable hooks. User-authored templates may generate scripts as output, but importing a template does not execute it.
