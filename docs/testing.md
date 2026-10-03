# Testing

empack uses deterministic in-process tests as the primary proof layer, then live E2E to confirm real CLI, filesystem, subprocess, and provider behavior.

## Test Commands

Use the `mise` tasks in [`../mise.toml`](../mise.toml):

```bash
mise run test              # unit + mock/integration + doctests, excludes E2E
mise run smoke             # deterministic subprocess runtime contracts
mise run e2e               # live E2E suite only
mise run e2e:strict        # telemetry build, missing prerequisites fail
mise run e2e:filter add    # filtered E2E slice
mise run coverage          # instrumented binary + workspace coverage
mise run check             # cargo check --workspace --all-targets
mise run clippy            # cargo clippy --workspace --all-targets -- -D warnings
```

`mise run test` is the fast default gate. `mise run e2e` is a separate live suite because it depends on external tools and network conditions.

`mise run smoke` builds the CLI and runs `crates/empack-tests/tests/smoke.rs`.
These tests also run in the default test gate. They use isolated projects, a
fake packwiz executable, and seeded HTTP responses, with a ten-second subprocess
deadline and a loopback proxy to prevent fallback to live providers. No Java,
provider credentials, or managed tool download is required. They verify:

- search-based dry-run previews include the resolved add command and preserve `empack.yml`, `pack/pack.toml`, and `pack/index.toml` byte for byte;
- a provider failure returns network exit code `3` in normal and dry-run sync, while preserving project files;
- empty dependency sets remain a successful no-op;
- dotenv precedence reaches actual command execution, and malformed dotenv files return configuration exit code `2`;
- installed datapacks in default and custom YAML/TOML folders are recognized;
- unreadable installed state aborts sync instead of being treated as an empty pack.

This smoke layer exercises the real CLI, configuration loading, disk cache,
planning, output, and exit status. It does not establish real packwiz execution,
provider availability, archive correctness, or interactive behavior.

The complementary `sync_workflow` tests exercise mixed successful and failed
resolution, preservation of installed unresolved dependencies, and successful
search resolution followed by an add. Partial success must not be reported as a
fully synchronized project.

## Safety and runtime review, 2026-10-03

This review starts at `3625a89`. The supplied source audit was treated as a set
of hypotheses, then checked against executable regressions and current code.
The implementation keeps the session/provider architecture.

| Contract | Change and evidence |
| --- | --- |
| Preview and declined initialization preserve project files | Forced reset now follows validation, confirmation and the dry-run return. Init tests compare the existing project tree after preview and rejection. |
| Explicit dependency intent cannot become a search | Deserialization dispatches on `status` and rejects unknown fields. Config tests cover missing identities, misspelled fields and malformed local entries. |
| Sync detects installed identity and pin drift | Live packwiz metadata is checked before manifest writes or subprocess calls. CLI smoke covers provider, project and pin changes under the same manifest key. Unsupported replacement returns an error. |
| Explicit roots cannot be removed by incomplete orphan analysis | `remove --deps` fails before mutation because installed metadata does not supply complete dependency edges. Ordinary explicit removal remains available. |
| Generated shell metadata remains data | The embedded installer uses shell-quoted assignments and `printf`. A regression executes hostile metadata in a temporary directory and checks that no command substitution runs. New project templates retain build-time placeholders. |
| Build and continuation outputs stay in selected roots | Artifact names reject path components. Live output checks reject symlinked ancestors. Continuation cache and destinations are checked against runtime-derived roots before writes. Tests cover traversal and crafted saved state. |
| Process deadlines cover child and pipe lifetimes | A shared deadline covers exit and stream drainage. Tests close pipes before sleeping and retain pipes in descendants. Cancellation stops owned processes; CLI smoke verifies exit 130, retained recovery markers and lock release. |
| Mutation ownership and document publication are explicit | Mutating commands hold an OS project lock. Manifests, markers and continuation records use atomic sibling-file replacement. Tests cover lock contention, symlinks, read-only documents and unchanged old content after failed publication. |
| Tool and network policy has observable failures | Tool resolution is lazy, probes require successful exit status, new managed downloads use pinned SHA256 digests, and installation locks survive process crashes. JAR identification uses shared retry/rate policy and distinguishes 404 from provider failures. |
| Resource use has defined bounds | Future rate permits are reserved, cache persistence is atomic and size-limited, and imports reuse one archive reader during extraction with entry/count/total limits. Focused tests cover exhausted budgets, stale headers and oversized archive metadata. |

