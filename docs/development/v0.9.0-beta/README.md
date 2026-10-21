# v0.9.0-beta: portable authoring

The release target is a portable modpack engine, native and browser adapters, and
an SDK acceptance harness. These are target contracts, not capabilities supplied
by the native baseline. Public signatures become callable only with their
implementation.

A project can retain unfinished work, including invalid author text and missing
custom files. Export requires globally valid authoring semantics and an immutable selection of inputs. The native
CLI and browser SDK use the same interpretation of dependencies, bindings,
templates, recipes and evidence.

## Release surface

| Deliverable | Required behavior |
| --- | --- |
| Native CLI | Existing authoring, distributions, publishing, instances and recovery through extracted services; executable remains `empack` |
| Portable engine | Draft editing, shared interpretation, revision-bound planning, content verification and export orchestration |
| Browser SDK | WASM, JavaScript bindings, TypeScript declarations and worker entry point usable by a separate application |
| Browser harness | File selection/drop, text edits, decisions, save/reopen, project backup and export; enough controls to verify the SDK |
| Project container | Versioned `.empack-project` ZIP preserving editable work across hosts |
| Release artifacts | Existing native binaries plus a version-matched SDK archive, harness archive, checksums and capability report |

The harness is a test client and integration example. A product SPA, workspace
management application, Tauri app, companion service, cloud storage, collaboration
and browser private-key custody are outside this release. Publishing an npm package
or deploying a public application is not required; the SDK archive is installable
as a local package and does not depend on registry credentials.

## Ongoing pack maintenance

The same engine serves general pack editors and specialized authoring tools.
Applications may coordinate recipes, documentation, configuration and local content
without giving empack knowledge of a game's progression or mechanics. Empack owns
pack interpretation, exact selections, inventories and native instance operations;
applications own domain analysis and the meaning of their findings.

Author sources, generated files, assembled candidates and observed installations
remain distinguishable. Saving an unfinished change is useful before generation
or export succeeds. An application can retain older validation and playtest results
without presenting them as observations of the current draft. Domain-specific
invalidation belongs to that application; empack supplies captured identities.

Use existing projects through scoped inspection and explicit edits. Inspecting a
checkout or instance does not enroll it, synchronize it, or authorize replacing
user files. A portable project restore still creates a new workspace; merging it
back into an existing checkout is outside this release. External tool integration
adds no implicit generator hooks, gameplay framework or local companion service.

## Required browser scope

Desktop Chromium, Firefox and Safari, iOS Safari, and Android Chrome are release
gates. Each must support the required workflow within its declared tested limits.
A desktop mobile viewport and Playwright WebKit do not establish physical-device
or branded Safari acceptance. Exact browser, OS, device and SDK versions belong
in release evidence. Missing device evidence blocks the release rather than
silently downgrading a required platform.

| Capability | Browser release requirement |
| --- | --- |
| Raw document editing, bindings and custom sources | Required, including invalid-text persistence |
| Local project save/reopen and portable round-trip | Required |
| Local JAR/configuration input and safe ZIP inspection | Required |
| Prism snapshot ZIP | Required for reference delivery and explicit bundled delivery, with independently checked inventories |
| Modrinth catalog, exact content and dependency resolution | Required on tested routes, including browser-visible rate information and bounded fallback |
| Fresh runtime resolution | Required Minecraft/Fabric catalog path without a preloaded lock; other loader routes explicitly classified |
| Restricted-file association | Required using exact obligations, original assertions and fresh preparation |
| Other consumers, imports, loader routes, TAR.GZ and 7z | Capability-gated until browser acceptance establishes support |
| Java processors, game launch, native installation/recovery | Native extensions; unavailable as browser effects |
| Signature verification | Shared semantics where used; browser signing deferred |

Known missing capabilities are diagnosed before execution. Permission, quota,
network and device failures remain possible after preflight. Required routes must
work in acceptance; a useful unsupported diagnostic alone does not satisfy them.
No CORS proxy or hosted backend is an implicit dependency.

## Author workflow

