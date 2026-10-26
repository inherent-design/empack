# Verification

The [acceptance contracts](design/verification.md) define the current release target.
Tests verify the checked-out implementation; a contract or test count is not evidence
that an unwired capability works. Record run-specific results in CI and PRs.

The v0.8.4 overhaul tracks the complete requirement denominator and compiled
discovery in the [verification registry](../tests/verification/README.md). Its
unbound requirements remain incomplete; structural validation is not a pass rate.

## Local checks

```sh
mise run check
mise run clippy
mise run test
mise run smoke
```

`mise run test` runs Python script contracts, the fixed deterministic workspace
suite (including executable tests), and doctests. `scripts/verify.py` is the shared
local/CI runner. Rust suites build the ordinary product package in a separate target
directory before compiling the workspace test graph. The `python` suite executes
script contracts independently, without selecting an empack binary. The `doctest`
suite runs the pinned rustdoc in both workspace feature graphs and records whether
each example ran, only compiled, or established an expected compilation failure.
Native CI requires these records on each host; a listing or a successful build does
not replace them. Use Nextest 0.9.148 for the executable and library selections.
Also check library tests with minimal features so workspace feature unification
cannot hide test-only dependencies. Architecture checks keep `empack-core` free of I/O/runtime dependencies.

```sh
cargo nextest run -p empack-lib --lib --no-default-features
```

The separated executable suites have explicit boundaries:

| Test binary | Evidence |
| --- | --- |
| `cli_arguments`, `cli_dependencies`, `cli_init` | Offline parsing, refusal and initialization |
| `cli_build` | Offline artifact creation, metadata and cleanup |
| `journey_import` | Pinned Modrinth/CurseForge archives, complete exported inventory and traversal refusal |
| `journey_authoring` | Literal layered authoring, local JAR update/adoption, rebuild and removal |
| `journey_pending` | Public build/sync inspection, verified manual association, retained partial inputs, restart, stale refusal and discard |
| `journey_pending_import` | Public import selectors, retained-archive restart, exact exported bytes and changed-destination refusal |
| `journey_pending_instance` | Public instance keys, partial retention, executable restart, stale installation and exact discard |
| `journey_publisher` | Local signed-channel enrollment, update conflicts, repair, rollback and trust changes |
| `journey_local_exports` | Offline resourcepack export and reference Prism preparation |
| `live_cli_init` | Official loader catalog resolution |
| `live_provider_cli` | Provider identity, pinned direct-URL acquisition and convergence |
| `live_server_cli` | Server acquisition/export and instance preparation; no game startup |

Live binaries contain ignored tests so a default run cannot silently claim their
coverage. Select them explicitly with strict prerequisites:

```sh
cargo nextest run -p empack-tests --test cli_arguments --test cli_dependencies --test cli_init --test cli_build --test journey_authoring --test journey_local_exports
EMPACK_E2E_STRICT=1 cargo nextest run -p empack-tests --test live_cli_init --test live_provider_cli --test live_server_cli --run-ignored ignored-only --test-threads 2
```

The local JAR authoring journey uses a source path with spaces and a distinct bound
installation path. Changing the source first proves ordinary sync refusal, then
explicit update installs the new bytes while retaining authored intent. Adoption
refuses mismatched source/destination bytes; matching changed bytes update only the
lock observation. Repeat synchronization and independently inspected Prism/mrpack
archives establish convergence and fresh output without a version bump. Removal
preserves source files, neighboring author work and empty directories, rejects an
occupied destination directory, and on Unix refuses linked ancestors leading outside
the project. These are executable fixtures; consumer startup and final-package
qualification require separate evidence.

The multi-file provider host journey connects explicit primary/companion roles through
add, synchronization, a renamed primary in an updated version, and adoption. It checks
complete native client/server inventories and both optional selections in bundled
Prism output against independent expected bytes. Native release descriptors retain
optional-choice metadata; Prism materializes the choice. Missing companions, missing
choices and unknown choices preserve author inputs and prior artifacts. These are
controlled catalog/transport and host-build checks, not public CLI or launcher
execution. Exact filename selectors remain version-sensitive; the `primary` role
is the stable selection when a provider renames its primary file.

The public `journey_content_kinds` scenario shares only literal archive inputs and
expected destinations with the host kind matrix. It drives the selected executable
from a written schema-5 project, refusing ambiguous ZIP inputs and malformed worlds
before adding mods, resource packs, shaders, datapacks and one wrapped world. Complete
native client/server and Prism file inventories check side selection, optional choices
and world-group metadata. Removing optional source/installed bytes and evicting the
cache proves that unselected content is unnecessary while selected bytes remain
mandatory. Host state lives outside the project so project snapshots include all
author files and artifacts without confusing journal retention with author changes.
Separate instance tests prove that initialized worlds retain player edits and missing
members across update, repair and rollback. None of these fixtures claims game startup.

The resourcepack scenario shares its assertions between offline client exports and
live server preparation. Missing Java cannot skip the offline client case. Component
fixtures created through codecs are distinct from the literal authoring journey;
neither proves that Minecraft or Prism consumed the resulting pack. `journey_import`
and `journey_verified_import` cover controlled local import paths. `cli_interactive`
uses real PTY/ConPTY sessions; `live_manual_assistance` adds an official provider
lookup with a fixture desktop opener. The pending-build journey starts with a component fixture, then obtains operation IDs
and association keys only from `pending list/show --json`. Fresh executable invocations
reject wrong bytes, resume renamed verified content and refuse stale drafts; terminal
coverage checks fresh approval. State snapshots assert preservation without interpreting
private continuation records. Synchronization also checks partial retention across
cache eviction. Import resumes a retained archive after deleting the original and
keeps incomplete requirement discovery explicit. Its unsupported-transport fixture
does not establish a live provider's restricted-download policy. Instance journeys
use a literal two-file manual release, retain a verified input
through a partial resume, remove the original download and descriptor, evict ordinary
cache and exercise retained cleanup, then complete from public keys through both
public and legacy continuation commands. They compare the complete game inventory and preserve player notes. A separate real installation
stales the saved operation; both discard interfaces preserve that installation and
remove only the selected record. Remaining interactive and corrupt-record variants
are separate obligations. A shared fault fixture corrupts actual saved records from
all four families without interpreting their private payloads. Malformed JSON and
unsupported schemas remain visible as unavailable; text and JSON inspection preserve
storage, resume refuses them, and explicit discard removes only the selected record.
Pure selector tests cover ambiguous prefixes separately from native stale-cleanup
ownership. Moving upstream pack inputs belong to `live_import_survey`, not the pinned
regression corpus.

