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

`empack adopt KEY...` accepts installed changes for tracked local files and provider files
with common-layer metadata. It verifies bytes before changing the lock and leaves payloads
untouched. Provider metadata must name the same project and an exact version; adoption
does not choose the newest release or override an authored pin. Use `--dry-run` to inspect
the proposed document changes. URL, side-layer and untracked adoption need further frontend
support.

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
