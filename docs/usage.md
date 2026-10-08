# v0.5 command contract

The CLI translates user input into typed engine requests. It does not install
content, mutate project documents or publish artifacts directly.

| Operation | Meaning |
| --- | --- |
| initialize/import | Prepare a complete project before replacing managed files |
| add/remove | Resolve logical identity and plan exact managed changes |
| sync | Restore the exact locked selection without implicit upgrades |
| update | Refresh eligible selections deliberately |
| adopt | Incorporate selected observed drift into intent and resolution |
| build | Package satisfied intent, or an explicitly chosen observed snapshot |
| clean | Remove owned outputs or eligible cache objects through scoped plans |
| inspect/recover | Classify interrupted publication and apply an approved recovery |

Preview uses the same resolver and planner with read-only durable storage
capabilities. Normal execution prepares, answers typed decisions, stages, verifies
and publishes. Batches default to all requested items verifying before publication.
Project batches report partial completion only under an explicit independent-batch policy.
Disposable cache eviction reports any completed removals if later maintenance fails;
it does not claim an atomic transaction across cache objects.

Provider MD5 compatibility retains weaker-integrity evidence. It never labels an
internally computed SHA-256 as authentication of the source. Optional requirements,
side layers and declared destinations must survive supported imports and builds.

The [API contract](design/api.md) defines requests and outcomes. Exact CLI spelling
and normalized document examples are finalized with the implementation; proposed
requests must not be advertised as working commands before their tests pass.
There is no requirement to retain old flags or manifest formats.

## Implemented recovery command

`empack recover` inspects interrupted engine publication without requiring valid
project documents. `empack recover finish` completes its approved changes;
`empack recover restore` restores retained preimages. A new-project creation can be
finished, but restore cannot authorize deleting that project root.

Use `--dry-run` to inspect the exact recovery footprint before execution. Recovery
requires confirmation or `--yes`. `--operation <id>` binds automation to the operation
reported by inspection; a different or no-longer-pending operation fails.

```sh
empack --workdir ./pack recover
empack --workdir ./pack --dry-run recover finish --operation <id>
empack --workdir ./pack --yes recover finish --operation <id>
```

`--state-dir` / `EMPACK_STATE_DIR` selects durable engine operation storage. Its
default is the platform application-data directory's `operations` child, separate
from disposable caches. Relative selections resolve from the invocation directory.
Inspecting missing state creates neither host state nor a project directory. This
command handles engine journals; it does not reinterpret older interruption markers.

`--cache-dir` / `EMPACK_CACHE_DIR` selects disposable storage; native verified
content lives in its `content-v1` child. Relative paths resolve from the invocation
directory. Build preparation can read and verify existing objects without creating
or changing the cache. Approved builds retain verified bytes for subsequent offline
builds. Missing, busy or corrupt cache objects leave the original content obligation
in place. Cache hits preserve the source's original integrity evidence, including
MD5 compatibility evidence. `clean cache` uses this same selected root.

For a build waiting on manual content, `--downloads-dir PATH --wait-downloads SECONDS`
scans the selected directory for at most 1–3600 seconds. The wait starts after approving
saved continuation state. It accepts renamed files only when their original assertions
verify, retains new verified inputs, then presents the resulting build plan for approval.
`--yes` answers both approvals. Timeout or interruption leaves the saved recipe intact;
`build --continue` resumes it. Replacing the recipe or changing captured project inputs
stops the wait. Dry runs and declined prompts do not wait or save state. Scans share a
cumulative byte allowance; unrelated files cannot reset it on each poll.

## Synchronization and remote content

`empack sync` reconciles authored intent and exact recorded selections. It restores
local and archive-member content, resolves unsatisfied roots, and retains remote
references without downloading their payloads. It does not remove installations
merely because they are absent from the explicit-root manifest.

`empack sync --materialize` also acquires every remote reference. The host displays
these obligations and requires confirmation or `--yes` before acquisition. Provider
lookup uses the exact recorded pin and file role; refreshed locators cannot replace
original digest or size assertions. After verification, the native engine previews
and publishes the complete file change. Failed downloads or unresolved manual
references publish nothing. Local sources still use captured filesystem evidence.

```sh
empack --dry-run sync --materialize
empack --yes sync --materialize
```

The materialization preview performs no remote payload downloads. It reports
acquisition obligations alongside the recorded synchronization plan; it does not
claim those bytes have verified. Ordinary `sync` after materialization retains the
installed bytes and does not refresh their versions. Explicit materialization
currently reacquires remote references; persistent content-store reuse is a
separate implementation step.

## Exit status

The executable maps typed failures to status codes: success `0`, general failure
`1`, invalid input or missing authorization `2`, network failure `3`, missing provider
content `4`, and interruption `130`. Wrapping an error with operation context does
not change its status. Incidental words in a diagnostic or imported metadata do not
select an exit code.

