# Testing and verification matrix

## Navigation

- Project overview and current command surface: [`../../README.md`](../../README.md)
- Current usage guidance: [`../usage.md`](../usage.md)
- Contributor workflow: [`../../CONTRIBUTING.md`](../../CONTRIBUTING.md)
- VCR-backed maintenance guidance: [`vcr-recording.md`](vcr-recording.md)

## Scope

This document is the current high-level verification guide for the Rust implementation. It separates trusted hermetic paths, isolated reruns, VCR-backed maintenance flows, deferred gaps, and the current grouped-workflow caveat.

## Trusted hermetic paths

These are the current baseline commands and promoted suites referenced by the spec:

```bash
cargo build --workspace --locked
cargo check --workspace --all-targets --locked
cargo nextest run -p empack-lib --features test-utils --lib
cargo nextest run -p empack-tests --test requirements_command
cargo nextest run -p empack-tests --test init_workflows
cargo nextest run -p empack-tests --test lifecycle_forge_full
cargo nextest run -p empack-tests --test build_command
cargo nextest run -p empack-tests --test build_server
cargo nextest run -p empack-tests --test build_server_full
cargo nextest run -p empack-tests --test build_client_full
```

What these cover today:

- workspace build and type-check health
- `empack-lib` contract and regression coverage
- requirements command behavior
- hermetic init flows
- lifecycle init, add, build, and clean behavior
- promoted build workflow suites and artifact contracts

## Isolated reruns

Use isolated sync reruns for touched sync behavior. These are trusted targeted checks, not a replacement for the default hermetic matrix:

```bash
cargo nextest run -p empack-tests --test sync_workflow test_sync_workflow_full
cargo nextest run -p empack-tests --test sync_workflow test_sync_dry_run_no_modifications
```

Use targeted command coverage when touching remove-specific behavior:

```bash
cargo nextest run -p empack-lib --features test-utils --lib handle_remove_tests
```

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

## Deferred gaps

These paths should stay explicit as deferred, not trusted by default:

- standalone `e2e_build_client_full_missing_installer`
- standalone `e2e_build_server_full_missing_installer`
- broader remove behavior beyond the targeted command tests listed above

## Known grouped-workflow caveat

Grouped reruns of `sync_workflow` can still fail with the message:

`Global configuration already initialized`

Treat that as known harness isolation debt. Do not promote grouped `sync_workflow` reruns to trusted release evidence until the issue is resolved.

## Reference-only surfaces

- `v1/` and `v2/` are useful for lineage and historical behavior only
- `docs/reference/` documents provider APIs, not the full trusted product matrix