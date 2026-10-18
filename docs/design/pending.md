# Shared pending inputs

`pending list`, `pending show ID`, `pending resume ID --file KEY=PATH` and
`pending discard ID` select the existing build, import, synchronization and
instance continuations in the current `--workdir`. They do not create another
pending index or change the saved-record schemas. One record per workflow/root
can coexist with records from the other workflows.

## Identity and inspection

A full operation ID hashes the workflow, native root-bound storage identity and
exact saved-record bytes. A unique hexadecimal prefix is accepted; ambiguity,
a different root and a replaced record fail explicitly. The ID is not execution
authority. Resume compares the selected full identity with the observation loaded
by the original workflow before it can request approval. Discard uses the existing
native cleanup handle and conditionally removes only unchanged selected bytes.

List and show read bounded existing metadata without network access, payload
scanning or durable changes. Existing state locks may be acquired; no lock files
or directories are created. Unknown files next to the selected record are not
interpreted as operations. Invalid or unsupported records remain inspectable and
explicitly discardable. Stale target/document bindings are reported and cannot be
resumed through the shared command.

`pending list --json` and `pending show ID --json` emit the same schema-1 report:

- `schema`, selected `root`, and `operations`.
- Each operation: full `id`, `kind`, `root`, `state`, `inputs`,
  `requirements_complete`, `diagnostic`, and argument-array `next` command.
- Each input: exact `key`, human `name`, `provider_selection`, filename hint,
  `digests`, `bytes`, `download_page`, `supplied_candidate`, `verification`, and
  whether it is known to be `needed`.

`provider_selection` contains separate `provider`, `project` and exact `selection`
strings. Null means unknown. An empty digest list means assertions are unavailable, not
that any file is acceptable. Filenames are display hints, never association keys.
Saved records intentionally do not retain arbitrary host candidate paths.
`retained; verification required` describes retained input evidence, not a completed
obligation. Every continuation rechecks the bytes and all original assertions.

Build records retain the recipe and verified-file associations; the report lists
remote lock inputs and leaves whether each is required unknown until the original
build preparation computes the selected output obligations. Import
records retain the source archive and supplied associations, not missing provider
metadata. Import inspection therefore reports incomplete requirements explicitly;
normal continuation reparses the retained archive and refreshes exact provider
facts. It never substitutes a guessed filename or newer provider selection.

## Continuation and assistance

Shared commands call the existing build/import/sync/instance hosts. Those hosts
retain their acquisition, expiry/trust, source-revalidation, publication, runtime
lease and fresh-approval rules. The shared adapter does not create grants or
change all-requested activation. Existing continuation spellings remain supported
and share the same implementation.

Use the displayed exact selector with `--file`: `locked:DEPENDENCY:SLOT` for
builds (components percent encoded), `DEPENDENCY/SLOT` for synchronization,
release file keys for instances, and `declared:`, `override:` or `provider:`
selectors for imports. Repeated or ambiguous keys fail before publication.
Import representation choices can be supplied again with
`--import-optional-default BOOL` and `--import-local-files`; optional participation
remains optional. Use the original `init --continue` options for other import
representation decisions that were not saved in import records.

`--downloads-dir DIR` explicitly requests a bounded scan of direct regular files.
The existing scanner matches original digests/selected content identities and
size, never an extension. Ambiguous candidates require explicit associations.
Candidates are reverified by the original continuation before use. Build scanning
uses its freshly prepared exact output obligations. Other scans use available
saved assertions for inputs not explicitly supplied. A retained presentation label
is not proof that its bytes remain available. Synchronization excludes retained
inputs only after its loader recovers and verifies the stored bytes. If explicit associations cover every input, scanning is skipped.
Without scan assertions, continuation still runs its normal verification and
missing-input handling. No unverified candidate is marked complete or persisted
as trusted evidence.

`--open-downloads` explicitly resolves permitted pages for known exact provider
selections through the existing provider catalog and bounded browser launcher.
Unknown provider pages are reported rather than constructed from arbitrary stored
URLs. Preview never launches browsers or scans Downloads. Timeout or cancellation
leaves the existing recipe and retained verified inputs available for continuation.

Discard removes only the selected pending recipe after approval. User downloads,
retained content stores, completed installations, active runtime markers and
recovery journals remain. Retained content follows the existing separate cleanup
policy; this command does not guess which neighboring bytes are safe to delete.
