# Implementation status

Release target: v0.5.0-alpha.1. The [target contracts](README.md) and
[feature requirements](parity.md) define completion. This page records what the
source implements; it does not make the target API sketches available by implication.

The normalized engine can prepare and recoverably publish all five distribution
shapes from a captured workspace. Server recipes have verified vanilla,
Fabric, Quilt, Forge and NeoForge runtime preparation. HTTP acquisition and
explicit build-content obligations feed these paths. The compiled `Engine` now owns
build preview, preparation, exact-plan authorization, acquisition, runtime assembly
and publication. The read-only provider catalog resolves canonical selectors and
exact Modrinth/CurseForge selections. Compatible selection, provider/import
composition, the remaining operation APIs,
continuation/cleanup and CLI cutover remain unfinished. Existing
commands retain their fixes and capabilities until their replacements pass parity
checks. There will be one implementation per operation, not a permanent legacy engine.

## Implemented boundaries

| Boundary | Current behavior | Contract evidence |
| --- | --- | --- |
| Build lifecycle | Read-only preview/preparation; engine-bound consumed grants; shared target acquisition; owned runtime/tool work; all-requested publication and retained receipts; abandoned preparation retirement | [Engine tests](../../crates/empack-lib/src/engine/api/tests.rs), [runtime tests](../../crates/empack-lib/src/engine/runtime/tests.rs) |
| Provider catalog | Canonical slug/ID/URL resolution and exact project-owned selections; all file assertions, roles, environment facts and dependency relations retained; fixed-origin authenticated API requests, shared rate budgets, bounded bytes/deadline/retries and owned parsing | [Catalog tests](../../crates/empack-lib/src/engine/providers/tests.rs), [official API smoke](../../crates/empack-lib/tests/provider_catalog_smoke.rs) |
| Semantic core | Dependency-free identities, pins, paths, requirements, digests, exact multi-file resolution, file plans, target prerequisites and inventory projection | [Core suites](../../crates/empack-core/tests/) |
| Documents | Intent schema 2 and lock schema 1; raw and semantic revisions; strict source/selection validation; original-byte no-op writes; stable credential-free persisted locators | [Codec tests](../../crates/empack-lib/src/engine/documents/tests.rs) |
| Project capture | Read-only recovery gate; retained native root; bounded bytes, identities, membership and absence; explicit local/archive sources; exact artifact destinations | [Reader tests](../../crates/empack-lib/src/engine/project/tests.rs), [snapshot tests](../../crates/empack-lib/src/engine/snapshot/tests.rs) |
| Source enumeration | Native traversal applies captured pack ignore rules before opening ignored bytes; explicit local/archive inputs remain captured; recovery retains the same filter; common/client/server layers remain separate | [Source matcher](../../crates/empack-lib/src/engine/source.rs), reader tests |
| Backend observations | Shared canonical provider/pin decoding; safe relative payload paths; requirements, optional defaults and digest declarations; captured metadata revisions | [Backend tests](../../crates/empack-lib/src/engine/backend/tests.rs) |
| Private content | Bounded quarantine, every source digest/size/observation checked, retained independent readers, original weak evidence separate from computed hashes | [Content tests](../../crates/empack-lib/src/engine/content/tests.rs) |
| HTTP acquisition | HTTPS/redirect/status policy, bounded channel, cumulative mirror bytes, one deadline, scope-owned verifier, redacted locators, reservations retained by leases/readers | [Transfer tests](../../crates/empack-lib/src/engine/acquisition/tests.rs) |
| Build acquisition | Read-only target/side/optional selection before acquisition; bootstrap and mrpack retain their distinct reference requirements; selected local files verify while excluded files need no bytes; failed download batches return no successful subset; manual/provider/missing-archive work remains explicit | [Build acquisition tests](../../crates/empack-lib/src/engine/build/acquisition/tests.rs) |
| Materialized game content | Shared captured identity/content checks; side and optional selection before missing-byte requirements; exact retained leases and source evidence; no unresolved entries in completed inventory | [Materialization tests](../../crates/empack-lib/src/engine/build/materialized/tests.rs), [core inventory tests](../../crates/empack-core/tests/inventory.rs) |
| Build batches | All five recipes prepare privately; duplicate outputs fail preflight; every candidate verifies before one journal publishes their union; original resolution and conversion evidence remain available | [Batch tests](../../crates/empack-lib/src/engine/build/batch/tests.rs) |
| Packwiz reference projection | Captured game selection feeds a generated pack/index/metadata tree; exact provider references, stable URLs, optional constraints and original declared-vs-acquired evidence remain explicit | [Projection tests](../../crates/empack-lib/src/engine/packwiz/tests.rs), game-content tests |
| Vanilla server runtime | Official catalog selection, per-version metadata digest, exact server bytes, bounded JAR main-section and launcher checks; owned parsing and verification | [Runtime tests](../../crates/empack-lib/src/engine/server_runtime/tests.rs) |
| Fabric and Quilt server runtimes | Exact catalog/library coordinates, declared digest sidecars where needed, verified Minecraft base, generated classpath or historical shaded launcher, service merging and retained source/tool evidence | [Library runtime tests](../../crates/empack-lib/src/engine/server_runtime/library/tests.rs), [live runtime smoke](../../crates/empack-lib/tests/runtime_server_smoke.rs) |
| Forge-family server runtimes | Exact official installer evidence, owned private execution, independently checked libraries/generated outputs, historical executable and modern host-specific argument launch contracts | [Installer tests](../../crates/empack-lib/src/engine/server_runtime/installer/tests.rs), live profile smoke |
| Installer assets | Reviewed bootstrap/main-installer versions, bounded governed acquisition, exact size/digest validation and retained tool identities; pins are maintained by empack | [Tool tests](../../crates/empack-lib/src/engine/bootstrap_tools.rs) |
| Lightweight client distribution | Generated packwiz tree, exact bundled tools, local byte projection, pinned launcher command, all archive formats and batch publication | [Client tests](../../crates/empack-lib/src/engine/build/client/tests.rs) |
| Server distributions | Full and reference game views, exact prepared runtime, preserved templates, executable launch scripts, pinned bootstrap tools and all archive formats; runtime selection must match the lock | [Server tests](../../crates/empack-lib/src/engine/build/server/tests.rs), batch tests |
| Full client distribution | `.minecraft` game view, captured templates, exact launcher components, generated defaults, all three archive formats and verified publication | [Client build tests](../../crates/empack-lib/src/engine/build/client/tests.rs) |
| Templates | Captured common/side projection, nested paths, binary and explicit literal copying, strict expressions, format helpers, bounded outputs and preserved portable attributes | [Template tests](../../crates/empack-lib/src/engine/templates/tests.rs) |
| Archive sources | Retained bounded ZIP reader; raw directory preflight; every member path/kind/collision checked; selected bytes, CRC and portable attributes verified; failed attempts consume extraction allowance | [Archive source tests](../../crates/empack-lib/src/engine/archive_source/tests.rs) |
| Staging | Private native storage, copied inputs, no project/cache hardlinks, closed writers before freeze, packed private backing for multi-file trees, bounded member reads and safe cleanup order | [Staging tests](../../crates/empack-lib/src/engine/staging/tests.rs) |
| File verification | Exact candidate inventory and portable attributes, explicit managed roles, collision checks, complete resulting read budgets | [Verification tests](../../crates/empack-lib/src/engine/verification/tests.rs) |
| Publication | Project lock, synchronized before/after images, durable intent, recorded sibling replacement, final inventory, retained receipt; recovery without replaying tools | [Publication and crash tests](../../crates/empack-lib/src/engine/publication/tests.rs) |
| Runtime ownership | Coordinated resource admission, owned async/blocking workers, stale-result rejection, retirement before completion, retained outcomes and explicit output reservations | [Resource tests](../../crates/empack-lib/src/engine/resources/tests.rs), [runtime tests](../../crates/empack-lib/src/engine/runtime/tests.rs) |
| Process ownership | Async supervision on the host runtime, deadline across exit and pipe lifetime, Unix groups/Windows jobs, bounded nonblocking progress | [Process runtime](../../crates/empack-lib/src/application/process_runtime.rs) |
| Containers | ZIP, TAR.GZ and 7z from frozen inputs; binary streaming, portable modes, empty directories, duplicate-name and encoded/expanded limits; independent exact inventory verification | [Artifact tests](../../crates/empack-lib/src/engine/artifacts/tests.rs) |
| Mrpack semantics | Every locked file/placement accounted for, exact reference evidence, layered replacements, retained observed content, optional conversion rules and inspected output archives | [Mrpack tests](../../crates/empack-lib/src/engine/mrpack/tests.rs), [observed-content tests](../../crates/empack-lib/src/engine/mrpack/observed.rs) |

