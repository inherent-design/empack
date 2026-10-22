# Bounded codecs and artifact delivery

The portable engine builds an artifact from frozen inputs, checks it against an
independently derived inventory, and then asks a host to deliver it. Artifact
validity and destination finalization have separate receipts.

## Data flow

```text
captured revision + selected recipe + retained inputs
    → expected semantic inventory
    → private candidate writer
    → bounded archive finalization
    → independent reader and inventory verification
    → VerifiedArtifact
    → destination transfer and finalization
```

`VerifiedArtifact` binds content identity, length, format, recipe, captured revision,
verification receipt and a retention lease. It cannot be manufactured from a writer's
success flag. The expected inventory comes from source selection and recipe rules,
not the encoder's list of entries. Native and browser outputs share these rules.

## Recipe and recipient contract

New profiles use the [reference defaults](README.md#extraction-baseline). Selection
and explanation expand the same authored recipe on both hosts. A capability failure
must retain that recipe in the diagnostic; it cannot silently choose bundled bytes,
change update authority or substitute a recipient installer for required preparation.

Verify a reference Prism snapshot's exact descriptor, sources, assertions and hooks,
as well as its included author assets. Its receipt states that the recipient needs
empack and that Prism prepares game/runtime files. Reference delivery does not mean
that no bytes are included or that preparation never acquires content.

An explicit bundled snapshot remains supported and tested with locally generated or
otherwise permitted fixture content. It requires a complete verified included
inventory. Project backup has its own coverage contract and is never substituted
for either distribution recipe.

## Processing model

Acquisition is asynchronous. Local retained processing uses seekable bounded access
inside a codec step. The browser first retains input in OPFS, then uses worker-local
synchronous handles where appropriate. Synchronous `Read + Seek` must not spin on
asynchronous browser reads.

A codec driver preserves state between bounded steps and yields to the host task
queue between them. Include header parsing, entry traversal, compression, hashing,
central-directory writing and final verification in the bounded-work contract.
A per-file loop is insufficient when one file is large or one finalization scans
many entries. Unsupported uninterruptible codec modes stay unavailable on browser.

Use incremental Rust hashes and bounded source/output buffers. Account for total
outstanding buffers, metadata tables, compression dictionaries, entry count, decoded
bytes, path depth/length, nested archives, scratch and retained artifact storage.
ZIP64 offsets use checked `u64` arithmetic; JS and wasm32 conversions reject overflow.

The initial required browser codec is ZIP, including project containers and Prism
reference and explicitly bundled snapshots. Checkpoint 3 establishes the shared
cooperative ZIP foundation and thin browser adapter before persistence depends on
bundle execution. This
checkpoint reuses that path for consumer recipes, inventory verification and
delivery; it does not introduce a second project or consumer codec implementation.
Native ZIP, TAR.GZ and 7z remain supported. Compilation alone
does not certify browser codecs. Select or adapt codec implementations only after
a step/finalization audit and measured cancellation/memory tests.

## Responsiveness across the pipeline

The [execution classes](drafts-and-ports.md#scheduling-shared-work) cover all shared
work, not just archive loops. Document parsing and composition, serialization,
semantic fingerprints, provider-response parsing, template parsing/rendering, graph
traversal and inventory construction each need a measured bounded synchronous step,
a resumable implementation, or a native-only classification.

The [minimum profiles](feasibility.md) fix supported workloads, memory allowances,
maximum step duration, cancellation acknowledgement and local retirement targets
before qualification. Measure cold and warm runs at the admitted structural limits.
A small input with dense metadata or expensive permitted templates belongs in the
suite alongside large byte streams. An async wrapper and a byte bound alone are
not evidence of worker responsiveness.

If a required route exceeds a target, adjust its implementation or explicitly revise
release scope before claiming support. Do not quietly shrink the fixture after
measurement. Report background suspension and external I/O waits separately from
foreground compute steps, with their own deadline and recovery outcomes.

## Revision and cancellation

Exports retain their captured inputs while the draft remains editable. Completing
revision 17's export at revision 18 is valid and is labeled accordingly. Cancellation
is observed between bounded work units. Ordinary cancellation cannot require killing
the worker; termination remains failure recovery.

Use `scheduler.yield()` where available and a tested task-queue fallback elsewhere.
A resolved promise is not the fallback. Do not require SharedArrayBuffer, WASM
threads, cross-origin isolation or JSPI for the initial portable contract.

## Delivery outcomes

| Outcome | Evidence and UI wording |
| --- | --- |
| Verified, awaiting destination | Artifact checks passed; “Ready to save” |
| `FileCommitted` | Host writer finalized successfully under its reported guarantee; “Saved” |
| `DownloadRequested` | Verified file-backed download offered to browser; “Download requested” |
| `CancelledBeforeCommit` | Host established no destination commit |
| `FinalizationUnknown` | Transfer/close acknowledgement lost or native publication uncertain; reconcile before claiming success |
| `Failed` | Typed diagnostic and known destination effects, if any |

A failed delivery does not discard the verified artifact until retry/release policy
allows it. Keep retention while a download still needs the object; revoking an object
URL immediately after triggering download is not proof of safe release. The browser
adapter defines and tests a lifecycle for its selected file-backed download path.

Acquire file-picker permission during a user gesture, or request another explicit
save action after preparation. A worker cannot open a picker after a long build by
assuming earlier activation persists. Bound writes and obey backpressure. A browser
writer's successful close provides its finalization receipt; a link activation does
not confirm the user's external save. Native publication continues to use native
recovery classifications, not browser download outcomes.

If the browser fallback copies an entire artifact into memory, capability limits
must reflect that measured behavior. Do not advertise large streaming export on that
route. The preferred fallback uses a retained OPFS-backed `File`, verified in each
required browser, without assembling the archive in WASM linear memory.

## Required consumer integration

Shared consumer preparation checks mandatory integration for every managed recipe:
reference snapshots and all empack-managed update recipes. Custom Prism settings
retain the generated `OverrideCommands`, preparation and wrapper commands while
preserving unrelated names, icons and memory settings. Managed server start,
GUI-start and install entry points must match the generated scripts; command
substrings do not establish shell semantics. A conflicting template fails
preparation without rewriting author input or delivering an artifact. Bundled
snapshots retain standalone command customization.

Required integration includes the supported consumer's effective interpretation
and the output attributes needed to execute its entry points. Managed Prism inputs
require an unambiguous supported `ConfigVersion` (`1.2` or `1.3`) in `[General]`,
canonical authority keys confined to that section, and balanced single-line values.
The restricted grammar treats only semicolon-prefixed lines as comments and rejects
hash-prefixed lines with an actionable diagnostic. Comment handling and continuations
must preserve effective lifecycle settings: text ignored by validation must also be
ignorable by the consumer. Shared fixtures cover hash-prefixed open quotes and
trailing continuations under both supported formats, with semicolon controls.
Structural padding is ASCII space/tab only; Unicode whitespace must not normalize
into a canonical key, section or comment. Accept LF and CRLF line endings and an
unterminated final line; reject bare CR and other raw controls. Preserve Unicode in
user-facing values, trimming only space/tab at value boundaries. Shared fixtures
cover U+00A0 around keys, U+2003 before keys, structural controls and Unicode names.
Whitespace, comments, quoting, continuations, key encoding and section rules belong
to one shared accepted grammar, not separate native and browser interpretations.
Missing markers, shadowed settings or multiline quoting cannot select a different
consumer interpretation after validation. Exercise the shared fixtures with Prism's
actual parser as well as the engine; text-field comparison alone is insufficient.

For byte-identical managed server entry points, output permissions come from the
recipe: shell scripts are writable/executable and batch scripts writable without
Unix executable bits. Normalize the candidate, leaving source bytes and attributes
unchanged, including sources from Windows. This rule covers only the six owned
entry points, not unrelated shell templates. Verify archive metadata in ZIP,
TAR.GZ and 7z where supported, and directly execute extracted Unix entry points
with controlled external tools. Native Windows acceptance checks emitted batch
diagnostics and exit status under `cmd.exe`.

Native and browser adapters use this same policy. An archive inventory pass does
not establish that a custom launcher will prepare its referenced content. Record
content inventory, consumer configuration, required preparation/activation
integration and actual consumer acceptance as distinct evidence. Missing host
capabilities cannot silently remove integration or change the selected recipe.

## Checkpoint acceptance

Use a generated asset larger than the configured WASM processing-memory allowance,
plus a many-small-files fixture. Record peak linear memory, admitted buffers,
metadata/scratch occupancy, largest uninterrupted step, cancellation latency and
output size. Tests cover hashing, compression and central-directory finalization.
The declared minimum profiles are requirements, not measurements. Publish support
claims only with fixture/device evidence meeting them; no universal browser memory
ceiling is inferred from a single desktop run.

Test ZIP64 boundaries with sparse/range fixtures and a real admitted archive,
traversal, collisions, unexpected/missing entries, changed bytes, expansion bombs,
permission attributes, output backpressure and failure during close. A correct
archive with a wrong semantic inventory fails verification.

Exercise file-writer delivery where supported and download fallback on every required
browser. Cancel before transfer, during transfer and near close; distinguish a
proven cancellation from uncertainty. Retrieve offered downloads in the harness
runner and verify their bytes without turning that test observation into a general
SDK claim that download requests are durable saves.

Import reference and explicitly bundled Prism snapshots through actual Prism
acceptance. Exercise the reference preparation hook with the recipient empack
executable and verify the resulting inventory from frozen inputs. Verify bundled
inventory and launcher interpretation separately. Import or startup alone does not
establish gameplay. Compare native/browser semantic inventories for each recipe,
including exact reference assertions and included author assets.

Build on checkpoint 4's durable project round-trip: reopen the edited valid fixture
on each host and compare its distribution inventory. Keep the incomplete-draft
fixture for backup tests; it must not bypass authoring validation to export.
Checkpoint 6 repeats these checks with the packaged SDK and native candidate.

Measure control-message handling during large permitted documents, dense provider
metadata, graph expansion, many small inventory entries and expensive templates.
Record cancellation acknowledgement separately from worker retirement and verify
both against the selected profile. Include final serialization and verification;
responsiveness during acquisition alone does not establish export responsiveness.

Exercise generated reference configuration and compatible custom Prism settings,
then remove or disable the preparation hook and wrapper separately. Require
rejection without mutation. Test all managed server shell/batch entry points with
conflicting replacements and unchanged generated copies, preserving custom server
properties. Keep bundled standalone overrides and subscribed lifecycle checks in
the same regression family. Shared preparation must enforce this policy before
native publication or browser delivery, rather than relying on CLI warnings.
