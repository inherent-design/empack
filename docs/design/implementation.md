# Implementation ledger

Release target: v0.5.0-alpha.1. Documentation adopts the full target; implementation
moves through the gates below. The baseline is `50c121f`.

| Landing | State | Observable completion gate |
| --- | --- | --- |
| Target documentation and policy | Landed | C01 to C14, API sketches, implementation order and user decisions have one target |
| Pure core and first consumers | Landed | Dependency-free core; portable paths and deterministic target prerequisites used by live adapters; negative cases and live workflow tests |
| Semantic identity, requirements and codec | Landed | Typed canonical identities, multi-file placements, explicit DTO dispatch and validated schema |
| Snapshots, lock and pure plans | In progress | Raw read sets, absence checks, exact selections, convergence and alias tests |
| Read-only preparation and acquisition | Pending | Every preview preserves durable project/cache/tool/state trees |
| Native staging and publication | In progress | No live writer bypass; retained roots, freeze, private verification proof, crash/restart tests |
| Build inventories | In progress | Every source kind contributes expected representation; omission fails while prior artifacts survive |
| Engine runtime and public API | In progress | One runtime, bounded admission, late-result rejection, retained terminal results, compiled usage examples |
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

## Before-image restoration and committed retention

The publisher can restore the files applied by an interrupted operation. It
persists the inverse change list before restoration starts, so a second crash
resumes that direction. Restoration refuses conflicting user edits and corrupt
before-images; it can recover when an unused forward candidate is corrupt.
Receipts distinguish published and restored outcomes. File restoration can leave
new empty managed directories; it does not claim recursive tree rollback.

Journal schema 2 records retained filenames and restoration direction. Committed
recovery files can be explicitly reclaimed without deleting the receipt. Active
records refuse reclamation, and unexpected directory members are not recursively
deleted. Retention of older operations and abandoned preparations still requires
the engine's cleanup policy.

All 45 engine tests and all-feature Clippy pass locally. A separate subprocess
fixture interrupts restoration at seven durable boundaries, resumes in a fresh
process and checks original bytes, removed additions, retained receipts and
idempotent reclamation. Windows privacy and permission-capability findings from
Greptile review 25 are being addressed before the next review.

## Review corrections: persisted URLs and native permissions

The document codec rejects recognized credential-bearing query fields in both
intent and lock URLs, including percent-encoded `access_token`, cloud signatures
and client secrets. Public locators remain supported. Arbitrary opaque path/query
values cannot be classified as credentials from their spelling alone; credential
acquisition remains a separate host capability.

Native file planning carries explicit filesystem capabilities. Windows does not
observe Unix execute bits, so that unobservable bit does not prevent publication
or cause repeated changes. Desired executable intent remains in the plan and
artifact model. Native receipts state whether execute bits were verified; archive
writers and readers must still preserve and verify their format's mode metadata.
Unix continues to compare and enforce executable permissions.

Windows recovery storage validates the owner and DACL through a retained native
handle. Only the current user, SYSTEM and Administrators may receive access;
unknown grant forms and unrestricted DACLs are rejected. New roots receive a
protected inheritable DACL during creation. Existing shared roots are refused
without altering their permissions. Staging uses a private parent and an inner
private directory before copying project data. This protects against other local
principals; administrators and processes running as the same user are outside
that boundary. Native Windows tests cover shared-parent inheritance, refusal and
executable publication. Their execution evidence belongs to the Windows CI head,
not a macOS test run or cross-compilation.

Verification for this correction: 46 engine tests and the core contract suite
pass on macOS; all-target/all-feature Clippy passes. `cargo check -p empack-lib
--all-features --tests --target x86_64-pc-windows-gnu` passes, including the Windows
regressions. This compiles those regressions but does not execute them.

## Owned operation drivers

