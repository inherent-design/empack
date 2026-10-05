# Implementation ledger

Release target: v0.5.0-alpha.1. Documentation adopts the full target; implementation
moves through the gates below. The baseline is `50c121f`.

| Landing | State | Observable completion gate |
| --- | --- | --- |
| Target documentation and policy | Landed | C01 to C14, API sketches, implementation order and user decisions have one target |
| Pure core and first consumers | Planned | Dependency-free core; portable paths and deterministic target prerequisites used by live adapters; negative cases and live workflow tests |
| Semantic identity, requirements and codec | Pending | Typed canonical identities, multi-file placements, explicit DTO dispatch and validated schema |
| Snapshots, lock and pure plans | Pending | Raw read sets, absence checks, exact selections, convergence and alias tests |
| Read-only preparation and acquisition | Pending | Every preview preserves durable project/cache/tool/state trees |
| Native staging and publication | Pending | No live writer bypass; retained roots, freeze, private verification proof, crash/restart tests |
| Build inventories | Pending | Every source kind contributes expected representation; omission fails while prior artifacts survive |
| Engine runtime and public API | Pending | One runtime, bounded admission, late-result rejection, retained terminal results, compiled usage examples |
| Command implementation | Pending | Build, then add/sync/remove, then init/import, then continuation/clean use shared lifecycle |
| Alpha release gate | Pending | Parity ledger, native platform suites and fault injection pass; no success stubs |

Current safety primitives remain reusable: OS project locks, atomic single-document
replacement, confined workflow paths, process-tree ownership, verified downloads,
canonical provider resolution and explicit partial-failure errors. They do not
establish the target's multi-file recovery or uniform capability boundary.

Delete replaced command paths. Retain useful provider and artifact fixtures as
behavioral requirements, not as obligations to reproduce broken behavior. There
is one implementation per operation and no permanent legacy engine.
