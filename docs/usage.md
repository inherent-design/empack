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

## Explicit partial dependency batches

`add --continue-independent` and `update --continue-independent` allow independently
verified groups to publish together when another resolved group fails preparation.
The preview names ready and blocked logical roots. Shared identities, required dependency
chains, overlapping destinations and source reads keep connected requests in one group.
A blocked group retains its previous intent, exact selections and files.

```sh
empack --yes add first.jar second.jar --continue-independent
empack --yes update first second --continue-independent
```

Partial publication returns a nonzero exit status even though the ready groups were
published. Inspect the named blocked groups before retrying; do not assume failure means
nothing changed when this policy was explicitly selected. Without the flag, every requested
item must verify before anything publishes. Preview and declined approval publish nothing.

The policy applies after source identity and dependency resolution. Missing or ambiguous
sources and unresolved dependency evidence still stop the whole request: they do not
establish a safe independent footprint. Network and local acquisition needed for resolution
must also complete before candidate grouping. The option does not enable partial imports,
builds or automatic orphan removal.

## Verified import file associations

`init --from` accepts repeated `--import-file SELECTOR=PATH` arguments for files
already downloaded by the user. A declared destination such as
`resourcepacks/theme.zip` selects that exact download obligation. Provider filenames
must identify one file; when ambiguous, use the exact selector printed in the missing
input diagnostic. Relative source paths resolve from the invocation directory.

```sh
empack --yes init --from ./pack.mrpack \
  --import-file resourcepacks/theme.zip=./Downloads/renamed.zip \
  --import-optional-default true ./project
```

Each supplied file must match the archive or exact provider selection's original
size and digests. Associations preserve provider or URL identity, destination and
client/server participation. They cannot replace embedded archive members. Unknown,
ambiguous, duplicate and symlinked sources fail before project publication. Preview
verifies selected inputs without creating a project or durable host state.

`--import-local-files` is a separate conversion choice: it retains verified downloads
as authored local files. Supplying `--import-file` alone does not request that conversion.
When files need manual acquisition, an approved invocation saves the source archive
and verified associations in the selected state directory. It leaves the destination
unchanged and exits unsuccessfully because the import is incomplete. Resume with the
same destination and any additional exact file associations:

```sh
empack --yes init --continue ./project \
  --import-file 'EXACT_SELECTOR=./Downloads/fixture.jar' \
  --import-optional-default true
```

Use the exact selector printed by your import. Continuation reads the retained archive,
resolves provider facts again, verifies supplied and retained bytes, and prepares a new
publication for approval. It works after the original archive is removed. Changed source
assertions or destination documents make the saved import stale; refreshed download URLs
alone do not. Conversion choices and replacement approval must be supplied again.

`init --continue --dry-run ./project` does not update saved state. To abandon an import,
use `empack --workdir ./project clean import --dry-run`, then repeat with `--yes` to
discard its record. The destination need not exist. Cleanup also accepts stale or malformed
records, checks that their bytes have not changed since inspection, and retains source
content. `clean all` does not discard pending imports.

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

`build --open-downloads` opens public provider pages for unresolved exact selections,
after continuation is approved and saved. It can be combined with `--wait-downloads`.
`--yes` alone never opens a browser. Preview and declined plans have no desktop effect.
Pages are resolved through the provider's canonical project and exact-file ownership;
installer messages, manifest instructions and signed download locators are not browser
commands. Each selection opens once per invocation, with a limit of 16 pages. If a
provider cannot supply a verified page, the saved build remains available for explicit
file association. The desktop application has its own lifetime; only its launcher is
subject to empack's timeout and cancellation.

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
references publish nothing. Missing manual inputs retain the exact candidate selections
and any verified supplied bytes outside the disposable cache. Local sources still use
captured filesystem evidence.

```sh
empack --dry-run sync --materialize
empack --yes sync --materialize
```

To supply restricted files, use the exact dependency and file role printed by sync:

```sh
empack --yes sync --continue --file example/primary=~/Downloads/example.jar
```

Each supplied file must match the original digest and size assertions. You can supply
files over several invocations; the project changes only when every obligation verifies
and publication is approved. Continuation refuses changed intent/lock documents or a
replaced project directory. `sync --continue --dry-run` leaves the saved record unchanged.
Use `clean sync --dry-run` to inspect abandonment, then `clean sync --yes` to discard the
selected record, including stale or invalid records. Ordinary `clean all` retains it.

After completing or discarding saved operations, `clean retained --dry-run` previews
reclaiming their stored inputs; repeat with `--yes` to apply. A category with any remaining
build, import or sync record is preserved in full. Cleanup leaves active private content
leases, unrelated files and publication recovery data intact. Discard records first,
then make a separate retained-input cleanup request.

The materialization preview performs no remote payload downloads. It reports
acquisition obligations alongside the recorded synchronization plan; it does not
claim those bytes have verified. Ordinary `sync` after materialization retains the
installed bytes and does not refresh their versions. Explicit materialization
checks verified cached content against the original assertions before acquiring missing
remote bytes.

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

## World archives

Configure `layout.world` in `empack.yml` to choose the destination directory, then add
a local ZIP with `empack add --type world ./adventure.zip`. The archive must contain
one world, identified by a nonempty `level.dat`, with no files outside that world's
root. Members are installed beneath the configured directory and archive stem.
They remain one dependency with individual byte assertions. Sync preserves those
assertions; explicit update or adoption accepts selected local changes. Removal leaves
untracked neighboring files intact.

For a direct HTTPS archive, add `--download-as-local` to choose tracked local ownership.
The original archive is verified before extraction. This flag does not authorize a
failed provider lookup to become unidentified content.

A CurseForge world retains provider ownership: use `--platform curseforge --type world`
with its project selector. Configure `layout.world` or supply `--file-plan` with the
selected archive role and destination roots. The default root uses the provider slug;
an explicit root stays fixed when updating. Empack verifies the archive before reading
its members and retains the original archive digests separately from member hashes.
An explicit file pin remains pinned. Adoption verifies installed members against that
selected archive; it cannot attribute edited local bytes to the original provider file. When
no download URL is available, supply the downloaded ZIP with `--platform curseforge`
and `--type world`; identification verifies the exact provider file before extraction.
Existing cache entries can satisfy the same original archive assertions. The source
archive's filename may change on update without moving the configured destination root;
only the previous tracked member inventory can be retired.

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
