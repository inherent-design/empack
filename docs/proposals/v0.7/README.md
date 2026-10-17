# v0.7.0-beta development checkpoints

These draft proposals simplify authoring and operation UX over the released native
engine. They are not implemented commands or final design contracts. Baseline:
`aef85b2`, the source of v0.6.0-beta; verified local main and origin/main agree.

The supplied source review informed scope. Local inspection confirmed schema-3
placement in dependency records, default recipe selection for explicit consumers,
client-default instance updates, separate continuation flags and the absence of an
outdated command. No new runtime behavior has been implemented or tested by these
planning commits. Existing release tests are not evidence for proposed v0.7 features.

## Drafts

| Order | Checkpoint |
| --- | --- |
| 01 | [Workspace status and human-readable receipts](https://github.com/inherent-design/empack/pull/109) |
| 02 | [Compact authoring and explicit integration bindings](https://github.com/inherent-design/empack/pull/110) |
| 03 | [Named build profiles and effective configuration](https://github.com/inherent-design/empack/pull/111) |
| 04 | [Shared pending-download presentation and continuation](https://github.com/inherent-design/empack/pull/112) |
| 05 | [Publisher setup and understandable instance updates](https://github.com/inherent-design/empack/pull/113) |
| 06 | [Outdated assessment and explicit runtime migration plans](https://github.com/inherent-design/empack/pull/114) |

Each draft starts independently from the same main commit and changes one proposal
file. The order below describes implementation dependencies, not a stack of branches.
Rebase onto accepted prerequisites before implementation; do not merge unresolved
proposals into released usage docs. Expand each draft with its implementation and
execution evidence, then remove/fold the checkpoint into maintained final contracts.
This overview closes last after combined acceptance; it is not a permanent ledger.

## Delivery order

1. Agree on the shared context/report vocabulary (01) and authoring composition
   boundary (02). Build status first; review schema choices before changing codecs.
2. Complete compact authoring/bindings (02), then named profile storage/selection (03).
   Pending inputs (04) and publisher/instance ergonomics (05) can be developed against
   existing engine requests once report conventions are agreed.
3. Add read-only outdated assessment (06). Its runtime-migration apply path follows
   composed author inputs (02) and pending decision/acquisition handling (04).
4. Validate the combined CLI, library and consumer behavior below. Finalize usage,
   examples and callable API docs only when implementation and tests agree.

This is not another engine rewrite. Keep exact identities, immutable released
inventories, provider assertions, explicit trust, sequence floors, user-data policy,
prepared approval, cancellation ownership and recoverable publication. Preserve all
five consumers, content kinds, provider/local/URL inputs, optional/environment layers,
manual downloads, templates, runtime families and independent author batches.

## Decisions before dependent implementation

| Decision | Proposed default | Boundary |
| --- | --- | --- |
| Advanced integration | Optional portable bindings document | One normalized intent and one exact lock |
| Existing v0.6 author input | Explicit offline conversion if approved | No automatic migration or version upgrades |
| Profile selection | Named complete profiles; ambiguous shorthand errors | Never silently reset update authority |
| Pending commands | One grammar backed by existing durable records | No second pending-state authority |
| Instance update | Infer installed side/layout; explicit check/apply composition | No implicit enrollment or lower replay floor |
| Resolver breadth | Bounded assessment using existing selection and closure | No global solver claim or unbounded search |

Command spellings and compact YAML shown in checkpoint discussions remain proposals.
Do not turn the external review's examples into accepted schema by copying them into
usage. Record accepted decisions in the relevant implementation PR, not an additional
planning document. An optional converter is a deliberate exception to v0.6's
no-migration policy and needs explicit agreement before coding.

## Combined acceptance gate

- [ ] A new user creates a compact pack, adds provider and local content, inspects
  status, selects a named profile and imports its output into a real consumer.
- [ ] An imported multi-file/world pack retains roles, exact selections, layouts,
  optional/environment semantics and artifact inventories through composition.
- [ ] Build/import/sync/instance all explain and resume missing input through one
  grammar; wrong bytes, stale context and wrong roots leave prior live state intact.
- [ ] Outdated distinguishes pins, unavailable evidence and joint conflicts. A
  migration plan remains read-only until approval and preserves compatible choices.
- [ ] Publisher setup, stage, host, check and update are executable from the guide.
  Client/server updates infer recorded context and retain edits, worlds and trust floors.
- [ ] Failure/cancellation/interruption tests cover acquisition, document publication,
  instance activation and recovery. Native mechanisms are tested on supported hosts.
- [ ] Text and JSON errors expose known effects and next actions; no secret material,
  generated cache paths or internal serialization jargon is required for ordinary use.
- [ ] Existing consumer, runtime and provider checks remain; new behavior has negative
  and cross-command tests. Record actual commands, commit and intentional exclusions.
- [ ] Final candidate passes formatting, architecture checks, Clippy, doctests,
  relevant Python contracts, full workspace/native CI, strict probes and actual
  consumer acceptance. Startup evidence is not called gameplay validation.
- [ ] Update README, usage, publisher and API/design docs together; remove obsolete
  command handlers/tests only after their behavioral contracts are covered.
- [ ] Complete Greptile review on implemented scope, then qualify one immutable tree
  before merge/tag. No release or review loop is triggered by these planning drafts.
- [ ] Verify prerelease metadata and downloadable versioned assets after publication.
  On inspection, GitHub currently reports v0.6.0-beta with `isPrerelease=false` despite
  its beta tag and workflow intent; investigate that separately rather than assuming
  tag naming alone determines release classification.

## Scope limits

No new scheduler, general backtracking solver, game launcher, marketplace uploader,
self-updater, hosted service, background downloader or automatic world/config/JAR
migration. Java's current minimum-major check must not be advertised as full runtime
compatibility. Raise a separate proposal if a concrete acceptance failure needs a
larger change.

## Draft validation

All referenced implementation paths were checked against the baseline. Proposal diffs
and links are checked before publishing the drafts. No Rust execution is required
for these documentation-only commits, and none is claimed. Preserve CI path filters;
these checkpoints do not justify expensive platform builds or new runner classes.
