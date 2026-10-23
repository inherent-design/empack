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
local/CI runner. It builds the ordinary product package in a separate target directory
before compiling the workspace test graph. Use Nextest 0.9.148 for these selections.
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
| `journey_authoring` | Literal layered authoring, rebuild and removal |
| `journey_pending` | Public build/sync inspection, verified manual association, retained partial inputs, restart, stale refusal and discard |
| `journey_pending_import` | Public import selectors, retained-archive restart, exact exported bytes and changed-destination refusal |
| `journey_publisher` | Local signed-channel enrollment, update conflicts, repair, rollback and trust changes |
| `journey_local_exports` | Offline resourcepack export and reference Prism preparation |
| `live_cli_init` | Official loader catalog resolution |
| `live_provider_cli` | Modrinth identity, acquisition and convergence |
| `live_server_cli` | Server acquisition/export and instance preparation; no game startup |

Live binaries contain ignored tests so a default run cannot silently claim their
coverage. Select them explicitly with strict prerequisites:

```sh
cargo nextest run -p empack-tests --test cli_arguments --test cli_dependencies --test cli_init --test cli_build --test journey_authoring --test journey_local_exports
EMPACK_E2E_STRICT=1 cargo nextest run -p empack-tests --test live_cli_init --test live_provider_cli --test live_server_cli --run-ignored ignored-only --test-threads 2
```

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
does not establish a live provider's restricted-download policy. Instance continuation
parity and remaining interactive/corrupt-record variants are still separate obligations. Moving upstream pack inputs belong to
`live_import_survey`, not the pinned regression corpus.

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
replace the broader pinned real-pack acceptance requirement.

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
its requirement even when its existing cases pass. Python and doctest commands still
run separately from the Rust receipts.

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
The pinned corpus split and scheduled rotations remain separate work; an upstream
survey is not a stable regression oracle.

Use `verification-gate` as the required PR status. It requires every selected job
to succeed, including the complete native matrix, and checks each uploaded suite
receipt against its compiler inventory and JUnit: source revision, target, product
features, selected identities, first-attempt results and evidence digests must agree.
Missing, cancelled, skipped, blocked or inconsistent evidence fails the gate.
For admitted Rust receipts, its `acceptance` section retains each bound case's exact
tests, required host/product variants, artifact digests and supporting receipt digests.
It rejects substituted evidence boundaries, mismatched revisions and conflicting
results. All 109 requirement groups remain in the report, including undecomposed
native work and staged portable work. A passing selected CI suite does not make those
groups complete. The report and source registry digests travel together.

The resulting record has `qualification: false`: the remaining case decomposition,
non-Rust/consumer/device evidence and final-package checks are not yet a complete
release qualification record. Selecting this required status in repository
branch protection is an administrative step; changing workflow YAML does not set it.

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
publishing. Tag pushes additionally publish the checked payload. These archive checks
are not complete candidate qualification: the remaining native/consumer requirement
results and approved-waiver policy still need their own gate. Missing platform
execution remains a failure, not a cross-build pass. A configured workflow is not
proof that its five-host execution has succeeded.

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
versions, compiler identity and build configuration. The probe compiles upstream `INIFile.cpp` and `Json.cpp`
without vendoring or changing their parsing code:

```sh
python3 scripts/check-prism-config.py --prism-source /path/to/PrismLauncher-11.1.1 --qmake /path/to/qmake
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