The review ran on macOS arm64. The default gate passed 1,303 tests and eight
doctests; the offline CLI smoke gate passed 12 tests. All-feature Clippy passed.
All 95 strict E2E tests passed. A fresh instrumented run passed 1,398 tests
with one manual rendering test ignored; production-source line coverage was
15,306 / 17,205 (88.96%), excluding separate `.test.rs` files but including inline
tests. All seven curated imports and client-full builds passed, including
restricted continuation, and every output ZIP passed CRC validation. Platform
CI results and review follow-up checks are recorded in PR #82.
A separate empty-cache `requirements` run downloaded the pinned managed tool,
verified its digest and successfully probed its version.

Run binary-producing validation tasks sequentially. The strict suite requires a
telemetry-enabled CLI; a concurrent default build can replace that executable
and invalidate telemetry assertions. Use `EMPACK_E2E_BIN` with an isolated copy
when separate runs need different binaries.

### PR review regressions

Greptile identified a later 429 cooldown being overwritten by an earlier reserved
window, and oversized cache entries preventing subsequent persistence. Both
regressions failed before their fixes. Budget generations now invalidate sleeping
reservations; callers re-acquire instead of using an obsolete slot. Cache entries
are bounded by total bytes as well as count, with tests for eviction, oversized
responses and preservation of unrelated cached data.

A separate remote-import test reproduced immediate failure on a recoverable 429.
Modpack metadata and import enrichment now use the same request policy as JAR
identification. The focused network/import suite passed 189 tests after these
changes. Platform CI also caught a Unix test missing its platform guard; the
guard was restored before the next run.

The next review found that restricted mrpack exports shared an import cache
across projects. Export and continuation now derive that cache from canonical
project identity. Tests cover identical filenames in two projects, path aliases
and non-UTF-8 path bytes. The full default gate passed 1,312 tests and eight
doctests before the additional alias regression; all 29 focused cache and
continuation tests passed afterward.

Windows children start suspended and resume only after joining an owned job.
Hosts that prohibit nested job registration receive an error before child code
executes. Windows-only regressions verify that rejected registration has no child
side effects, job cleanup closes inherited pipes after ancestors exit, and
stopping one job preserves another. This avoids depending on process snapshots
that can miss short-lived ancestors. Native CI must verify these Windows-only
cases. Windows E2E fixtures retain native known-folder environment values and
compare destinations as paths.

### Remaining boundaries

Atomic document replacement and project locks are not a multi-file transaction.
A failed external action can leave partial state, and forced initialization does
not yet stage a replacement with rollback. Direct library mutation callers must
hold the project lock themselves. External editors do not participate in it.

Sync rejects identity/version drift instead of replacing installed dependencies.
Automatic orphan cleanup remains unavailable until graph edges and provider
identities can be verified. These are explicit feature gaps, not successful
reconciliation. Existing user-owned shell templates are executable code; projects
must adopt the new quoting pattern themselves. Path validation does not provide
a sandbox against another process swapping paths during an operation.

Cancellation reaches subprocesses and restricted-download waits, but is not yet
threaded through every synchronous filesystem or bootstrap operation. Display
state still has process-global components. Cache writers in separate processes
can lose disposable cache updates; project document locks do not cover the cache.
Import planning still reads archive bytes, while execution reuses a file-backed
reader. These limits remain inputs to the transaction and command-service design.

## Runtime contract review, 2026-10-02

Review started at `b9ecfc772c5268fab98ec298826d2e8737a1ac38` on macOS arm64,
with Rust 1.94.0, Java 21.0.2, and managed packwiz-tx. The bootstrap notes at
`~/.atlas/bootstrap/empack.md` describe an April snapshot, not current evidence.
The initial default suite passed 1262 tests; the live suite reported 93 passes,
but prerequisite checks could return early and still count as passing functions.

Regression tests reproduced six runtime defects before their fixes:

