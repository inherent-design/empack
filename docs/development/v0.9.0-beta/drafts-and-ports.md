# Drafts and portable ports

This target defines the state and I/O boundary of `empack-engine`. It extends the
[release scope](README.md); its types describe responsibilities rather than existing
callable signatures.

## Editable state

| Value | Meaning |
| --- | --- |
| `DraftProjectId` | Local workspace identity, distinct from distributed pack ID |
| `DraftRevision` | Lineage, monotonically advancing generation and content fingerprint |
| `ProjectDraft` | Virtual tree, import inbox and unresolved author decisions |
| `DocumentAnalysis` | Revision-tagged parse/composition result or scoped diagnostics |
| `ProjectSnapshot` | Immutable captured revision with retained inputs |
| `EditProposal` | Expected revision, requested changes and reasons; application creates a new revision |

Raw bytes in the virtual tree are authoritative. Parsed forms and UI models are
views of those bytes. Invalid YAML remains saveable within document/storage limits.
Structured edits require valid interpretation and an expected revision; they cannot
replace invalid current text with a stale last-valid view. Preserve the existing
bounded editor's comment retention, refusal cases and candidate re-decode check.

An inbox asset need not become a dependency immediately. Unresolved source,
placement and association decisions remain explicit, bounded draft metadata.
Unsafe archive names never become virtual paths, even when other draft content
is invalid. Paths retain portable Unicode/case collision and traversal rules.

```text
registered input → retained inbox asset → inspection → proposed tree edit
                                                       ↓ accept at revision
                                                    new draft
```

## Assets and leases

`AssetId` identifies a logical source before its bytes are available. `ContentId`
identifies observed bytes. An `AssetLease` grants session-bound access and retention;
it is not serialized. Asset descriptors include display name, provenance, original
assertions, optional observed digest/length and a host-observed availability state.

Availability distinguishes retained, external, permission-required, missing and
corrupt content. A saved label is not evidence that backing bytes still exist.
Registering an external file does not establish immutability. Preparation retains
and verifies a private copy or obtains an equivalent stable-read guarantee.

Storage deduplicates bytes without merging provenance, assurance, roles or logical
assets. No cache hit upgrades accepted local observation to provider authentication.
A virtual entry is a directory or an asset reference with portable executable and
read-only attributes. Native symlinks require an explicit supported interpretation;
they are not silently followed or flattened.

## Immutable content publication

```text
private writer → sealed candidate → verified candidate
    → registered immutable object → retained references
```

A host seals by ending all write access and establishing a stable read view. The
writer is consumed; neither its owner nor an alias can mutate published backing.
Verification computes identity and checks the original assertions against that
sealed view. A storage seal alone proves neither source integrity nor publisher
authentication. Failure leaves private output unavailable to other consumers.

Registration chooses one ready backing object for a content identity. Concurrent
candidates never write to a shared canonical filename. A losing candidate can
release only its own private backing; it cannot delete the winner. Registration,
retention and collection must preserve live readers and unfinished operations.
The browser host specifies the metadata transaction and reader coordination;
native adapters must establish equivalent immutability with their own mechanisms.

## Constructor authority

These are proof boundaries, not public structs with writable proof fields. Each
crate exposes narrow requests and read-only results instead of a generic trusted
host interface. A host implementation remains responsible for its physical I/O
guarantees; the API boundary is not a sandbox against a malicious host.

| Value | Constructor owner and proof | Consumer and limit |
| --- | --- | --- |
| Prepared operation | Engine preparer captures valid operation inputs, effects and revision | Engine authorization; preparation is not approval |
| Execution grant | Host expresses an explicit decision for a plan through a narrow request | Engine checks plan identity and permissions; a grant cannot certify inputs |
| Retained immutable input | Host store seals backing; engine verifier binds bytes and original assertions | Engine operations retain access; observation is not publisher authentication |
| Verified artifact | Engine inventory verifier checks sealed output | Delivery adapter; verification does not prove destination finalization |
| Verified native change | Native verifier binds an authorized engine plan to native observations | Private native publisher; no arbitrary file-plan constructor |
| Prepared draft restore | Native restore preparer binds a verified container, absent destination and draft metadata | Native restore executor; no valid-authoring or resolution claim |

Cross-crate access must preserve these constructors' ownership. External-crate
compile-fail tests cover attempts to forge proofs or bypass authorization. Shared
low-level publication helpers remain private and accept purpose-specific proofs.

## Revision rules

Every edit, undo and redo advances the generation. Returning from A to B to A does
not make a result computed for the first A current again. History stores mappings
and retained references, has count/byte bounds, and releases content only after its
references are retired. Cross-session undo is optional; save/reopen of the current
draft is required.

| Operation | Completion after a newer edit |
| --- | --- |
| Analysis/resolution | Return an old-revision result or proposal; never apply silently |
| Draft edit | Reject expected-head mismatch |
| Snapshot save | Save captured revision; newer revision remains dirty |
| Snapshot export | May finish using retained old inputs; receipt names captured revision |
| Native mutation | Revalidate native read set and approval before commit |

A stale result may contribute verified immutable bytes to a cache. It cannot change
the current selection, saved indicator or export indicator by doing so.

## Capability ports

