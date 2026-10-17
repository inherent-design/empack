# Shared pending-download presentation and continuation

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

Build, import, sync and instance workflows expose different continuation dialects
for the same user action: supplying exact missing bytes.

## Proposed contract

Provide `pending list`, `pending show ID`, `pending resume ID --file KEY=PATH` and
`pending discard ID` in the selected root, with consistent text and versioned JSON.
Short display IDs resolve uniquely to full operation identity; ambiguity is an error.
Keep operation kind and root identity in the record. Do not use display filenames
as authoritative keys or permit an ID to resume work in a different root.

A shared read model reports human dependency name, exact provider selection, expected
filename/digest/size when available, permitted download page, supplied candidate,
verification state and next command. Missing facts remain explicitly unknown.
Adapters read existing durable build/import/sync/instance records; they do not invent
a second authoritative pending store. Each adapter reparses and prepares through its
existing engine route, then requests a fresh grant.

Unify browser opening and bounded Downloads scanning as explicit assistance options.
Never open browsers during preview, trust a unique extension match, or count an
unverified candidate as complete. Timeout/cancellation retains resumable ownership.
Discard removes only the selected owned pending inputs after approval, not recovery
journals, active leases, completed instances or user downloads.

Existing spellings may be thin aliases during this release if inexpensive. Remove
duplicated handlers rather than keeping independent implementations. Saved v0.6
records need explicit support or a diagnostic explaining safe continuation/discard;
do not silently lose retained content during schema cutover.

## Acceptance

- [ ] Build/import/sync/instance all resume through the shared grammar.
- [ ] Ambiguous IDs/keys, wrong roots, wrong digests and stale records reject safely.
- [ ] Tree snapshots prove list/show/preview preserve durable state and never open URLs.
- [ ] Interrupted or timed-out assistance retains verified files and original recipes.
- [ ] Multiple operations can coexist without crossing selections or file associations.
- [ ] Discard preserves active leases, recovery data and neighboring unknown files.
- [ ] Continued work revalidates source and trust and obtains new approval.

## Non-goals

No download daemon, automatic browser scraping, bypass of provider restrictions or
unification of fundamentally different publication journals into acquisition state.

## Source entry points

- [crates/empack-lib/src/application/cli.rs](https://github.com/inherent-design/empack/blob/aef85b2d14c50f1fc8a6c8032773cb50bed870a1/crates/empack-lib/src/application/cli.rs)
- [crates/empack-lib/src/application/engine_host/build/assistance.rs](https://github.com/inherent-design/empack/blob/aef85b2d14c50f1fc8a6c8032773cb50bed870a1/crates/empack-lib/src/application/engine_host/build/assistance.rs)
- [crates/empack-lib/src/application/engine_host/cli/import.rs](https://github.com/inherent-design/empack/blob/aef85b2d14c50f1fc8a6c8032773cb50bed870a1/crates/empack-lib/src/application/engine_host/cli/import.rs)
- [crates/empack-lib/src/application/engine_host/cli/synchronization/materialization.rs](https://github.com/inherent-design/empack/blob/aef85b2d14c50f1fc8a6c8032773cb50bed870a1/crates/empack-lib/src/application/engine_host/cli/synchronization/materialization.rs)
- [crates/empack-lib/src/engine/api/instance/continuation.rs](https://github.com/inherent-design/empack/blob/aef85b2d14c50f1fc8a6c8032773cb50bed870a1/crates/empack-lib/src/engine/api/instance/continuation.rs)

## Delivery boundary

Uses status/report conventions from checkpoint 01. Can proceed independently of authoring schema changes; integrate profile continuation semantics from checkpoint 03.

Before ready-for-review, replace proposed spellings with tested help examples, update
applicable final-state contracts and usage, and attach revision-specific execution
evidence to the PR. Remove or fold this temporary checkpoint into the final docs
when implementation lands; do not retain a second authoritative specification.
