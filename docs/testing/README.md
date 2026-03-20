# Testing and verification matrix

## Navigation

- Project overview and current command surface: [`../../README.md`](../../README.md)
- Current usage guidance: [`../usage.md`](../usage.md)
- Contributor workflow: [`../../CONTRIBUTING.md`](../../CONTRIBUTING.md)
- VCR-backed maintenance guidance: [`vcr-recording.md`](vcr-recording.md)

## Scope

This document is the current high-level verification guide for the Rust implementation. It separates the primary nextest-based release gate, narrower grouped-test advisory coverage, targeted isolated reruns, VCR-backed maintenance flows, and the remaining real gaps.

## Primary trusted release gate

These are the accepted must-pass commands referenced by the spec:

```bash
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets
cargo nextest run -p empack-lib --features test-utils
cargo nextest run -p empack-tests
```

What these cover today:

- workspace type-check and lint health
- 351+ passing tests in the accepted nextest checkpoint
- `empack-lib` contract and regression coverage
- promoted workflow and command coverage across `empack-tests`

CI uses `cargo nextest` exclusively for test execution because grouped `cargo test` is not stable enough to serve as the primary blocker.

## Advisory grouped `cargo test` coverage

Broad grouped trust is narrower than the nextest gate and should be treated as advisory-only:

- grouped `empack-lib` smoke excluding `env::tests` and `capabilities::tests`
- `config_integration`
- `empack-tests` lib
- `add_command`
- `lifecycle_forge_full`

## Isolated reruns

Use isolated reruns for touched grouped-unstable workflow behavior. These are trusted targeted nextest checks, not evidence that grouped `cargo test` is stable:

```bash
cargo nextest run -p empack-tests --test sync_workflow test_sync_workflow_full
cargo nextest run -p empack-tests --test sync_workflow test_sync_dry_run_no_modifications
```

Use targeted command coverage when touching remove-specific behavior:

```bash
cargo nextest run -p empack-lib --features test-utils --lib handle_remove_tests
```

The missing-installer coverage that used to be deferred now exists in the promoted nextest workflow suites and is no longer an open gap.

## VCR-backed flows

VCR-backed maintenance is useful for recorded HTTP fixtures, but it is not the default release gate.

Current repo-backed references:

```bash
./scripts/record-vcr-cassettes.sh --help
./scripts/record-vcr-cassettes.sh --dry-run
cargo test -p empack-tests fixtures::tests::test_load_vcr_cassette -- --exact
cargo test -p empack-tests fixtures::tests::test_load_vcr_body_string -- --exact
```

Notes:

- recorded cassettes live under `crates/empack-tests/fixtures/cassettes/`
- cassette loader helpers live in `crates/empack-tests/src/fixtures.rs`
- live recording requires `curl`, `jq`, and a local `.env.local` with `EMPACK_KEY_CURSEFORGE`
- use [`vcr-recording.md`](vcr-recording.md) for the recording workflow and follow-up checks

## Remaining explicit gaps

These paths should stay explicit, not promoted into the primary trusted matrix:

- broader remove behavior beyond the targeted command tests listed above

## Known grouped `cargo test` instability

Grouped instability is broader than `sync_workflow` alone. The current unstable set includes workflow files such as `sync_workflow`, `build_command`, `build_server`, `build_server_full`, `build_client_full`, `clean_command`, `build_with_missing_template`, `init_error_recovery`, `init_workflows`, and `requirements_command`, plus `env::tests` and `capabilities::tests` in `empack-lib`.

Common interference sources include `Display::init` global state and environment variable conflicts.

Treat grouped `cargo test` as advisory-only release evidence until that instability is resolved. Prefer the nextest gate and targeted isolated reruns when verifying touched behavior.

## Reference-only surfaces

- `v1/` and `v2/` are useful for lineage and historical behavior only
- `docs/reference/` documents provider APIs, not the full trusted product matrix