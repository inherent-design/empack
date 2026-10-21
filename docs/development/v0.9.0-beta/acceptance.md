# v0.9.0-beta release acceptance

Release acceptance binds the extracted native CLI and packaged browser SDK to one
immutable commit and dependency graph. These are required checks, not recorded
passes. Run results belong in PRs, CI artifacts and release evidence.

## Gates

| Gate | Required evidence |
| --- | --- |
| Portable boundary | Native and wasm32 engine builds, target dependency audit, local-future host and pure/semantic fixture parity |
| Native parity | The merged baseline's command, import, provider, build, publication, process, instance and recovery contracts through extracted services |
| Draft restore | Invalid text, no lock, missing source and unresolved decision survive directory/metadata crash recovery and repack |
| First browser project | Local JAR/config input, edits, invalid-text save, reload, correction and verified reference and explicitly bundled Prism ZIPs |
| Portable round-trip | Native → bundle → browser edit/save/reopen → bundle → native export with preserved author work |
| Persistence failure | Real worker termination, lost acknowledgement, quota/error injection, two-tab ownership, inherited storage leases, concurrent immutable registration and collection/save race |
| Resource/delivery | Bounded processing, large input, many entries, ZIP64, backpressure, cancellation and truthful finalization |
| Network/manual input | Uncached Modrinth/runtime route checks and exact restricted-file continuation |
| Package/install | Native archives, SDK/client/worker compatibility, static harness and checksums from the release candidate |

The engine target dependency graph excludes mandatory native effects and browser
objects. Source-text import checks alone are insufficient; inspect transitive normal
dependencies/features and compile the actual target. Tests enforce public proof and
mutation visibility from an external crate.

## Required browser matrix

| Browser | Minimum acceptance environment |
| --- | --- |
| Desktop Chromium | Current stable branded Chromium-family browser on a recorded desktop OS |
| Desktop Firefox | Current stable Firefox on a recorded desktop OS |
| Desktop Safari | Actual stable Safari on macOS; Playwright WebKit is supplemental |
| iOS Safari | Actual iPhone/iPad and OS version, touch/file selection, reload and downloaded-file retrieval |
| Android Chrome | Actual Android device and stable Chrome, file selection, reload and downloaded-file retrieval |

Freeze exact versions for the candidate and record device model, OS, sink type,
storage mode, configured limits and fixture identities. Private browsing is a
capability/failure case, not a promise of durable persistence. Where a platform
lacks a file writer, its tested staged-download path satisfies delivery with the
weaker `DownloadRequested` receipt. Required devices must pass the same semantic
workflow within their published limits.

The harness can use automated Chromium/Firefox/WebKit tests for frequent feedback.
Actual Safari and mobile checks may be manual or device-automated. Lack of a device
or permission is missing evidence and remains a release blocker.

## Cross-host fixtures

A shared offline fixture includes exact runtime metadata, local JAR/config sources,
comments, bindings, profiles, templates, optional choices, portable attributes,
empty directories, an unresolved inbox item and an explicit missing custom source.
Run complete-source and partial-project variants. Save intentionally invalid YAML
and reopen its exact bytes before repairing it.

Compare inventory path, digest, length, participation, file role and permissions
from equivalent frozen inputs. Check world-initialization ownership and disjoint
client/server conflicts in native and portable planners. Preserve accepted-observed
versus asserted evidence. Archive byte-for-byte reproducibility is a separate claim.

A second fixture resolves Minecraft/Fabric and Modrinth from an empty browser
workspace. A third exercises a restricted input with an intentionally wrong file,
then the verified file. Deterministic fixtures establish decisions; live checks
establish current endpoint access. Both are required.

## Baseline behavior fixtures

