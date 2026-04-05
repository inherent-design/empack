# Testing

empack tests across two tiers: unit tests for pure functions and mock-based command logic, and E2E tests against live providers with the real binary.

## Philosophy

- **Real providers for E2E.** E2E tests run the empack binary as a subprocess with real filesystem, real packwiz, real network. No mocks. Interactive flows use `expectrl` (cross-platform PTY). Non-interactive flows use `assert_cmd`.
- **Mock-based unit tests.** Command handlers, config parsing, state machines, URL classification, override categorization, and sync logic are tested in isolation through provider mocks.
- **VCR cassettes for contract verification.** Recorded API responses verify that deserialization structs match real response shapes. Contract tests live in the unit tier.

## Unit Tests

769 tests across two crates, all run via mise tasks:

```bash
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets
cargo nextest run -p empack-lib --features test-utils
cargo nextest run -p empack-tests
```

**empack-lib** (676 tests): co-located `.test.rs` files via `include!()`. Feature-gated behind `test-utils`. Includes command handler tests via `MockCommandSession`, API contract tests deserializing VCR cassettes, config/state/search/build/sync/parser/import unit tests.

**empack-tests** (93 tests, 1 skipped): mock-based workflow tests via `MockSessionBuilder` + live E2E subprocess tests via `assert_cmd` and `expectrl`. E2E tests self-skip when prerequisites (packwiz, java, CF key) are missing.

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
cargo nextest run -p empack-e2e       # full E2E suite
```

Mise task definitions (`e2e`, `e2e:filter`, `e2e:container`) are planned but not yet implemented. E2E tests are advisory, not gating. Failures indicate API drift, environment issues, or real bugs.

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
crates/empack-tests/
  src/e2e.rs              # TestProject, empack_bin(), skip macros, empack_assert_cmd()
  tests/
    e2e_version.rs        # version output, help, TestProject smoke
    e2e_init.rs           # fabric, neoforge, missing modloader, existing project, force, scaffolding
    e2e_build.rs          # mrpack export, clean
    e2e_add.rs            # uninitialized, live sodium, nonexistent mod
    e2e_interactive.rs    # expectrl PTY init flow (#[ignore])
    e2e_matrix.rs         # macro-generated: modloader variants, bad flags, requires-modpack, build targets, help
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

### Resolved since audit

- Vacuous integration tests deleted: `test_init_packwiz_unavailable`, `test_build_template_error_specificity`, `e2e_requirements_packwiz_missing`
- `MockBehavior::Conditional`, `ConditionalRule`, `HermeticSessionBuilder`, `TestEnvironment` deleted
- Weak loader version tests strengthened with value assertions
- 8 error-path tests corrected from `is_ok()` to `is_err()` (exit code fix)

### Remaining known issues

- `handle_remove_tests::it_rejects_incomplete_project_state` calls `handle_sync`, not `handle_remove`
- 3 duplicate test pairs covering identical logic
- 27 display infrastructure tests with zero assertions (untestable in unit tier; display output requires E2E)

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

### Completed

- E2E harness scaffolded in empack-tests (TestProject, skip macros, assert_cmd, expectrl)
- 38 E2E tests across 6 files (init, build, add, interactive, version, matrix)
- mise.toml with inline tasks; packwiz via Go backend
- CI unified: lint, test (3 platforms), coverage, cross-check
- HermeticSessionBuilder and dead infrastructure deleted
- Vacuous integration tests deleted; weak tests strengthened
- Coverage includes E2E via instrumented binary (82.8% line, 75.2% branch)

### Remaining

1. Fix the misplaced `handle_remove` test (calls handle_sync)
2. Remove 3 duplicate test pairs
3. Add `cargo-fuzz` targets for `classify_url`, `parse_curseforge_zip`, `parse_modrinth_mrpack`, `sanitize_archive_path`
4. Add regression tests for the 9 review-round API contract bugs
5. Containerized E2E via Colima (deferred)
