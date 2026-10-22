# Native host and CLI

`empack-native` supplies native effects to the [portable engine](drafts-and-ports.md)
and owns filesystem application, runtime preparation and recovery. The CLI remains
usable throughout extraction.

## Shared services and native extensions

| Shared engine service | Native adapter or extension |
| --- | --- |
| Document composition, edits and inspection | Capture raw files and expected absence; apply approved document changes |
| Provider/runtime request interpretation and policy | HTTP credentials, readable bounded responses, clocks and rate waiting |
| Verified asset access and retention | Private staging, pooled storage, content cache and leases |
| Import normalization and archive interpretation | Native source capture and acquisition |
| Template rendering and recipe inventories | Captured source view; native scratch storage |
| Artifact encoding and independent verification | Seekable candidate storage and verified publication |
| Operation lifecycle and diagnostic meaning | Tokio scheduling, worker/process retirement and native admission |
| Typed runtime requirements and inventory | Java discovery, installer execution and containment |
| Instance reconciliation and release verification | Root identity, launch coordination, journals and recovery |

Native extensions do not appear as browser methods that partly execute and then
fail. The portable engine can report a runtime-preparation requirement; the native
host owns the verified result and capability needed to satisfy it.

```text
CLI input and invocation permissions
    → native captured project view
    → shared assessment / preparation
    → present requested effects and decisions
    → authorize exact operation
    → shared construction and verification
    → native publication / delivery
    → structured outcome and process exit
```

Preparation that needs network or installers still requires the relevant grants.
Native publication constructors remain internal. External clients cannot manufacture
a file-level proof and bypass semantic preparation or authorization.

## External authoring tools

Native callers use the same typed preparation, inventory and instance services as
the CLI. An external application may run its own generators or validation tools;
this extraction does not provide a general process runner, HTTP companion or
JavaScript native bridge. Callers must not rely on terminal prose as an API or
bypass the existing authorization and publication boundaries.

For isolated validation, use existing build and instance operations to establish a
captured candidate in a separate destination. A preparation preview is insufficient
to claim that exact files were executed. Keep the intended pack inventory distinct
from an instrumented test inventory containing an exporter, test configuration or
other additions. Record that delta, the tool identity, runtime and configuration
inputs, and any world/test context in application-owned evidence. Tools receive
private test copies or equivalent isolation; mutable live-instance files do not
become immutable candidate inputs merely through a symlink.

Installed observations identify the inspected root, observed content and coverage.
They are separate from intended or generated content. An external tool's successful
report cannot authorize applying its directory to a playtest instance. Applying a
captured pack uses the native instance lifecycle, rechecks current observations and
preserves world and unknown-neighbor protection. Existing unmanaged installations
retain current ownership restrictions; inspection does not silently adopt them.
Supporting an additional adoption or checkout-merge workflow needs a separate design.

## Extraction behavior

Move semantic services into `empack-engine` and make existing callers forward to
those services before removing their old definitions. Move native storage/runtime
backing into `empack-native`; it depends on engine ports, not the reverse. A moved
workflow has one implementation and shared tests.

A native `ProjectView` captures only the required read set. Ordinary CLI commands do
not copy a whole project into browser-shaped storage or require a project bundle.
Native source paths establish observations, not immutable identity. Retained input
copies or equivalent stable reads bind long operations.

Local orchestration may run on a native current-thread lane. Owned native blocking
jobs remain on the worker pool and preserve admission, attempt identity and result
retirement. No artificial `Send` wrappers or loss of native concurrency is required.

The `empack-cli` package declares a binary named `empack`. Update workspace paths,
internal dependency names, test launch paths, scripts, generated workflow installers,
Cargo source-install commands, release packaging and docs together. In particular,
`cargo build -p empack` and package-based `cargo install` references need deliberate
replacement; an unchanged binary name does not preserve a renamed package selector.

A forwarding `empack-lib` facade is permitted only while downstream callers move.
The final workspace and public documentation use the extracted crates. API examples
and compile-fail tests enforce the final mutation boundary.

## Draft restoration and project initialization

`PreparedProjectInitialization` proves valid authoring and the resolution required
by initialization, including its existing intent/lock and create-only postconditions.
`PreparedDraftRestore` proves safe paths, bounded metadata, verified included
objects, explicit missing inputs and an expected-absent destination. It can contain
invalid YAML and no lock. Neither proof substitutes for the other.