File-level verification is not a semantic proof. Container integrity is not evidence
that every required dependency was included. The composed recipes connect normalized obligations, verified acquisitions,
independently checked output and the journal publisher. Runtime adapters still need
the semantic Engine and CLI composition; adapter tests do not establish CLI parity.

The compiled `engine::providers::ProviderCatalog` is concrete and read-only. Its
`resolve_selector` and `resolve_exact` methods return admitted retained data. They
cannot publish a manifest, select a compatible update or claim a resolved dependency
closure. Provider environment facts remain separate from user requirements. A
restricted file keeps its identity, size and original hashes even without a locator.
Transient signed URLs are execution data; the document codec still rejects them in
persistent alternatives. Search ranking, compatible selection, identification,
import composition and command cutover remain completion gates.

## Composed build behavior

`prepare_mrpack_build` retains one workspace read set through publication. Preparation
writes private candidates only. Source changes block publication before the previous
artifact is replaced. Unrelated distributions are outside the read set. Existing and
new selected outputs use artifact budgets; those allowances cannot widen source-file
limits. Recovery persists and reuses each capture group's limits. Publication
preflights the complete candidate against those captured allowances before copying;
private staging uses the exact planned sizes rather than an unrelated default ceiling.

`plan_build_acquisitions` distinguishes sufficient reference evidence from required
materialization. It accounts for layered replacements and unlisted backend files.
`acquire_http` verifies every requested download before returning a batch; unresolved
manual, provider-locator and missing-archive work remains pending. Captured ZIP
members are acquired through one retained reader per archive, and the source archive
is not silently included as game content. Outputs cannot overlap source scopes,
including portable case aliases. Neither step can
publish. Final build verification still checks supplied content against its exact
logical file and captured installation.

