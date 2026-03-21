# empack

empack is a Rust CLI for Minecraft modpack project setup, dependency reconciliation, and build or export workflows around `packwiz`.

## Getting started

See [`docs/usage.md`](docs/usage.md) for installation prerequisites, command reference, and typical workflows.

## Command surface

| Command | Purpose |
| --- | --- |
| `empack requirements` | Check external tool availability and setup guidance |
| `empack version` | Print version information |
| `empack init` | Create or complete a modpack project |
| `empack add` | Add mods by name, URL, or project ID |
| `empack sync` | Reconcile `empack.yml` with installed pack state |
| `empack build` | Build `mrpack`, `client`, `server`, `client-full`, `server-full`, or `all` |
| `empack remove` | Remove mods from the current project (alias: `rm`) |
| `empack clean` | Remove build outputs from `dist/` |

## Current status

- Active branch: `dev`
- Test surface: 325 passed, 15 skipped in `empack-lib`; 46 passed in `empack-tests` (371 total)
- CI uses `cargo nextest`; grouped `cargo test` is advisory-only due to shared global state conflicts. See [`docs/testing/README.md`](docs/testing/README.md) for details.

## Project structure

| Path | Contents |
| --- | --- |
| `crates/empack` | CLI entry point |
| `crates/empack-lib` | Application logic, state, resolver, and build system |
| `crates/empack-tests` | Workflow and integration tests |
| `docs/usage.md` | Usage and workflow reference |
| `docs/testing/README.md` | Verification matrix and test caveats |
| `docs/testing/vcr-recording.md` | VCR fixture maintenance |
| `docs/reference/` | Provider API reference (Modrinth, CurseForge) |

## Documentation

- [`docs/usage.md`](docs/usage.md): command reference and workflows
- [`docs/testing/README.md`](docs/testing/README.md): verification matrix
- [`docs/testing/vcr-recording.md`](docs/testing/vcr-recording.md): VCR fixture maintenance
- [`CONTRIBUTING.md`](CONTRIBUTING.md): contributor workflow
- [`docs/reference/`](docs/reference/): Modrinth and CurseForge API notes
- [`docs/ARCHITECTURAL_DECISION_RECORD.md`](docs/ARCHITECTURAL_DECISION_RECORD.md): historical architecture context

## Historical context

The `v1/` and `v2/` directories contain earlier Bash implementations, retained for lineage reference only.
