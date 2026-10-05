# Implementation ledger

Release target: v0.5.0-alpha.1. Documentation adopts the full target; implementation
moves through the gates below. The baseline is `50c121f`.

| Landing | State | Observable completion gate |
| --- | --- | --- |
| Target documentation and policy | Landed | C01 to C14, API sketches, implementation order and user decisions have one target |
| Pure core and first consumers | Landed | Dependency-free core; portable paths and deterministic target prerequisites used by live adapters; negative cases and live workflow tests |
| Semantic identity, requirements and codec | In progress | Typed canonical identities, multi-file placements, explicit DTO dispatch and validated schema |
| Snapshots, lock and pure plans | In progress | Raw read sets, absence checks, exact selections, convergence and alias tests |
| Read-only preparation and acquisition | Pending | Every preview preserves durable project/cache/tool/state trees |
| Native staging and publication | In progress | No live writer bypass; retained roots, freeze, private verification proof, crash/restart tests |
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

At `52a4c53`, all-feature Clippy and the full default suite passed: 1,365 tests
and ten doctests, including 23 offline smoke tests. The first core landing at
`c1386d4` also passed all 100 strict E2E tests. The prepared-import landing also passed all 100 strict E2E tests. Greptile
reviewed the subsequent documentation head `f0f3dd4` at 5/5 with no findings.

## Provider identity landing

The pure core owns provider-qualified project IDs and distinct Modrinth version
and CurseForge file IDs. Parsing preserves case and rejects whitespace, slugs,
URLs, malformed base62 values, zero, decimal padding and overflow. Syntax does
not establish existence or pin ownership; provider resolution still checks those.

Sync and removal compare typed identities. Installed metadata decodes pins in
its provider namespace, and command planning rejects invalid IDs before invoking
the backend. Human labels and observed metadata keys remain separate. Existing
wire structs still belong to the command adapter; the normalized codec now exists
and command integration remains pending.