Unlisted installed content remains an observed obligation instead of becoming invented
manifest intent. Its bytes must match its declaration. Locked backend comparisons
record whether they matched acquired bytes, a same-algorithm declaration, or an
independent locked reference with no comparable backend algorithm. Mismatches block
preparation. Backend URLs never replace locked download assertions.

Verified observed bytes with a transient/non-persistable URL are embedded without
exporting the locator. Ordinary optional references retain optional participation.
Mrpack cannot represent optional embedded content or a selectable replacement with a
common fallback losslessly. Those cases require an explicit representable choice;
acknowledging missing description/default fields does not make optional content mandatory.

`prepare_game_content` projects complete client/server game views from the same
captured obligations as mrpack. It preserves source assurance per logical owner,
exact locked resolution and observed-backend evidence. Unacquired content on the
other side or behind a disabled choice does not become a required download. This
is game-content completeness, not launcher/server runtime completeness.
`prepare_bootstrap_game_content` uses the same captured obligations for client/server
references. `PreparedGameContent::packwiz` checks generated metadata through the shared
reader; unlisted metadata-only references retain `actual: None` rather than a fabricated
byte observation. Full targets still require actual selected bytes.
Vanilla, Fabric, Quilt, Forge and NeoForge runtime resolution and byte/launcher
verification are implemented; server recipes bind that preparation to the captured lock. Server output records original runtime evidence; computed
SHA-256 does not turn a SHA-1 declaration into strong source assurance.