Download assistance separates pure provider-page interpretation, native file scans,
host preparation, real terminal decisions and live metadata lookup. Scans verify
content assertions rather than accepting a unique filename. The wait fixtures reject
a sole wrong same-length file, preserve downloads and compare the complete resulting
archive against an independent inventory shared with the pending-build journey.
The controlled desktop launcher checks arguments, timeout, failure and retirement
following cancellation; it does not launch or supervise a real browser. The live
Modrinth case verifies page selection but still uses fixture payload bytes. Windows
opener behavior and remaining page-limit/failure cases stay explicit gaps in UX-037.

Remote-acquisition fixtures count bytes across failed mirrors, exercise stalled
headers and bodies, and retain author files through refused replacement. A shared
native HTTP fixture signals when a request actually arrives before triggering
cancellation or caller abandonment. It distinguishes EOF/reset from a socket timeout;
a timeout cannot stand in for connection retirement. Tests also inspect the operation
outcome so a worker panic cannot pass as successful cleanup. Credential fixtures
use the existing private origin mapping to exercise authenticated same-origin and
cross-origin redirects. These are controlled wire checks, not live TLS/provider or
public-executable HTTPS qualification.

The [archive header corpus](../tests/fixtures/archives/README.md) supplies literal
ordinary ZIP and ZIP64 bytes to source inspection, opaque inspection, and artifact
verification. It covers malformed record arithmetic and metadata admission in
addition to the existing member-name, CRC, and extraction-budget tests. Boundary
cases retain valid ZIP64 extension data at the combined metadata limit and reject
the next byte. The fixture driver uses no production archive encoder to construct
its expected inventory. Native captured-input faults and future browser execution
remain distinct evidence obligations.

The mixed CurseForge host fixture uses controlled provider records with no download
URL plus embedded author overrides. It resumes after the original archive is gone,
rejects mismatched supplied bytes, and retains exact provider identity separately
from local override ownership. Converting the provider file into a local or URL
source is refused before publication, even with valid supplied bytes. Re-export
checks the complete literal manifest and override inventory: supplied restricted bytes remain provider references. Optional
metadata loss requires a separate export acknowledgement, and refusal preserves
prior artifacts. This is controlled host evidence; the public pinned archive fixture
and actual credentialed provider checks establish different parts of the workflow.

Import-decision journeys refuse auxiliary-file exclusion without explicit consent
and refuse a requested runtime that contradicts the archive. They then import and
re-export the retained content against a literal inventory. Missing optional-default
and metadata-loss decisions have separate diagnostic assertions: refusal preserves
author documents, supplied content, the original archive and prior artifacts.
Controlled host tests cover provider-kind routing and world archive interpretation;
they do not substitute for executable evidence of every routing combination.

The populated migration host journey starts from literal authoring and bindings,
materializes controlled provider JARs, and updates an automatic root alongside an
exact pin. It captures the displayed JSON plan before migrating, then compares it
with application and independently expected client/server native archive contents.
A primary file role survives provider filename changes. A retained library must be
compatible with the new runtime; local sources keep dependency uncertainty explicit.
Preview, refusal and repeated synchronization check source preservation. These are
in-process host checks with controlled services. `live_provider_cli` separately
exercises ordinary CLI pin/update commands against Modrinth and checks the retained
selection report. Neither boundary claims actual consumer startup or complete
migration qualification.

Canonical provider selection has separate syntax, DTO ownership, controlled HTTP,
host-alias and live executable cases. Both controlled catalogs check slug, ID and
project-URL equivalence; CurseForge additionally verifies exact-slug search versus
direct numeric lookup. The shared live journey checks one canonical root after
approved re-adds, no-op synchronization, and preservation on refused replacements
and failed lookup. It snapshots author files without reading disposable caches or
recovery records. Commands have a 120-second deadline.

Search has separate host, executable and terminal evidence. Controlled catalog
responses distinguish an empty result from authentication, not-found, throttling
and server errors. Headless queries refuse before an observed proxy request and
preserve the project, lock and prior artifacts; this is not a network sandbox.
The live terminal journey filters displayed Modrinth labels by canonical project
identity, cancels the search, declines publication, then explicitly approves the
selected root. Scratch files remain separate from author files, and selection
alone does not authorize publication. Each terminal expectation is bounded at
120 seconds. Host pagination tests do not substitute for public terminal paging;
that path and credentialed CurseForge search remain separate qualification work.

Direct URL integrity has distinct parser, host, executable and live evidence.
The host journey covers compact/expanded declarations with authored assertions or
first-observation consent. It commits the lock, checks repeat-sync convergence,
then removes retained content and serves changed bytes at the same URL; publication
must fail without changing the lock or prior artifacts. Its private loopback adapter
does not establish production TLS behavior. The ordinary executable separately
acquires the immutable 3,573-byte More Chat History JAR from the pinned import corpus,
checks its recorded SHA-512/size and retained observation, and verifies preview and
repeat-sync preservation. That exercises production HTTPS without modifying trust
configuration or adding a product test endpoint.

`python3 scripts/verify.py live-provider-cli` selects the Modrinth and direct-URL
executable cases.
`python3 scripts/verify.py live-curseforge` also includes the CurseForge executable
journey and requires explicit `EMPACK_KEY_CURSEFORGE` before discovery/execution.
An absent credential is blocked evidence, not an empty passing selection. These
tests use current compatible provider selections; the independently hashed real-pack
corpus below retains a separate immutable-input contract. Public alias editing and
the remaining provider permutations still require their own acceptance evidence.

