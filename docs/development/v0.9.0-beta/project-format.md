# Portable project format 1

An `.empack-project` file preserves editable author work across native and browser
hosts. It is a ZIP container with bounded JSON metadata and immutable content
objects. It is distinct from an `.empack` distribution or a launcher archive.

## Container

```text
project.json
objects/sha256/<64 lowercase hexadecimal characters>
```

Only those regular-file members and their ancestor directories are allowed.
Readers reject duplicates, unknown required metadata, unsafe paths, conflicting
names, symlinks, encrypted entries, unsupported compression methods and unreferenced
objects. Internal names in a retained opaque JAR are bytes inside an object,
not project-container member paths. `example/con.class` and case-distinct JVM names
may survive in that object; extracting either still requires destination validation.
Object names bind SHA-256 and length; every included object is verified
before the imported project is reported retained. ZIP CRC alone is insufficient.
Writers use stored objects by default to avoid recompressing JARs. ZIP64 is used
where sizes require it; readers support stored and deflated entries under limits.

The metadata wire contract has these fields. Exact enum spellings and schemas are
frozen with executable fixtures before checkpoint 3 is complete.

| Field | Meaning |
| --- | --- |
| `format` | Integer `1`; independent of author/bindings/lock versions |
| `project` | Portable lineage and captured generation; imported as a new local workspace by default |
| `entries` | Portable virtual paths mapped to directories or logical assets and portable attributes |
| `assets` | Logical IDs, display/provenance/evidence records, optional expected content/length and object reference |
| `inbox` | Registered assets awaiting an explicit integration decision |
| `decisions` | Versioned portable pending author decisions; no live plan or approval |
| `scope` | Bounded selected roots/assets and whether selection was discovered or explicitly chosen |
| `source_discovery` | `assessed` or `incomplete`, with diagnostic references for unenumerated scope |
| `asset_coverage` | `complete` or `missing`, with known missing/external asset IDs |
| `resolved_coverage` | `not_requested`, `incomplete` or `complete`, bound to the assessed operation and inventory |

Raw author documents, bindings, templates and lock bytes are ordinary asset-backed
entries and preserve exact bytes, including comments or invalid YAML. Empty
directories and executable/read-only attributes survive round-trip. A supplied lock
is advisory until rebound to current intent and evidence.

Logical assets sharing a content object retain separate provenance and original
assertions. Missing assets can carry an expected digest without an included object;
that is distinct from an included object that is corrupt. Imported content identity
does not prove the bundle or its publisher is authentic.

## Backup modes

| Mode | Required included bytes |
| --- | --- |
| Complete custom sources, default | Assessed source scope and every author-maintained asset reachable from its tree/inbox/decisions |
| Partial, explicit | Available assets plus precise missing/external references and diagnostics |
| Fully materialized offline, optional | Custom sources plus verified resolved content required by a stated operation/recipe |

An unresolved provider request does not make a complete custom-source backup
incomplete. A missing custom JAR or incomplete source discovery does. Report custom-source completeness separately
from semantic validity and offline reproducibility. Do not use a single `complete`
boolean for these independent promises. Offline mode cannot be advertised until
its operation/recipe inventory has been checked.

Discovery and byte coverage are independent. `asset_coverage: complete` means every
enumerated required asset is retained; it does not prove every source was discovered.
Invalid author text can leave `source_discovery: incomplete` even with no missing
asset IDs. Preserve the diagnostic and scope on reopen, inspect and repack.

Default complete backup refuses incomplete discovery or missing custom bytes. The
author can choose partial backup or explicitly confirm a bounded selection. A
bounded selection can be complete for that selection, but the receipt must retain
unknown project-wide discovery and must not label it a complete project backup.
The wire metadata and receipt carry the same distinction.

Backup scope differs from build scope. Build exclusion patterns do not omit editable
source files from a selected backup root. Invalid text does not authorize a recursive
sweep of arbitrary neighbors, external paths or host secrets. The default scope is
known author documents, source roots and enumerated custom references; additional
roots require explicit selection and portable path validation.

History is excluded by default. Saving captures and retains current inputs while
encoding; changes after capture do not alter the bundle. A bundle receipt names its
revision and completeness. It may be valid even when its author documents cannot
be parsed. Limits on document bytes, metadata, entries and paths still apply.

## Authoring tools and derived content

Explicitly selected authoring roots may contain recipe sources, generator source,
configuration rationale and application-owned reports as ordinary project entries.
They remain eligible for backup when excluded from distribution builds. Empack
preserves their bytes and portable attributes without interpreting domain schemas.
Opening a bundle never executes an included generator or installs a toolchain.

Applications can store a versioned generation record beside those files. It names
the captured source inputs and digests, generator identity/version, relevant options
and environment, and output paths/digests. A validation record names its captured
pack or test inventory, producer, context, coverage and findings. Such records are
opaque author data to empack; restoring them preserves a claim, not its truth or
current applicability. Missing tools, unobserved inputs and external context remain
explicit in the producing application's record. No new executable manifest hook
or project-container proof field is introduced for these records.

Include these records through explicit backup scope. Do not sweep in local caches,
logs, worlds or external tools just because a report mentions them. Portable records
use logical paths/content references rather than live capabilities or host paths.
A restored report can remain historical when referenced test context was deliberately
excluded. Reopening does not silently regenerate outputs or reconcile source files
with an existing checkout.

## Authority and disclosure

Exclude host credentials, signing key stores, cookies, live browser permissions,
execution grants, native journals, instance enrollment, OPFS locations and host
absolute paths from metadata. A source needing reattachment uses a display hint and
expected identity; the hint grants no access and must not become a destination path.

