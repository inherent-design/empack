# Testing

This document covers the test strategy, verification matrix, test health inventory, and VCR fixture maintenance for the empack workspace.

## Release gate

The trusted must-pass commands for CI:

```bash
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets
cargo nextest run -p empack-lib --features test-utils
cargo nextest run -p empack-tests
```

Current checkpoint: 630 tests (561 in `empack-lib`, 15 skipped; 69 in `empack-tests`).

CI uses `cargo nextest` exclusively. Grouped `cargo test` is advisory-only due to global state conflicts between workflow tests.

## Test architecture

Tests are organized across two crates with distinct patterns:

**empack-lib** (576 tests, 15 skipped): co-located `.test.rs` files included via `include!()`. Unit tests use `MockCommandSession` with full mock providers. Some modules use `mockito` for HTTP mock servers. Feature-gated behind `test-utils` for mock access.

**empack-tests** (69 tests): workflow and integration tests (55) plus lib/infrastructure tests (14). Three session construction patterns:

- **MockSessionBuilder** (40 tests, cross-platform): in-memory filesystem and mock process provider via builder API. Covers build targets (mrpack, server, server-full, client), init workflows, remove, sync, dry-run, and lifecycle tests. Uses `MockArchiveProvider` for archive verification without real zip/unzip.
- **MockProcessProvider** (12 tests, cross-platform): pre-registered argument-to-result mappings via `CommandSession::new_with_providers`. Covers add (with type variants and dry-run), clean, requirements, and version commands.
- **HermeticSessionBuilder** (3 tests, 1 `#[cfg(unix)]`): creates shell script mocks on disk via PATH manipulation. Exercises real process execution and filesystem interaction. Used only for init error recovery scenarios.

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

---

## Test health inventory

Audited 2026-03-24 across 515 test functions (358 unit + 39 integration + 118 infrastructure). Subsequent work added unit tests in empack-lib and migrated integration tests from HermeticSessionBuilder to MockSessionBuilder. The tables below reflect the 2026-03-24 audit; tests added after that date are not individually categorized.

### Assertion quality

| Category | Tests | Strong | Weak | Vacuous |
|----------|-------|--------|------|---------|
| Unit (commands, config, state, search, builds) | 358 | 290 (81%) | 42 (12%) | 26 (7%) |
| Integration (hermetic + mock sessions) | 39 | 25 (64%) | 7 (18%) | 7 (18%) |
| Infrastructure (networking, display, terminal, API) | 118 | 82 (69%) | 4 (3%) | 32 (27%) |
| **Total** | **515** | **397 (77%)** | **53 (10%)** | **65 (13%)** |

### Vacuous tests (zero or tautological assertions)

These tests cannot fail regardless of implementation correctness.

**Unit tests (tautological: construct-then-assert-fields-match-construction):**

| Test | File | Line | Pattern |
|------|------|------|---------|
| `test_sync_action_creation` | commands.test.rs | 2297 | Constructs SyncExecutionAction::Add, asserts fields equal construction values |
| `test_sync_action_remove` | commands.test.rs | 2325 | Same pattern for SyncExecutionAction::Remove |
| `test_build_result_structure` | builds.test.rs | 417 | Constructs BuildResult, asserts fields equal construction values |
| `test_build_artifact_structure` | builds.test.rs | 613 | Same pattern for BuildArtifact |
| `test_pack_info_structure` | builds.test.rs | 626 | Same pattern for PackInfo |

**Integration tests (both Ok and Err branches pass):**

| Test | File | Issue |
|------|------|-------|
| `test_init_packwiz_unavailable` | init_error_recovery.rs | Both Ok and Err just print and return Ok(()) |
| `test_build_template_error_specificity` | build_with_missing_template.rs | Err assertion `!err_msg.is_empty()` is always true |
| `e2e_requirements_packwiz_missing` | requirements_command.rs | Both Ok and Err just println and return Ok(()) |

**Infrastructure tests (zero assertions, only "does not panic"):**

| File | Count | Issue |
|------|-------|-------|
| display/status.test.rs | 9 | All 9 tests: call methods, assert nothing |
| display/structured.test.rs | 10 | All 10 tests: call methods, assert nothing |
| display/interactive.test.rs | 5 of 10 | Construction-only tests with no assertion |
| display/progress.test.rs | 2 of 10 | Clear and unicode/ascii tests with no assertion |
| networking/mod.test.rs | 1 of 4 | `test_mock_mod_resolution`: compile-time type check only |

### Misplaced test