The offline `journey_selection` CLI fixture starts from literal authoring and bindings
plus a fixed provider lock assembled as component setup. Pin/unpin preserve the
selected inventory, custom placement, comments, logical alias and unrelated YAML
anchor/alias. Invalid grouped requests and previews preserve every project file;
two syncs after each edit retain the same content. Pinning removes the root release
preference; unpinning keeps the selected version and returns to the project preference.
The fixture does not prove live provider ownership: a separate controlled provider
case rejects a version belonging to another project. The native publication test
also checks stale-input refusal and preservation of locally modified payloads.

The populated synchronization matrix in `cli_manifest` distinguishes current, absent,
stale, malformed and unsupported locks under both `sync` modes. It verifies exact
restoration and repeated convergence, preserves unrelated author work, and refuses
implicit adoption even when untracked bytes match the source. A missing source and
installed copy is tested after cache eviction. Local hashes remain observations,
not declared provider evidence. The shared model fixture now uses literal payload
digests instead of creating a native staging lease; its pure scope tests cover
retained selections and required closure. Native fixtures separately cover reference
preservation, publication races and foreign metadata. Final-package execution of
the public matrix is a separate five-target obligation, not established by local tests.

Selected-update tests connect frozen provider records to the host preview, completion
receipt and materialized inventory. Updating an automatic root beside an exact pin
must report one changed selection and one retained selection, preserve unselected
records and authoring, and retain explicit file-role destinations. Pure candidate
cases cover reverse and exact-version incompatibilities, side separation, grouped
updates and preservation of negative evidence when the selected version is unchanged.
The host refusal matrix checks the whole populated project after both dry-run and
approved requests; no conflicting retained dependency is silently removed.

The live CLI sentinel checks an exact pin, unpins without upgrading, then performs
an automatic update. It compares the actual change count with the resulting pin and
independently hashes materialized bytes. A live catalog may already be current; that
run does not replace the frozen fixture proving a changed selection. Broader public
release-policy permutations and other-platform execution remain separate obligations.

Assessment fixtures compare complete report fields with independent expectations,
including exact pins and the distinction between permitted, compatible and upstream
versions. Controlled catalogs exercise malformed records, shared transfer/deadline
limits, retained incompatibilities and incomplete pagination. The public executable
checks populated manual and unavailable-provider rows in text and JSON, preserving
all author files and printing the report before strict-mode failure. A manual-only
root set stays complete even when the lock retains a forgotten provider dependency;
that does not establish compatibility of the retained inventory. The live pinned
journey checks a successful populated strict assessment through actual Modrinth
routes. Live observations do not replace the controlled failure and policy cases.

Runtime migration has a populated live executable journey alongside frozen host
cases. Real catalog selection changes Minecraft 1.21.1 to 1.21.2 while retaining an
exact pin, custom placements and local configuration. Plan JSON is compared with
preview/application text and the applied inventory; the public CLI intentionally
requires `--plan` for JSON. Client/server native outputs and regenerated Prism
components are inspected independently. A stale authored Prism profile must refuse
output while retaining author files and prior artifacts. Deliberate provider
replacement resets only the selected root; a redundant canonical replacement must
not erase an exact pin, even when authoring uses a selector. Offline executable
refusals and native publication tests cover invalid arguments, stale captures and
unrelated edits. These are catalog, content and configuration checks, not launcher
startup or gameplay evidence.

Independent-batch fixtures distinguish default all-or-none execution from explicit
partial completion. The public CLI test verifies named ready/blocked groups, nonzero
partial results, preserved files, a real changed-content update and truthful retry
counts. Malformed source interpretation still fails before independent groups are
prepared and preserves the whole request. Pure planners cover coupled requests; native
API tests cover coherent partial publication, stale approval and resource retirement. None of these checks permits
an unrelated failed request to disappear from the result.

The authoring journey accepts changed local content only after both source and installed
copies agree. Preview and refusal preserve all authoring and previous artifacts;
approved adoption changes the lock observation while retaining payload modification
times and attributes. Its SHA-256 and size checks distinguish accepted local bytes
from provider-declared evidence. First-lock reconstruction is tested separately: it
retains identity and byte evidence, but cannot retain an inferred display title that
was present only in the missing lock. Controlled provider cases check exact byte
identification, incompatible identities, copied placements and absence of latest-version
queries. A handler matrix refuses unknown identity, competing provider matches and
multiple matching file roles without edits or a completion receipt. The live CLI
reuses the pinned JAR corpus for URL, exact-version and byte-identified adoption,
checking absent/invalid-target refusal, provider-byte mismatches, payload attributes,
declared versus observed identity and repeated convergence. These fixtures still require execution on every
required host and product configuration; their presence is not release qualification.

Removal fixtures distinguish forgetting a root from deleting its selected content.
The offline CLI checks canonical identity under an author alias, grouped refusal,
reserved `--deps`, incomplete retained evidence and known dependents. Forgetting
changes documents while retaining the exact selection; acknowledging uncertainty
never bypasses a known requirement. Full file snapshots retain sources, worlds,
unowned directories and previous artifacts. A live provider journey adds an authored
alias, re-adds by canonical identity and removes through that identity after repeated
synchronization. Pure selection tests, native publication/confinement tests and
operation-authority tests supply separate evidence; live success does not replace
native conflict or stale-observation checks.

The publisher journey builds literal native projects, stages them using a disposable
private key, and checks the signed payload independently with Ed25519. Fresh CLI
processes enroll, observe local channel envelopes, install, update, repair, roll back,
and change trust. Assertions cover refused tampering and expiry, explicit conflict
resolution, replay floors, and preservation of player configuration and initialized
worlds. These are the documented local-envelope routes. They do not execute hosted
HTTPS publication, Java, or Prism, and do not substitute for those acceptance cases.