Preserve the [native baseline](README.md#extraction-baseline) through shared engine
fixtures and the host-specific presentations:

- Create new reference profiles and reopen existing bundled shorthand. Explanation
  and export agree; unsupported preparation cannot change the selected recipe.
- Inspect an opaque JAR with case-distinct members and `example/con.class`. Backup
  retains exact bytes. Native-reserved and colliding portable destinations still
  fail extraction; inspection cannot grant extraction authority.
- Assess one malformed provider root beside a valid root under one shared budget,
  including an otherwise valid listed Modrinth version whose file list is empty.
  Report the unavailable evidence without suppressing the valid row or resetting
  limits. Transport, authorization, throttling and deadline failures retain their
  stopping behavior. Native strict mode displays the report before failing.
- Show checked, changed and retained selections with dependency identities in text,
  and preserve exact selection details in machine-readable output.

## Cross-boundary failure fixtures

- Valid Prism and valid Java-dependent server profiles share a document. Prism
  export succeeds without Java; malformed server semantics block export while
  exact invalid text remains saveable.
- Invalid YAML obscures one custom reference, another known source is missing and
  a retained source is excluded from builds. Bundle inspection distinguishes
  discovery, retained-byte coverage and backup scope after reopen/repack.
- Native restore terminates at every directory/metadata transition and after
  receipt commit. Recovery preserves invalid text, no lock, missing input and
  inbox decisions without adopting a foreign destination or overwriting edits.
- A session retains shared storage while collection is requested and save starts.
  The save inherits ownership; collection defers without a nested acquisition cycle.
- Two project writers ingest identical content with a live reader and collector.
  Fail before sealing and after registration, lose acknowledgement, then recover.
  Readers never see partial backing and failed writers cannot delete the winner.

- Terminate collection around each retirement transaction and exact-backing deletion.
  Reopen, re-ingest identical content with new backing, then recover old retirement.
  No broken ready mapping survives successful cleanup and replacement backing remains
  readable. Deletion failures retain reclaimable ownership records and diagnostics.
- Hold project writer ownership in another tab while storage coordination works:
  read-only access remains protected. Separately disable storage coordination:
  managed-content access and verified export are unavailable, not unprotected reads.

These fixtures supplement component tests. Storage verification cannot authorize
execution, and a native in-memory adapter cannot prove browser persistence.

## Resource evidence

The [minimum workload and timing profiles](feasibility.md) are fixed requirements,
not values chosen after a successful run. Select representative devices before
qualification and report their identity alongside each result. Each supported
profile declares its tested input/entry range, processing-memory allowance, scratch
estimate and sink limit; measured support must meet the declared minimum. Test a
retained source larger than that processing allowance without whole-source buffering. Measure linear memory and
accounted JS buffers separately; do not describe them as total browser RSS.

Include many tiny files, metadata expansion, ZIP64 arithmetic, one large compressed
member, finalization, slow output and cancellation in every expensive phase. Record
maximum uninterrupted step and cancel acknowledgement/retirement separately.
Include parsing, schema composition, serialization/fingerprints, provider metadata,
templates, graph expansion and inventory construction. Measure cold and warm runs
and control-message handling between admitted units. Record the maximum step time,
cancellation acknowledgement and local retirement against the explicit profile
thresholds. A valid eventual artifact does not excuse a responsiveness failure.

Mobile checks include background/suspend/resume and terminated-worker recovery.
Actual quota errors may vary by device; deterministic injection must identify the
fault boundary and real storage checks must still prove the normal commit path.

## Native and distribution evidence

Retain Linux/macOS/Windows tests, ARM64 cross-build, strict native recovery and
process checks, generated workflow contracts and source-install/package naming tests.
Run applicable live provider/runtime and curated-pack checks under the existing
explicit prerequisite policy. Missing credentials or Java do not count as a pass.

Import the resulting reference and explicitly bundled Prism archives using actual
consumer acceptance. For reference delivery, exercise recipient preparation with
empack and verify the acquired inventory. Identify what was observed; import or
startup does not establish gameplay. The CLI executable
remains `empack`, and release archives keep their existing platform naming pattern.

The release includes native binaries, an SDK archive, a static harness archive and
checksums naming final downloadable filenames. Publish a machine-readable capability
report plus a concise human summary of tested routes, formats, devices, sinks and
limits. The SDK has no mandatory cloud endpoint. Public hosting, npm publication
and a product SPA are separate actions.

## Checkpoint completion

Implementation PRs contain code, contract updates and exact validation evidence.
The [checkpoint boundaries](checkpoints.md#checkpoint-completion-boundaries) assign
local and integration evidence without requiring later features prematurely. All
release gates on this page still bind to one immutable packaged candidate; earlier
passes do not replace them. A draft contract commit alone cannot satisfy its
checkpoint. Before release, remove
temporary forwarding facades and stale package references, update user/API guides
for implemented behavior, and ensure the current-design index describes the shipped
architecture. Keep candidate acceptance receipts outside ideal-state design docs.

Batch automated code review after substantial completed checkpoints, respect the
remaining review budget, and resolve actionable findings before merging. Do not
spend review calls on empty implementation placeholders. Full native/browser matrix
runs are for meaningful combined candidates; documentation-only planning does not
need paid platform reruns. No new paid ARM64 runner is required.
