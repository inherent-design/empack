# Resolution and acquisition

Resolution establishes exact semantic selections. Acquisition obtains bytes that
satisfy original assertions; it cannot silently replace the selection.

## Sources

| Source | Resolution evidence | Acquisition |
| --- | --- | --- |
| Modrinth | Canonical project, exact version and named files | Verified HTTPS origins |
| CurseForge | Project/file ownership and exact assertions | API-resolved URL or manual input |
| Direct URL | Declared destination, requirements and digests | Bounded alternatives with original assertions |
| Local file | Captured source, content kind and placement | Immutable local content lease |
| Archive member | Exact archive and normalized member identity | Bounded extraction into private staging |
| Native release | Authenticated exact release inventory | The release's approved acquisition obligations |

Slugs, search results and project URLs are selectors, not persisted canonical IDs.
Explicit pins must belong to the selected project. Compatible selection prefers
stable releases when available unless the author chooses another release policy.
Required dependency expansion retains evidence completeness; unknown edges never
become permission to delete an apparent orphan.

Known incompatibility assertions remain attached to their exact provider selection
in the lock. A target can name a canonical project, an exact provider version/file,
or both. Candidate validation checks them against every retained or newly selected
provider entry before publication. A pair conflicts only when its resolved files can
participate together on the client or together on the server. Participation includes
all file placements and their layers; optional choices count even when disabled by
default. Client-only and server-only selections can coexist, and their assertions
remain recorded so a later placement change is checked. The same rule applies to
add, update, adoption and synchronization. A conflicting mod is never removed
automatically.

Provider closure resolution retains negative assertions without deciding installation
participation. It validates required relationships and assertion identities; the
composed candidate supplies the file placements needed for conflict rejection.

Synchronization can add negative evidence to an unchanged explicit root reached
through an affected root's required closure. Its exact selection, files, placement,
required edges and coverage remain unchanged; previous assertions cannot disappear.
Out-of-scope records retain their complete prior description.

Changing the source selection replaces its assertions; refreshing the same selection
preserves known assertions. Independent batches connect requests whose old or new
selections can participate together in a declared conflict, so a failed member cannot leave its
partner in an invalid combined candidate. Explicit replacement is checked against
the resulting inventory, not the files that existed before the operation.

Older locks without the optional `incompatibilities` field retain no negative
assertions. Missing assertions do not establish compatibility; refreshed metadata
can supply additional evidence. Required-edge coverage remains separate from a
claim of complete compatibility or a search for alternative solutions.

## Import normalization

Local and remote mrpack/CurseForge archives produce normalized intent, exact files,
placements, side requirements, choices and provenance. Import inspection does not
mutate or reset a project. Unknown-host URLs remain first-class URL files with their
original hashes, sizes and destinations; they never become empty provider IDs.

Preserve common/client/server layers and optional participation. Unsupported
conversions require explicit decisions. World archives retain exact archive identity
and bounded member ownership. Recognize supported formats by validated structure,
not filename alone. No packwiz detection or execution is part of import.

## Transport policy

Use shared provider status classification, retries, budgets, deadlines and cancellation.
Distinguish not-found, authentication, throttling and transient failure. Per-process
rate reservations are not a global distributed quota. Redirects preserve HTTPS and
authentication-origin boundaries. Optional telemetry and diagnostics redact locators.

Stream remote input into private storage while enforcing a running byte limit;
reject an excessive advertised length early without trusting it as the only bound.
Bound compressed input, manifest bytes, entry count, expanded bytes and member size
separately. Reject duplicate/colliding members, links and escaped destinations.

## Evidence and storage

`ExpectedContent` retains original digest/size assertions. `AcquiredContent` owns a
verified lease and observed content ID. Verify cache hits against the original
assertions, including weaker provider compatibility evidence. Cache corruption or
unavailability leaves the original obligation; it does not change the expected file.

Cache insertion is best-effort and resource-admitted. Failure of disposable caching
cannot invalidate a prepared authoritative mutation. Retained stage data, receipt
reservations, recovery copies and continuation inputs outlive transient worker slots.
Eviction respects leases and explicit ownership; last-writer-wins HTTP caches are
not a consistency model for durable project or instance records.

## Restricted content

Manual association names an exact obligation. Verify bytes against original assertions,
not a filename, extension, recent modification or unique-looking candidate. Discovery
caches observations within a bounded pass and handles unreadable unrelated candidates
without failing explicitly selected valid files. Explicit selection errors remain fatal.

Browser assistance opens verified provider pages only after approval; `--yes` alone
does not open a browser. Bounded waiting is attached to the saved recipe and cumulative
scan allowance. Timeout leaves continuation intact. Resume revalidates release,
assertions, choices, supplied files and destination observations before publication.

## Opaque archive inspection

Direct JAR and ZIP content is inspected within its case-sensitive archive namespace.
Case-distinct JVM members remain distinct because inspection does not extract them
onto a filesystem. Duplicate names, file/ancestor conflicts, unsafe paths, links,
encryption, CRC failures and bounded expansion remain rejected. A separate inspection
entry point returns member names after verification and grants no extraction access.
World/import member extraction retains portable caseless destination checks.
