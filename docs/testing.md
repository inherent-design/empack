# Testing

empack uses deterministic in-process tests as the primary proof layer, then live E2E to confirm real CLI, filesystem, subprocess, and provider behavior.

## Test Commands

Use the `mise` tasks in [`../mise.toml`](../mise.toml):

```bash
mise run test              # unit + mock/integration, excludes E2E
mise run e2e               # live E2E suite only
mise run e2e:filter add    # filtered E2E slice
mise run coverage          # instrumented binary + workspace coverage
mise run check             # cargo check --workspace --all-targets
mise run clippy            # cargo clippy --workspace --all-targets -- -D warnings
```

`mise run test` is the fast default gate. `mise run e2e` is a separate live suite because it depends on external tools and network conditions.

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
- injected interactive and process-provider tests still cover browser-opener invocation semantics on every platform without brittle prompt matching

`scripts/import-smoke-test.py` defaults to a curated 7-pack golden import and `client-full` build flow. On POSIX it uses a PTY path so failures surface while the run is still in progress, while still capturing structured results for the final report.

## E2E Prerequisites

Live E2E coverage requires:

- `packwiz-tx` or the managed download path
- Java 21+
- `mise`
- network access
- `.env.local` with `EMPACK_KEY_CURSEFORGE` for CurseForge-backed cases

Some live tests self-skip when prerequisites are missing. That is expected.

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

As of 2026-04-09:

- `mise run test` completes with 1149 passed and 77 skipped across 23 binaries.
- `mise run e2e` runs 76 active E2E tests across 20 binaries, with 46 skipped and one slow path (`e2e_build_server_sevenz`).
- `mise run coverage` runs 1225 tests with 1 skipped across 24 binaries, with two slow paths (`e2e_build_server_sevenz`, `e2e_init_yes_neoforge_legacy_1_20_1`).
- primary coverage on non-`.test.rs` files under `crates/empack-lib/src` and `crates/empack/src` is 88.02%.
- `TOTAL` coverage is 94.14%.
- `mise run coverage` is the combined instrumented path for unit and E2E coverage.
- there is no `mise run e2e:container` task in the current repo.