The existing initialization constructor and journal checks retain their intent and
lock requirements. Factor private create-only publication machinery beneath the
two operations; do not weaken initialization or expose a public arbitrary-file
publication constructor. The [constructor table](drafts-and-ports.md#constructor-authority)
defines the cross-crate boundary.

Restoration produces a directory and its host-side draft metadata. Unresolved
inbox decisions, missing assets and source-discovery status are part of that result,
not optional post-publication bookkeeping. One recoverable operation owns both.

```text
capture and verify draft inputs → durable restore intent
    → prepare inactive draft metadata → publish absent root
    → bind metadata to observed root/revision → commit restore receipt
```

Before publishing files, persist an operation ID, idempotency key, retained input
identity (including container identity when restoring a bundle), destination and
parent observations, exact file inventory, prepared draft metadata and progress.
The durable record retains enough content and metadata to
recover without parsing the author's text as valid authoring.

Metadata remains inactive until associated with the published root's native identity
and captured draft revision. Reopen detects an unfinished restore and requires
recovery instead of treating the visible directory as a complete project. An
inactive metadata record for an unpublished root is not a usable workspace.

Recovery compares the actual directory and metadata against the durable record. It
can finish the association, return the committed receipt, or report a conflicting
edit. A destination created by another actor is never adopted merely because its
path matches. Rollback may remove only unchanged, operation-owned entries under
the existing native ownership rules; unknown neighbors remain protected.

Completion requires both results and a durable receipt. A lost acknowledgement is
resolved by status lookup using the same operation ID; retry does not create a
second workspace or fail solely because this operation already created its root.
The protocol provides recoverability, not an atomic transaction across files and
the host metadata store.

## Native guarantees

Keep root confinement, no-follow checks, expected-old observations, native file
identity, coordination and recoverable multi-file publication. Preserve committed
recovery-data reclamation and unknown-neighbor protection. Portable delivery receipts
do not weaken filesystem recovery classification.

Keep whole initialized-world protection, client/server participation, optional-file
compatibility checks, stable file roles, source ownership, explicit URL observation,
retained negative evidence and comment-preserving edits. The
[extraction baseline](README.md#extraction-baseline) also binds reference defaults,
opaque JAR handling, bounded partial assessment and command diagnostics. Keep the
same errors and exit outcomes when moving presentation into `empack-cli`; do not
make browser portability an implicit change to native selection policy.

Native signing keeps its existing custody/approval boundary. Instance subscriptions,
sequence floors, repair, rollback and launch coordination remain native features.
Browser extraction must not turn missing capability into successful no-op execution.

## Checkpoint acceptance

Run existing CLI, provider, import, artifact, native process and publication tests
through the new composition root. Include real crash/restart and conflicting-edit
recovery tests; mock project stores cannot replace them. Confirm source bytes and
artifact inventories for fixed local fixtures, plus real loader/consumer checks at
the final release candidate where the release policy requires them.

Check default, minimal and telemetry feature graphs on Linux, macOS and Windows.
Compile the portable target separately. Keep the existing ARM64 cross-build; no
new paid ARM64 runners are needed for this extraction. Update API doctests and
architecture checks to reject native imports in portable production code and public
lower-level mutation construction by external crates.

The checkpoint is complete when ordinary commands run through extracted services,
all moved workflows have one implementation, and native behavior passes the revised
suite. Creating crate directories or forwarding every call back into the old library
is not completion.

At this checkpoint, feed the draft verifier retained raw objects and portable
metadata from a deterministic fixture, without a project ZIP decoder. The verifier
must establish the restore proof through the same path later used for decoded
containers. Tests cannot manufacture that proof through a public unchecked
constructor. The retained input identity binds the fixture's exact tree and metadata;
container provenance is added by the format reader at checkpoint 3.

Restore invalid YAML, no lock, a missing custom asset and an unresolved inbox
decision. Terminate before and after each directory/metadata transition and after
receipt commit. Recovery and the native project-store reopen API must preserve all
four conditions, including unknown discovery. Inject a conflicting destination and
post-publication user edits; recovery must refuse to overwrite them.

Checkpoint 3 owns the actual `project pack`, `inspect` and `unpack` commands and
container-to-native-to-container regression. Repacking is an integration gate there,
not a prerequisite for this checkpoint's private restoration machinery. Final release
acceptance repeats the complete restore path against the packaged candidate.

Audit synchronous shared work during extraction against the execution classes and
resource profiles. Moving parsing or template work behind an async API does not
establish browser responsiveness or native worker retirement.

Use a small native fixture with a private test copy and a synthetic observer.
Verify that adding instrumentation changes only the test inventory and does not
alter retained pack inputs. A changed live source fails the applicable observation
check rather than being attributed to the captured candidate. Inspection of an
unmanaged destination cannot confer installation authority. This checkpoint uses
fixture inputs through existing native boundaries; real consumer artifacts remain
checkpoint 5 evidence, and no game-specific exporter is a new release gate.
