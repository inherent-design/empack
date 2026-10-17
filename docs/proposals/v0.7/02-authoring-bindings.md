# Compact authoring and explicit integration bindings

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

Schema 3 serializes source, version policy, environment and advanced placement in
each dependency. Ordinary packs inherit the verbosity needed for unusual imports.

## Proposed contract

Keep `empack.yml` as the ordinary editing surface. Add optional, version-controlled
`empack.bindings.yml` for non-default file-role selection, archive-member mapping,
destination/layer overrides and explicit integration decisions. `empack.lock` remains
the sole exact-resolution output. Cache paths and observed installations never
become binding authority. Ordinary defaults are derived, not serialized everywhere.

Compose decoded author input plus bindings into the existing `ProjectIntent` before
resolution. Track raw revisions of both documents (including expected absence) in
the captured read set. Bind the lock to canonical composed meaning. Preserve
per-document provenance for diagnostics; formatting-only changes must not cause
new provider selections, but still invalidate an overwrite prepared earlier.

Bindings reference logical dependency keys and stable file roles. Reject dangling
keys, duplicate/conflicting rules, unsafe destinations and ambiguous member matches.
Version-bearing filenames must not silently become permanent role identities.
Deletion/rename of a dependency updates its bindings in the same approved publication.
Init, imports, add, update, adopt, remove, sync and build all consume composition;
none may keep decoding a separate legacy intent behind the new layer.

### Proposed surface

A common provider dependency should need a selector, optional side and optional
version policy, rather than generated placement records. Defaults preserve current
semantics. Exact syntax and schema numbers require review before codec work; do not
copy the illustrative syntax in the external review into production unvalidated.

An explicit `empack project convert --dry-run` may move v0.6 intent into the new
representation. It must retain pins, evidence, optional groups, file roles and
inventories without downloads or version selection. Conversion is never automatic.
This is a bounded conversion of the immediately preceding schema, not a framework
for all historical schemas. Confirm this exception to the existing no-migration
policy before implementing the converter; normal v0.7 composition does not depend
on supporting older schemas.

## Acceptance

- [ ] Golden fixtures for provider, URL, local, multi-file and provider-world inputs.
- [ ] Default composition equals existing intent and output inventories.
- [ ] Binding changes invalidate affected resolution; comment edits preserve meaning.
- [ ] Concurrent edits, newly created bindings and deleted bindings reject stale plans.
- [ ] Rename/remove/import/update retain or explicitly reject ambiguous role mappings.
- [ ] Failure between document publications recovers a coherent author/bindings/lock set.
- [ ] If approved, conversion is idempotent, offline and inventory-preserving.
- [ ] Normal manifests stay compact; advanced capabilities remain reachable.

## Non-goals

No second installed-state database, automatic provider switching, filename heuristics
that silently accept ambiguity, or full resolver/runtime rewrite.

## Source entry points

- [crates/empack-core/src/model.rs](https://github.com/inherent-design/empack/blob/aef85b2d14c50f1fc8a6c8032773cb50bed870a1/crates/empack-core/src/model.rs)
- [crates/empack-lib/src/engine/documents.rs](https://github.com/inherent-design/empack/blob/aef85b2d14c50f1fc8a6c8032773cb50bed870a1/crates/empack-lib/src/engine/documents.rs)
- [crates/empack-lib/src/engine/documents/intent.rs](https://github.com/inherent-design/empack/blob/aef85b2d14c50f1fc8a6c8032773cb50bed870a1/crates/empack-lib/src/engine/documents/intent.rs)
- [crates/empack-lib/src/engine/providers/addition.rs](https://github.com/inherent-design/empack/blob/aef85b2d14c50f1fc8a6c8032773cb50bed870a1/crates/empack-lib/src/engine/providers/addition.rs)
- [crates/empack-lib/src/engine/project.rs](https://github.com/inherent-design/empack/blob/aef85b2d14c50f1fc8a6c8032773cb50bed870a1/crates/empack-lib/src/engine/project.rs)
- [crates/empack-lib/src/engine/publication.rs](https://github.com/inherent-design/empack/blob/aef85b2d14c50f1fc8a6c8032773cb50bed870a1/crates/empack-lib/src/engine/publication.rs)

## Delivery boundary

Agree on composition and schema boundaries before named profiles and assessment apply paths land. Status implementation can proceed independently.

Before ready-for-review, replace proposed spellings with tested help examples, update
applicable final-state contracts and usage, and attach revision-specific execution
evidence to the PR. Remove or fold this temporary checkpoint into the final docs
when implementation lands; do not retain a second authoritative specification.