- Search-based sync previews rewrote `empack.yml` instead of resolving intent in memory.
- Unresolved searches and partial planning failures could return success. Sync now retains the typed resolution error and reports incomplete work, including after applying valid actions.
- Executable startup bypassed dotenv loading. CLI arguments now override existing environment values, which override `.env.local`, then `.env`.
- Relative workdirs reached packwiz twice relative to the subprocess directory. Startup now makes workdirs absolute before session construction, including nonexistent init targets.
- Installed-mod discovery omitted default and configured datapack folders.
- Failed installed-state reads were treated as an empty pack. Sync now aborts the reconciliation instead.

The harness now prefers instrumented binaries during coverage, uses successful
exit status for Java detection, and shares the CLI's built-in CurseForge key
default. `EMPACK_E2E_STRICT=1` makes missing prerequisites fail, including the
optional live-test opt-in and telemetry trace output. Exit-code tests use the
selected executable instead of rebuilding it concurrently through `cargo run`.
A negative strict run with a nonexistent packwiz override failed as intended.

Two NeoForge tests now use cached installer fixtures and assert the actual Java
invocation and missing-run-script error. They previously downloaded from live
services in the default suite and accepted unrelated failures. A cache test's
10 ms expiry race was replaced with an explicitly expired entry and assertions
on renewed expiry, ETag, body, and conditional request. Six broken public API
examples were repaired; doctests are now part of `mise run test`.

### Review follow-up

The first PR review exposed three additional cases, each reproduced by an offline
CLI smoke assertion before its fix: a parent-relative datapack path accepted by
init was rejected by sync; an installed-state scan failure could follow a search
resolution write; and malformed dotenv files prevented Clap help/version output.
Sync now honors configured datapack paths and scans installed state before
persisting resolutions. Help/version flags bypass dotenv errors. All nine smoke
tests pass, including YAML/TOML datapack paths, invalid option types, and
normal/dry-run scan failures. An injected manifest-write failure also verifies
that sync preserves installed dependencies and fails, while dry-run succeeds
without attempting the write.

The first Windows CI run failed before executing tests because cmd.exe passed
single-quoted nextest filters literally. Windows task commands now use double
quotes and separate command steps so a failed test/build cannot be masked by a
later successful command. The ignored project-level shell setting was removed.
The corrected Windows default and E2E test commands passed in PR CI.

### Runtime evidence map

Paths below are relative to `crates/empack-tests/tests/` unless stated otherwise.
These checks establish the listed contracts, not every possible option combination.

| Runtime path | Evidence exercised |
| --- | --- |
| Startup, CLI/env/dotenv precedence, relative workdir, exit codes | `smoke.rs`, `e2e_exit_codes.rs`, `e2e_build_relative_workdir_exports_valid_mrpack`; application parser/config tests |
| Init, force, loader/version choices, local and remote imports | `init_workflows.rs`, `init_matrix.rs`, `e2e_init.rs`, `e2e_live_import.rs`, `e2e_import_build.rs`; curated imports across Forge, Fabric, Quilt, NeoForge |
| Add, provider preference, project type, version pinning, local dependencies | `add_command.rs`, `add_matrix.rs`, `e2e_add.rs`, including opted-in live Sodium resolution |
| Sync, dry-run, partial failures, installed-state preservation | `sync_workflow.rs`, `smoke.rs`, `dry_run_matrix.rs`; application command tests |
| Remove, dependency state, cleanup | `remove_command.rs`, `clean_command.rs`, `e2e_add.rs`, `e2e_exit_codes.rs`; source preservation and failure assertions |
| Five build targets, all-target dispatch, ZIP/tar.gz/7z, templates and loaders | `build_matrix.rs`, `e2e_build.rs`, `lifecycle_forge_full.rs`; relative-workdir test extracts and validates the generated mrpack manifest |
| Restricted builds, persisted sessions, cache/fingerprint checks, continue/browser/watcher flows | `build_continue.rs`, `e2e_restricted_build.rs`, `e2e_import_build.rs`; curated CurseForge Fabric import required two restricted files and completed continuation |
| Live and injected sessions, filesystem/process/archive/network/display providers | inline tests in `crates/empack-lib/src/application/session.rs` and provider tests; live subprocess tests exercise their composition |
| HTTP cache, retries, timeouts and shared rate budgets | `crates/empack-lib/src/networking/*.test.rs`, API fixture tests, live provider requests |
| Interrupt, interactive prompts, color/log options, telemetry | `e2e_exit_codes.rs`, `e2e_interactive.rs`, `e2e_version.rs`, display tests; a separate CLI run parsed JSON stdout logging and a flushed Chrome trace |
| Managed packwiz and Java dependencies | Platform tool-resolution tests and live imports/builds; strict prerequisite failures cannot silently pass |

