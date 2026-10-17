# Workspace status and human-readable receipts

## Status and evidence

Draft proposal for v0.7.0-beta, not implemented behavior. This checkpoint is based
on local source inspection at `aef85b2` (released v0.6.0-beta). The supplied review
is input to the proposal, not execution evidence. This PR initially changes only
this checkpoint; its implementation checks remain open.

Retain the native engine, exact provider/content identities, original integrity
assertions, explicit approval, all-requested default, owned cancellation and
recoverable publication. A CLI convenience must not create an alternate mutation
path. No packwiz integration is reintroduced.

## Problem

Users must infer whether a directory is an author checkout, publisher workspace or
installed instance, and interpret hashes without enough human context. `version`
does not describe a pack, and `instance inspect` is not authoring status.

## Proposed contract

Introduce read-only `empack status` with optional `--context author|publisher|instance`
and versioned `--json` output. Detect context from validated native records without
creating them. If contexts overlap, show the candidates and require a selection;
never infer mutation authority from a directory name. Status performs no network
request, recovery action, cache publication or document rewrite.

A report includes context/root, pack name and human version where known, exact
selection/release identity, runtime, game directory, update owner, lock agreement,
pending work and recovery/runtime blockers. Missing or malformed records are
reported as unavailable/invalid, never as an empty healthy project. Inspect only
metadata needed for status; full byte verification is a separately requested check.

Use a shared presentation value such as `WorkspaceStatus` with typed findings and
stable codes. Each context adapter supplies facts; the formatter chooses human or
JSON presentation. Operation receipts add human labels alongside exact identities,
known effects, remaining inputs and a runnable next command. Do not expose secrets,
private signing paths or arbitrary debug dumps.

## Acceptance

- [ ] Empty, author, publisher, instance, mixed-context and corrupted-record fixtures.
- [ ] Before/after tree comparison proves status creates no durable files.
- [ ] Locked/absent/stale/invalid states remain distinguishable in text and JSON.
- [ ] Pending acquisition and interrupted publication have distinct next actions.
- [ ] Client and server reports name the actual game directory and update owner.
- [ ] CLI and external-library tests use the same report schema; help explains the
  two update meanings and that neither updates the executable.

## Non-goals

No automatic repair, implicit provider lookup, full filesystem scan, global machine
inventory or generic event bus. Later checkpoints add their own report fields through
versioned, additive presentation changes.

## Source entry points

- [crates/empack-lib/src/application/cli.rs](https://github.com/inherent-design/empack/blob/aef85b2d14c50f1fc8a6c8032773cb50bed870a1/crates/empack-lib/src/application/cli.rs)
- [crates/empack-lib/src/application/engine_host.rs](https://github.com/inherent-design/empack/blob/aef85b2d14c50f1fc8a6c8032773cb50bed870a1/crates/empack-lib/src/application/engine_host.rs)
- [crates/empack-lib/src/application/engine_host/instance.rs](https://github.com/inherent-design/empack/blob/aef85b2d14c50f1fc8a6c8032773cb50bed870a1/crates/empack-lib/src/application/engine_host/instance.rs)
- [crates/empack-lib/src/engine/documents.rs](https://github.com/inherent-design/empack/blob/aef85b2d14c50f1fc8a6c8032773cb50bed870a1/crates/empack-lib/src/engine/documents.rs)
- [crates/empack-lib/src/engine/layout.rs](https://github.com/inherent-design/empack/blob/aef85b2d14c50f1fc8a6c8032773cb50bed870a1/crates/empack-lib/src/engine/layout.rs)

## Delivery boundary

Independent first implementation slice. Its report conventions are shared by pending inputs, assessments and publisher UX.

Before ready-for-review, replace proposed spellings with tested help examples, update
applicable final-state contracts and usage, and attach revision-specific execution
evidence to the PR. Remove or fold this temporary checkpoint into the final docs
when implementation lands; do not retain a second authoritative specification.