`handle_remove_tests::it_rejects_incomplete_project_state` (commands.test.rs:1310) calls `handle_sync`, not `handle_remove`. Located in the remove test module but tests sync behavior.

### Duplicate coverage

| Test A | Test B | Both verify |
|--------|--------|-------------|
| `test_invalid_build_target_single` (2448) | `test_parse_build_targets_invalid_target` (2273) | `parse_build_targets(vec!["invalid"])` |
| `test_empty_build_target_list` (2476) | `test_parse_build_targets_empty_list` (2284) | `parse_build_targets(vec![])` |
| `test_build_with_uninitialized_project` (2522) | `handle_build_tests::it_handles_uninitialized_project` (1996) | Build on uninitialized project |

### Weak tests (is_ok/is_err only, no value or side-effect verification)

| Test | File | Why weak |
|------|------|----------|
| `it_displays_version_information` | commands.test.rs:25 | Asserts is_ok(); display output not capturable in unit tests |
| `it_accepts_valid_loader_version_from_cli` | commands.test.rs:393 | is_ok() only; does not verify which version was selected |
| `it_accepts_compatible_loader_fallback` | commands.test.rs:454 | is_ok() only; does not verify fallback selection |
| `test_add_mod_curseforge_success` | packwiz.test.rs:55 | is_ok() without verify_call on mock |
| `test_refresh_index_success` | packwiz.test.rs:194 | is_ok() without verify_call on mock |
| `test_export_mrpack_success` | packwiz.test.rs:287 | is_ok() without verify_call on mock |
| `test_remove_mod_not_found` | packwiz.test.rs:164 | is_err() without checking error variant or message |

---

## Coverage map

### By command (integration tests)

| Command | Happy Path | Error Path | Edge Cases | --dry-run | --force |
|---------|-----------|------------|------------|-----------|---------|
| init | 8 tests (NeoForge, Quilt, Vanilla, Forge, older MC, zero-config, explicit, directory) | 3 tests (packwiz failure, filesystem error, empty loaders) | Existing project (1) | Not tested | Not tested |
| add | 1 test (moderate) | Not tested | Type variants (2: resourcepack, shader) | 1 test | Not tested |
| remove | 2 tests | Not tested | Empty list (1) | 1 test | N/A |
| sync | 1 test | Not tested | Noop (1) | 1 test | N/A |
| build mrpack | 5 tests (2 build_command + 3 build_matrix: neoforge, quilt, vanilla) | 2 tests (refresh fail, export fail) | clean flag (1) | Not tested | N/A |
| build client | 1 test (fabric) | Not tested | Not tested | Not tested | N/A |
| build client-full | 2 tests | 1 test | Pack structure (1) | Not tested | N/A |
| build server | 2 tests | 1 test | Templates (1) | Not tested | N/A |
| build server-full | 2 tests | 1 test | Templates (1) | Not tested | N/A |
| build (cross-loader) | 9 tests via build_matrix (neoforge, quilt, vanilla x mrpack/server/server-full; fabric client) | Not tested | Not tested | Not tested | N/A |
| clean | 2 tests | Not tested | No artifacts (1), specific targets (1) | 1 test | N/A |
| requirements | 2 tests (result-only) | 1 test (vacuous) | Not tested | N/A | N/A |
| version | 1 test (moderate) | Not tested | Not tested | N/A | N/A |

### By feature (integration tests)

| Feature | Integration Tested? |
|---------|-------------------|
| --dry-run for sync | Yes (1 test) |
| --dry-run for add | Yes (1 test, add_matrix) |
| --dry-run for remove | Yes (1 test, dry_run_matrix) |
| --dry-run for clean | Yes (1 test, dry_run_matrix) |
| --dry-run for build | No (unit tests exist but no integration) |
| --force for init | No |
| --force for add | No |
| --deps flag for remove | No |
| Build target "client" standalone | Yes (1 test, build_matrix: fabric client) |
| NeoForge loader path | Yes (1 init_matrix + 3 build_matrix: mrpack, server, server-full) |
| Quilt loader path | Yes (1 init_matrix + 3 build_matrix: mrpack, server, server-full) |
| Vanilla loader path | Yes (1 init_matrix + 2 build_matrix: mrpack, server) |
| Forge lifecycle | Yes (2 tests, lifecycle_forge_full) |
| CurseForge platform preference | No |
| CF restricted pre-flight (build) | Yes (unit tests: 12 in cf_restricted_downloads_tests) |
| Multiple build targets in single command | No (except "all" via lifecycle) |

### By module (unit tests, invariance coverage)

