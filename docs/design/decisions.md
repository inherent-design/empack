# Accepted decisions and implementation qualifications

| Decision | Target |
| --- | --- |
| Release | v0.5.0-alpha.1; architecture and library contracts justify a new minor alpha |
| Default batch | `AllRequested`; publish nothing unless every requested item verifies |
| Partial batches | Explicit `ContinueIndependent` policy only; combined candidate and partial receipt |
| Weak source hashes | Permit provider compatibility with explicit weaker-integrity evidence; internal SHA-256 never upgrades that evidence |
| Documents | Use the normalized v0.5 schema; no automatic legacy-format compatibility requirement |
| Exact resolution | A separately versioned lock records selections; sync preserves them and update refreshes them |
| Backend | Native metadata, acquisition and distribution assembly; preserve packwiz-compatible project metadata and installer distributions without requiring the Go executable |
| Build behavior | Require satisfied intent by default; never silently sync or upgrade |
| Preview | No project, artifact, journal, tool-install or persistent-cache writes; owned temporary scratch is permitted |
| Publication | File-level expected-old checks and durable recovery; no claim of atomic multi-file visibility |
| Runtime | One host Tokio runtime; engine-owned retirement; no private runtime per backend call |
| Cutover | Ordinary commands use the engine; retired handlers and project implementations are removed |

The supplied design correctly distinguishes cancellation from retirement. Tokio
[blocking tasks](https://docs.rs/tokio/latest/tokio/task/fn.spawn_blocking.html)
cannot be aborted after starting; a
[task tracker's close](https://docs.rs/tokio-util/latest/tokio_util/task/task_tracker/struct.TaskTracker.html#method.close)
also does not prevent new tasks. Admission must therefore be enforced separately.
The async process provider uses the host runtime; blocking filesystem/archive work
remains cooperatively cancellable.

Directory-relative capabilities are appropriate, but
[`cap-std::fs::Dir`](https://docs.rs/cap-std/latest/cap_std/fs/struct.Dir.html)
provides directory-relative I/O, but does not alone establish no-follow and
reparse-point behavior. Native tests must establish that policy. A
[filesystem rename](https://doc.rust-lang.org/std/fs/fn.rename.html)
is not a portable transaction across an editable project tree.

Implementation qualifications:

- Private proof types are constructed only by their invariant owner. Wire DTOs
  never deserialize into approvals, verified candidates or publication leases.
- Portable paths validate syntax only. Root binding, native link checks and
  expected-old checks remain adapter responsibilities. No lexical value is a sandbox.
- The path parser rejects backslashes instead of interpreting them differently
  across platforms. It preserves Unicode, spaces and brackets in valid components.
  Projection applies a Unicode collision index; it never silently renames paths.
- Lock schemas, host-state layout and the crash protocol have implementation and
  restart tests. Placeholder verifiers and success-returning recovery stubs remain
  prohibited.
- Dependency IDs, files, placements and root ownership retain distinct types.
  Callable interfaces and examples live in the Rust API documentation.
- Existing code is reusable only where it satisfies the target. Obsolete formats,
  bypasses and broken workflows are removed rather than supported indefinitely.
- Unsupported document schemas fail explicitly; automatic schema migration is not supported.

The current development version remains `0.0.0-dev`; release builds derive their
version from a tag. Publishing requires an explicit release tag.


Compatible dependency selection defaults to stable preference: choose
among compatible stable files when any exist, otherwise allow compatible beta/alpha
files. Hosts can select stable-only or any-channel policies explicitly, and exact
pins remain independent. This does not change sync's lock-retention rule.
