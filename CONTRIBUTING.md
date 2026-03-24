# Contributing to empack

## Prerequisites

- [Rust toolchain](https://rustup.rs/) (stable)
- [cargo-nextest](https://nexte.st/) (test runner; CI uses it exclusively)
- [packwiz](https://packwiz.infra.link/) (required for live CLI workflows)
- [Java](https://adoptium.net/) (required for Quilt, NeoForge, and Forge server builds)
- Optional: `jq` (VCR cassette recording)

## Getting Started

```bash
git clone https://github.com/inherent-design/empack.git
cd empack
cargo build --workspace
cargo check --workspace --all-targets
```

## Testing

CI uses `cargo nextest` exclusively. The trusted release gate:

```bash
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets
cargo nextest run -p empack-lib --features test-utils
cargo nextest run -p empack-tests
```

Grouped `cargo test` is advisory-only due to global state conflicts between workflow tests. Prefer nextest and targeted isolated reruns.

Use isolated reruns for touched workflow behavior:

```bash
cargo nextest run -p empack-tests --test sync_workflow test_sync_workflow_full
cargo nextest run -p empack-lib --features test-utils --lib handle_remove_tests
```

See [docs/testing.md](docs/testing.md) for the full verification matrix and VCR fixture maintenance.

## Development Workflow

1. Create a feature branch from `dev`
2. Make changes
3. Lint: `cargo clippy --workspace --all-targets`
4. Test: run the relevant nextest commands above
5. Submit PR against `dev`

## Project Structure

```
empack/
  crates/
    empack/              CLI entry point (clap)
    empack-lib/          Application logic, resolver, build system
    empack-tests/        Workflow and integration tests
  docs/
    usage.md             Command reference
    testing.md           Test strategy and verification
    reference/           Provider API documentation (Modrinth, CurseForge)
  scripts/               VCR recording and utility scripts
  v1/, v2/               Historical Bash implementations (reference only)
```

## Commits

Conventional-style prefixes: `feat:`, `fix:`, `chore:`, `ci:`, `docs:`, `test:`, `refactor:`

Subject line in imperative mood, under 72 characters. Body explains why, not what.

```
docs: refresh usage guide
fix: preserve dist metadata on clean
test: harden sync workflow assertions
```

## Code Style

### General

Run `cargo clippy` before submitting. Follow existing patterns in the codebase. When in doubt, match the surrounding code.

### Logging

Use structured logging at appropriate levels:
- `error!` for failures that affect command outcome
- `trace!` for operational detail during development
- Remove temporary `debug!`/`println!` logging before finishing a change

### Comments

Default to no comments. Code should be self-explanatory through naming and structure. Comment when:
- The "why" is non-obvious (a workaround, an API quirk)
- The behavior has surprising side effects
- A constant comes from an external specification

Do not comment what the code already says.

## Documentation

### Where Things Live

**README.md** is the hub document: project description, quick start, command table, and links to `docs/`. Keep it scannable.

**docs/*.md** files are deep reference, one file per topic. These are the source of truth for user-facing documentation.

**CONTRIBUTING.md** covers development workflow, code style, and conventions. Not user-facing.

### When to Update Docs

When behavior changes, update the affected docs in the same change. Treat it as part of the change, not a follow-up.

### Writing Style

Technical reference tone. Use complete sentences with natural compound structure.

**Prohibited in prose:** em-dashes, en-dashes, double-hyphens. Use semicolons, commas, or colons instead. Double-hyphens in CLI flags and code are fine.

**Avoid:** superlatives, fragment-sentence drama, marketing language. Always write `empack` in lowercase.

## VCR Fixture Maintenance

If you touch recorded API fixtures or cassette helpers:

1. Preview first: `./scripts/record-vcr-cassettes.sh --dry-run`
2. Record: `./scripts/record-vcr-cassettes.sh`
3. Verify: `cargo test -p empack-tests fixtures::tests::test_load_vcr_cassette -- --exact`

Live recording requires `curl`, `jq`, and `.env.local` with `EMPACK_KEY_CURSEFORGE`. Copy `.env.local.template` as a starting point.

## Pull Request Checklist

- [ ] Scope is narrow and explicit
- [ ] Docs match the current verified behavior
- [ ] Verification commands are listed in the change summary
- [ ] Tests pass: `cargo nextest run -p empack-lib --features test-utils && cargo nextest run -p empack-tests`

## License

By contributing, you agree that your contributions will be licensed under the [Apache 2.0 License](LICENSE).
