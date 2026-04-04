# Testing

empack tests across two tiers: unit tests for pure functions and mock-based command logic, and E2E tests against live providers with the real binary.

## Philosophy

- **Real providers for E2E.** E2E tests run the empack binary as a subprocess with real filesystem, real packwiz, real network. No mocks. Interactive flows use `expectrl` (cross-platform PTY). Non-interactive flows use `assert_cmd`.
- **Mock-based unit tests.** Command handlers, config parsing, state machines, URL classification, override categorization, and sync logic are tested in isolation through provider mocks.
- **VCR cassettes for contract verification.** Recorded API responses verify that deserialization structs match real response shapes. Contract tests live in the unit tier.

## Unit Tests

728 tests across two crates, all run with `cargo nextest`:

```bash
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets
cargo nextest run -p empack-lib --features test-utils
cargo nextest run -p empack-tests
```

**empack-lib** (659 tests): co-located `.test.rs` files via `include!()`. Feature-gated behind `test-utils`. Includes:
- Command handler tests via `MockCommandSession`
- API contract tests deserializing VCR cassettes into production structs
- Config, state machine, search, build, sync, and parser unit tests

**empack-tests** (69 tests): workflow tests using `MockSessionBuilder` (in-memory filesystem, mock process provider) and `CommandSession::new_with_providers` (real filesystem + mock process).

Use isolated reruns when iterating on specific behavior:

```bash
cargo nextest run -p empack-lib --features test-utils --lib test_name
cargo nextest run -p empack-tests --test sync_workflow
```

## E2E Tests

E2E tests run the compiled empack binary as a child process against live providers. Requires packwiz and Java installed.

### Prerequisites

