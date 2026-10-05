# Implementation ledger

Release target: v0.5.0-alpha.1. Documentation adopts the full target; implementation
moves through the gates below. The baseline is `50c121f`.

| Landing | State | Observable completion gate |
| --- | --- | --- |
| Target documentation and policy | Landed | C01 to C14, API sketches, implementation order and user decisions have one target |
| Pure core and first consumers | Landed | Dependency-free core; portable paths and deterministic target prerequisites used by live adapters; negative cases and live workflow tests |
| Semantic identity, requirements and codec | In progress | Typed canonical identities, multi-file placements, explicit DTO dispatch and validated schema |
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

At `52a4c53`, all-feature Clippy and the full default suite passed: 1,365 tests
and ten doctests, including 23 offline smoke tests. The first core landing at
`c1386d4` also passed all 100 strict E2E tests. The prepared-import landing is
undergoing a separate strict rerun and Greptile review.

## Provider identity landing

The pure core owns provider-qualified project IDs and distinct Modrinth version
and CurseForge file IDs. Parsing preserves case and rejects whitespace, slugs,
URLs, malformed base62 values, zero, decimal padding and overflow. Syntax does
not establish existence or pin ownership; provider resolution still checks those.

Sync and removal compare typed identities. Installed metadata decodes pins in
its provider namespace, and command planning rejects invalid IDs before invoking
the backend. Human labels and observed metadata keys remain separate. Existing
wire structs still belong to the adapter; the normalized codec is not yet landed.

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