The [pinned import corpus](../tests/fixtures/import-corpus/README.md) uses small
repository-owned archives with fixed hashes and separately reviewed expectations.
Run `python3 scripts/check-import-corpus.py` or `mise run test:import:corpus` for
that selection; it also runs in the ordinary CLI and deterministic suites.
`scripts/survey-imports.py` deliberately resolves moving upstream packs. Its reports
identify themselves as rolling observations, record acquired archive and executable
digests, and cannot qualify the pinned corpus. `scripts/import-smoke-test.py` remains
a compatibility entry point for the survey. The small synthetic corpus does not
replace real-pack acceptance. The separate `python3 scripts/verify.py pinned-import`
suite acquires the fixed Fabulously Optimized 6.5.0 archive and checks complete
index/override preservation, two-sync convergence, and every downloaded mod's
bytes in a bundled export. Edited imported local dependencies require explicit
adoption; the fixture checks refusal preservation and fresh output without a version
bump after that acceptance. Its fixture contains metadata and hashes, not upstream
assets. This is live executable evidence, not launcher/game acceptance or coverage
of all real-pack formats.

Scheduled compatibility runs on the default branch each Monday. The shared runner
selects the synthetic and real-pack pinned corpora, official provider/catalog probes, and
current runtime checks. During the first seven UTC dates of each month it substitutes the broader
runtime suite, with both Java 8 and 21 installed. Missing credentials or prerequisites
remain blocked evidence. `python3 scripts/check-live-compatibility.py --plan-only`
writes the dated selection without claiming execution; omit that flag to run it.

A separate job runs `scripts/survey-imports.py --profile weekly`, selecting one of
the curated packs per UTC week without backfilling a failed slot. It admits at most
100 MiB per pack archive and 60 seconds per archive transfer, checks the transfer
deadline between bounded socket reads, and retains failed observations. Socket timeouts
bound a blocked read; they do not make Python reads instantaneously cancellable.
Manual fixture downloads have a 256-file/2-GiB total allowance and a 1-GiB per-file
limit. Each empack command has a 600-second deadline. The probe and survey jobs have
90- and 60-minute budgets respectively; no whole-command retry masks a failed mutation.

Survey archive caches bind the exact provider/project/file/URL selection and retain
an observed digest. Reuse verifies both identity and bytes and is labeled
`retained-observation`, not fresh provider execution. A new upstream selection cannot
reuse the previous version's bytes. The jobs upload reports even on failure and have
read-only repository access; manual dispatch on another branch cannot obtain their
credentials. This workflow is scheduled evidence, not a substitute for qualification
of the exact release candidate. Broader pinned real-pack coverage remains outstanding.

The executable harness lives in `empack-tests::harness::process`, outside the
product dependency graph. Local runs may discover a binary and print the selected
path. Qualification requires `EMPACK_QUALIFY=1`, an absolute `EMPACK_E2E_BIN`,
`EMPACK_EXPECT_BINARY_SHA256`, and `EMPACK_EXPECT_VERSION`. Wrong bytes or version
fail before the scenario. This is executable selection, not full package
qualification; target/features and consumer evidence still need the release record.
Resolve and verify the selected product before starting an observer or interaction
deadline. Hashing a large debug executable is harness setup, not time spent waiting
for a running command to reach its prompt.

Children receive a positive environment allowlist and fixture-local home, cache,
state and temporary paths. Tests explicitly add any needed fixture tools or product
settings. Harness controls and ambient credentials are not forwarded. A failed
mutating command is not replayed after matching a timeout diagnostic. Nextest CI
retries are disabled so the original failure remains visible. Live runtime probes
retain bounded, best-effort launcher output on timeout, with at most 64 KiB per
stream. These tails help diagnose the failure; they do not turn a timeout into a
pass or replace complete successful output assertions. Managed runtime probes execute
the generated installer first (300-second bound), require completed ownership and
selected content, then execute the generated start script (90-second bound). Startup
still invokes preparation again, exercising convergence before reaching Minecraft.
Installation failure cannot be counted as startup evidence. These help/EULA probes
are separate from interactive console and gameplay acceptance.

After building the executable, compare the editor schemas with the actual CLI:

```sh
uv run scripts/check-authoring-schema.py target/debug/empack
```

This exercises accepted and rejected authoring forms without creating host state or
cache storage. JSON Schema checks complement composed domain validation; they do not
replace it.

Formatting and Clippy apply to all targets/features. Native filesystem/process
claims require real temporary roots and OS primitives; pure planner tests cannot
prove link handling, locks or crash durability.

### Inherited terminal regression

On macOS and Linux, the normal native suite includes a real controlling-PTY test:

```sh
cargo test -p empack-lib --lib inherited_runtime_owns_terminal_and_restores_it -- --nocapture
```

It supervises an isolated runtime process, checks foreground ownership before any
console reads, sends input and terminal Ctrl-C, and verifies normal shutdown,
cancellation, nonzero exit, failed exec, disabled terminal suspend keys, external
stop/resume, cancellation while stopped, and restoration of terminal ownership and
settings. The fixture changes terminal settings to make restoration observable.
It proves host process/terminal behavior; it is not Minecraft gameplay acceptance.
Managed Prism tests separately verify the root icon's seed policy and rejection of
an explicit managed policy before publication, while keeping other PNGs managed.
Engine launch fault injection separately establishes that failed terminal cleanup
after confirmed retirement reports the error without retaining runtime ownership;
uncertain-retirement tests still require recovery. Signed-channel tests use the
native release producer and a persisted Prism layout, reject managed icon updates,
and preserve launcher bytes through corrected seed updates and launch preparation.
Historical managed-icon fixtures verify migration to seed without conflict overrides.

Executable finalization is tested separately from process supervision:

```sh
cargo test -p empack --bin empack
cargo test -p empack-lib --lib launch_failure_summary
```

These checks retain cleanup, recovery, uncertainty and partial-effect diagnostics
when cancellation is set, retain success after a late interrupt, and verify that
launch failure headlines do not imply that the runtime never ran. The executable
fixtures test result presentation; the engine and PTY fixtures establish ownership
cleanup and actual process retirement.

The `managed_runtime_exit_uses_empack_classification` host test runs a fixture child
with exit statuses 0 and 7, then verifies managed CLI classification and the retained
runtime-status diagnostic. It does not launch Java. The Unix
`cli_interrupt_exits_130` test sends SIGINT to the actual empack executable during
a bounded local HTTP request and checks both exit 130 and stderr. Run it against a
freshly built binary to test executable wiring:

```sh
cargo build -p empack --all-features
cargo test -p empack-tests --all-features --test cli_exit_codes cli_interrupt_exits_130
```

## Shared suite runner

```sh
python3 scripts/verify.py deterministic
python3 scripts/verify.py cli
python3 scripts/verify.py telemetry
python3 scripts/verify.py live-catalog live-provider-cli live-modrinth server-cli --product-profile release
```

Fixed selections live in `scripts/verification/suites.py`. `mise run e2e` remains a
compatibility alias for the offline executable suite. `mise run e2e:strict` explicitly
selects live CLI routes and the rolling import survey as well. Telemetry checks are
ignored by default and fail if an explicitly selected product cannot produce a trace;
absence of the feature is not a successful test.

`live-provider` includes both providers and requires `EMPACK_KEY_CURSEFORGE`.
The separate `live-modrinth` and `live-curseforge` selections name exact discovered
provider tests. `runtime-current` selects the managed vanilla and NeoForge launcher
checks plus the Fabric classpath check. `live-runtime` retains the full historical
matrix and additionally requires `EMPACK_TEST_JAVA8_HOME`. Missing named tests or
prerequisites cannot silently reduce one of these selections. CI live probes and
`e2e:strict` build an independent release-profile product; normal deterministic and
CLI checks retain their development-profile builds. The test driver remains separate
from that product. Use `--product-profile release` for comparable local live evidence.
The receipt records the selected profile, and the CI gate rejects a development
artifact as a substitute for the live release-profile run. This does not change
installer/startup deadlines or retry a failed operation.

Each invocation creates a fresh directory under `target/verification/` containing
compiler discovery, Nextest JUnit, bounded redacted logs, a source-file digest manifest,
and `receipt.json`. The Rust inventory must match the reviewed platform and test-feature membership,
including ignore policy, before execution. `--test-features minimal` uses the
workspace with `--no-default-features`; `--test-features all` uses `--all-features`.
The harness still requests its explicit `test-utils` dependency in the minimal
graph. These flags describe the test graph, not the separately built product.
Receipts retain both discovery and execution commands; the gate checks their exact
selection and graph rather than trusting a feature label. Every selected test must
produce a passing outcome. Empty required binaries, missing results, skipped selected
tests, retries, failed commands and changed
source/binary bytes cannot yield a suite pass. The final CI gate also checks the
classification digest and the compiler inventory's actual host target. Missing tools
or prerequisites produce `BLOCKED`; a Unix-only desktop fixture on Windows produces `NOT_APPLICABLE`.

The runner does not retry an attempt. Re-running it creates another record and retains
the earlier failure. These are suite records, not final release qualification or proof
that every requirement has a bound case. The CI report joins validated Rust results to
the concrete cases and required platform/product-feature variants in the registry.
Missing variants remain `NOT_RUN`; a partial or pending decomposition cannot qualify
its requirement even when its existing cases pass.

Run Python tooling through the same entry point:

```sh
python3 scripts/verify.py python --output target/verification-python
```

This fixed suite uses unittest discovery, an isolated worker environment and a
15-minute deadline. It records each test's lifecycle alongside discovery, source,
classification and interpreter identities. Admission replays those events: subtest
failures, expected failures, skipped assertions, setup errors and incomplete runs
cannot become passing cases. Missing or repeated identities are rejected. PR CI
requires this record even for documentation-only changes; a green tooling job alone
is insufficient. Python cases use the `independent` feature variant and cannot
substitute for empack, packaged artifacts or actual consumers. Raw `unittest` remains
useful for focused debugging but does not produce an admissible receipt.

The worker uses the existing Rustup installation and selects the repository's
pinned compiler with automatic toolchain installation disabled. Cargo home and
application home/state/cache remain isolated; host Cargo credentials, configuration
and compiler wrappers are not inherited. The receipt identifies the compiler and
Cargo used by the small offline build fixtures. A missing pinned installation
blocks the suite before tests run; Cargo's `--offline` alone does not prevent
Rustup from downloading a toolchain.

Doctests are discovered and executed separately and their receipts are joined to
their own acceptance cases. Source-edited or interrupted tooling attempts remain
nonqualifying. `report-verification.py --receipt` accepts both native and Python
records and preserves earlier failed attempts.

To select an independently built executable, supply all identity fields explicitly:

```sh
python3 scripts/verify.py cli --binary /absolute/path/empack --sha256 EXPECTED_SHA256 --version EXPECTED_VERSION --target TARGET_TRIPLE --product-features default --product-profile release
```

The runner verifies digest and version. For an external executable, target, features and build profile
are caller-supplied provenance; final release qualification must bind them to the build
record. A locally built product records the runner's ordinary-package build selection.
Neither mode establishes actual consumer startup, platform coverage elsewhere, or final
archive installation.

## Change selection and final gate

The Verification workflow starts for every PR, including documentation and stacked
branches, and for pushes to `main` or `dev`. The former branch and post-merge
workflows are consolidated into this path. Pushes compare the exact before/after
trees; a new branch selects every tracked path. A missing comparison revision or
checkout mismatch fails planning. There is no path filter that can suppress the
terminal gate.
`scripts/ci-verification.py` compares immutable base/head revisions with renames
disabled, so moving code into a documentation path cannot hide its removal. Markdown
and editor-schema changes run Python, registry structure, Markdown and executable
schema checks. Other changes also select the deterministic native matrix, feature
checks, ARM64 compilation and one deterministic coverage run.
Each event gets fresh evidence for its checked-out revision. No PR-result reuse is
implemented; even an equivalent-looking merge is not treated as previously tested.
Lockfile, toolchain and verification changes therefore select fresh checks. ARM64
cross-build remains compilation evidence only.

Each native target runs the deterministic suite against the ordinary default product,
under the minimal workspace graph, then repeats the deterministic suite plus telemetry
contracts under the all-features graph against a separately built telemetry product.
The gate requires both product/graph pairs; a telemetry build or an all-features
inventory cannot stand in for the default product and minimal test graph.

