# Implementation ledger

Release target: v0.5.0-alpha.1. Documentation adopts the full target; implementation
moves through the gates below. The baseline is `50c121f`.

| Landing | State | Observable completion gate |
| --- | --- | --- |
| Target documentation and policy | Landed | C01 to C14, API sketches, implementation order and user decisions have one target |
| Pure core and first consumers | Landed | Dependency-free core; portable paths and deterministic target prerequisites used by live adapters; negative cases and live workflow tests |
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

## First core landing

`empack-core` is `no_std` with allocation support and no dependencies. It owns
`PortableRelPath`, `InstallDestination`, `ArtifactStem`, `PathSyntax`, `PathError`
and pure build-target prerequisite planning. Private path fields prevent unchecked
construction. Native root authority and Unicode collision indexing are not supplied
by these syntax types.

Live artifact naming, URL-file validation, tracked local paths and import
extraction use the shared parser. Import checks override destinations before
initialization. Build execution calls the core prerequisite planner through a
format adapter; the duplicate algorithm is removed.

Verification: `cargo test -p empack-core` passes three contract tests and a
compile-fail doctest. Target-order properties enumerate every request up to five
targets. The pre-fix reserved-device regression fails on `CON`; the new parser
rejects it on every host. The affected import/build/path/URL suite passed 178 tests.
`python3 scripts/check-core-boundary.py` and all-feature Clippy passed. The full
default suite passed 1,363 tests and nine doctests, including offline CLI smoke.
The expanded import preflight regression preserves original intent and makes no
backend call on a reserved destination.

## Import preparation boundary

`prepare_import` now returns a private-field `PreparedImport` consumed by its
execution method. Production command code prepares before forced reset. The old
combined helper is available only to test fixtures. Preparation checks override
and embedded destinations, environment representability and provider-free source
bytes. An owned, bounded temporary copy retains the archive independently of the
original path; actual entry limits, CRCs and referenced entry existence are checked
before replacement starts.

The command-level regression reproduced removal of the original manifest after
an invalid override was rejected. It now preserves intent, pack configuration,
side overrides, previous artifacts and user templates. A second regression checks
corrupt ZIP content. Both pass, alongside the 108 affected command/import tests.

This boundary is not `VerifiedChange`: provider/backend execution and multi-file
publication still need staging and the journaled publisher. It does not establish
all of C03 or make the old materializer transactional. The remaining engine work
must replace that live materializer rather than adding a compatibility branch.
