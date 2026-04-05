# Contributing to empack

## Prerequisites

- [Rust toolchain](https://rustup.rs/) (stable)
- [cargo-nextest](https://nexte.st/) (test runner; CI uses it exclusively)
- [mise](https://mise.jdx.dev/) (task runner)

For E2E tests (not required for unit tests or development):

- [packwiz](https://packwiz.infra.link/) (real CLI invocation in E2E)
- [Java 21+](https://adoptium.net/) (server build E2E tests)
- `.env.local` with `EMPACK_KEY_CURSEFORGE` (CurseForge API E2E tests)
- Optional: [Colima](https://github.com/abiosoft/colima) (containerized E2E)
- Optional: `jq` (VCR cassette recording)

## Getting Started

```bash
git clone https://github.com/inherent-design/empack.git
cd empack
cargo build --workspace
cargo check --workspace --all-targets
```

## Testing

empack uses two test tiers. See [docs/testing.md](docs/testing.md) for the full strategy, health inventory, and VCR fixture maintenance.

### Unit tests (CI gate)

Mock-based, cross-platform, fast. Run on every commit:

```bash
mise run test
# or directly:
cargo nextest run -p empack-lib --features test-utils
cargo nextest run -p empack-tests
```

Use isolated reruns when iterating on specific behavior:

```bash
cargo nextest run -p empack-lib --features test-utils --lib test_name
cargo nextest run -p empack-tests --test sync_workflow
```

### E2E tests (advisory)

Run the compiled binary with real providers (real filesystem, real packwiz, real network). Not part of the CI gate; requires external tools.

```bash
mise run e2e                # full suite (requires packwiz, java)
mise run fe2e "init"        # filtered subset
mise run e2e:container      # containerized (requires Colima)
```

E2E tests self-skip when prerequisites are missing. Tests that hit live APIs are gated behind `EMPACK_KEY_CURSEFORGE` and `EMPACK_E2E_SKIP_LIVE`.

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

Scopes are optional; use when the change targets a specific module or subsystem:

```
feat(import): add modpack import pipeline
fix(sync): handle local-only dependency sources
test(add): URL classification coverage
```

Subject line in imperative mood, under 72 characters. Body explains why, not what. For contributor PRs, the maintainer squash-merges with a clean conventional subject line.

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

### Doc Comments

Every exported type, function, and trait gets a `///` doc comment. Write with `cargo doc` and future doc-gen tooling in mind. The first line is a summary; subsequent lines cover inputs, side effects, and error conditions.

```rust
/// Classify a URL into a known platform or direct download target.
///
/// Returns `UrlClassifyError` for URLs that do not match any supported
/// platform pattern or recognized file extension.
pub fn classify_url(url: &str) -> Result<UrlKind, UrlClassifyError> { ... }
```

Internal helpers and private functions do not require doc comments unless the behavior is non-obvious.

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

## Changelog

Release notes are generated from conventional commit history. Do not edit CHANGELOG.md manually; write good commit messages and the changelog follows.

## Agent Guidelines

These apply to LLM agents (Atlas sub-agents, Claude Code teammates) writing code, commits, or documentation for empack.

**Code:** Follow all conventions in this file. Default to no comments. Write `///` doc comments on exports. Match surrounding style. Do not "improve" adjacent code, comments, or formatting that is not part of the task.

**Commits:** Use conventional commits with scoped prefixes where applicable. Body explains why, not what.

**Communication:** No preamble ("I aim to help"), no flattery ("Great question"), no superlatives. Direct answers. Use the standard status protocol (STATUS/PROGRESS/BLOCKERS/QUESTIONS/NEXT) for handoffs.

**Errors:** State what happened, what was expected, and what to do about it. Do not apologize or catastrophize. Extract information from the error and move on.

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