`empack adopt KEY...` accepts installed changes for tracked local/member files, URL files
and provider files in any supported side layer. It verifies bytes before changing the lock and leaves payloads
untouched. Available provider metadata must name the same project and an exact version. Without
metadata, provider identification must verify the observed bytes and file role; adoption
does not choose the newest release or override an authored pin. Use `--dry-run` to inspect
the proposed document changes. URL adoption keeps its declared origins and side placements without downloading remote
bytes; authored content pins remain binding. When the lock is absent, select every
root declared in `empack.yml`. Each placement must already contain the verified bytes;
adoption creates the first lock without installing payloads. Non-vanilla runtimes need
an exact authored loader version. Provider roots need an authored pin, exact observed
metadata, or explicit placements that provider byte identification can verify. A malformed
or stale existing lock remains an error.

Use `empack adopt --from INPUT...` to describe content that is already installed but
not tracked. Local files use the same content-type and folder rules as add. A direct
HTTPS URL records that origin while verifying bytes at the expected installed destination;
it does not download a replacement. Provider selectors require `--version-id` or
`--file-id`. A supplied local file with `--platform` can instead establish the provider
selection through byte identification. `--file-plan` preserves explicit provider roles,
renamed destinations, side layers and optional requirements. Missing or differing copies
fail the whole operation. Source options cannot be combined with tracked-key selection.

```sh
empack adopt --from pack/mods/example.jar --dry-run
empack adopt --from renderer --platform modrinth --version-id VERSION --file-plan files.yml --yes
```

Use `empack clean continuation --dry-run` to inspect saved-build cleanup, then
`empack clean continuation --yes` to discard that project's recipe. This works for
stale or malformed saved recipes and leaves content-cache objects and recovery journals
in place. `clean all` keeps pending recipes; request `continuation` explicitly.

## Identify a supplied file

`empack add --platform modrinth ./renamed.zip` identifies the supplied bytes before
choosing their content type and destination. A unique provider kind supplies the type;
`--type` is needed for an ambiguous selection and must agree with provider evidence.
The published file keeps the supplied bytes and exact provider pin. Unknown or ambiguous
identities fail without changing the project. Omit `--platform` to choose direct-file
tracking deliberately; direct ZIP inputs still require a type.

## Select companion files

Use `empack add --platform modrinth PROJECT --file-plan ./files.yml` when a provider
selection contains companion files or needs explicit destinations. A plan applies to
one project and uses provider filenames as exact role names. It can also accompany
supplied-file identification. Paths resolve from the invocation directory.

```yaml
schema: 1
environment: {client: required, server: unsupported}
files:
  renderer.jar:
    - destination: mods/renderer.jar
      layer: common
      environment: {client: required, server: unsupported}
  resources.zip:
    - destination: resourcepacks/renderer-assets.zip
      layer: common
      environment: {client: required, server: unsupported}
```

Each file can have several placements. Layers are `common`, `common-override`,
`client`, or `server`; destinations are relative to that layer. A side requirement
is `required`, `unsupported`, or an optional choice such as
`{optional: extra-art, default-enabled: false, description: Extra artwork}`.
The top-level environment declares the dependency's participation; individual files
retain their own requirements. Required companions cannot be omitted. Unknown filenames,
unsafe paths, conflicting participation and publication collisions fail the batch.
`--dry-run` resolves and previews the plan without publishing project changes.

## Direct world archives

Configure `layout.world` in `empack.yml` to choose the destination directory, then add
a local ZIP with `empack add --type world ./adventure.zip`. The archive must contain
one world, identified by a nonempty `level.dat`, with no files outside that world's
root. Members are installed beneath the configured directory and archive stem.
They remain one dependency with individual byte assertions. Sync preserves those
assertions; explicit update or adoption accepts selected local changes. Removal leaves
untracked neighboring files intact.

For a direct HTTPS archive, add `--download-as-local` to choose tracked local ownership.
The original archive is verified before extraction. This flag does not authorize a
failed provider lookup to become unidentified content. Provider-owned world addition
remains unavailable until its member semantics are implemented.

## Named file placement

Multi-file dependencies record each file's placements in `empack.yml`, independently
of the source path or provider filename used to acquire it:

```yaml
placement:
  files:
    settings:
      - destination: config/settings.toml
        layer: common
        environment: {client: required, server: required}
```

For a local member group, `settings` must also appear in `source.members`. A source
may be `seeds/settings.toml` while its installation remains `config/settings.toml`.
Update preserves that distinction. A flat placement list describes copies of one file;
use named roles for multiple files. Provider `--file-plan` and world/import workflows
record these associations automatically.