```text
register files or open project
    → retain assets and inspect input
    → propose edits and decisions
    → edit raw documents / accept structured edits
    → save draft, including incomplete work
    → resolve selected operation
    → prepare immutable export
    → build and independently verify artifact
    → request download or commit through a file writer
```

Saving does not synchronize dependencies. Exporting does not update selections.
Opening a bundle grants no network access, execution permission or publisher
trust. A distribution is not a lossless editable-project backup.

## Architecture

```text
empack-cli                         SDK acceptance harness / future SPA
    │                                           │
empack-native                               empack-wasm
    └──────────────── empack-engine ─────────────┘
                            │
                        empack-core
```

| Crate | Authority |
| --- | --- |
| `empack-core` | Pure validated identities, requirements, inventories and planners; retains its `no_std` boundary |
| `empack-engine` | Documents, drafts, semantic services, codecs, recipes and verification; portable standard library facilities allowed |
| `empack-native` | Native observations, storage, HTTP, scheduling, process containment, publication and recovery |
| `empack-wasm` | Browser content/storage/Fetch adapters, worker protocol and capability discovery |
| `empack-cli` | Arguments, presentation, decisions, approval, exit status and native composition |

A temporary `empack-lib` facade may forward during extraction. Every migrated
workflow has one implementation; the facade is removed before release. Native
extensions expose installation and recovery separately from portable operations.
The engine has no mandatory Tokio executor, native filesystem/process backend,
reqwest response or browser object in its public contracts. Feature unification
must not introduce those effects through transitive dependencies.

## Decisions

| Concern | Target choice |
| --- | --- |
| Baseline | Preserve the native behavior and format meanings in the extraction baseline below |
| Author formats | Keep author schema 5, bindings schema 1 and lock schema 2 unless a separately reviewed semantic change requires otherwise |
| Raw documents | Authoritative bytes; invalid text is saveable, but export requires whole-document semantic validation |
| Assets | Logical asset identity separate from immutable content identity and session access tokens |
| Imported custom content | Retained copies by default; explicit external/missing references supported |
| Portable save | ZIP format 1 with `project.json` and content-addressed objects; complete-custom-source and explicit partial modes required |
| Offline materialized backup | Optional stronger capability; cannot be claimed from a custom-source backup |
| Browser storage | OPFS blobs and scratch; IndexedDB revision/ownership records |
| Worker execution | One dedicated worker, local futures, bounded compute steps and real task-queue yields |
| Writer ownership | One writable owner per project, transactional expected-head commits, coordinated collection |
| Undo | Bounded session history retaining asset references; current draft survives reopen, cross-session undo optional |
| Completion | Verified artifact, requested download, committed file and uncertain finalization are distinct outcomes |

## Feasibility before interface freeze

The [feasibility gates and minimum profiles](feasibility.md) precede public API,
project-format and storage-protocol freeze. A small experiment alongside checkpoint
1 tests OPFS-backed cooperative ZIP, task-queue cancellation, file retrieval and
actual mandatory browser catalog/CDN routes. It supplies evidence for the shared
implementation, not a second permanent archive or network path.

Checkpoint 3 owns the minimum reusable cooperative ZIP foundation and its thin
browser adapter. Checkpoint 4 consumes that tested layer for bundle save/reopen.
Checkpoint 5 adds consumer export, independent verification and destination delivery.
Baseline tests, dependency audits and behavior-preserving extraction may proceed
while feasibility runs; unresolved assumptions block the relevant interface freeze.

## Development entry points

Implementation checkpoints are separate from these target contracts. Each draft PR
starts with its scoped contract and acceptance gates; its existence does not mean
the implementation is complete. Merge only after its native regression and portable
boundary checks pass. Restack dependent PRs as their prerequisites merge.

### Extraction baseline

