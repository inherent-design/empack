---
spec: testing-architecture
status: draft
created: 2026-04-08
updated: 2026-04-08
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
- PTY and interactive flows

## Harness Behavior

Current E2E harness behavior includes:

- `empack_bin()` resolution from `EMPACK_E2E_BIN`, llvm-cov output, debug build, release build, then PATH
- `NO_COLOR=1` in subprocess helpers
- prerequisite skip macros for missing packwiz-tx or Java
- support for fake `packwiz-tx` via `EMPACK_PACKWIZ_BIN`

## PTY Coverage

Interactive tests use `expectrl` and PTY-backed execution when terminal behavior matters.

Current PTY-relevant paths include:

- interactive init flows
- interactive search and selection
- browser-open confirmation during restricted download handling
- smoke-style import visibility through PTY-backed scripts

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

As of 2026-04-08:

- `mise run test`: 1007 tests
- `mise run e2e`: 72 non-ignored E2E tests
- primary non-`.test.rs` coverage metric on `feat/test-coverage`: 86.86%
- `TOTAL` coverage on `feat/test-coverage`: 93.34%

## Scope Boundaries

Current non-goals and deferred items:

- containerized E2E is not an active task path today
- E2E is not the primary proof layer
- coverage planning is active, but draft future-state work is not part of this spec unless it is already in the codebase
