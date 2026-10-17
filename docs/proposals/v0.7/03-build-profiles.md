# Named build profiles and effective configuration

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

`build prism` currently constructs a default Prism recipe, while `build` uses authored
recipes. Naming a consumer can therefore change delivery or update policy silently.

## Proposed contract

Store named profiles in authoring configuration. Each profile contains one complete
recipe: consumer, delivery, environment, update authority and archive format. Keep an
explicit default profile selection. Names are portable logical identifiers, not output
paths or authority tokens.

Proposed commands are `build --profile NAME` (repeatable) and `build --profile NAME
--explain`. The latter is read-only and shows effective values and their origins,
output paths, recipient empack requirement, Java expectations, optional decisions,
and known manual inputs. Unknown acquisition facts stay unknown unless separately
resolved; explanation is not a successful build or byte verification.

No-argument builds use authored defaults. Explicit consumer shorthand may remain,
but must either select exactly one configured profile or clearly request an ad hoc
recipe; never silently reset a matching configured profile. Multiple matches require
an explicit name. Review the final shorthand before implementation. CLI overrides
produce an explicit effective recipe and never persist themselves implicitly.

Resolve profiles once into existing `Recipe` values and build requests. Saved
continuation retains those effective recipes, so later profile edits cannot mutate
the saved build. Detect output collisions before acquisition. Preserve combined
verification/publication across requested outputs and all five consumers.

## Acceptance

- [ ] Configured empack-update Prism profile stays managed when selected by name.
- [ ] Default, named, ad hoc, missing and ambiguous profile selection fixtures.
- [ ] Explain/text/JSON agrees with the recipe actually executed.
- [ ] Overrides are visible and invalid consumer/delivery/update combinations fail early.
- [ ] Multiple profiles with colliding output names fail without overwriting artifacts.
- [ ] Resume uses saved recipes or reports stale context, never new implicit defaults.
- [ ] Reference exports disclose possible author-side acquisition and recipient requirements.
- [ ] Existing consumer inventory, optionality and source-integrity tests remain passing.

## Non-goals

No new distribution format, marketplace uploader, arbitrary recipe scripting, hidden
profile inheritance or change to update trust.

## Source entry points

- [crates/empack-lib/src/application/engine_host/build.rs](https://github.com/inherent-design/empack/blob/aef85b2d14c50f1fc8a6c8032773cb50bed870a1/crates/empack-lib/src/application/engine_host/build.rs)
- [crates/empack-lib/src/application/cli.rs](https://github.com/inherent-design/empack/blob/aef85b2d14c50f1fc8a6c8032773cb50bed870a1/crates/empack-lib/src/application/cli.rs)
- [crates/empack-core/src/distribution.rs](https://github.com/inherent-design/empack/blob/aef85b2d14c50f1fc8a6c8032773cb50bed870a1/crates/empack-core/src/distribution.rs)
- [crates/empack-lib/src/engine/documents/recipe.rs](https://github.com/inherent-design/empack/blob/aef85b2d14c50f1fc8a6c8032773cb50bed870a1/crates/empack-lib/src/engine/documents/recipe.rs)
- [crates/empack-lib/src/engine/build/batch.rs](https://github.com/inherent-design/empack/blob/aef85b2d14c50f1fc8a6c8032773cb50bed870a1/crates/empack-lib/src/engine/build/batch.rs)

## Delivery boundary

Depends on the authoring schema decision in checkpoint 02; effective recipe resolution and its tests can be prototyped against current Recipe values first.

Before ready-for-review, replace proposed spellings with tested help examples, update
applicable final-state contracts and usage, and attach revision-specific execution
evidence to the PR. Remove or fold this temporary checkpoint into the final docs
when implementation lands; do not retain a second authoritative specification.