### Verification snapshot

The implementation and tests in this change passed these local checks. Counts are
historical, not permanent guarantees; live downloads depend on provider state.

| Command/check | Result |
| --- | --- |
| `mise run test` | 1276 non-E2E tests and 8 doctests passed |
| `cargo test --workspace --doc --all-features` | 8 passed |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | Passed |
| `cargo check --workspace --all-targets --features telemetry` | Passed |
| `cargo llvm-cov --no-report run -p empack --features telemetry -- version`, then `EMPACK_E2E_STRICT=1 EMPACK_RUN_LIVE_TESTS=1 cargo llvm-cov --no-clean nextest --workspace --features test-utils,telemetry --lcov --output-path lcov.info` | 1370 passed, one ignored manual prompt-rendering test; includes all seven offline smoke tests |
| LCOV production-source line coverage, excluding `.test.rs` under `empack-lib/src` and `empack/src` | 14765 / 16613 lines, 88.88%; inline tests in those files remain included |
| `scripts/import-smoke-test.py --profile curated --empack-bin target/debug/empack`, with its output root redirected to a fresh temporary directory | All seven imports and client-full builds passed, including restricted continuation |
| Python `zipfile.ZipFile.testzip()` on all seven artifacts | All CRC checks passed; JAR counts: 58, 48, 90, 14, 35, 2, 55 in curated order |
| CLI `version` with JSON logging and `EMPACK_PROFILE=chrome` | Successful exit; one JSON log record and two trace events parsed |
| `git diff --check` | Passed |

This does not verify launching Minecraft or every loader's generated server.
Linux and Windows execution must be established by CI. One manual PTY rendering
test remains ignored. `--cpu-jobs` and the Modrinth credential fields are parsed
but have no runtime consumers; help and usage now label them reserved. Packwiz
directory imports remain unsupported, local dependencies cannot be exported to
mrpack, and real YAML rewrites do not preserve comments. `requirements` reports
capabilities but is not a strict prerequisite exit-status gate.

The review also used the sibling playground project's `docs/ui/TESTING.md`,
`docs/ui/ASYNC.md`, `docs/platform/RUNTIME.md`, and `docs/render/CONTRACTS.md`.
Applicable rules are language-independent: distinguish intent from committed
state, make failure observable, preserve pending work where promised, and assert
final state as well as command completion. Playground's networking document is
a design direction, not an implemented transport API to reuse.

## Dependency migration review, 2026-10-02

The dependency integration retains the ten Renovate changes together so the
telemetry crates never land with incompatible trait versions. The original
telemetry PRs failed when upgraded independently. The sevenz and XML PRs had
failed at Windows task quoting, before their tests ran; the runtime review fixes
that task path. An older Codecov failure log had expired, so the updated action
must be validated by a fresh coverage job.