Provider, runtime and managed-client paths select additional relevant probes.
Shared dependency/toolchain or verification changes select representative routes
across those boundaries. Changes to the CurseForge adapter or its mixed-provider
probe require the credentialed suite as well. This is change-based PR/branch evidence;
it does not replace the complete provider/loader matrix required for a candidate.

Live and consumer jobs run only on repository branches or a maintainer's manual
dispatch. Forks use GitHub-hosted runners for secretless checks, receive no provider
or coverage credentials, and cannot satisfy a required live job by skipping it.
The final gate reports the missing trusted execution. A maintainer can review and
test the change on a repository branch; the workflow does not use
`pull_request_target` to execute fork code with secrets. The selected CurseForge
job expects the `EMPACK_KEY_CURSEFORGE` repository or organization secret and records
`BLOCKED` if unavailable.

Manual dispatch also runs the rolling import survey and the existing import-smoke
driver. Broad surveys are excluded from ordinary PRs and instrumented coverage.
Pinned real-pack inputs and scheduled rotations use separate selections; an
upstream survey is not a stable regression oracle. Import/export changes select
the fixed `pinned-import` suite, while docs-only changes do not acquire packs.

Use `verification-gate` as the required PR status. It requires every selected job
to succeed, including the complete native matrix, and checks each uploaded suite
receipt against its compiler inventory and JUnit: source revision, target, product
features, selected identities, first-attempt results and evidence digests must agree.
Missing, cancelled, skipped, blocked or inconsistent evidence fails the gate.
For admitted Rust receipts, its `acceptance` section retains each bound case's exact
tests, required host/product variants, artifact digests and supporting receipt digests.
It rejects substituted evidence boundaries, mismatched revisions and contradictory
records claiming the same receipt identity. All 109 requirement groups remain in the report, including undecomposed
native work and staged portable work. A passing selected CI suite does not make those
groups complete. The report and source registry digests travel together.

The gate writes `gate.json` and a readable `gate.md` together; CI uploads both and
adds the Markdown to its job summary. The view lists requirement groups, their
binding status, exact execution variants, and source/executable/receipt hashes.
Failed evidence admission produces a diagnostic summary without a passing acceptance
table. Report generation time is not an execution timestamp. The renderer does not
revalidate standalone JSON, apply waivers, or calculate coverage from test counts;
the JSON and original job artifacts remain the evidence to inspect.

Doctest cases retain their required mode (`run`, `compile`, or `compile_fail`) and
workspace test graph independently of product features. The release candidate gate
admits both graphs from each native host using the same observed-rustdoc verifier.
Compiler-only examples cannot replace executed examples, and failed attempts remain
visible even when a later run passes.

The resulting CI record has `qualification: false`: the remaining case decomposition,
script, consumer and device evidence and final-package checks are not yet a complete
release qualification record. Selecting this required status in repository
branch protection is an administrative step; changing workflow YAML does not set it.

To inspect failed runs alongside later attempts, supply each receipt explicitly:

```sh
python3 scripts/report-verification.py --revision FULL_COMMIT_SHA --version 0.8.4-beta --receipt /path/to/first/receipt.json --receipt /path/to/second/receipt.json --output /path/to/new-report
```

This command writes `attempts.json` and `attempts.md` after rechecking the selected
receipts against their retained inventories, JUnit and the checkout's classification.
Keep each attempt directory intact. Use the source revision and product version
actually recorded by the selected runs; this command does not execute tests or qualify
a different candidate. Select all relevant attempts explicitly: it cannot discover
omitted history. Identical receipt files are counted once. Distinct attempts retain
their dates, suite names, test graph and executable hashes. A failed test followed by
a pass stays failed in the combined view, including when two suites execute that test
within one receipt. Missing tests remain unexecuted.

A blocked preflight can explain the missing prerequisite without supplying an
executable or passing tests. Unreadable, malformed, mismatched or changed evidence
is listed as rejected and contributes no acceptance results. Failure of the runner's
final source or product check invalidates its earlier test results. Reports preserve
failed and blocked outcomes ahead of unstarted suites; they never turn an incomplete
run into success. Exit status is `0` for passing selected attempts, `2` for a blocked
selection, and `1` otherwise. Even `0` does not establish release qualification.
Strict CI and archive gates still require one complete passing attempt per selected
configuration. The report does not grant waivers or replace those gates.

## Strict external checks

```sh
mise run e2e:strict
mise run smoke:runtime
mise run smoke:providers
```

Strict execution fails when prerequisites are missing. Runtime checks need public
Mojang, loader and Maven access plus Java 21 through `JAVA_HOME` or `PATH`. Historical
Forge cases use `EMPACK_TEST_JAVA8_HOME`. Provider checks use declared credentials;
`EMPACK_KEY_CURSEFORGE` supplies the explicit live CurseForge suite. Never record
credentials, signed locators or downloaded third-party bytes in committed reports.

The runtime task builds the CLI before running. Managed-server probes use
`EMPACK_E2E_BIN` when supplied, otherwise the workspace debug executable. They
extract a reference distribution, run its generated installer and launcher, require
a completed native instance record, and reach Minecraft help through local Java.
Host state and caches remain in temporary directories.

The runtime matrix covers vanilla, Fabric launcher layouts, Quilt, historical and
modern Forge, and early/current NeoForge families. Validate official installer
profiles, expected libraries and launch arguments. Help output or EULA refusal
proves launcher execution, not gameplay; tests never accept the EULA.

## Consumer and release evidence

Inspect exported manifests and archives independently, then test the actual supported
consumer. Prism ZIP imports, Modrinth and CurseForge format acceptance, server startup
and native instance updates are separate gates. Do not assume a TAR/7z tree is
importable wherever a ZIP is accepted.

Use a separate Prism profile through `--dir` and import each generated archive
through `--import`. Authenticate through the launcher when needed; never copy
account files into test fixtures or publish launcher logs containing credentials.
Check the imported runtime components and selected file bytes. Prism may use
`minecraft/` for platform imports and `.minecraft/` for native instance archives.

