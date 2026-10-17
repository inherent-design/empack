# Outdated assessment and explicit runtime migration plans

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

There is no outdated report. Update previews mix candidate preparation with assessment,
and runtime migration currently starts by editing live author intent. Compatible
selection plus closure expansion is not a general satisfiability solver.

## Proposed contract

Add `outdated` with text/JSON rows for current lock, newest permitted candidate,
newest compatible candidate, newest observed upstream selection, and typed blockers.
“Newest” is qualified by catalog completeness and observation time. Unavailable,
rate-limited, bounded/incomplete and malformed evidence are not up-to-date results.
Pins remain pinned. Manual/local sources are unsupported or unchanged by policy,
not secretly converted to providers.

Keep per-root assessment separate from a joint candidate. Validate the joint set with
the existing required-closure machinery; report selection conflicts and incomplete
evidence without claiming no solution exists. Report formats expose stable codes and
human labels, with provider identity and exact pin retained separately.

Implement in two internal steps: first read-only assessment; then a proposed
`migrate --minecraft VERSION --loader FAMILY --plan` candidate against captured
inputs. Runtime and explicit replacement mappings are evaluated without editing live
authoring files. Apply requires revalidation and normal prepared-operation approval,
then publishes author inputs, bindings and lock together. Preserve compatible pins;
list unavailable dependencies and binding decisions explicitly. An assessment is not
an execution grant and cannot be replayed after its inputs change.

Publish release-policy choices supported by the existing resolver with explicit
semantics; distinguish provider stability policy from pack subscription channels.
Keep assessment bounded by shared request/deadline budgets and cooperative cancellation.
No network credential may enter reports or persisted candidates.

## Acceptance

- [ ] Pinned, newer, incompatible, unknown, manual and rate-limited rows are distinct.
- [ ] Deterministic ranking and incomplete catalog tests; no false all-clear on errors.
- [ ] Two individually eligible updates that conflict cannot yield a safe joint result.
- [ ] Assessment and migration preview preserve author files, lock, bindings and game bytes.
- [ ] Game/loader migration retains valid selections and requires explicit replacements.
- [ ] Changed source documents or provider evidence reject stale candidate application.
- [ ] Publication interruption recovers coherent documents without half-applied intent.
- [ ] Java minimum checks are described accurately; no claim of world/config/JAR porting.

## Non-goals

No unbounded backtracking/SAT rewrite, proof of global unsatisfiability, cross-provider
identity guessing, automatic world conversion or comprehensive Java compatibility
solver. A bounded backtracking resolver is a separate proposal only if concrete
fixtures demonstrate a need.

## Source entry points

- [crates/empack-lib/src/engine/providers/compatible.rs](https://github.com/inherent-design/empack/blob/aef85b2d14c50f1fc8a6c8032773cb50bed870a1/crates/empack-lib/src/engine/providers/compatible.rs)
- [crates/empack-lib/src/engine/providers/closure.rs](https://github.com/inherent-design/empack/blob/aef85b2d14c50f1fc8a6c8032773cb50bed870a1/crates/empack-lib/src/engine/providers/closure.rs)
- [crates/empack-lib/src/engine/runtime_catalog.rs](https://github.com/inherent-design/empack/blob/aef85b2d14c50f1fc8a6c8032773cb50bed870a1/crates/empack-lib/src/engine/runtime_catalog.rs)
- [crates/empack-lib/src/application/engine_host/cli/update.rs](https://github.com/inherent-design/empack/blob/aef85b2d14c50f1fc8a6c8032773cb50bed870a1/crates/empack-lib/src/application/engine_host/cli/update.rs)
- [crates/empack-lib/src/application/engine_host/cli/synchronization.rs](https://github.com/inherent-design/empack/blob/aef85b2d14c50f1fc8a6c8032773cb50bed870a1/crates/empack-lib/src/application/engine_host/cli/synchronization.rs)
- [crates/empack-lib/src/engine/api.rs](https://github.com/inherent-design/empack/blob/aef85b2d14c50f1fc8a6c8032773cb50bed870a1/crates/empack-lib/src/engine/api.rs)

## Delivery boundary

Read-only assessment can start after checkpoint 01. Migration application depends on checkpoint 02 composition and checkpoint 04 handling of unresolved inputs.

Before ready-for-review, replace proposed spellings with tested help examples, update
applicable final-state contracts and usage, and attach revision-specific execution
evidence to the PR. Remove or fold this temporary checkpoint into the final docs
when implementation lands; do not retain a second authoritative specification.
