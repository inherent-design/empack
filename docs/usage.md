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