Curated import smoke exposed another selector boundary: older Modrinth CDN paths
contain version names. The importer now treats only canonical-shaped IDs from the
provider's CDN host as identity hints. Version names remain unresolved until the
[version-from-hash endpoint](https://docs.modrinth.com/api/operations/versionfromhash/)
identifies the exact version using SHA-512 or SHA-1. If no exact provider selection
can be resolved for a downloaded file, preparation retains a verified URL dependency
with the original destination, requirements and digest declarations. It never
substitutes an unpinned provider installation. Tests reproduce both pin-loss paths,
reject changed fallback bytes and reject identity hints from unrelated hosts.

The parser follows the [Modrinth identifier contract](https://docs.modrinth.com/api/)
and [CurseForge ID fields](https://docs.curseforge.com/rest-api/). Test fixtures use
provider-shaped IDs. Negative tests retain malformed selectors and pins.

## Requirement projection landing

The core represents required, unsupported and optional participation separately
for client and server. Optional choices retain a logical key, default and
description. A uniform-format projection refuses mixed required/optional sides
and distinct choices, rather than collapsing them.

The current format adapter resolves unspecified sides to required before entering
the core. URL metadata and provider/embedded imports use the same projection;
the competing import-side matcher is removed. The current wire format cannot
express choice metadata yet, so full round trips depend on the new codec gate.

## Source digest landing

The core owns exact-width source digest values, nonempty digest sets, content
addresses and evidence data. Unknown algorithms and conflicting duplicate values
fail. Every declared digest must match; a matching stronger digest does not excuse
a mismatched weaker declaration.

URL acquisition and cached-byte verification use these values. They hash a bounded
stream once, verify every declared algorithm, and keep source declarations separate
from observed export hashes. MD5-only compatibility retains its original evidence
and emits a diagnostic explaining its weaker assurance. Internal SHA-256 addressing
does not change that evidence. Evidence data is not a publication capability.

The unknown-algorithm regression failed before this change and passes afterward.
Tests also cover digest widths, duplicate conflicts, mixed-digest mismatches and
MD5 evidence preservation. The adapter uses RustCrypto's MD5 implementation solely
for the accepted compatibility policy; the semantic core has no dependencies.

A real-backend lifecycle fixture also covers MD5-only import, repeated sync,
export, fallback downloads and offline metadata repair. It checks exported SHA-512
observations while requiring the original source declarations to remain unchanged.
The fixture exposed sync bypassing verified cache bytes during repair. Sync and
build now share cache verification; cached bytes must still match every declared
digest and their content address before reuse.

## Document value validation

Dependency decoding and publication validate provider IDs and pins, environment
representability, local file paths and SHA-256 declarations. Import preparation
also validates provider IDs, pins and destinations before forced replacement.
Malformed explicit intent cannot enter execution as an implicit search.

This hardens the existing wire adapter while it is replaced. It does not complete
the normalized schema, exact lock or lossless document editing gates. Invalid
manual-document tests bypass the writer deliberately and assert that command
failure preserves the original document and outside files.

## Sync preparation failure gate

A sync batch with any resolution or command-planning failure returns before
publishing resolved searches, URL metadata or backend changes. The regression
first reproduced a backend installation despite another failed request; it now
requires unchanged intent and pack metadata and zero backend calls. A second
regression reproduces search-intent publication before a URL digest failure. URL
verification now completes before those document writes.

This gate implements the preparation part of `AllRequested`. Runtime failures
still need the staged candidate and journaled publisher. It does not imply rollback
or atomic publication for the current command executor.

## Continuation verification

At source revision `9623a9c`, `mise run test` passed 1,381 tests and ten doctests,
including all 23 offline CLI smoke tests. The fixtures exercise alias removal,
provider-qualified pins, repeated reconciliation and native deletion confinement.
Invalid document fixtures are seeded directly, so validation in the authoring
writer does not hide command-boundary safety checks.

Focused identity, requirements, digest, codec and sync checks passed during their
respective landings. The source core remains dependency-free and `no_std`.
Final strict E2E, Clippy, CI and Greptile evidence is recorded on PR #82 for its
reviewed head. These results cover the current adapters; they do not complete
the pending engine publication and recovery gates.

## Normalized model and codec

The core now represents intent and exact resolution separately, including multiple
file slots, repeated placement of identical bytes, source evidence, provenance,
optional choices, loader variants, distribution targets and extensions. Checked
resolution rejects stale intent, missing roots, mismatched provider pins, changed
content kinds, lost requirements, discarded source declarations and duplicate
same-layer destinations. Dependency coverage remains explicit; missing edges do
not authorize removal.

`engine::documents::DocumentCodec` implements intent schema 2 and lock schema 1.
It rejects unknown fields and unsupported schemas rather than trying another
interpretation. Semantic revisions use tagged, length-delimited canonical values;
raw revisions protect comments and formatting separately. No-op edits return the
original bytes. Changed documents explicitly report reformatting, which the
operation plan must disclose. These codecs create data, not write authority.

Eight contract tests cover round trips, multiple files with identical content,
weak source evidence, mixed per-side requirements, raw-revision conflicts, URL
ordering, credential rejection and invalid explicit intent. All-target/all-feature
Clippy passes. Command cutover follows snapshot, staging and publication work;
existing live command paths have not yet been replaced by these codecs.

## Native observations and private candidates

`engine::snapshot::ProjectReadRoot` retains a directory handle. Scoped capture
walks each component without following links, rejects special files and binds
native object identities. Windows bindings use the full 128-bit file ID. File
hashes protect raw documents, directory membership protects selected trees, and
absence observations include missing ancestors. Entry, depth and byte limits
apply during capture; cancellation is checked between bounded reads.

`engine::staging::MutableStage` copies observed files into new private objects.
It does not copy unrelated root files or use writable hardlinks to project/cache
content. Failed writes retain the preceding candidate. Consuming the writer
freezes an inventory with retained read handles; copying a frozen candidate
rechecks its bytes without reopening its former pathname.

Thirteen native and streaming tests pass on macOS, alongside the eight codec
contracts and all-feature Clippy. They cover replaced roots, raw edits, changed
membership, absent ancestors, unsafe links, special files, bounded reads,
interrupted reads, short writes, hardlink isolation and post-freeze tampering.
Cross-platform CI still must verify the native implementations.

These adapters do not yet admit backend processes. Project coordination and the
hot-journal gate belong to their engine caller, and frozen inventory alone does
not authorize publication. The durable publisher, independent semantic verifier
and command cutover remain required before claiming the complete staging gate.


## File plans, inventory proofs and journaled publication

`empack-core::files::FilePlan` plans explicit file replacements and removals from
observed states. It retains unlisted files, requires observations for new targets,
rejects directory-valued file operations and converges after applying a valid
plan. This is the file-level planner; semantic request planning remains separate.

`ProjectLayout` maps logical destinations into managed namespaces. Collision
checks preserve spelling and reject canonical Unicode caseless aliases, including
file/directory prefix conflicts. `VerifiedFileChange` checks the complete selected
candidate inventory against its plan and native base. Missing, additional or
changed files, inappropriate roles and over-budget candidates fail verification.
This proof does not replace provider, normalized-document or artifact verification.

The native publisher owns a project-specific OS lock and host-private journal.
It retains synchronized before-images and verified candidates before recording
publication intent. Each replacement uses a journal-owned sibling on the target
filesystem. Recovery checks before/after content and native identities, verifies
all pending candidates and retained inputs, and rolls forward without invoking
an installer. Unknown schemas, corrupted data and external edits block recovery.
Successful publication verifies the final selected inventory and persists its
receipt. Cancellation is deferred once durable publication starts.

Root bindings include native identity and creation time to distinguish reused
object numbers after deletion. The publisher refuses filesystems that cannot
supply a durable directory creation identity. Unix receipts record directory
synchronization; Windows receipts explicitly report file synchronization only.
Those platform limits are not claims of hardware-independent power-loss safety.

Thirty-six engine tests and the core contract suite pass locally, with
all-feature Clippy. The subprocess fixture exits at nine durable boundaries,
including sibling write/sync, and repeats four boundaries during the second file
change. A fresh process recovers each recorded operation. Other fixtures check
unchanged-input conflicts, corrupt later candidates, busy locks, file-only
removal, missing/extra candidate output and preview state isolation.

This remains an engine foundation. Backend task retirement, semantic/artifact
proofs, before-image restoration, committed-data reclamation, admission ownership
and command integration are still pending. Old command paths have not acquired
these guarantees merely because the publisher compiles. Cross-platform checks
and full-suite evidence are recorded per commit on PR #82.

At `cd2e1b6`, the local default suite passed 1,423 tests and ten doctests; strict
E2E passed all 101 tests. Linux CI then exposed `EBADF` from trying to synchronize
or change permissions through an `O_PATH` directory handle. The adapter now opens
read access relative to the retained directory, checks its identity and uses that
handle for these operations. The added native regression and all 37 engine tests
pass locally, with all-feature Clippy. Linux confirmation belongs to the next CI
run; the earlier local pass did not establish that platform guarantee.

## Resource admission

`engine::resources::ResourceGovernor` reserves job, memory, scratch and open-file
estimates under one lock. Oversized requests fail immediately; busy requests can
wait with cancellation on the host runtime. Closing admission wakes waiters.
A unique `AdmissionPermit` remains charged until retirement, and splitting a
permit transfers a retained output's share without releasing it globally.

Five tests cover multidimensional and maximum-value arithmetic, failed transfers,
blocking-worker cancellation, wakeups and concurrent close/admission. They pass
with all-feature Clippy. This is scheduling accounting, not a physical-memory
ceiling. Operation-driver registration, result ownership and command integration
still must use these reservations.
