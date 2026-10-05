# Accepted decisions and implementation qualifications

Reviewed 2026-10-05 against the supplied design and source baseline `50c121f`.
The user selected the batch and digest policies below on that date.

| Decision | Target |
| --- | --- |
| Release | v0.5.0-alpha.1; architecture and library contracts justify a new minor alpha |
| Default batch | `AllRequested`; publish nothing unless every requested item verifies |
| Partial batches | Explicit `ContinueIndependent` policy only; combined candidate and partial receipt |
| Weak source hashes | Permit provider compatibility with explicit weaker-integrity evidence; internal SHA-256 never upgrades that evidence |
| Documents | Use the normalized v0.5 schema; no automatic legacy-format compatibility requirement |
| Exact resolution | A separately versioned lock records selections; sync preserves them and update refreshes them |
| Backend | Keep the pinned packwiz adapter while restructuring ownership |
| Build behavior | Require satisfied intent by default; never silently sync or upgrade |
| Preview | No project, artifact, journal, tool-install or persistent-cache writes; owned temporary scratch is permitted |
| Publication | File-level expected-old checks and durable recovery; no claim of atomic multi-file visibility |
| Runtime | One host Tokio runtime; engine-owned retirement; no private runtime per backend call in the target |
| Scope of first landing | Pure portable file values and build prerequisite planning used by existing workflows |

The supplied design correctly distinguishes cancellation from retirement. Tokio
[blocking tasks](https://docs.rs/tokio/latest/tokio/task/fn.spawn_blocking.html)
cannot be aborted after starting; a
[task tracker's close](https://docs.rs/tokio-util/latest/tokio_util/task/task_tracker/struct.TaskTracker.html#method.close)
also does not prevent new tasks. Admission must therefore be enforced separately.
These are target requirements, not claims about the current synchronous adapter.

Directory-relative capabilities are appropriate, but
[`cap-std::fs::Dir`](https://docs.rs/cap-std/latest/cap_std/fs/struct.Dir.html)
is a candidate implementation rather than proof of the required no-follow and
reparse-point behavior. Native tests must establish that policy. A
[filesystem rename](https://doc.rust-lang.org/std/fs/fn.rename.html)
is not a portable transaction across an editable project tree.

Implementation qualifications:

- Private proof types are constructed only by their invariant owner. Wire DTOs
  never deserialize into approvals, verified candidates or publication leases.
- Portable paths validate syntax only. Root binding, native link checks and
  expected-old checks remain adapter responsibilities. No lexical value is a sandbox.
- The first path parser rejects backslashes instead of interpreting them differently
  across platforms. It preserves Unicode, spaces and brackets in valid components.
  A full Unicode collision index is a later projection gate, not an implicit rename.
- A lock schema, host-state layout and crash protocol must be specified and tested
  before their writers become reachable from commands. Placeholder verifiers and
  success-returning recovery stubs are prohibited.
- The supplied API signatures remain sketches until compiled implementations land.
  Dependency IDs, files, placements and root ownership must retain distinct types.
- Existing code is reusable only where it satisfies the target. Obsolete formats,
  bypasses and broken workflows are removed rather than supported indefinitely.
- There are no established-user compatibility obligations. The user explicitly
  chose ideal-state replacement on 2026-10-05; migration machinery is not a goal.

The current development version remains `0.0.0-dev`; release builds derive their
version from a tag. This work sets a release target and does not publish a release.
