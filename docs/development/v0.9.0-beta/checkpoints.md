# v0.9.0-beta implementation checkpoints

These draft PRs carry target contracts. Their implementation and execution evidence
remain outstanding until the PR acceptance criteria are satisfied. This index is a
dependency map, not a completion ledger. The [release target](README.md) defines scope.

| Checkpoint | Draft PR | Dependency | Contract |
| --- | --- | --- | --- |
| Target and scope | [#130](https://github.com/inherent-design/empack/pull/130) | Merged native baseline | [README.md](README.md) |
| 1. Drafts and portable ports | [#131](https://github.com/inherent-design/empack/pull/131) | #130 | [drafts-and-ports.md](drafts-and-ports.md) |
| 2. Native host and CLI | [#132](https://github.com/inherent-design/empack/pull/132) | #131 | [native.md](native.md) |
| 3. Project format and cooperative ZIP | [#133](https://github.com/inherent-design/empack/pull/133) | #132 | [project-format.md](project-format.md) |
| 4. Browser worker and persistence | [#134](https://github.com/inherent-design/empack/pull/134) | #133 | [browser-host.md](browser-host.md) |
| 5. Consumer export and verified delivery | [#135](https://github.com/inherent-design/empack/pull/135) | #134 | [export.md](export.md) |
| 6. Network, SDK and release acceptance | [#136](https://github.com/inherent-design/empack/pull/136) | #135 | [sdk-and-network.md](sdk-and-network.md) |

## Checkpoint completion boundaries

The [baseline](README.md#extraction-baseline) distinguishes the published release
from the merged native source used by this stack. Each checkpoint retains that
source's behavior while adding its own implementation and evidence.

| Checkpoint | Evidence required to complete this checkpoint | Later qualification |
| --- | --- | --- |
| 1 | Shared semantic fixtures, storage/execution proofs, local-future ports, native/WASM builds and early feasibility | Durable host effects and artifact delivery |
| 2 | Native extraction, verified draft fixture restoration, journal crash/restart and project-store reopen | Container decoding and actual repack in 3 |
| 3 | Executable format fixtures, public native pack/inspect/unpack, restore/repack and cooperative ZIP in thin real-browser adapter | Durable browser save/reopen in 4 |
| 4 | Actual browser storage failures and native bundle → browser edit/save/reopen → bundle → native reopen | Consumer inventories and sinks in 5 |
| 5 | Reference and explicit bundled Prism export, independent verification, native/browser inventory parity and actual delivery | Packaged same-candidate acceptance in 6 |
| 6 | Combined native/browser/device matrix, SDK integration, mandatory routes and release artifacts | Product SPA and other deferred products are outside this release |

Fixture proofs must pass the production verification boundary. They cannot replace
later integration tests or require unchecked public constructors. Intermediate
checkpoints do not claim later features complete, and final qualification does not
excuse missing checkpoint-local tests.

## Integration rules

Each checkpoint adds implementation to its existing draft branch. While stacked,
its diff is relative to the preceding checkpoint. After a prerequisite merges,
restack dependent work onto the accepted commit and retarget it before merge.
Do not merge a contract-only draft as if its feature exists.

The native CLI remains usable at every checkpoint. Extracted services replace their
old definitions; forwarding facades cannot preserve a competing mutation path.
Checkpoint 3 supplies project-container fixtures, the shared cooperative ZIP path
and a thin real-browser adapter before checkpoint 4 executes browser bundles.
Checkpoint 5 reuses that foundation for consumer recipes and delivery. Mandatory
browser routes are probed early, then qualified again with the packaged SDK.

## Authoring integration ownership

Checkpoint 1 owns grouped draft edits, stale generated-output proposals and bounded
read-only project views. Checkpoint 2 preserves native observation, isolated test
inputs and instance authority using synthetic fixtures. Checkpoint 3 preserves
selected author sources and opaque provenance/report files without executing tools.
Checkpoint 4 reuses those entries in the existing save/reopen lifecycle; it needs
no separate evidence store. Checkpoint 5 exposes scoped verified candidate inventory
and identity. Checkpoint 6 carries those views through the SDK and repeats the
combined synthetic authoring fixture.

Applications own generation workflows, gameplay analysis, selective invalidation
and their specialized UX. The checkpoint additions do not introduce implicit
scripts, a general process API, a companion service, automatic instance adoption
or merging portable projects into existing checkouts.

## Feasibility and freeze gates

The [early experiment and resource profiles](feasibility.md) accompany checkpoint 1;
they are not an eighth permanent implementation path. Baseline tests, dependency
audits and behavior-preserving extraction may proceed while the experiment runs.
Do not freeze a dependent interface before its assumptions are resolved.

| Before | Required decision or evidence |
| --- | --- |
| Checkpoint 1 public types | Whole-authoring semantic validation, storage/execution proofs, constructor ownership, sealing and execution classes |
| Checkpoint 2 mutation extraction | Separate initialization/restore proofs and recoverable directory/metadata association |
| Checkpoint 3 format freeze | Executable discovery/coverage fixtures, restore transitions and minimum cooperative ZIP/browser proof |
| Checkpoint 4 persistence | Inherited storage owners, nonwaiting collection, immutable registration/retirement, distinct writer/storage capability failures and cross-store failure tests |
| Checkpoint 5 export completion | Shared codec reuse, all-stage responsiveness and truthful destination outcomes on required browsers |
| Checkpoint 6 release closure | Mandatory routes/devices, package compatibility, removed facades and same-candidate native/browser acceptance |

Early OPFS/ZIP/yield/cancellation and actual browser catalog/CDN experiments test
feasibility, not finished persistence or release acceptance. Their results cannot
substitute for the final immutable candidate matrix.

## Review and validation

Run focused tests for each boundary and a combined native/portable suite after
integration. Use automated code review for substantial completed implementations,
within the remaining review budget. Full browser/device and native acceptance binds
to the final candidate; earlier passes are evidence of earlier trees.

Documentation-only checkpoints need link, contract consistency and diff checks.
They do not need paid platform reruns. Mobile emulation is useful development
feedback, but physical iOS Safari and Android Chrome remain release gates.

## Release readiness

Checkpoint 6 closes the [acceptance contract](acceptance.md), including facade
retirement, command/package references, API/user documentation, native binaries,
SDK/harness packaging and capability evidence. A missing mandatory route, device
or storage/export check remains a blocker. A product SPA, Tauri application, public
hosting and cloud services are outside this release.
