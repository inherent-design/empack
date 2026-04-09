---
spec: testing-architecture
status: draft
created: 2026-04-08
updated: 2026-04-09
depends: [overview, session-providers]
---

# Testing Architecture

empack uses a two-layer test strategy: deterministic in-process tests first, then subprocess E2E confirmation.

## Test Commands

Current `mise` tasks:

| Command | Behavior |
| --- | --- |
| `mise run test` | unit and integration tests, excludes `e2e_` tests |
| `mise run e2e` | live E2E tests only |
| `mise run e2e:filter <expr>` | filtered live E2E slice |
| `mise run coverage` | workspace coverage with instrumented binary and telemetry feature |

## Layer Split

### Deterministic tests

Primary locations:

- `crates/empack-lib/src/*.test.rs`
- `crates/empack-tests/tests/*.rs` for non-`e2e_` workflow coverage

Typical coverage:

- pure parsing and classification
- config and state logic
- sync planning
- build orchestration with mocks
- rate-budget behavior
- command flows through mock sessions

### Live E2E tests

Primary locations:

- `crates/empack-tests/tests/e2e_*.rs`
- shared harness in `crates/empack-tests/src/e2e.rs`

Typical coverage:

- real CLI parsing
- subprocess exit codes
- live filesystem behavior
- real packwiz-tx resolution
- targeted PTY and interactive flows

## Harness Behavior

Current E2E harness behavior includes:

- `empack_bin()` resolution from `EMPACK_E2E_BIN`, debug build, release build, coverage-only llvm-cov output, then PATH
- `NO_COLOR=1` in subprocess helpers
- isolated `EMPACK_CACHE_DIR` per E2E workdir to avoid host-cache leakage across subprocess tests
- prerequisite skip macros for missing packwiz-tx or Java
- support for fake `packwiz-tx` via `EMPACK_PACKWIZ_BIN`

## PTY Coverage

Interactive tests use `expectrl` and PTY-backed execution when terminal behavior matters, but PTY scope is intentionally narrow.

Current PTY-relevant paths include:

- interactive init flows
- one manual-only prompt-sequence PTY check for dialoguer rendering
- smoke-style import visibility through PTY-backed scripts
- CI PTY coverage for restricted-download browser confirmation reachability and persisted pending-state behavior
- injected interactive and process-provider coverage for browser-opener invocation semantics on every platform

## VCR Fixtures

Recorded fixtures live under:

```text
crates/empack-tests/fixtures/cassettes/
```

Current provider groups include:

- `curseforge`
- `loaders`
- `minecraft`
- `modrinth`

These fixtures are used for stable response-shape and contract testing. They are not a substitute for live E2E confirmation.

## Coverage Pipeline

`mise run coverage` currently runs:

1. `cargo llvm-cov clean --workspace`
2. `cargo llvm-cov --no-report run -p empack --features telemetry -- version`
3. `cargo llvm-cov --no-clean nextest --workspace --features test-utils,telemetry --lcov --output-path lcov.info`

The `--no-clean` step is required so E2E subprocess tests can find the instrumented binary.

## Current Counts

These counts are a dated snapshot, not a timeless contract.

As of 2026-04-09:

- `mise run test`: 1149 passed, 77 skipped, across 23 binaries
- `mise run e2e`: 76 active E2E tests across 20 binaries, 46 skipped, one slow path (`e2e_build_server_sevenz`)
- `mise run coverage`: 1225 tests, 1 skipped, across 24 binaries, with two slow paths (`e2e_build_server_sevenz`, `e2e_init_yes_neoforge_legacy_1_20_1`)
- primary non-`.test.rs` coverage metric on `empack-lib/src/**` and `empack/src/**`: 88.02%
- `TOTAL` coverage: 94.14%

## Scope Boundaries

Current non-goals and deferred items:

- containerized E2E is not an active task path today
- E2E is not the primary proof layer
- coverage planning is active, but draft future-state work is not part of this spec unless it is already in the codebase