| Module | Happy Path | Error Path | Boundary | Concurrency |
|--------|-----------|------------|----------|-------------|
| handle_init | Yes | Yes (5 paths) | Yes (force, cancel, NeoForge, Quilt, Vanilla, older MC) | No |
| handle_add | Yes | Yes (5 paths: packwiz fail, no results, stderr, CF restricted, CF restricted + Modrinth) | Yes (empty, slug, dry-run, type variants) | No |
| handle_remove | Yes | Yes (3 paths) | Yes (empty, dry-run) | No |
| handle_sync | Yes | Yes (3 paths) | Yes (noop, dry-run) | No |
| handle_build | Yes | Yes (3 paths) | Yes (dry-run, clean, CF restricted pre-flight, cached bypass, browser open) | No |
| handle_clean | Yes | No | Yes (empty, absent, dry-run) | No |
| config (serde) | Yes | Yes | Yes (62 tests) | No |
| state machine | Yes | Yes | Yes (46 tests) | No |
| fuzzy matching | Yes | No | Yes (unicode) | No |
| search/resolve | Yes | Yes | Yes (27 tests) | No |
| networking/cache | Yes | Yes | Yes (TTL, ETag, 404) | Yes (concurrent writes) |
| networking/rate_limit | Yes | Yes | Yes (429, exhaustion) | Yes (concurrent) |
| dependency_graph | Yes | Yes | Yes (cycles, diamond) | No |
| display/status | No (vacuous) | No | No | No |
| display/structured | No (vacuous) | No | No | No |

### Platform coverage

54 of 55 integration tests are cross-platform. Only 1 test remains `#[cfg(unix)]`: `test_init_filesystem_error` (tests Unix file permissions). The migration from HermeticSessionBuilder to MockSessionBuilder eliminated the previous unix-only constraint. Two lib/infrastructure tests in test_env (`test_mock_executable_logging`, `test_mock_executable_logging_preserves_argument_boundaries`) are also `#[cfg(unix)]` since they execute shell script mocks.

### Unused test infrastructure

- `MockBehavior::Conditional { rules }` and `ConditionalRule`: defined in test_env.rs but never used in any test
- `MockSessionBuilder::with_interactive_provider()`: defined in test_env.rs but never called from any test file

### New test infrastructure (since migration)

- **MockSessionBuilder**: builder for `MockCommandSession` with in-memory filesystem, mock process provider, and `MockArchiveProvider`. Replaced most HermeticSessionBuilder usage for cross-platform compatibility.
- **MockArchiveProvider**: records `create_archive` calls via `Arc<Mutex<Vec>>` for assertion without invoking real zip/unzip. Used by all build_matrix, build_server, build_server_full, and build_client_full tests.

---

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

The script manages fixtures across four directories: `modrinth/`, `curseforge/`, `loaders/`, `minecraft/`.

```bash
find crates/empack-tests/fixtures/cassettes -maxdepth 2 -type f | sort
```

### Boundaries

Recording touches live network services and can fail due to rate limits or API drift. Keep VCR-backed work separate from the hermetic release gate.

---

## External tool dependency map

| Tool | Used by | Tested via | Test fidelity |
|------|---------|-----------|---------------|
| packwiz CLI | init, add, remove, sync, build (refresh, export) | MockProcessProvider (MockSessionBuilder tests); shell script mocks (HermeticSessionBuilder, 3 tests) | Verifies args passed; does not run real packwiz |
| java | build fabric, quilt, neoforge, forge (server installer); build client-full, server-full (packwiz-installer) | MockProcessProvider (MockSessionBuilder tests) | Verifies args; does not run real Java |
| zip/unzip | build client, server, client-full, server-full | MockArchiveProvider (MockSessionBuilder tests) | Records create_archive calls for assertion; does not invoke real zip/unzip |
| reqwest (HTTP) | build server JAR download (all loaders: vanilla, fabric, quilt, neoforge, forge); ServerStarterJar download (neoforge, forge) | mockito (unit tests); live HTTP (integration tests) | Unit tests verify JSON/XML parsing and URL construction; integration tests download from real APIs |

---

## Remaining gaps

1. Client build with CurseForge JAR overrides fails at runtime (not yet investigated in tests)
2. No integration test for --dry-run on build (add, remove, and clean now have integration tests)
3. No integration test for --force flag on init or add
4. No integration test for --deps flag on remove
5. Server JAR integration tests depend on live network access to Mojang, Fabric, Quilt, NeoForge, and Forge APIs
6. CurseForge platform preference not tested at integration level
7. Multiple build targets in single command not tested (except "all" via lifecycle)
