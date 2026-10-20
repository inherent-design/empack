# Authoring contract

Schema 5 describes content requests. Exact provider selections, resolved runtime,
files and original integrity assertions belong in `empack.lock`. Parsing and
shorthand expansion perform no network requests or filesystem mutation.

## Inputs and ownership

| Input | Responsibility |
| --- | --- |
| `empack.yml` | Pack identity, runtime request, explicit roots, selection defaults and build profiles |
| `empack.bindings.yml` | File-role selection, placements, member mappings and installation policy |
| `empack.lock` | Canonical source bindings, exact selections, expanded inventory and dependency evidence |

Both author inputs contribute to semantic identity. Their raw revisions protect
prepared writes independently. Comments can invalidate a prepared overwrite without
changing the resolution. Earlier author schemas fail with a schema-specific error;
there is no conversion command or automatic fallback.

## Schema expansion

| Field | Meaning and default |
| --- | --- |
| `pack.id` | Stable portable identity, established once during initialization |
| `pack.name`, `pack.version` | Human labels; version labels have no ordering semantics |
| `pack.authors` | Ordered attribution strings, default empty |
| `runtime.minecraft` | Exact requested game identifier |
| `runtime.loader` | Family string or `{kind, pin}` for an exact loader request |
| `runtime.java` | Declared minimum Java major, independent of a consumer |
| `runtime.acceptable-versions` | Explicit extra provider compatibility declarations, default empty |
| `dependency-policy.releases` | `stable-only`, `prefer-stable` or `any`; default `prefer-stable` |
| `dependencies` | Logical root requests, default empty |
| `build.profiles` | Named output recipes, default empty |
| `build.default` | Ordered default profile names, default empty |
| `sources.exclude` | Existing layer-relative source filtering rules |

A dependency is a qualified provider string or a mapping. Ordinary mappings select
exactly one of `source`, `path` or `url`. `type` defaults to `mod`; ordinary placement
is automatic and participation is required on both sides. Non-mod archive inputs
require explicit content interpretation. Advanced tagged search and multi-file
sources retain their existing capabilities.

Provider mappings accept `pin` and `releases`. A pin inherits the provider namespace
and must belong to the selected canonical project. A dependency cannot specify both
an exact pin and its own automatic release preference. Local and URL mappings reject
provider-only fields. URL alternatives use `mirrors` and assert the same bytes with
`sha256` or an explicit digest set. Downloaded bytes never replace an authored digest
implicitly. Local sources remain distinct from installation destinations. CLI additions
preserve project-relative file sources; external files are imported into portable project
storage. Interpreted world archives retain their individual member sources.

`side` accepts `client`, `server` or `both`; it cannot coexist with `environment`.
`optional: true` creates a default-disabled choice using the logical dependency key.
`optional: {choice: KEY, default: BOOL}` declares durable choice identity. Full
per-side requirements remain available and cannot compete with shorthand. Shared
choice keys must have consistent declarations. Optional content still verifies when
selected. World initialization remains a group policy, never per-file replenishment.

## Selection and canonical identity

```text
raw source request
  -> local syntax validation and default expansion
  -> composed integration intent
  -> exact provider lookup, when a new binding is required
  -> canonical source binding
  -> compatible candidate or owned exact pin
  -> required closure and role validation
  -> exact lock
```

A readable provider selector is not a canonical ID. An unchanged selector with an
existing binding remains attached to that canonical project, even if the provider
renames or reassigns its slug. A changed source requires a new explicit binding.
Search is never a fallback for failed exact lookup. Without a lock, readable names
cannot establish historical identity.

The normalized `DependencySelection` has `Automatic`, `ProviderPin` and
`ContentAssertions` variants. Source-specific validation rejects provider pins on
manual files and authored content assertions on provider selections. There is no
universal dependency version expression. Pack labels and engine protocol version
requirements keep their separate meanings.

Effective automatic policy is schema default, then project preference, then a
per-root override. An explicit operation override applies only to that operation
and is shown in its explanation. Implicit required dependencies use the project
preference, or an explicit operation override. Explicit roots retain their own preference. This avoids traversal-order
inheritance when multiple roots share a library. Conflicting exact selections remain
errors. Exact pins bypass preference ordering, not runtime, ownership or integrity validation.

Sync retains valid exact selections. Update deliberately refreshes eligible selected
roots and required dependencies. Outdated reports evidence without changing intent.
Build consumes a satisfied lock and never refreshes dependency selections. Migration
assesses the entire resulting active inventory. These operations share source,
policy and file-role interpretation; only their refresh policy differs.

## Build profiles

`prism` expands to client, bundled, snapshot, ZIP. `server` expands to server,
bundled, snapshot, ZIP. Expanded profiles use `target`, with optional `delivery`,
`environment`, `updates` and `format`. All supported consumer combinations retain
their recipe validation. Presets are fixed by schema, not tool version or profile
name. Explanation and execution use the same expansion and selection.

An author-only project is valid without profiles. Building requires a selected
configured profile or an explicit ad-hoc target. Empack update authority never
implicitly enrolls publisher trust. Secrets and channel sequence floors do not
belong in authoring documents.

## Editing and inspection

Semantic edits modify the affected YAML nodes and preserve unrelated comments,
ordering and formatting. Formatting is explicit, not a side effect of dependency
resolution. Duplicate keys, unknown fields, conflicting shorthand and inapplicable
options fail before network work, with the document path in the diagnostic.

`manifest check` validates both author documents locally. `manifest explain KEY`
shows effective source, policy provenance, requirements, bindings and available
locked selection. `manifest diff` classifies differences between current authoring
and the available lock; it does not claim to predict a resolved candidate.
`pin KEY` records its current exact provider selection. `unpin KEY` restores
compatible intent while retaining the current selection; neither refreshes files.
Pinning removes a per-dependency automatic preference; unpinning returns to the project
preference. Pin edits capture and publish author documents only. They preserve every
locked file, provider selection, graph edge and runtime. Stale raw inputs still reject
publication. Unsupported changed YAML syntax fails explicitly; it is never a reason
to reformat unrelated content. Existing inline integration retains its document ownership.
The local diff compares constraints and recorded selections. It cannot reconstruct prior
display metadata or profiles because the lock does not store old author documents.

Display metadata and output-format edits do not select provider versions. Runtime,
source, selection and binding edits invalidate the corresponding evidence. Every
prepared mutation still captures both raw author inputs and uses the existing
verified publication boundary.

## Conformance

Short and expanded forms normalize identically. Tests exercise renamed provider
slugs, changed provider filenames, per-root policy overrides, explicit pins,
local source drift, URL digest failures, preserved optional choices, bindings and
all output consumers. Editing one dependency preserves unrelated comments and
formatting. Old schemas and invalid field combinations fail locally. Native tests
retain expected-old checks and interruption recovery across both author documents.