Template preparation selects common and target-side inputs before rendering. Exact
side replacements retain their replaced source identity; same-layer duplicates,
portable aliases and file/ancestor conflicts fail. `.template` files require UTF-8;
other files preserve the text-or-binary convention, with an explicit `Copy` mode
for literal UTF-8. Expressions use current bound metadata and exact runtime versions.
Missing values and rendering/size failures return no partial set. Project templates
remain untouched. `BOOTSTRAP` distinguishes lightweight from full-target defaults.
Full client preparation adds a format-1 launcher component manifest tied to the
locked Minecraft/loader versions and maps selected game content under `.minecraft`.
Default full-client settings do not run an absent bootstrap installer. A captured
user configuration remains an explicit input; arbitrary user commands are not
certified by artifact verification. User component manifests must preserve the
locked game/loader. Template/game collisions fail before publication.

Full client archives materialize pack content, not Minecraft binaries and assets:
the launcher still resolves its normal game/runtime components. ZIP supports direct
launcher import; TAR.GZ and 7z contain the same instance tree for explicit extraction.

`prepare_build_batch` composes mrpack, client, server and both full recipes under the
AllRequested rule. A later failed recipe drops earlier private candidates. Source
changes block the whole publication, and output collisions fail preflight. One
recoverable journal owns the combined artifact changes; this does not claim an
atomic filesystem-wide visibility switch. Each result retains exact resolution,
original backend evidence, conversions and expected archive members. Remaining
loader runtime contracts must join this boundary before CLI `all` can cut over.

## Existing command guarantees

Earlier fixes remain in the live commands: canonical add/sync/remove identities and
pins, conservative retention of unlisted dependencies, failure on required manifest
publication errors, import preflight before forced reset, bounded archive downloads,
preserved environment/optional semantics, fresh build prerequisites, confined local
removal and process-tree ownership. Core values and the shared archive writer already
serve these paths. Their continued presence does not give them the new engine's full
multi-file publication contract.

Use the [feature requirements](parity.md) when replacing each path. Retain provider,
loader, import, archive, template, continuation, cleanup and host-configuration
capabilities. Old experimental schemas do not need migration machinery; useful
features are not optional migration work.

## Verification snapshots

Results describe the stated revision, not every later edit.

