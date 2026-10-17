# Workspace inspection

`engine::status::inspect_workspace` returns a `WorkspaceStatus` without execution
or publication authority. The CLI renders the same value as text or schema-1 JSON.
The report contains its selected root, detected contexts, metadata state, available
human labels and exact identities, update ownership, and findings with stable codes
and next-command arguments. `content_verified` is false: no game content is scanned.

| Context | Inspected input | Meaning |
| --- | --- | --- |
| Author | `empack.yml`, optional `empack.bindings.yml`, `empack.lock` | Validated intent and lock agreement; the exact value is the raw lock document SHA-256 |
| Instance | Root-bound instance record, exact retained release, optional subscription | Completed pack version, release ID, runtime, game directory and enrolled update owner |
| Publisher | Native `dist/releases` and `dist/channels` directories | Recognized storage only; release validity and authentication remain unavailable |

Missing, invalid, unavailable and stale states are separate. A publisher directory
name never establishes release trust. Multiple contexts produce `context-ambiguous`;
explicit selection controls presentation and does not grant mutation authority.

Inspection opens existing native coordination only, refuses unsafe ancestors and
non-regular documents, bounds document bytes, checks observed file identity and
modification during reads, and rechecks the workspace binding. It does not create
state directories or repair malformed records. It is a point-in-time metadata view,
not a snapshot guarantee against uncooperative external writers.

A hot publication journal produces `publication-recovery-required` before live
project records are interpreted. A retained runtime lease produces
`runtime-retirement-unconfirmed`. Continuation stores produce `pending-input` or
`pending-unavailable`; opaque presence is not evidence that a pending recipe is
valid or ready. Resumption remains responsible for binding and input verification.

Human presentation includes labels alongside hashes. JSON preserves typed context
and record states plus stable finding codes. Future additive report fields retain
schema 1; changing existing field meanings requires a schema revision. No report
contains signing keys or grants permission to reuse an earlier approval.