For a referenced Prism export, launch the imported instance and verify both native
preparation and the `Prism → empack → Java` process chain. Change the test release
descriptor without updating its expected hash, then launch again: empack must fail
preparation, Prism must propagate the nonzero result, and Java must not start.
Restore the descriptor afterward. Modrinth and CurseForge snapshot imports must
launch without empack hooks. Record the consumer version and distinguish successful
import, game startup and gameplay; these are different assertions.

Native release acceptance includes A-to-B updates, choices, user-edited configuration,
manual content, offline policy, trust failures, runtime changes and rollback. Crash
tests terminate a real subprocess at durable boundaries and recover in a new process.
Use a played-world sentinel to prove update/rollback confinement.

Curated large-pack workflows run through the production CLI and resource profile.
Inspect inventory and selected bytes, not only artifact size or exit status. Compare
repeated sync and rebuild behavior with unchanged version labels.

## Extracted archive checks

Run the fixed offline CLI suite against an existing distribution archive on its
native execution host:

```sh
python3 scripts/check-release-archive.py --archive /absolute/path/empack-0.8.4-beta-aarch64-apple-darwin.tar.gz --sha256 EXPECTED_ARCHIVE_SHA256 --version 0.8.4-beta --target aarch64-apple-darwin
```

The checker captures and hashes the archive before extraction, admits only the
expected regular executable, checks Unix executable attributes, and extracts outside
the checkout into a path containing spaces. It invokes the shared `cli` suite with
the extracted executable's exact digest and version, without discovery or rebuilding
the product. Each public command still uses the harness's isolated home, state and
cache. Missing native execution support is `BLOCKED`; cross compilation is not a
substitute. The test harness itself is compiled from the current test checkout.

`target/verification-packages/` retains one package record and the underlying suite
receipt, compiler inventory, JUnit and bounded logs per attempt. Archive and executable
digests are rechecked afterward. A changed, failed or unexecuted archive cannot pass.
The record distinguishes the test-source revision from the product's caller-supplied
identity; it does not establish the product's build provenance by itself. The release
workflow joins this evidence to the corresponding build record. It executes each of
the five archives on its native target, including ARM64 Linux; cross-compilation alone
does not satisfy execution. The gate rechecks source, version, profile, features,
archive/executable digests and the underlying compiler inventory and JUnit results.
Unix packaging excludes host filesystem metadata, including macOS AppleDouble entries.
Only those tested archives enter the publication payload. Publication verifies its
checksums and the gate's evidence digest again, without rebuilding.

`workflow_dispatch` accepts the reviewed version and exercises this pipeline without
publishing. Tag publication now requires both `archive-gate` and `candidate-gate`.
The latter retains all 88 native obligations and makes the 21 staged portable
obligations mandatory from v0.9. Incomplete case decomposition, absent required
evidence boundaries and missing observations block qualification, even when every
archive test passes. Missing platform execution remains a failure, not a cross-build
pass. A configured workflow is not proof that its five-host execution has succeeded.

The current registry is incomplete, so the candidate gate deliberately blocks
publication. Completing its native/consumer case bindings, collecting their evidence
and adding the corresponding workflow producers remain v0.8.4 implementation work.
The workflow currently supplies the final-archive receipts; it does not yet supply a
complete candidate matrix. No approved low-risk waiver policy is installed, and this
gate accepts no waivers. Critical or actual-consumer omissions cannot be excused by a
caller-supplied result label.

Release builds now run `scripts/prepare-release.py` in a clean disposable checkout.
The requested version must match the reviewed workspace release target. Stamping
changes only the workspace version and its local package entries in `Cargo.lock`;
external dependencies and their checksums remain unchanged. The input record retains
the source commit/tree, original and stamped Cargo-file digests and toolchain digest.
Build metadata uses the source commit's date, and both Cargo and cross build with
`--locked`. A new dependency or an ambiguous lockfile shape requires a source change,
not implicit dependency resolution during publication. `Cross.toml` forwards only the
public `GIT_HASH` and `BUILD_DATE` stamping variables into the cross-build container.

Build artifacts retain `build.json`; execution artifacts retain the package and suite
receipts. `scripts/release-artifacts.py gate` validates their complete five-target set
and writes `archive-evidence.json` plus `SHA256SUMS` alongside the unchanged archives.
The evidence explicitly records that full candidate qualification remains separate.

To evaluate an already checked payload without publishing:

```sh
python3 scripts/qualify-release.py gate --payload /path/to/checked-native-archives --consumer /path/to/prism.json --archive-evidence-sha256 EXPECTED_GATE_DIGEST --revision FULL_COMMIT_SHA --version 0.8.4-beta --output /path/to/candidate.json
```

Run from the exact clean candidate checkout. This rechecks the archive payload's
identity and evaluates the source registry, then writes JSON and Markdown with
explicit outstanding obligations. Supplemental `--receipt` inputs are rechecked
against inventory/JUnit, the same source/version and the actual shipped binary for
that target. Bare suite receipts cannot admit arbitrary feature builds.

For a required telemetry variant, run the fixed auxiliary producer from the clean
candidate checkout:

```sh
python3 scripts/check-auxiliary-build.py --version 0.8.4-beta --output .git/auxiliary
```

It stamps a disposable source clone, builds only the ordinary `empack` package with
`--features telemetry --release --locked`, and runs the shared CLI and telemetry
suites against the retained executable. The build uses an isolated Cargo home, a
separate target directory, the pinned native compiler and the candidate's build date.
The original source checkout and default archives remain unchanged. Add each emitted
`build.json` to candidate evaluation with repeated `--auxiliary` options; keep its
executable and `suite/` evidence alongside it. Admission checks the prepared inputs,
compiler, fixed command, executable bytes and independently replayed suite results.
These are trusted workflow producer records, not third-party signed attestations.

