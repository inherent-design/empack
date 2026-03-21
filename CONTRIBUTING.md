# Contributing to empack

## Reference documents

- Project overview and structure: [`README.md`](README.md)
- Command reference and workflows: [`docs/usage.md`](docs/usage.md)
- Verification matrix and test caveats: [`docs/testing/README.md`](docs/testing/README.md)
- VCR fixture maintenance: [`docs/testing/vcr-recording.md`](docs/testing/vcr-recording.md)
- Provider API notes: [`docs/reference/MODRINTH.md`](docs/reference/MODRINTH.md), [`docs/reference/CURSEFORGE.md`](docs/reference/CURSEFORGE.md)

## Scope

This repository treats the Rust workspace as the active implementation line. The Bash implementations in `v1/` and `v2/` are historical reference material only.

See [`README.md`](README.md) for the repository layout.

## Local setup

Run the smallest relevant checks first:

```bash
cargo build --workspace --locked
cargo check --workspace --all-targets --locked
```

Tooling notes:

- `cargo nextest` is the default test runner for trusted workflow paths; CI uses it exclusively
- Live CLI workflows may require external tools such as `packwiz`
- Hermetic workflow tests use mocked toolchains where possible
- VCR maintenance uses `curl`, `jq`, and `.env.local` as described in [`docs/testing/vcr-recording.md`](docs/testing/vcr-recording.md)

## Verification expectations

Before claiming a workflow is trusted, check it against [`docs/testing/README.md`](docs/testing/README.md).

Rules:

1. Prefer the smallest exact command that proves the touched behavior.
2. Treat grouped `cargo test` workflow runs as advisory-only until the broader global-state and env-conflict instability is fixed.
3. Keep VCR-backed flows separate from the default hermetic matrix.
4. If a path is deferred or only partially covered, document that directly.

## Documentation rules

- Always write `empack` in lowercase.
- Keep prose factual, technical, and concise. No em-dashes, no superlatives, no marketing language.
- Do not add badges, support promises, or release statements the repo cannot prove.
- When behavior changes, update the affected docs in the same change when practical.
- Keep historical Bash content in reference sections only, not in active product guidance.
- Label current truth versus historical context directly instead of mixing them.

## Coding notes

- Follow surrounding Rust patterns unless a narrower contract improvement is clearly better.
- Keep changes scoped.
- Avoid broad refactors during feature or docs slices.
- Remove temporary logging before finishing a change.
- Prefer trace and error logging where durable logging is needed.

## Commits

Use a short conventional subject line:

- `docs: refresh usage guide`
- `fix: preserve dist metadata on clean`
- `test: harden sync workflow assertions`

Guidelines:

- imperative mood
- under 72 characters when possible
- explain why in the body if more context is needed

## VCR and fixture maintenance

If you touch recorded API fixtures or cassette helpers:

1. Read [`docs/testing/vcr-recording.md`](docs/testing/vcr-recording.md).
2. Prefer `./scripts/record-vcr-cassettes.sh --dry-run` before a live recording pass.
3. Re-run the targeted cassette loader checks after updating fixtures.

## Pull request checklist

- [ ] Scope is narrow and explicit
- [ ] Docs match the current verified behavior
- [ ] Verification commands are listed in the change summary
- [ ] Deferred gaps or caveats remain explicit where relevant