The extraction baseline is [tagged v0.8.2-beta source](https://github.com/inherent-design/empack/tree/v0.8.2-beta)
at [commit `f29064d`](https://github.com/inherent-design/empack/commit/f29064dcf9136d0cf4cb001222e00be63dbe9a9b).
It includes reference defaults and diagnostics in
[#137](https://github.com/inherent-design/empack/pull/137), archive and catalog
corrections in [#138](https://github.com/inherent-design/empack/pull/138), the Tokio
update in [#139](https://github.com/inherent-design/empack/pull/139), managed-launch
validation in [#140](https://github.com/inherent-design/empack/pull/140), consumer
format and output-attribute checks in
[#141](https://github.com/inherent-design/empack/pull/141), and restricted Prism
grammar plus supported-dependency probe tooling in
[#142](https://github.com/inherent-design/empack/pull/142).

This source baseline includes the opaque archive-member distinction and typed
rejection of empty Modrinth file lists. Their regressions are native behavior to
preserve during extraction; browser execution remains a separate acceptance gate.
Checkpoints 5 and 6 require effective Prism configuration, including comment,
continuation and structural-whitespace semantics, and executable managed entry
points. The supported headless parser probe does not establish actual consumer
import/preparation, gameplay or browser acceptance.
Preserve these contracts across native and portable interpretation:

| Boundary | Required behavior |
| --- | --- |
| New profiles | Initialization, import and ad-hoc Modrinth, Prism and server recipes prefer references; generated workflows use the authored profiles |
| Existing profiles | Schema-5 `prism` and `server` string presets keep their bundled snapshot meaning; expanded profiles retain authored delivery and update policies |
| Recipe explanation | Show included content, update authority and recipient requirements from the same effective recipe used for execution |
| Archive inspection | Opaque JARs may contain case-distinct members and native-reserved names; inspection grants no extraction authority, and portable destination checks remain strict |
| Assessment | Root-local malformed provider evidence, including empty Modrinth file lists, is isolated under one shared byte/deadline allowance; internal failures are not globally relabeled as provider errors |
| Diagnostics | Strict assessment emits its report before failure; updates distinguish checked, changed and retained selections; text names dependencies and JSON retains exact details |

Reference delivery can still include authored assets and prepared runtime files,
and can require author-side acquisition for verification. It is not a permission
or redistribution guarantee. Reference Prism/server recipients require empack;
preparing their archives in a browser does not imply browser execution of Java.
A missing preparation capability produces a diagnostic, never a silent switch to
bundled delivery or a different installation recipe.

New SDK/harness profiles follow the same explicit reference defaults. Existing
shorthand must not be reserialized as a reference recipe merely because defaults
for newly generated profiles changed. The required Prism suite exercises both the
reference default and an explicitly selected bundled fixture. Other browser
consumers remain capability-gated under the release scope above.

### Source boundaries

The source boundaries below identify what must be separated, not public APIs to
preserve indefinitely.

| Current source under `crates/empack-lib/src/` | Extraction boundary |
| --- | --- |
| `engine/documents/` | Raw-byte editing/composition moves into engine, with draft analysis layered over it |
| `engine/content.rs`, `content/pool.rs`, `content/store.rs` | Evidence and bounded access separate from native stage/storage leases |
| `engine/providers/`, `runtime_catalog.rs`, `networking/` | Request interpretation and policy separate from HTTP, clocks and scheduling |
| `engine/runtime.rs`, `resources.rs` | Lifecycle/retention semantics separate from Tokio jobs and native admission |
| `engine/build/`, `templates.rs`, `artifacts/` | Recipe inventory and verification separate from directory staging and artifact sinks |
| `engine/snapshot.rs`, `publication.rs`, `server_runtime/`, `instance/` | Native observations, processes and apply/recovery remain native-owned |
| `application/engine_host/`, `application/cli.rs` | CLI adapters compose shared services; browser receives no terminal dependency |

See [platform constraints](platforms.md) for researched boundaries. Checkpoint
contracts are added alongside this overview as separate draft PRs. Run-specific
results, implementation status and review findings belong in those PRs and CI.

## Contract reference

- [Drafts and portable ports](drafts-and-ports.md)
- [Native host and CLI](native.md)
- [Portable project format](project-format.md)