Auxiliary builds can satisfy their explicit feature/host cases, but never package
cases or replace any of the five shipped archives. Failures remain failures even if
a later attempt passes. The candidate workflow produces telemetry evidence on Linux,
macOS and Windows; it does not treat cross-compilation as native execution.
Repeated `--consumer` inputs admit the fixed Prism parser producer for the same clean
source. Admission checks the supported Qt/compiler environment, probe and fixture
identities, and every recorded setting. A claimed PASS with broken required commands
is rejected; an accurately recorded failure remains failed. This evidence contributes
only to the parser case, independently of product features. It cannot qualify actual
Prism import/startup or any missing native case. The release workflow runs the same
parser action as PR CI and requires its record before candidate evaluation.
Repeated `--tooling` inputs admit the fixed Python producer for the same clean source.
The candidate workflow runs it before evaluation and retains its discovery, lifecycle,
log and receipt files. It contributes only to explicitly bound tooling cases; it does
not supply product-feature, archive or consumer proof.
The command returns `0` only for full qualification, `2` for missing obligations and
`1` for failed or rejected evidence.

Package cases reference ordinary executable test IDs, but require additional proof
from the archive gate. Running the same test through ordinary CLI CI does not satisfy
its package case. Each package variant identifies an exact target triple; Intel and
ARM64 evidence cannot substitute for each other merely because they share an OS.
The initial `UX-085` package case checks local content export and instance preparation
on all five targets. It is a partial binding, not complete package acceptance.

The release job rechecks both the archive-evidence digest and the qualification
digest from its prerequisite jobs, including source, version and payload association,
before publishing unchanged bytes. Candidate diagnostics stay in a separate workflow
artifact; they do not become extra distribution files. A blocked candidate cannot
reach publication through a passing archive job.

## Coverage and release qualification

Measure production-source coverage separately from inline test bodies where tooling
permits. Coverage supplements behavioral evidence and must use the candidate revision.
Do not combine unrelated historical runs into a same-head acceptance claim. Native
platform, provider, runtime and consumer results identify their tested revision,
tool versions and explicit exclusions. Release publication follows successful
acceptance of one immutable candidate and dependency graph.

Generated project automation is covered by `scripts/tests/test_project_automation.py`
and the executable `smoke_generated_workflow` contract. The latter initializes a pack
with spaces in its name, builds Modrinth and Prism profiles through the generated
helper, rejects a mismatched tag and excludes stale output. Python tests verify
download digests, staging names, checksum contents and draft publication ordering.
Windows CI executes generated batch scripts with native `cmd.exe`; Java fixtures
check argument selection and exit status without claiming Minecraft gameplay.

Server archive tests check both console and GUI launchers in ZIP, TAR and 7z outputs.
Native shell and Windows command fixtures verify the default `nogui` argument, its
absence from GUI launchers, argument boundaries, preparation ordering and exit codes.
Live runtime probes exercise console launchers; they do not certify desktop GUI rendering.

## Managed consumer interpretation

Managed Prism configuration tests include absent, unsupported, duplicated and
misplaced version markers, command shadowing, multiline quoting, and hash-prefixed
settings with open quotes or trailing continuations under both supported versions.
Include ordinary space/tab controls and Unicode whitespace at key, section, comment
and value boundaries. Accept LF/CRLF and unterminated final lines; reject bare CR and
unsupported controls. Unicode display-value fixtures remain accepted without changing
author bytes. Semicolon controls preserve the effective lifecycle settings. Compare the
same fixtures with Prism 11.1.1's actual `INIFile::loadFile` implementation, using
a clean upstream checkout and Qt 6.4 or newer within major version 6, as required
by that Prism revision. Qt is opt-in test tooling, not an empack runtime dependency.
The probe rejects unsupported Qt versions before compilation. Earlier Qt 5 runs are
supplemental diagnostics, not qualification of Prism 11.1.1. Consumer-parser execution
is separate from launching Minecraft. Its receipt records application and empack
revisions, dirty state, fixture SHA-256, platform/architecture, compile/runtime Qt
versions, compiler identity, build configuration, probe executable digest, observation
time and source-input digests. The schema-2 record retains every fixture observation;
settings failures return a nonzero status after writing the observations. The gate
recomputes the outcome from those settings rather than trusting the status field.
Dirty-source results remain diagnostic only. The producer currently runs on Linux
and macOS; the required parser case selects Linux, matching the CI runner. This does
not qualify other consumer operations or hosts. The probe compiles upstream `INIFile.cpp` and `Json.cpp`
without vendoring or changing their parsing code:

```sh
python3 scripts/check-prism-config.py --prism-source /path/to/PrismLauncher-11.1.1 --qmake /path/to/qmake > /path/to/prism.json
```

Use a clean checkout at `7d2d3c1ec6fcf8255ebacd02d9f792688575dfb4` and supported Qt 6
development tools plus `make`. The shared fixtures also run through empack's public build
preparation. The probe verifies accepted settings and reproduces the legacy
shadow and hash-prefixed quote/continuation counterexamples; it does not certify
every rejected representation.

Managed server tests check archive attributes in ZIP, TAR.GZ and 7z for identical
scripts captured with and without executable source permissions. On Unix, extract
and directly execute the shell entry points with controlled fake external tools.
Windows tests execute batch entry points with `EMPACK_NO_PAUSE=1`, checking full
diagnostic text, empty syntax-error output and preserved exit status. Source
permissions and authored bytes remain unchanged after candidate preparation.

## Documentation checks

The repository-relative validator checks inline local Markdown links and anchors,
fenced blocks and selected prose rules. It does not check external URLs or claim to
parse every Markdown extension. File checks work without Git metadata:

```sh
python3 scripts/check-docs.py docs/testing.md docs/design/builds.md
```

Git-dependent checks are explicit. After fetching the relevant branches, validate a
stacked contract snapshot without switching the working tree:

```sh
python3 scripts/check-docs.py --ref origin/dev-v0.9-verified-export --base origin/dev-v0.9-browser-store --docs-only --strict-prose docs/development/v0.9.0-beta
```

This checks parent ancestry, changed-file scope and diff whitespace in addition to
the selected snapshot's documents. Ref names are inputs; no developer-local branch
list, fixed main revision or `.git/` helper is required. Documentation checks do not
run the native or browser/device acceptance matrix.