`engine::runtime::OperationRuntime` starts drivers on the existing Tokio runtime
and serializes registration against shutdown. UI handles observe and cancel work;
they do not own worker permits. A supervisor retains the task scope through driver
failure, closes admission and waits for registered async and blocking workers.
Unobserved worker panics cannot become successful completion. Preparation has a
child cancellation scope, so retiring it does not cancel authorized publication.

Owner-side acceptance checks operation, attempt and preparation state. Failed
replacement admission leaves a usable prior result valid. Task outputs keep their
memory, scratch and file reservations after releasing job capacity. Terminal
outcomes are stored before notification and remain available until explicit
completed-result eviction; the registry has a configured size bound.

The shared process supervisor now has an async entry point on the host runtime.
Bounded progress hints can be dropped when observers are slow or disconnected;
captured output remains bounded and available in the result. The synchronous
adapter still exists for commands awaiting cutover. It must be removed with those
callers, not used by the engine to create another runtime per backend call.

Nine operation tests and three async-process tests pass. The combined affected
runtime/session run passed 41 tests, with one pipe-leak warning on an existing
cancellation fixture; its isolated repeat passed without a warning. This remains
an observation to check in broader runs. All-feature Clippy passes. These generic
lifetime primitives do not yet constitute the semantic `Engine` public API or
complete command integration.

At `8353752`, native Windows CI passed the new private-ACL and executable-file
regressions. The sole Windows test failure was a Unix-assuming root-rename fixture:
cap-std deliberately opens Windows directories without `FILE_SHARE_DELETE`.
The platform-specific fixture now checks that live retention blocks the rename,
then releases the handle, replaces the root and rejects the original snapshot.
Linux and macOS tests and Linux lint/coverage passed for that head. Import-smoke
was skipped because of the Windows fixture failure and still requires a fresh run.

## Verified content leases

`engine::content::verify_stream` copies bounded input into private quarantine and
checks every declared digest, expected length and accepted prior observation.
The frozen object must still match those verified bytes. Its lease retains the
private storage, exposes independent seekable read cursors and rechecks content
when copying into another candidate. Readers do not reopen the original source.

Compatibility mode preserves MD5/SHA-1 source assertions as weak evidence.
Internally computed SHA-256/SHA-512 values support addressing and export, but never
upgrade that source assurance. Strong-source policy requires an independent strong
declaration. A source without declarations requires an explicit initial-observation
choice; recorded observations are enforced on subsequent acquisition.

Three native content tests cover independent cursors, lease retention, all-digest
matching, weak-evidence policy, size limits, cancellation and initial-observation
checks. They pass with the snapshot suite, core contracts and all-feature Clippy.
Transport policy, durable cache lookup/insertion and command integration remain
separate work; this component has no cache or project write capability.


## Read-only workspace preparation

`engine::project::ProjectReader` binds normalized documents to retained native
observations. It checks publication recovery before ordinary reads, takes an
existing shared coordination lock when available, and rechecks coordination and
raw inputs before returning. It creates no host directory, lock file, cache entry
or continuation record. Missing and malformed documents remain distinct outcomes.

A decoded prior lock now has a separate structurally validated type. Editing
intent does not discard prior exact selections needed by sync, but the prior lock
cannot become a resolved build input until it binds successfully to current
intent. Unknown schemas, malformed records and inconsistent lock structure remain
errors. No legacy-schema fallback is added.

Thirteen affected reader, codec and journal tests pass locally. They verify preview
state isolation, raw comment conflicts, stale-lock separation and shared-lock/hot-
journal behavior. Native Windows execution remains part of the CI gate.

## Expected build content

The core inventory planner projects independently enumerated input obligations
into each build target. It retains semantic owners, exact embedded content or
download references, side requirements and portable permissions. Side layers
replace common content explicitly. Optional choices retain their descriptions
and defaults; full distributions require resolved choices and acquired bytes.
An optional side override with a common fallback requires a selection before
flattening, because a single selectable file cannot preserve both alternatives.