| Revision | Executed checks |
| --- | --- |
| `c7b1c64` | [Native PR CI 37387301198](https://github.com/inherent-design/empack/actions/runs/37387301198) passed Linux/macOS/Windows tests and import-smoke, lint and coverage |
| `180c4c3` | `mise run test`: 1,498 tests and ten doctests passed |
| `efb7799` | `mise run e2e:strict`: 101 tests passed; [native CI 37388992838](https://github.com/inherent-design/empack/actions/runs/37388992838) passed |
| `1901434` | 17 template tests and all-feature Clippy passed; six generated configurations each round-tripped through Qt QSettings and Java Properties |
| `3733d71` | 1,514 default tests passed; the subsequent doctest compile overlapped a source edit and required a repeat |
| `c5cd762` | 1,517 default tests and ten doctests passed; Greptile review 35 reported no new blocking findings |
| `713fcc0` | 1,524 default tests and ten doctests passed; strict E2E passed 101 tests |
| `7eaba64` | 46 affected core/build/mrpack tests and all-feature Clippy passed |
| `bbe4ee1` | 1,530 default tests and ten doctests passed; native CI 37396023965 passed Linux/macOS/Windows tests and import-smoke, lint and coverage; Greptile review 38 reported no actionable findings |
| `719735a` | 101 strict E2E tests passed; Greptile review 39 reported no actionable findings |
| Vanilla runtime integration | Live official-catalog resolution and exact Minecraft 1.20.1 server acquisition passed; the actual server completed `--help` under process supervision without accepting the EULA |
| Lightweight client integration | 46 affected tests passed across templates, tools and builds. Governed acquisition fetched both pinned assets; the published archive installed with its bundled Java tools; Qt read the exact command |
| Bootstrap projection integration | 29 affected build/mrpack/packwiz tests passed; the actual pinned Java installer consumed a generated selected tree and produced the expected local bytes |
| Publication budget integration | 32 affected tests passed; a passing acquisition test reported a pipe-leak warning and its isolated repeat passed without one. Large-size accounting uses synthetic metadata, not a 65 GiB allocation |
| Build batch integration | 13 composed build tests and all-feature Clippy passed |
| Full client assembly | 138 affected build/template/core tests passed; the final rebuild regression and all-feature Clippy passed. Archives were published and independently read in ZIP, TAR.GZ and 7z |
| Materialization integration | 30 affected core inventory, build, mrpack and reader tests passed |
| Captured template integration | 20 affected template tests passed, including all four standalone target selections, malformed inputs, output budgets, cancellation and source changes |
| Archive and budget integration | 21 affected acquisition/archive/build/reader tests passed; the final five archive tests passed after tightening per-member reads. Three focused source-ownership/scratch tests and all-feature Clippy passed |

The HTTP and build tests use deterministic local fixtures. They do not establish
live provider authorization or catalog behavior. Windows cross-compilation checks
types and configuration; native CI establishes execution on Windows.

An earlier full run at `4be1023` had four tool-probe deadline failures; isolated
repeats and the quiet full run at `0455614` passed without weakening assertions.
A build-acquisition test run reported one nextest pipe-leak warning; its targeted
repeat passed without a leak. These observations do not establish a production fix.

The provider-catalog landing passed 1,589 tests plus eleven doctests before the
final adjustment to reserve parsing memory from actual response lengths. Its final
fourteen focused regressions and all-feature Clippy passed. Three official API
probes verified canonical identity, exact ownership and downloaded bytes for a
Modrinth mod, a Modrinth resource pack and a CurseForge mod. The earlier default
library configuration passed 1,431 tests, followed by all 38 search tests after
enabling their ordinary unit-test configuration.

## Remaining integration and limits

- Extend the compiled build `Engine` lifecycle to semantic project mutations,
  provider catalogs and import normalization. All five build recipes and all loader
  runtime families now enter the same approved build driver.
- Replace command orchestration with the shared lifecycle. Wire manual acquisition,
  provider-locator refresh, continuation and scoped
  clean through the same verified obligations.
- Add persistent content lookup/store capabilities with read-only preview authority.
  Coordinate provider authentication, retry and rate policy; the new content HTTP
  port does not replace the existing catalog clients yet.
- Native build capture filters ignored pack content before opening its bytes. Directory
  enumeration remains bounded; captured rules and explicit input exceptions survive
  revalidation and journal recovery.
- Windows publication reports file synchronization, not Unix directory synchronization.
  Filesystems without durable root creation identity are refused. Portable permission
  checks cover read-only/executable intent, not arbitrary ACL equivalence.
- Restoration can leave newly created empty managed directories. Retained committed
  data has explicit reclamation, but a complete retention catalog remains pending.
- The 7z decoder exposes no configurable allocation ceiling. Current packaging
  verification reads privately generated candidates, not untrusted downloaded 7z.
- Existing disposable HTTP-cache snapshots have last-writer-wins cross-process
  behavior. They are not the consistency model for intent or recovery state.
- The synchronous process bridge and global display/error state still constrain
  independent embedded sessions. Their removal belongs to command cutover.

The upstream installer v0.5.14 enables all optional entries in headless mode,
regardless of their declared default. This was observed in a disposable installer
run; projection now requires resolved choices for headless execution. Interactive
reference packages retain supported optional metadata. Lightweight client defaults
bundle both exact tools and disable bootstrap updates. Captured user configuration
remains explicit input; its arbitrary commands are not certified by these checks. Grouped optional files and
optional embedded bytes need explicit selections because the format lacks those
relationships. These checks precede any publication.

No alpha release is ready while these feature and lifecycle gates remain incomplete.
