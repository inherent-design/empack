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
- Interactive paths use `expectrl`
- Non-interactive paths use `assert_cmd`
- `packwiz-tx` is auto-managed, but live E2E can still be pointed at an override binary with `EMPACK_PACKWIZ_BIN`

E2E is confirmation, not the only proof. If behavior depends on rare server headers, throttling, timing, or concurrency, add a deterministic in-process test instead of waiting for a live environment to reproduce it.

### Category C: Interactive and PTY-backed flows

Use PTY-backed tests or smoke scripts when the UX itself matters:

- interactive init/search flows
- subprocess output that only appears correctly under a terminal
- long-running smoke runs where live error visibility matters

`scripts/import-smoke-test.py` now uses a POSIX PTY path when available so import failures can surface warning/error lines while the run is still in progress, while still capturing full output for the final report.

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

As of 2026-04-08:

- `mise run test` runs 1007 tests.
- `mise run e2e` runs 72 non-ignored E2E tests.
- coverage on `feat/test-coverage` is 86.86% on the primary non-`.test.rs` metric.
- `TOTAL` coverage on `feat/test-coverage` is 93.34%.
- `mise run coverage` is the combined instrumented path for unit and E2E coverage.
- there is no `mise run e2e:container` task in the current repo.
