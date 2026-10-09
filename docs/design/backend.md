# Metadata and process contracts

The native engine reads and writes packwiz-compatible metadata. Ordinary project
operations do not invoke a packwiz executable. The format is an observation and
interchange boundary; it cannot authorize publication or replace source assertions.

## Native metadata

`engine::backend::BackendFile::parse` captures provider identity, exact selection,
destination, environment, optionality and the declared download digest from one
`.pw.toml` record. `provider_observation` is shared by command adapters and build
observation. Ambiguous identities, invalid fields and unsafe destinations fail.

`engine::packwiz` produces installer reference trees from expected game inventories.
`engine::mrpack` emits Modrinth manifests and override layers. The artifact verifier
checks their represented content independently of writer success. Add, sync and
removal prepare exact managed metadata changes through the shared publisher.

## Owned processes

[`execute_async`](../../crates/empack-lib/src/application/process_runtime.rs)
accepts a native `Command`, deadline, cancellation token and optional bounded progress
sender. It runs on the host Tokio runtime and returns captured `ProcessOutput`.

The owner retains Unix process-group or Windows job ownership. The deadline includes
stdout/stderr lifetime: closing pipes does not authorize unbounded waiting, and an
immediate-child exit does not establish descendant retirement. Cancellation or timeout
terminates the owned tree. A disconnected or slow progress consumer cannot block
supervision; capture has explicit per-stream bounds.

Arguments remain separate native arguments. Shell interpretation occurs only for an
explicit tool invocation. Generated shell templates require shell quoting, including
safe handling of metadata in comments; HTML escaping is insufficient.

## Installer assets

`engine::bootstrap_tools::InstallerArtifact` supplies exact reviewed Java installer
and bootstrap assets with URL, size and SHA-256 assertions. Those maintainer pins
identify expected bytes; they are not upstream signatures. Bounded acquisition
verifies them before use.

`engine::server_runtime` prepares exact loader assets. Installer execution requires
an explicit grant and uses private staging and owned processes. A successful exit
alone cannot establish a complete server distribution; the candidate inventory and
launcher checks must also pass. Help, version and preview do not install tools.

### Installed payload paths

Packwiz-compatible metadata resolves `filename` relative to the metadata file's
parent. A `.index` component has no implicit meaning. For example, metadata at
`mods/.index/renderer.pw.toml` with `filename = "renderer.jar"` resolves to
`mods/.index/renderer.jar`; `filename = "../renderer.jar"` resolves to
`mods/renderer.jar`. The adapter normalizes relative components and rejects any
escape from the pack root, absolute/prefixed paths or invalid portable components.
It never silently strips `.index`. The [native parser](../../crates/empack-lib/src/engine/backend.rs) enforces these rules.

### Derivative digest observations

A backend digest is compared with acquired bytes when available, or with an exact
locked declaration using the same algorithm. A same-algorithm mismatch blocks the
build. When the algorithms differ, a reference export may use sufficient independent
locked evidence; it records that the backend digest was not compared. Backend URLs
and unmatched digests do not replace locked reference assertions. Unlisted content
still needs acquisition that verifies its own declaration.
