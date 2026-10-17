# Publisher setup and understandable instance updates

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

Publisher setup requires external key-format conversion. Instance updates require
remembering a server-side flag and a separate channel observation step.

## Proposed contract

Add `release init` to create an explicit publisher workspace/key setup using the
existing Ed25519 implementation. Keys are created exclusively with restrictive native
permissions outside hosted/export content. No overwrite, private-key logging or
implicit trust enrollment. Show public fingerprint, public key and deployment steps.
Preview generates no durable key. Failure cannot leave a success message with missing
key or configuration; recovery must never delete a pre-existing key.

Add `instance check-updates` as the human entry point for authenticated observation.
Explain before approval that it saves an anti-replay floor even if installation does
not follow. Preview saves nothing and does not authorize later installation.
`instance update` applies the saved checked release; an explicit `--check` convenience
may compose observation and update through existing requests and fresh approvals.
Do not change the meaning silently or conflate a connection failure with up-to-date.

Infer side/layout from an existing validated instance. Explicit conflicting options
fail before acquisition. First installation still requires a deliberate environment
selection or a clearly displayed documented default. Retain trust/expiry checks,
sequence floors across failed installs and rollback, offline policy, runtime leases
and stopped-launcher requirements.

Show installed and offered human versions alongside exact release IDs, observation
sequence, expiry and update owner. Publisher output must say which local files to
upload and in what order. Expiry renewal is a new signed observation, never an
unannounced daemon or relaxed expiry check.

## Acceptance

- [ ] Key generation/sign/verify round-trip, existing-path refusal, native permissions.
- [ ] Preview and failure do not expose or accidentally publish signing material.
- [ ] Server updates infer server; an explicit client override fails unchanged.
- [ ] Check-only saves authenticated floor but changes no game content.
- [ ] Replay, expired/revoked trust, unavailable metadata and installation failure
  have distinct diagnostics; rollback never lowers the floor.
- [ ] Combined check/apply preserves fresh approval and all-requested activation.
- [ ] Publisher and player walkthroughs execute in disposable roots without external
  key conversion, and preserve local edits/worlds through update and rollback.

## Non-goals

No hosted service, marketplace uploader, executable self-update, key escrow, automatic
trust-on-first-use, or change to the signed release protocol without a separate need.

## Source entry points

- [crates/empack-lib/src/engine/release/signing.rs](https://github.com/inherent-design/empack/blob/aef85b2d14c50f1fc8a6c8032773cb50bed870a1/crates/empack-lib/src/engine/release/signing.rs)
- [crates/empack-lib/src/engine/instance/subscription.rs](https://github.com/inherent-design/empack/blob/aef85b2d14c50f1fc8a6c8032773cb50bed870a1/crates/empack-lib/src/engine/instance/subscription.rs)
- [crates/empack-lib/src/application/engine_host/instance.rs](https://github.com/inherent-design/empack/blob/aef85b2d14c50f1fc8a6c8032773cb50bed870a1/crates/empack-lib/src/application/engine_host/instance.rs)
- [crates/empack-lib/src/application/engine_host/release.rs](https://github.com/inherent-design/empack/blob/aef85b2d14c50f1fc8a6c8032773cb50bed870a1/crates/empack-lib/src/application/engine_host/release.rs)
- [crates/empack-lib/src/engine/api/subscription.rs](https://github.com/inherent-design/empack/blob/aef85b2d14c50f1fc8a6c8032773cb50bed870a1/crates/empack-lib/src/engine/api/subscription.rs)
- [docs/publishing.md](https://github.com/inherent-design/empack/blob/aef85b2d14c50f1fc8a6c8032773cb50bed870a1/docs/publishing.md)

## Delivery boundary

Uses checkpoint 01 report conventions; independent of the authoring schema. Pending manual acquisition joins checkpoint 04 at integration.

Before ready-for-review, replace proposed spellings with tested help examples, update
applicable final-state contracts and usage, and attach revision-specific execution
evidence to the PR. Remove or fold this temporary checkpoint into the final docs
when implementation lands; do not retain a second authoritative specification.
