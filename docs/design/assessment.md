# Dependency assessment and runtime migration

`outdated` inspects provider versions without changing author documents, installed
content or persistent cache. `migrate` resolves a proposed Minecraft and loader
request before editing the author project. Both require a coherent current lock.

## Update evidence

`ProviderCatalog::assess_updates` returns a schema-1 report. Each explicit root has
its logical key, human title, canonical provider/project identity and exact current
selection. Catalog observations retain human version labels when the provider
supplies them.

| Field | Meaning |
| --- | --- |
| `current` | Exact locked selection |
| `permitted` | Selection allowed by author pin and requested release policy |
| `compatible` | Best available selection matching the requested game/loader, without the release preference |
| `upstream` | Most recently published available selection in the observed project catalog |
| `catalog_complete` | The bounded available catalog was read completely |
| `reasons` | Stable codes for pins, policy, incompatibility or unavailable evidence |

Compatible ranking prefers the primary Minecraft version, then explicitly accepted
alternatives in order, then publication time. `prefer-stable` ranks stable releases
before prereleases; `stable-only` excludes prereleases; `any` has no stability
preference. These policies govern provider versions, not publisher subscription
channels. A different permitted selection need not be newer than the current pin.
An exact pin remains the permitted selection even when upstream has newer files.

The scan shares a transfer allowance and deadline across roots and the joint
closure check. Exceeding a catalog bound fails the scan; a partial page set cannot
be reported as the newest selection. Authentication, throttling, malformed
records and unavailable providers remain unknown evidence. After a failed scan,
later queries are not given a fresh allowance. Manual sources have no implicit
provider update. Reports contain an observation time and no credentials or download
locators.

Per-root candidates and joint validation are separate. The joint check uses the
required-dependency closure, including exact root pins. `joint.state: validated`
means the required relationships are complete and consistent. It does not establish
that the full retained inventory can be installed together.

`joint.candidate_compatibility` remains `not-assessed`. The report lists
`joint.declared_incompatibilities` between the proposed closure and retained exact
selections, including retained reverse assertions. These are provider declarations,
not placement-aware conflict decisions. Disjoint client/server content may satisfy
them. `joint.declarations_complete` is false when the scan did not run or reached
its output limit or shared deadline. Reaching a bound preserves completed rows and
the partial list with a `catalog-limit` or `deadline` reason. The scan uses the
original catalog deadline, including while comparing retained selections. Missing assertions do not prove
compatibility. Update preparation composes
the complete candidate and checks actual file participation before publication.

Assessment does not verify new file-role bindings, native destinations, runtime
startup or gameplay. `blocked` does not prove that no alternative combination exists;
this resolver does not perform an unbounded search over version combinations.

## Runtime candidates

`migrate --minecraft VERSION --loader FAMILY --plan` composes candidate intent in
memory. Loader versions can be explicit; otherwise a valid locked runtime remains
selected or the runtime catalog supplies a compatible version. New runtime requests
are checked against official game/loader catalogs. `--accept-version` supplies the
complete alternative-game list for the candidate; omitting it clears that list.

`--replace KEY=PROVIDER:PROJECT_ID` explicitly replaces an existing provider root.
Its logical key, content kind, environment and integration bindings remain. The
replacement uses compatible version policy rather than carrying an exact pin from
the old provider project. Unknown keys, duplicate mappings and non-provider roots
are rejected. Empack does not infer replacements across loader ecosystems.

Still-compatible provider pins and original content assertions remain unchanged.
Unsatisfied roots use the normal provider/file resolution and closure checks.
Implicit required libraries are revalidated against the candidate runtime. A
compatible locked selection stays exact; an incompatible selection is resolved
again under the shared catalog budget. A new exact requirement from a selected
root is honored without promoting the library into author intent. Existing logical
keys remain stable. Explicit root pins still require deliberate policy changes.
After resolution, sync and migration also check retained provider entries outside
the new closure: these entries still ship through their locked placements. One
bounded exact-selection batch shares its transfer allowance and deadline across
all remaining entries. An incompatible retained entry blocks the operation with
its logical key and instructions to remove or replace it deliberately. Unavailable
or invalid provider evidence also blocks preparation; it is not compatibility
proof. No migration deletes unused libraries automatically.

An ambiguous file set or incompatible exact pin stops preparation with a diagnostic;
edit the binding or version policy deliberately before retrying. Local content is
retained, but its runtime compatibility remains the author's responsibility.

The schema-2 plan report compares the union of current and proposed lock keys,
including explicit roots and transitive entries. JSON and text include both exact
selections, file roles, destinations, layers, side requirements and one shortest
known root-to-dependency reason chain. It separates retained/changed/added/removed
entries from required/newly-required/no-longer-required retained status, and marks
explicit replacements and manual compatibility verification. Missing edges prove
that an entry is no longer required only when reachable dependency coverage is
complete; otherwise the report marks its requirement status unknown. Before and
after runtime selections include the accepted alternative game versions. `--plan`
never saves pending work or changes the project. It is a semantic candidate report, not an authorization token or proof
that native replacement will succeed.

## Publication and continuation

```text
captured author + bindings + lock
    -> proposed runtime and explicit replacement identities
    -> compatible selections and dependency evidence
    -> coherent candidate intent and lock
    -> native capture and exact replacement preview
    -> approval -> verified publication
```

Each file's before/after evidence includes its logical role, expected size and digests,
accepted content observation, acquisition source, and provenance. The report uses the
lock's wire representation for these fields, so a changed source or byte assertion
remains visible even when the provider pin and destination stay the same.

`SyncRequest::Migration` carries a fresh `ProjectRevision`, resolved candidate and
verified acquired content. Its constructor grants no write authority. Preparation
rejects unrelated authoring changes, invalid retained selections and stale raw
input documents. Publication uses the same native file verifier, exact grant and
recoverable multi-document journal as other author mutations.

With `--materialize`, unavailable exact bytes can suspend the operation. Pending
synchronization schema 2 retains its migration mode, candidate and original document
binding. `pending resume ID --file KEY=PATH` rechecks that binding and the supplied
bytes, then requests new approval. The saved candidate does not authorize overwriting
a subsequent author edit.

Migration does not port configurations, worlds or private JARs. Java launch checks
establish a declared minimum major, not a complete supported range. Build and test
the resulting pack separately before distributing it.
