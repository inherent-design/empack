# Browser SDK and transport

The SDK exposes the shared engine through a typed worker client. The acceptance
harness demonstrates integration and failure behavior; application navigation,
workspace-management UX and a product SPA remain separate work.

## Package contract

The versioned SDK archive contains WASM, generated ES modules, TypeScript types,
the worker entry point, licenses and a machine-readable compatibility manifest.
The harness is a separate static archive built against that exact SDK. A consuming
application supplies asset/worker URLs and can serve all files from its own secure
origin without a CDN, registry or service dependency.

The compatibility manifest names SDK version, worker protocol, project-container
format, readable author/bindings/lock schemas and capability-report format. Reject
mixed incompatible worker/client assets before opening a writable project. Cache
updates cannot combine old JavaScript with a new incompatible WASM module.

Typed client methods cover capabilities, registration, inspection, raw/structured
edits, proposals, save/status, portable project import/export, resolution/assessment,
restricted-file association, artifact preparation/export and cancellation. Return
stable diagnostic records and receipts; do not require consumers to parse CLI text.
JS handles are opaque, scoped to an epoch and explicitly releasable. The SDK owns
no global UI state and implements no duplicate TypeScript dependency resolver.

## Authoring application integration

Expose grouped revision-checked edits and the shared read-only project/candidate
views through typed client results. Keep partial metadata previews distinct from
verified artifact inventories. A host can display source, generated, candidate and
installation observations without assigning one readiness boolean to the project.
The browser SDK reports native-only observation and application operations as
unavailable; it does not supply a companion connection implicitly.

Applications retain their own source-to-output relationships and domain findings as
explicit project files. A file edit creates a new draft revision but does not make
the engine decide whether a gameplay report or world survey is still relevant.
Show historical or unknown coverage accurately using the identities supplied by
the engine and the producer's declared inputs. Tool reports never grant native
mutation or executable proof authority. Generic editors need not implement domain
analysis, and specialized editors reuse the same pack semantics.

## Browser network contract

Request construction, response parsing, canonical identity binding, release policy,
closure/incompatibility evidence and content verification remain shared with native.
Browser transport controls Fetch access, readable response limits, cancellation,
credentials and visible metadata.

Public catalog requests use fixed provider origins and reject redirects. Downloads
use credential-free source-specific redirect policy with final-origin checks where
observable. A policy requiring validation of every redirect hop is unavailable when
the browser cannot expose that evidence. Opaque/opaqueredirect responses cannot be
interpreted as content or a successful empty catalog.

Bound decoded response bytes and total request/deadline allowance, including local
postprocessing and retained-inventory assessments. Do not rely solely on declared
Content-Length. Missing exposed rate headers are unknown evidence; conservative
bounded admission remains in effect. Cancellation aborts transport and retires owned
readers before releasing their buffers.

Use `BrowserNetworkUnavailable` when the browser hides whether the cause was CORS,
TLS, connectivity or policy. Preserve distinct observable HTTP unauthorized,
not-found, rate-limited and server-error outcomes. No browser failure becomes fuzzy
search, “up to date,” or absence of dependency evidence.

## Required routes

| Route | Release evidence |
| --- | --- |
| Modrinth project/version catalog | Canonical selector binding, compatible/exact selection and required closure through real browser Fetch |
| Modrinth content/CDN | Readable bytes and original assertions; test actual download route separately from catalog |
| Minecraft and Fabric catalogs | Fresh uncached runtime resolution and a resulting Prism profile |
| Restricted acquisition | Missing exact input → selected file → verification → retained asset → fresh preparation |
| Unsupported/blocked route | Actionable missing capability/input with unchanged draft and no incomplete artifact publication |

Use a frozen deterministic catalog fixture for semantic parity and separate live
route checks for access. A live route is not assumed available forever. Other loader
catalogs and providers receive explicit tested/unsupported/permission-required
capabilities, with the native alternatives preserved.

Modrinth client identification must work within browser header controls. Validate
behavior against provider documentation and actual supported browser requests; do
not assume the native User-Agent configuration was transmitted. No shared credential
is embedded in the SDK or harness. Optional user-owned credentials are session-bound,
origin-scoped and excluded from persistence/bundles; credential-dependent features
remain unavailable unless their route and permission contract is tested.

## Early route feasibility

Before public transport assumptions are frozen, run actual browser Fetch probes for
mandatory Modrinth catalog/content routes and Minecraft/Fabric discovery alongside
the early ZIP experiment. Use the required desktop and physical mobile families;
record request identification, readable bytes, visible headers, redirect behavior
and applicable provider policy. An HTTP success alone does not prove a permitted
client integration or verifiable download path.

An unavailable mandatory route blocks that interface decision; it does not silently
introduce a proxy, companion or shared secret. Resolve it within the selected scope
or explicitly revisit the requirement. Deterministic fixtures and native probes
remain useful but cannot replace this browser evidence.

These early results guide adapters. Repeat the complete mandatory scenarios against
the packaged SDK on the same immutable candidate as final native acceptance.

## Restricted input and decisions

An obligation includes exact identity/selection where known, logical file role,
expected original assertions/size, source page and requiring revision. Filenames
only suggest candidate associations. Wrong or ambiguous files remain rejected.
Provider identity does not change because a user supplied a local file.

After verification, retain bytes and prepare again against current revision and
recipe. A stale operation may reuse verified content but not serialized approval.
The browser harness supports file selection on every required device and drop
where supported. Importing into local origin storage does not upload the content.

## Harness contract

Expose a small project tree, raw document editor, diagnostics/proposal controls,
asset/missing-input list, effective recipe, save/backup actions and operation status.
Show current/saved/captured revision, source discovery and retained-byte coverage,
capability limits, verification state and delivery outcome. This is sufficient test presentation;
visual product design is not a release prerequisite.

Use explicit actions for network work, file association and destination selection.
Support touch/file-picker interaction on mobile without requiring drag-and-drop.
Show interrupted/uncertain states after worker loss. Distinguish project writer
contention from unavailable storage coordination: only the former permits protected
read-only access. Metadata-only diagnostics cannot enable content/export actions. Imported HTML is not rendered
as active content, template text is escaped in the harness, and no source input
runs scripts. The harness uses a restrictive script/network policy suited to its
configured routes. Browser origin access is not a secrecy boundary.

New profile creation writes explicit reference delivery for the applicable consumers.
Show recipient requirements and included authored assets before export. Reopening an
existing schema-5 shorthand preserves its bundled meaning; changing delivery is an
explicit edit. A valid server profile unsupported in this browser remains visible
and does not block a supported Prism operation. Malformed authoring blocks distribution
export while exact draft save remains available.

## Acceptance

Test consuming the packaged SDK from a minimal separate static client, worker/client
version mismatch, stale handles, lossless large offsets, cancelled Fetch, hidden
headers, opaque responses, redirects and wrong restricted inputs. Exercise required
live routes in all release-gate browsers; cache-disabled runtime discovery is a
separate scenario from opening a pre-resolved project.

Do not substitute a native HTTP probe for Fetch acceptance or a desktop emulation
for mobile file delivery. The [release gates](acceptance.md) define the evidence
needed to publish the SDK with native binaries. Use the checkpoint 4 persistence
and checkpoint 5 delivery paths through the packaged SDK; the harness cannot route
around them with its own store, recipe interpretation or archive writer. Check both
new reference profiles and reopened bundled shorthand without rewriting their meaning.