Five new core tests cover layer precedence, deterministic ordering, optional
fallbacks, conflicting choices, duplicate destinations and full-materialization
requirements. Core tests, the dependency boundary check and all-feature Clippy
pass. Provider/source enumeration and format-specific semantic verification still
need to consume these projections; this landing alone does not complete builds.

At `f7ad34b`, the default suite passed 1,453 tests and ten doctests without a
pipe-leak warning. The 101 strict E2E tests also passed with the new pure inventory
module present but no command changes. Native CI tests passed on Linux, macOS and
Windows for `f7ad34b`; import-smoke results are tracked separately on PR #82.

## Distribution containers

ZIP, TAR.GZ and 7z now share frozen-input packaging and independent file-inventory
verification. The writer streams binary inputs, preserves portable executable and
read-only intent, retains empty directories and enforces encoded-byte limits while
writing. Verification checks actual hashes, sizes, permissions, directory/file
collisions and missing or unexpected files. ZIP central-directory records are
checked before the library builds its name index, which can hide duplicate names.

Existing distribution packaging calls this implementation; the three old writers
are removed. A private sibling replaces the previous archive only after verification
and file synchronization. Build preparation no longer deletes published archives;
explicit clean still removes them. The current preparation tree is not yet an
isolated operation candidate, and mrpack export still needs semantic verification
and the shared publisher. Container verification cannot establish that its input
tree contains every required dependency or runtime output.

The affected archive/build/provider suite passes 112 tests. New cases cover binary
round trips, cross-host executable intent, empty directories, encoded limits,
duplicate ZIP records, unsafe inputs and preservation of previous artifacts.
The 7z decoder has no configurable allocation ceiling; member limits are not a
process memory guarantee. This adapter verifies privately generated containers,
not arbitrary downloaded 7z input. Untrusted acquisition keeps its separate limits.

Greptile review 27 found that the private candidate's Unix mode became the
published archive's mode. The regression reproduced `0600` instead of ordinary
creation permissions. Publication now preserves an existing artifact's mode and
uses an empty creation probe to honor the process umask for new artifacts, without
exposing candidate bytes or changing the process-global umask. The test passes for
all three formats. At `8b6a87b`, the full default suite passed 1,466 tests and ten
doctests; strict E2E passed 101 tests. Native CI for the permission correction is
tracked separately from those results.

## Normalized mrpack planning

The mrpack planner enumerates every exact locked file slot and placement before
writing. It builds the index from normalized intent and runtime selections, retains
all three override layers, and uses verified leases for embedded inputs. References
require SHA-1, SHA-512, exact size and stable allowed download alternatives. Missing
export evidence requires acquisition; changed acquired bytes and unrelated file
associations fail preparation. Original source declarations and provenance remain
in the retained resolution, separate from hashes computed for export.

Mrpack optional participation survives export. Choice keys, descriptions and
defaults have no native fields, so that conversion requires explicit acknowledgement
and produces a conversion record. Embedded optional files need a representable
conversion or a verified reference. Files with portable executable/read-only
attributes are embedded because download references cannot express those attributes.

Five planner/format tests inspect real output archives. They cover multi-file and
multi-placement dependencies, different common/client/server bytes, weak-source
evidence, executable metadata, ambiguous paths and explicit optional conversion.
The combined mrpack/archive regression suite passes eleven tests with all-feature
Clippy. This component has no network, cache or publisher capability. Workspace
source enumeration, acquisition scheduling and CLI build cutover remain pending.

## Build read sets and artifact publication

Build capture includes declared local/archive sources outside managed namespaces,
alongside standard source/template roots and selected artifact destinations. Both document revisions and
native root identity must survive scope discovery. Captured regular files can be
copied into private leases with their original source assertions and explicit
integrity policy. Uncaptured paths and changed bytes fail.

Artifact-only verification now separates the write footprint from the broader read
set. Its candidate contains only selected distribution files. The journal retains
postconditions for unchanged inputs, including local sources outside `pack/`, but
does not grant them replacement or removal actions. A concurrent source edit blocks
publication before the artifact changes.