Exact raw author files can themselves contain secrets or private URLs. The writer
cannot guarantee arbitrary user bytes are secret-free while preserving them. Show
the selected project scope and known sensitive references, never auto-include host
secret stores, and require a deliberate backup/export action. Saving a local bundle
does not upload or authorize redistribution of its contents.

Opening imports into a new workspace identity and records portable lineage. An
explicit replacement of an existing workspace requires expected-head approval.
No network request, code execution or trust enrollment follows from parsing a bundle.
Unknown future formats fail without mutating the open project. Readers must not
silently drop unfamiliar decisions to manufacture a valid older project.

## Native commands and cross-host API

The following command surface is a target for implementation, not v0.8 syntax:

```text
empack project pack --output <file.empack-project> [--partial]
empack project inspect <file.empack-project>
empack project unpack <file.empack-project> --into <new-directory>
```

`pack` captures author documents and the safe source scope without requiring valid
YAML or resolving dependencies. The default includes known source roots and custom
references; unrelated files and referenced paths outside the project require an
explicit inclusion decision. Invalid documents can hide unresolved references, so
default complete packing stops and offers partial or explicitly bounded selection.
Its receipt retains incomplete discovery rather than claiming full project coverage.

`inspect` is bounded and read-only, reports structural/object validity and missing
assets, and does not create a workspace or fetch URLs. `unpack` restores into an
expected-absent directory through `PreparedDraftRestore` and the
[native restore protocol](native.md#draft-restoration-and-project-initialization). It does not
merge into an existing author checkout or initialize an installed instance.

Portable pending decisions that cannot be encoded in author documents live in the
native host's project-store metadata, bound to the restored root and revision.
Native reopen and repack must preserve them. Host-owned storage is not a second
editable author manifest and its paths never enter portable metadata. A directory
copy alone is not promised to preserve host-only pending state; the project bundle
is the portable backup for that state. Directory publication and metadata association
share a durable restore record; success requires both and a queryable receipt.
Recovery does not require valid authoring or a lock.

The engine exposes equivalent inspect/write/open operations over content and project
ports; these commands adapt them rather than implementing another ZIP interpretation.

## Shared cooperative ZIP foundation

Checkpoint 3 implements the minimum reusable ZIP reader, writer and independent
verifier used by project containers and later Prism artifacts. Stored and deflated
members, ZIP64 arithmetic, metadata limits, bounded buffers, finalization and
verification all obey the [execution profiles](feasibility.md). Large members and
many-entry archives yield to the actual host task queue between bounded work units.

A thin dedicated-worker adapter exercises retained OPFS input, synchronous random
access, staged output retrieval and cancellation in real browsers at this checkpoint.
It uses uniquely owned temporary backing with explicit cleanup; it does not claim
checkpoint 4's durable save or shared-store protocol. The early feasibility probe
informs this shared implementation and leaves no competing archive path.

Checkpoint 4 consumes this tested foundation for browser project save/reopen and
bundle round-trips. Checkpoint 5 adds consumer recipes, verified delivery and broader
qualification; it does not introduce the first bounded browser ZIP implementation.

## Checkpoint acceptance

Run native source → bundle → native restore/reopen → bundle through the public
project commands and the verified restore path from checkpoint 2. Preserve raw
bytes, invalid text, comments, bindings, templates, profiles, optional choices,
attributes, empty directories, inbox and pending decisions. Repacking reads the
associated host metadata as well as restored files. Compare object identities,
raw entries, discovery and coverage; archive-byte identity is not required.

Test complete and partial sources, shared bytes with distinct evidence, corrupt or
missing objects, stale locks, wrong sizes, unknown versions, extra members, ZIP64
arithmetic, hostile names and metadata/expansion limits. Include an opaque JAR with
`example/con.class` and case-distinct entries: backup/reopen preserves its bytes,
while an attempted unsafe portable extraction fails. Failed import leaves the
open project unchanged. Failed complete backup leaves the draft intact and never
reports a partial container as complete.

Include invalid YAML containing an undiscovered custom reference, a separately known
missing asset and a source excluded from builds. The container must distinguish
incomplete discovery from missing enumerated bytes and preserve the excluded source
when it lies in the selected backup scope. Test explicit bounded selection without
upgrading unknown project-wide coverage.

Exercise restore termination at every directory/metadata transition using invalid
text, no lock and an unresolved inbox decision. Recover, reopen and repack all
metadata as well as files. The browser ZIP adapter must meet the minimum profiles
before checkpoint 4 can claim executable bundle acceptance. Its temporary browser
adapter proves bounded codec execution, not persistent project save/reopen.

## Cross-host integration and release gates

Checkpoint 4 adds native bundle → browser edit/save/reopen → bundle → native reopen
using the established format and native commands. It must preserve exact author work,
including incomplete drafts. This requires real OPFS/IndexedDB persistence and cannot
be satisfied by checkpoint 3's temporary backing or an in-memory project store.

Checkpoint 5 adds native/browser distribution inventory comparison for equivalent
valid frozen inputs. An invalid or unresolved draft can round-trip without being
exportable; correct it or select the valid fixture before distribution export.
Checkpoint 6 repeats the combined round-trip and export on the same immutable release
candidate and required devices. These later gates do not defer this checkpoint's
native restore/repack, wire validation or cooperative ZIP obligations.

Extend the format fixture with an authored source excluded from builds, generated
output, generator source, and an application-owned provenance/report file. Pack,
restore and repack preserve those selected bytes and attributes without executing
the generator. Deliberately exclude a referenced external test context and retain
that absence; a restored report must not become an engine verification receipt.