| Port | Inputs and outcome |
| --- | --- |
| `ContentAccess` | Lease plus checked offset/read bound; bounded chunk, EOF or typed access failure |
| `ContentRetention` | Retain/seal verified content and release ownership |
| `ScratchStore` | Admitted temporary writers/readers; incomplete output remains private |
| `CatalogTransport` | Authorized request, origin/header policy and limits; readable bounded response |
| `ProjectStore` | Captured save inputs, expected head and idempotency key; commit/status receipt |
| `ArtifactDelivery` | Verified artifact and destination grant; host-specific finalization outcome |
| `ExecutionControl` | Monotonic time, cancellation, task-queue yield, progress and admission |
| `RuntimePreparation` | Optional typed runtime request; verified native candidate or missing capability |

Operations receive only the ports they use. The host constructs implementations;
there is no universal filesystem/process service locator. Pure decoding and draft
edits do not make network requests or create persistent host state.

Portable orchestration accepts local futures without mandatory `Send + Sync`.
Native adapters can use stronger bounds for jobs they own. JS handles never acquire
unsafe `Send` implementations. Clocks, randomness for local IDs and admission are
injected capabilities rather than ambient assumptions about the target runtime.

Read bounds apply to each chunk and total outstanding buffers. Offsets/counts use
checked arithmetic. Large assets are not exposed through an unbounded whole-file
`Vec<u8>` method. Host leases retain resource reservations beyond job completion;
resource accounting is not an OS allocator limit or subprocess sandbox.

## Lifecycle and diagnostics

```text
persistable draft
    → valid authoring view
    → resolved operation inputs
    → prepared operation bound to revision and grants
    → verified candidate
    → delivery or native application receipt
```

Saving and restoring a draft require storage verification: safe paths, bounded
metadata and verified included objects. They do not require valid authoring or a
lock. Export requires the complete author document and bindings to compose into
valid authoring. Version 0.9 does not provide partial semantic interpretation or
silently discard malformed unselected sections.

Capability and acquisition requirements apply to the selected operation. A valid
server profile requiring unavailable Java does not block a selected Prism export.
A malformed server profile does block export because the authoring is invalid.
Diagnostics distinguish parse failure, invalid authoring semantics, missing
selected-operation capability and missing selected-operation content.

Diagnostics include stable code, phase, severity, document byte range/path where
known, logical object, expected/observed facts and action-specific blocking status.
The JS bridge converts ranges to editor coordinates explicitly. Missing, unknown,
permission-required, unsupported, rejected and corrupt are distinct states.

Cancellation requests do not imply retirement. Terminal outcomes remain owned and
queryable even when progress subscribers disappear. Progress delivery cannot hold
a resource lock or prevent terminal completion.

## Scheduling shared work

Every expensive operation declares one execution class:

| Class | Admission and execution contract |
| --- | --- |
| Small synchronous | Structural/input bounds and measured maximum step time within the browser profile |
| Resumable | Bounded work units with real host-task yielding and cancellation checks |
| Native blocking | Native worker scheduling, cancellation and retirement; unavailable to browser callers |

Classify document parsing, composition, serialization, semantic fingerprints,
template parsing/rendering, provider-response parsing, inventory construction,
graph processing, hashing and codecs. An async wrapper does not change a
synchronous operation's class. Byte limits alone do not prove responsiveness.
The [resource profiles](feasibility.md) set admission and timing requirements;
measure dense metadata and expensive allowed syntax as well as ordinary inputs.

## Interpretation shared by hosts

Document analysis uses the same project-level satisfaction checks as reconciliation,
including automatic layout, bindings, source identity and exact selection. A draft
can be persisted while these checks fail; its diagnostic view must still explain
which constraint is unsatisfied. Metadata inspection is not content verification.

Recipe selection preserves the [baseline defaults and authored meanings](README.md#extraction-baseline).
The host presents an effective recipe; it does not infer a different policy from
its available capabilities. Asset inspection also distinguishes an opaque JAR from
an archive whose members become portable tree entries. Only the latter authorizes
extraction and requires destination collision validation. `ArchiveMemberName`
preserves the exact internal namespace, including `example/con.class`; it cannot
serve as a `PortableRelPath` without separate destination validation. Keep structural
archive safety and bounded verification shared while retaining distinct name types.

## Checkpoint acceptance

Extract document composition/editing first and run the same semantic fixtures
through old native call sites forwarding to the new engine. Add draft tests for
invalid-text round-trip, stale structured edits, A/B/A generations, optional missing
assets, duplicate/colliding paths and undo retention.

Compile `empack-core` and `empack-engine` for native and `wasm32-unknown-unknown`.
Inspect normal transitive dependencies/features for the WASM target. Compile a
local-future host with non-Send handles; test read overflow, bounded outstanding
buffers, cancellation/retirement and surviving retained results. No native I/O,
Tokio executor or browser API may be required by the engine's public boundary.
Use in-memory port fixtures here to prove interpretation and ownership. They do not
establish durable native restoration, browser persistence or final artifact delivery;
those guarantees belong to their consuming checkpoints.

Test operation analysis with a valid Prism profile beside a valid server profile
lacking browser Java: analysis admits the selected Prism operation. Replace that
server profile with malformed configuration: analysis stops at semantic validation
while an in-memory draft save/reopen preserves the exact text. Actual browser
persistence and Prism export are tested at their consuming checkpoints. Test storage verification separately from execution verification,
including a draft without a lock and a sealed object whose source assertion fails.