- [packwiz](https://packwiz.infra.link/)
- [Java 21+](https://adoptium.net/) (for server builds)
- [mise](https://mise.jdx.dev/) (task runner)
- `.env.local` with `EMPACK_KEY_CURSEFORGE` (for CurseForge API tests)

### Running Tests

```bash
mise run e2e              # full E2E suite
mise run e2e:filter init  # run only init-related E2E tests
```

E2E tests are advisory, not gating. Failures indicate API drift, environment issues, or real bugs.

### Test Patterns

Non-interactive commands via `assert_cmd`:

```rust
Command::cargo_bin("empack")?
    .args(["init", "--yes", "--modloader", "fabric", "--mc-version", "1.21.1", "test-pack"])
    .current_dir(&workdir)
    .assert()
    .success();

let yml = fs::read_to_string(workdir.join("test-pack/empack.yml"))?;
assert!(yml.contains("loader: fabric"));
```

Interactive prompts via `expectrl`:

```rust
let mut session = expectrl::spawn(format!("{} init my-pack", empack_bin.display()))?;
session.expect("Minecraft version")?;
session.send_line("1.21.1")?;
session.expect("Mod loader")?;
session.send_line("fabric")?;
session.expect("Initialized")?;
```

Live API verification:

```rust
Command::cargo_bin("empack")?
    .args(["add", "sodium"])
    .current_dir(&project_dir)
    .assert()
    .success();

let yml = fs::read_to_string(project_dir.join("empack.yml"))?;
assert!(yml.contains("sodium"));
```

### Containerized E2E

For CI, E2E tests run in a Colima container (aarch64 Linux) with Java, packwiz, and network access:

```bash
mise run e2e:container    # build container + run E2E
```

### E2E Cleanup

E2E tests create temporary directories. No external resources need cleanup (unlike cfgate's Cloudflare resources). Each test starts from a fresh temp directory.

### E2E Test Structure

```
test/e2e/
  helpers.rs            # binary path resolution, project creators, disk assertions
  init_test.rs          # init: interactive, --yes, --from, error recovery
  add_test.rs           # add: by name, URL, direct download, version pins
  sync_test.rs          # sync: reconciliation, dry-run
  build_test.rs         # build: all targets, templates, clean
  import_test.rs        # init --from: real .mrpack and .zip files
```

---

## VCR Fixtures

Recorded HTTP fixtures under `crates/empack-tests/fixtures/cassettes/` provide real API response data. Used by contract tests in the unit tier.

### Recording cassettes

Prerequisites: `curl`, `jq`, `.env.local` with `EMPACK_KEY_CURSEFORGE`.

```bash
./scripts/record-vcr-cassettes.sh --help
./scripts/record-vcr-cassettes.sh --dry-run
./scripts/record-vcr-cassettes.sh --only modrinth/version_file_sha1
./scripts/record-vcr-cassettes.sh
```

The script supports GET and POST endpoints, sanitizes API keys, and validates JSON output. 18 cassettes across four directories: `modrinth/`, `curseforge/`, `loaders/`, `minecraft/`.

### Verifying cassettes

```bash
jq empty crates/empack-tests/fixtures/cassettes/modrinth/search_sodium.json
cargo nextest run -p empack-tests --lib fixtures::tests
```

---

## Test Health Inventory

Audited 2026-03-24 across 515 test functions. Subsequent work added contract tests and import/content tests; these are not individually categorized below.

### Assertion quality

| Category | Tests | Strong | Weak | Vacuous |
|----------|-------|--------|------|---------|
| Unit (commands, config, state, search, builds) | 358 | 290 (81%) | 42 (12%) | 26 (7%) |
| Integration (mock sessions) | 39 | 25 (64%) | 7 (18%) | 7 (18%) |
| Infrastructure (networking, display, terminal) | 118 | 82 (69%) | 4 (3%) | 32 (27%) |
| **Total** | **515** | **397 (77%)** | **53 (10%)** | **65 (13%)** |

### Vacuous tests

65 tests that cannot fail regardless of implementation correctness. See the audit tables below for specifics.

**Unit (tautological construct-then-assert):** `test_sync_action_creation`, `test_sync_action_remove`, `test_build_result_structure`, `test_build_artifact_structure`, `test_pack_info_structure`.

**Integration (both branches pass):** `test_init_packwiz_unavailable`, `test_build_template_error_specificity`, `e2e_requirements_packwiz_missing`.

**Infrastructure (zero assertions):** 27 display tests, 1 networking test.

### Known issues

- `handle_remove_tests::it_rejects_incomplete_project_state` calls `handle_sync`, not `handle_remove`
- 3 duplicate test pairs covering identical logic
- 7 weak tests (is_ok/is_err only, no value verification)
- `MockBehavior::Conditional` and `MockSessionBuilder::with_interactive_provider()` are unused infrastructure

---

## External Tool Dependencies

| Tool | Used by | Unit test fidelity | E2E fidelity |
|------|---------|-------------------|-------------|
| packwiz | init, add, remove, sync, build | MockProcessProvider verifies args | Real packwiz |
| java | server builds, packwiz-installer | MockProcessProvider verifies args | Real Java |
| zip/unzip | build client, server, full | MockArchiveProvider records calls | Real archive operations |
| HTTP (Modrinth, CurseForge) | add, search, import, JAR identification | mockito + VCR cassettes | Real API calls |

---

## Gaps and Next Steps

### Architecture

1. Scaffold `empack-e2e` crate with `assert_cmd` and `expectrl` dependencies
2. Add `mise.toml` with `test`, `e2e`, `e2e:filter`, `lint`, `build` tasks
3. Add Dockerfile for containerized E2E (Colima aarch64)
4. Remove HermeticSessionBuilder (replaced by E2E subprocess tests)

### Test coverage

1. Add `cargo-llvm-cov` to CI for branch coverage measurement
2. Write E2E tests for init, add, sync, build, import
3. Add `cargo-fuzz` targets for `classify_url`, `parse_curseforge_zip`, `parse_modrinth_mrpack`, `sanitize_archive_path`
4. Add regression tests for the 9 review-round bugs

### Cleanup

1. Eliminate 65 vacuous tests (strengthen or remove)
2. Fix the misplaced `handle_remove` test
3. Remove 3 duplicate test pairs
4. Strengthen 7 weak tests with specific value assertions
5. Remove unused test infrastructure (`MockBehavior::Conditional`, `with_interactive_provider`)
