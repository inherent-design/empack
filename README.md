# empack

empack is a Rust CLI for Minecraft modpack project setup, dependency reconciliation, and build or export workflows around `packwiz`.

## Start here

- Using the current Rust CLI: [`docs/usage.md`](docs/usage.md)
- Checking the trusted verification matrix: [`docs/testing/README.md`](docs/testing/README.md)
- Contributing or maintaining fixtures: [`CONTRIBUTING.md`](CONTRIBUTING.md)
- VCR-backed maintenance and provider reference material: [`docs/testing/vcr-recording.md`](docs/testing/vcr-recording.md), [`docs/reference/`](docs/reference/)

## Current status

- Active release target: the Rust workspace in `crates/*`
- Historical Bash implementations in `v1/` and `v2/` are reference context only
- Current trusted verification matrix lives in [`docs/testing/README.md`](docs/testing/README.md)
- Primary trusted test runner: `cargo nextest`; the accepted release gate is `cargo check`, `cargo clippy`, `cargo nextest run -p empack-lib --features test-utils`, and `cargo nextest run -p empack-tests`
- Grouped `cargo test` instability is broader than `sync_workflow` alone; several workflow suites remain advisory-only under grouped execution because of shared global state and env conflicts
- Standalone missing-installer coverage now exists and passes under nextest isolation

## Command surface

| Command | Current scope | Verification status |
| --- | --- | --- |
| `requirements` | Check external tool availability and setup guidance | Trusted command surface; see `requirements_command.rs` |
| `version` | Print version information | Command surface verified via CLI help |
| `init` | Create or complete an empack project | Trusted under nextest-backed workflow coverage; grouped `cargo test` remains advisory-only for workflow files |
| `sync` | Reconcile `empack.yml` with installed pack state | Trusted in isolated nextest reruns; grouped `cargo test` is unstable |
| `build` | Build `mrpack`, `client`, `server`, `client-full`, `server-full`, or `all` | Trusted under promoted nextest workflow suites; grouped `cargo test` remains unstable for several promoted files |
| `add` | Add mods by name, URL, or project ID | Trusted on current hermetic and hybrid test paths |
| `remove` | Remove mods from the current project | Targeted nextest-backed command coverage exists; broader remove breadth is still the remaining explicit gap |
| `clean` | Remove build outputs from `dist/` | Trusted through lifecycle, command, and state coverage |

## Project model

- `empack.yml` is the declared project configuration
- `pack/` contains the managed `packwiz` workspace
- `dist/` is the canonical artifact root for build outputs
- `build --clean` removes build artifacts while preserving config and pack metadata

## Trusted verification summary

The primary trusted matrix is grounded in the accepted release-gate checkpoint and current test surface:

- `cargo check --workspace --all-targets`
- `cargo clippy --workspace --all-targets`
- `cargo nextest run -p empack-lib --features test-utils` (325 passed, 15 skipped in the accepted checkpoint)
- `cargo nextest run -p empack-tests` (46 passed in the accepted checkpoint)
- CI uses `cargo nextest` for tests; grouped `cargo test` remains advisory-only because several workflow suites are unstable when run together

See [`docs/testing/README.md`](docs/testing/README.md) for exact command examples and caveats.

## Repository layout

- `crates/empack`: CLI entry point
- `crates/empack-lib`: application, state, build, and resolver logic
- `crates/empack-tests`: workflow and integration tests
- `docs/usage.md`: current usage and workflow notes
- `docs/testing/README.md`: trusted verification matrix
- `docs/testing/vcr-recording.md`: VCR fixture maintenance guidance
- `docs/reference/`: provider reference notes

## Documentation

- User and workflow guidance: [`docs/usage.md`](docs/usage.md)
- Verification guidance: [`docs/testing/README.md`](docs/testing/README.md)
- VCR-backed maintenance: [`docs/testing/vcr-recording.md`](docs/testing/vcr-recording.md)
- Contributor workflow: [`CONTRIBUTING.md`](CONTRIBUTING.md)
- Provider reference notes: [`docs/reference/MODRINTH.md`](docs/reference/MODRINTH.md), [`docs/reference/CURSEFORGE.md`](docs/reference/CURSEFORGE.md)
- Historical architecture context: [`docs/ARCHITECTURAL_DECISION_RECORD.md`](docs/ARCHITECTURAL_DECISION_RECORD.md)

## Historical context

The repo still contains `v1/` and `v2/` Bash implementations. They remain useful for lineage and product-history reference, but they are not the active implementation line and should not be used as release-facing guidance.