# empack

empack is a Rust CLI for Minecraft modpack project setup, dependency reconciliation, and build or export workflows around `packwiz`.

## Current status

- Active release target: the Rust workspace in `crates/*`
- Historical Bash implementations in `v1/` and `v2/` are reference context only
- Current trusted verification guidance lives in [`docs/testing/README.md`](docs/testing/README.md)
- Known grouped-workflow caveat: grouped `sync_workflow` reruns can hit `Global configuration already initialized`
- Deferred gaps remain for standalone `client-full` and `server-full` missing-installer propagation

## Command surface

| Command | Current scope | Verification status |
| --- | --- | --- |
| `requirements` | Check external tool availability and setup guidance | Trusted command surface; see `requirements_command.rs` |
| `version` | Print version information | Command surface verified via CLI help |
| `init` | Create or complete an empack project | Trusted hermetic workflow coverage |
| `sync` | Reconcile `empack.yml` with installed pack state | Trusted in isolated reruns; grouped caveat remains |
| `build` | Build `mrpack`, `client`, `server`, `client-full`, `server-full`, or `all` | Trusted for promoted workflow suites; deferred standalone gaps remain |
| `add` | Add mods by name, URL, or project ID | Trusted on current hermetic and hybrid test paths |
| `remove` | Remove mods from the current project | Available and covered by targeted command tests |
| `clean` | Remove build outputs from `dist/` | Trusted through lifecycle, command, and state coverage |

## Project model

- `empack.yml` is the declared project configuration
- `pack/` contains the managed `packwiz` workspace
- `dist/` is the canonical artifact root for build outputs
- `build --clean` removes build artifacts while preserving config and pack metadata

## Trusted verification summary

The current trusted matrix is grounded in the spec checkpoints and current test surface:

- `cargo build --workspace --locked`
- `cargo check --workspace --all-targets --locked`
- `cargo nextest run -p empack-lib --features test-utils --lib`
- Promoted workflow suites in `crates/empack-tests/tests/`:
  - `lifecycle_forge_full.rs`
  - `build_command.rs`
  - `build_server.rs`
  - `build_server_full.rs`
  - `build_client_full.rs`
- Isolated sync reruns for `test_sync_workflow_full` and `test_sync_dry_run_no_modifications`

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

- [`docs/usage.md`](docs/usage.md)
- [`docs/testing/README.md`](docs/testing/README.md)
- [`docs/testing/vcr-recording.md`](docs/testing/vcr-recording.md)
- [`docs/reference/MODRINTH.md`](docs/reference/MODRINTH.md)
- [`docs/reference/CURSEFORGE.md`](docs/reference/CURSEFORGE.md)
- [`CONTRIBUTING.md`](CONTRIBUTING.md)

## Historical context

The repo still contains `v1/` and `v2/` Bash implementations. They remain useful for lineage and product-history reference, but they are not the active implementation line and should not be used as release-facing guidance.