The combined reader, verifier and recovery suite passed twenty tests, including the
subprocess crash fixtures. Nextest reported a pipe-leak warning on an existing
reader-only test; its isolated reader-suite repeat is checked separately. All-feature
Clippy and Windows test cross-compilation passed before the final explicit-policy
parameter adjustment. Native execution remains a CI gate.

## Backend observations

Commands and the normalized backend adapter now share canonical provider and pin
decoding. Backend observations also preserve installed destination, source digest,
environment and optional defaults/descriptions. Metadata filenames remain separate
from provider identity. Malformed paths, fields and ambiguous providers fail.
These observations describe installed state; they do not prove downloaded bytes.

The wire interpretation follows the pinned
[packwiz metadata model](https://github.com/mannie-exe/packwiz-tx/blob/v0.2.0/core/mod.go).
The normalized lock also rejects a manual acquisition pin that differs from its
owning exact selection. That regression failed before the correction. Sixteen
affected backend/document tests pass; build enumeration and command cutover remain
pending.

## Layered export and selected outputs

Greptile review 29 identified two reproduced preparation failures. Downloads with
the same destination in different layers now require verified acquisition and
materialize the effective client/server bytes into separate override locations.
The mrpack index never contains competing references for one path. Optional
fallbacks still require an explicit representable choice; embedding cannot silently
make optional content mandatory.

Build capture now receives exact artifact destinations. It observes their existing
bytes or absence without recursively reading unrelated retained distributions.
Selected output before-images use a separate artifact capture budget. A retained archive larger than the input file budget no longer blocks an
unrelated build, and edits to that unrelated output do not invalidate preparation.
Both regressions failed before the fixes; the nine affected reader/mrpack tests
pass afterward. These remain engine components awaiting complete CLI integration.

Captured backend observation now reads through the retained project root and
compares each document's native identity and raw bytes against preparation. It
rejects portable destination aliases before consumers receive installed records.
A real-filesystem fixture covers changed pins after capture and two metadata files
that claim case-equivalent destinations. Seven affected reader/backend tests pass.
Tree observation remains distinct from index membership and byte verification.

## Source inclusion

Captured source enumeration includes fresh files independently of backend index
output. It preserves all three environment roots and applies captured
`.packwizignore` rules only to common pack content. The default exclusions follow
[packwiz's index rules](https://github.com/mannie-exe/packwiz-tx/blob/v0.2.0/core/index.go).
The matcher uses [ignore's Git rule parser](https://docs.rs/ignore/0.4.33/ignore/gitignore/index.html)
without reading host or global Git configuration. Rules and control documents are
not distributed as game content. Backend records are observed separately.

Tests cover default exclusions, negation, ignored-parent traversal, changed rules,
unindexed files and separate side overrides. A negated child initially exposed a
matcher/traversal difference; checking ignored ancestors fixes it. Six affected
tests pass. Enumeration currently follows a complete bounded snapshot, so ignored
file bytes still count toward capture limits. Filtered native traversal and final
locked-placement ownership reconciliation remain integration work.

## Composed mrpack preparation

`engine::build::prepare_mrpack_build` connects one captured normalized workspace
to source acquisition, semantic mrpack planning, container verification, artifact
file proof and recoverable publication. It retains the snapshot until publication
and never reruns build work during recovery. Preparation writes only private
candidates. Local sources and existing placements must agree on bytes and output
attributes; supplied acquisitions cannot replace conflicting captured content.

Backend records must match exact locked ownership, pins, requirements and byte
evidence. Unaccounted records initially stopped this entry point instead of being
omitted. The retained-observation composition below now preserves verified
unlisted content; acquisition scheduling and explicit observed-snapshot planning
remain separate work. Remote acquisition scheduling,
other targets and a combined multi-target publication remain pending.

The composed filesystem regression checks no artifact before publication, rejection
after a source edit, successful publication, fresh source inclusion without a
version bump, preservation of unrelated archives, and refusal to omit an unlisted
backend record. The combined reader/mrpack/publication suite passes 24 tests,
including subprocess crash recovery; all-feature Clippy passes.

Greptile review 30's proposed implicit `.index` stripping was checked against the
pinned backend source and an executed local HTTP/export fixture. Packwiz-tx v0.2.0
exported `mods/.index/renderer.jar`, matching the observer. The adapter now also
accepts safe relative filenames such as `../renderer.jar` while rejecting pack-root
escapes. A contract test covers both forms.

The optional-overlay finding describes a format limitation, not a lossless export
that the implementation can produce. A regression requires refusal of a selectable
client replacement with a common fallback; the mrpack contract documents why
metadata-loss acknowledgement cannot authorize that different conversion.

Seekable archive candidates now use the same protected native storage as staging.
The candidate file closes before its parent is released. Mutable-stage fields also
close the retained root before temporary-directory cleanup, which matters for
Windows handles without delete sharing. Eleven affected storage/reader/build tests,
all-feature Clippy and Windows test cross-compilation pass; native Windows execution
remains a CI gate.

At `0455614`, the full default suite passed 1,485 tests and ten doctests; strict
E2E passed 101 tests. Native CI run `37384663249` passed. A preceding full run
had four tool-probe deadline failures; their isolated repeat and the quiet full
rerun passed without changing their timeouts or assertions.

## Retained backend content

Build acquisitions now distinguish exact locked files from observed metadata paths.
Unlisted backend content is retained as an observed inventory obligation rather
than inserted into authoring intent or assigned an invented provider content type.
Its declared bytes must verify before export; missing acquisition stops preparation.
Claims that conflict with a locked identity or destination remain errors.

Observed requirements and portable attributes select references or embedded bytes.
Optional choice metadata still requires the existing explicit format conversion.
Original MD5 evidence remains separate from computed export hashes, and strong-source
policy rejects weaker declarations. Persisted reference URLs share the document
validator, including credential rejection. Captured materialized files cannot be
replaced by conflicting supplied acquisitions or emitted twice as loose overrides.

The composed fixture now retains a previously unlisted installation, checks its
mrpack reference and original MD5 declaration, and leaves both manifest and backend
record unchanged. Additional tests cover optional participation, digest mismatches,
strong-policy bypass and secret-bearing references. Twenty-five affected tests and
all-feature Clippy pass before the final explicit wrong-kind guard.

## Source and artifact budgets

Capture groups retain their own file, total-byte, depth and entry budgets through
verification, journal persistence and recovery. Build outputs default to the archive
budget; increasing that allowance does not permit larger project source files.
Candidate verification checks the complete resulting read set before publication.

Greptile review 31's large-output regression failed before the correction. Tests
now cover an existing output and a generated archive larger than the source-file
limit, smaller explicit artifact limits, interrupted publication and recovery with
an oversized source. Twenty-nine affected reader, snapshot, verifier and publication
tests pass, including the subprocess crash fixtures.

## Backend digest comparison

Composed builds retain the basis of each backend digest comparison: acquired bytes,
a same-algorithm declaration, or an independent locked reference with no comparable
backend hash. This last case keeps valid reference-only exports usable while making
the missing comparison visible. Format verification still requires exact reference
hashes, size and stable download alternatives from the lock.

Greptile review 31's cross-algorithm fixture failed before the correction. The
regression inspects the published index, checks all three evidence cases, and proves
that both a conflicting locked digest and conflicting acquired bytes preserve the
previous artifact. Nineteen affected reader/backend/mrpack tests and all-feature
Clippy pass. This does not claim remote acquisition or the CLI cutover is complete.

## Retained content with transient locators

A verified observed file whose URL cannot be persisted is embedded from its retained
bytes. The URL never enters the archive. Optional files still need a representable
reference or explicit selection; a transient URL does not authorize making them
mandatory. Greptile review 32's signed-URL fixture failed before this correction.
The regression now reads the embedded client-only bytes from the actual archive
and separately checks refusal of unrepresentable optional content.
