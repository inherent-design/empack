# Testing

This document covers the test strategy, verification matrix, and VCR fixture maintenance for the empack workspace.

## Release gate

The trusted must-pass commands for CI:

```bash
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets
cargo nextest run -p empack-lib --features test-utils
cargo nextest run -p empack-tests
```

Current checkpoint: 539 tests (489 in `empack-lib`, 15 skipped; 50 in `empack-tests`).

CI uses `cargo nextest` exclusively. Grouped `cargo test` is advisory-only due to global state conflicts between workflow tests.

## Isolated reruns

Use targeted nextest checks when touching specific workflow behavior:

```bash
cargo nextest run -p empack-tests --test sync_workflow test_sync_workflow_full
cargo nextest run -p empack-tests --test sync_workflow test_sync_dry_run_no_modifications
cargo nextest run -p empack-lib --features test-utils --lib handle_remove_tests
```

## Advisory grouped coverage

Grouped `cargo test` passes reliably for a narrower subset:

- `empack-lib` smoke (excluding `env::tests` and `capabilities::tests`)
- `config_integration`
- `empack-tests` lib
- `add_command`
- `lifecycle_forge_full`

## Known grouped instability

The following test files are unstable under grouped execution: `sync_workflow`, `build_command`, `build_server`, `build_server_full`, `build_client_full`, `clean_command`, `build_with_missing_template`, `init_error_recovery`, `init_workflows`, `requirements_command`, plus `env::tests` and `capabilities::tests` in `empack-lib`.

Common interference sources: `Display::init` global state and environment variable conflicts.

## VCR fixtures

Recorded HTTP fixtures under `crates/empack-tests/fixtures/cassettes/` provide API response data for deterministic testing. The cassette loader helpers live in `crates/empack-tests/src/fixtures.rs`.

VCR fixtures are useful for maintaining recorded API examples and enabling future higher-fidelity provider tests. They are not part of the release gate.

### Recording cassettes

Prerequisites: `curl`, `jq`, and `.env.local` with `EMPACK_KEY_CURSEFORGE`. Copy `.env.local.template` as a starting point.

```bash
./scripts/record-vcr-cassettes.sh --help
./scripts/record-vcr-cassettes.sh --dry-run
./scripts/record-vcr-cassettes.sh --only modrinth/search_sodium
./scripts/record-vcr-cassettes.sh
```

The script records responses, sanitizes API keys, and validates JSON output.

### Verifying cassettes

After updating fixtures, confirm validity:

```bash
jq empty crates/empack-tests/fixtures/cassettes/modrinth/search_sodium.json
cargo test -p empack-tests fixtures::tests::test_load_vcr_cassette -- --exact
cargo test -p empack-tests fixtures::tests::test_load_vcr_body_string -- --exact
```

### Cassette buckets

The script manages fixtures across four directories:

- `modrinth/`
- `curseforge/`
- `loaders/`
- `minecraft/`

Inspect the fixture tree:

```bash
find crates/empack-tests/fixtures/cassettes -maxdepth 2 -type f | sort
```

### Boundaries

Recording touches live network services and can fail due to rate limits or API drift. Keep VCR-backed work separate from the hermetic release gate.

## Remaining gaps

Broader remove behavior beyond the targeted command tests listed above is not yet promoted into the release gate.