| Update | Compatibility review and verification |
| --- | --- |
| Rust 1.94 to 1.99, PR #69 | [Release notes](https://blog.rust-lang.org/2026/10/01/Rust-1.99.0/); keep edition 2024 and update the pinned toolchain. Clippy required boxing the compatibility diagnostic payload and using `sort_by_key` for descending confidence. Error text and exit classifications remain covered by existing tests. |
| thiserror 2.0.21, compiler migration follow-up | [Generated-code fix](https://github.com/dtolnay/thiserror/releases/tag/2.0.20); 2.0.18 generated fields trigger Rust 1.99 Clippy warnings. Upgrade the generator instead of suppressing application warnings. |
| OpenTelemetry 0.33 and tracing-opentelemetry 0.34, PRs #74/#68 | [Tracing migration](https://docs.rs/crate/tracing-opentelemetry/0.34.0/source/CHANGELOG.md) and [OTLP changes](https://github.com/open-telemetry/opentelemetry-rust/blob/opentelemetry-0.33.0/opentelemetry-otlp/CHANGELOG.md); empack already uses the supported exporter builder. New default retries warrant real local-collector export and failure/shutdown checks. |
| quick-xml 0.42, PR #67 | [Changelog](https://github.com/tafia/quick-xml/blob/v0.42.0/Changelog.md); low-level byte/string API changes do not affect empack's `de::from_str` consumers. Existing Forge/Quilt metadata fixtures and a malformed XML rejection check cover that boundary. |
| sevenz-rust2 0.23, PR #64 | [Changelog](https://github.com/hasenbanck/sevenz-rust2/blob/v0.23.0/CHANGELOG.md); MSRV 1.93 is satisfied. Compression still uses `compress_to_path`; verification now extracts nested files and compares exact binary contents and root-relative paths. |
| serde-saphyr 0.0.29, PR #63 | [Release notes](https://github.com/bourumir-wyngs/serde-saphyr/releases/tag/0.0.29); default features remain serialize/deserialize. Property interpolation and filesystem includes are not enabled. A manifest regression verifies literal `${...}` names remain literal. |
| expectrl 0.9, PR #66 | No upstream release notes were published. The published crate API retains the `Session`, `Expect`, and `Regex` surface used here; compile and active PTY workflows provide the migration evidence. |
| checkout v7 and setup-go v7, PRs #75/#76 | [Checkout changes](https://github.com/actions/checkout) and [setup-go releases](https://github.com/actions/setup-go/releases); current workflows use push/pull_request triggers, not privileged fork-checkout triggers. Hosted runner execution validates the Node runtime requirements. |
| Codecov v7, PR #70 | [Action notes](https://github.com/codecov/codecov-action); uploader verification-key handling changed. The fresh coverage upload/check validates integration rather than relying on the expired historical log. |

The library constructor `SearchError::IncompatibleProject { ... }` now takes
`SearchError::IncompatibleProject(Box::new(IncompatibleProject { ... }))`; all
diagnostic fields remain available on the public payload. CLI behavior is unchanged.

A release-preparation probe also reproduced `remove` reporting success after
packwiz removed a mod but a read-only `empack.yml` rejected the manifest update.
The handler now records a failure and explains how to inspect or restore a stale or damaged manifest, matching tracked
local removal. A regression checks both ordinary and `--deps` removal. This
reports partial mutation accurately; it does not roll back packwiz state.

PR CI now runs strict E2E and strict coverage with optional live tests enabled.
Missing prerequisites fail instead of returning a passing test function. Chrome
trace tests parse the flushed JSON and require the executed sync span. OTLP
smoke tests require an HTTP/protobuf request containing that span and a successful
process exit with both accepting and failing collectors, under a subprocess
deadline. These checks do not establish delivery during arbitrary collector
outages or server launch correctness.

## Test Layers

### Category A: Unit and deterministic integration

This is the first line of proof.

- Pure functions, parser behavior, state transitions, config formatting, dependency graph logic, and mock-backed command flows live here.
- Networking contract tests use recorded fixtures rather than live services when possible.
- New branch-heavy behavior should land here first, especially if it can be driven without a subprocess or live API.
- Adaptive rate-budget coverage belongs here first: header parsing, pacing, shared-budget behavior, and request-path integration should be proven with deterministic tests before relying on E2E.

### Category B: Live E2E

The E2E suite runs the compiled `empack` binary against real tools and, where required, live providers.

- Location: `crates/empack-tests/tests/e2e_*.rs`
- Supporting matrix/workflow coverage also lives in `crates/empack-tests/tests/`
- Harness utilities live in `crates/empack-tests/src/e2e.rs`
- Interactive PTY paths use `expectrl` where terminal behavior itself is the contract
- Non-interactive paths use `assert_cmd`
- Exit-code coverage includes subprocess checks for usage/config failures, general packwiz/process failures, and network/provider failures
- `packwiz-tx` is auto-managed, but live E2E can still be pointed at an override binary with `EMPACK_PACKWIZ_BIN`

E2E is confirmation, not the only proof. If behavior depends on rare server headers, throttling, timing, or concurrency, add a deterministic in-process test instead of waiting for a live environment to reproduce it.

### Category C: Interactive and PTY-backed flows

Use PTY-backed tests or smoke scripts when the UX itself matters:

- interactive init flows
- subprocess output that only appears correctly under a terminal
- long-running smoke runs where live error visibility matters

Current CI-enforced PTY scope is intentionally narrow:

- one active interactive `init` PTY test validates resulting config data rather than exact prompt strings
- one prompt-sequence PTY test remains `#[ignore]` as a manual-only dialoguer rendering check
- one active restricted-build PTY test validates the browser-confirm decline path by checking persisted pending state instead of prompt text
- one Unix-only PTY test validates that accepting the browser confirmation launches the platform opener through a fake browser command
- one Unix-only PTY test validates that accepting the browser confirmation, waiting for a watched manual download, and auto-continuing the build succeeds without an extra rerun
- injected interactive and process-provider tests still cover browser-opener invocation semantics on every platform without brittle prompt matching

`scripts/import-smoke-test.py` defaults to a curated 7-pack golden import and `client-full` build flow. On POSIX it uses a PTY path so failures surface while the run is still in progress, while still capturing structured results for the final report.

## E2E Prerequisites

Live E2E coverage requires:

- `packwiz-tx` or the managed download path
- Java 21+
- `mise`
- network access
- a valid CurseForge credential for CurseForge-backed cases; the CLI and harness use the built-in default unless `EMPACK_KEY_CURSEFORGE` overrides it

Ordinary live runs may self-skip when prerequisites are missing. Use
`mise run e2e:strict` to require them and enable optional live tests and telemetry.
For the same guarantee during coverage, run
`EMPACK_E2E_STRICT=1 EMPACK_RUN_LIVE_TESTS=1 mise run coverage`.

## VCR Fixtures

Recorded HTTP fixtures live under `crates/empack-tests/fixtures/cassettes/`.

Use them for contract verification and response-shape coverage:

```bash
./scripts/record-vcr-cassettes.sh --help
./scripts/record-vcr-cassettes.sh --dry-run
./scripts/record-vcr-cassettes.sh --only modrinth/version_file_sha1
./scripts/record-vcr-cassettes.sh
```

These fixtures should carry API-shape assertions that do not need live network timing or throttling behavior.

## Current State

The structural statements above are the maintained contract. Exact counts below are historical snapshots and will drift as the suite changes; use current CI or a fresh local run for authoritative totals.

Latest documented snapshots:

- 2026-04-10: `mise run test` completed with 1185 passed and 80 skipped across 24 binaries.
- 2026-04-10: `mise run e2e` ran 79 active E2E tests across 21 binaries, with 46 skipped and one slow path (`e2e_build_server_sevenz`).
- 2026-04-09: `mise run coverage` ran 1225 tests with 1 skipped across 24 binaries, with two slow paths (`e2e_build_server_sevenz`, `e2e_init_yes_neoforge_legacy_1_20_1`).
- 2026-04-09: the latest documented coverage snapshot was 88.02% on non-`.test.rs` files under `crates/empack-lib/src` and `crates/empack/src`, and 94.14% on `TOTAL`.
- `mise run coverage` is the combined instrumented path for unit and E2E coverage.
- there is no `mise run e2e:container` task in the current repo.

## Workflow reconciliation follow-up

The second audit starts at `e69baf4`. Four focused regressions failed before the
first fixes: dropped add pins, successful exit after failed manifest publication,
stale continuation deletion, and a missing import leaf behind a symlink. The
boundary suite then passed 151 tests. Stale previews have a separate regression.

The identity refactor adds real CLI sequences for both providers: pinned add,
required-library installation, two no-op syncs, a pin change, and convergence
after one reinstall. Further cases cover manifest aliases, duplicate identities,
unlisted content retention, and backend success without matching installed
metadata. The focused planner/command suite passed 46 tests and CLI smoke passed
15 tests at this stage. These checks do not establish a complete dependency
closure or authorize automatic orphan removal.

A stale-export regression failed before build freshness was corrected. A live
filesystem and ZIP round-trip fixture now imports distinct common/client/server
bytes for one path, builds all four distribution targets, and re-exports mrpack.
It repeats after changing common content without a version bump, checking output
bytes and side-exclusive files. External tool responses are deterministic fixtures;
this test does not launch Minecraft. Another live-filesystem test makes a manifest
read-only after installation and requires import to return a structured partial
failure. A sparse oversized input verifies the compressed archive limit before
ZIP parsing or whole-file allocation.
