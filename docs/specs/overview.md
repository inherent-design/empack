---
spec: overview
status: ratified
created: 2026-04-04
updated: 2026-04-11
depends: []
---

# System Overview

empack is a Rust CLI for Minecraft modpack lifecycle management. It wraps a packwiz-tx managed workspace with project state, search, import, build, logging, and testing workflows.

## Crate Boundaries

| Crate | Role |
| --- | --- |
| `empack` | CLI entry point and process startup |
| `empack-lib` | Runtime logic, provider seams, search, import, build, logging, platform support |
| `empack-tests` | Subprocess E2E harness, workflow coverage, VCR-backed fixtures |

## Runtime Subsystems

| Subsystem | Primary files | Responsibility |
| --- | --- | --- |
| Application | `application/cli.rs`, `application/commands.rs`, `application/config.rs`, `application/session.rs`, `application/sync.rs` | CLI surface, command dispatch, session construction, sync and add contracts |
| empack domain | `empack/*.rs` | Search, import, config, state, build, templates, packwiz integration, archive handling |
| Primitives | `primitives/empack.rs`, `primitives/project_platform.rs` | Shared enums, transition identities, platform types |
| Display | `display/*.rs` | Status output, progress bars, structured tables |
| Logger | `logger/mod.rs` | Tracing subscriber setup, log formatting, optional telemetry layers |
| Networking | `networking/*.rs` | HTTP cache, rate budgets, rate-limited clients, resource-aware network manager |
| Platform | `platform/*.rs` | Cache paths, browser launch helpers, system resource detection, managed `packwiz-tx` resolution |
| Terminal | `terminal/*.rs` | Terminal capability detection and cursor recovery |
| API | `api/dependency_graph.rs` | Dependency graph analysis used by add and remove workflows |
| Testing | `testing/*.rs`, `crates/empack-tests/**` | Temp fixtures, subprocess E2E harness, PTY coverage, cassettes |

## Workflow Map

| Workflow | Primary modules | Result |
| --- | --- | --- |
| `init` | `application/cli.rs`, `application/commands.rs`, `empack/state.rs`, `empack/templates.rs`, `empack/versions.rs`, `empack/packwiz.rs` | Creates project structure, initializes packwiz state, installs templates |
| `init --from` | `application/commands.rs`, `empack/content.rs`, `empack/import.rs`, `empack/config.rs`, `empack/packwiz.rs` | Imports a Modrinth or CurseForge modpack into an empack project |
| `add` | `application/commands.rs`, `application/sync.rs`, `empack/search.rs`, `empack/content.rs` | Resolves a query, URL, or direct download into packwiz add operations and manifest updates |
| `sync` | `application/sync.rs`, `empack/config.rs`, `empack/packwiz.rs` | Reconciles `empack.yml` intent with installed packwiz state |
| `build` | `application/commands.rs`, `empack/state.rs`, `empack/builds.rs`, `empack/templates.rs`, `empack/archive.rs`, `empack/packwiz.rs` | Produces build artifacts under `dist/` |
| `clean` | `application/commands.rs`, `empack/state.rs`, `empack/builds.rs`, `platform/cache.rs` | Removes build artifacts and, optionally, cache data |

## Module Map

- `crates/empack-lib/src/lib.rs` re-exports `api`, `application`, `display`, `empack`, `logger`, `networking`, `platform`, `primitives`, `terminal`, and `testing`.
- `application/session.rs` is the runtime seam. Commands operate on `&dyn Session`, not on raw filesystem or process handles.
- `application/session.rs` constructs the live network provider through an async path that loads the persisted HTTP cache from the empack cache root.
- `empack/config.rs` and `empack/packwiz.rs` model the split between user intent in `empack.yml` and packwiz state in `pack/pack.toml`.
- `empack/state.rs` discovers project state from files on disk. Intermediate operations use a marker file for interruption recovery.
- `platform/packwiz_bin.rs` resolves `packwiz-tx` from an override, PATH, or a managed cached binary.
- `platform/cache.rs` is the source of truth for cache layout under the empack cache root.
- `logger/mod.rs` and `terminal/cursor.rs` are part of process lifecycle, not just diagnostics. Startup and shutdown paths restore cursor state and flush telemetry.

## Source of Truth

1. Live source under `crates/empack-lib/src/**` and `crates/empack/src/**`.
2. Completed plans from 2026-03-25 through 2026-04-07, but only where the code matches.
3. Operational docs such as `CONTRIBUTING.md`, `README.md`, `docs/usage.md`, `docs/testing.md`, and `~/.atlas/bootstrap/empack.md`.

## Cross References

- [types.md](types.md)
- [state-machine.md](state-machine.md)
- [session-providers.md](session-providers.md)
- [session-security.md](session-security.md)
- [cli-surface.md](cli-surface.md)
- [config-and-manifest.md](config-and-manifest.md)
- [search-and-resolution.md](search-and-resolution.md)
- [import-pipeline.md](import-pipeline.md)
- [build-and-distribution.md](build-and-distribution.md)
- [display.md](display.md)
- [terminal.md](terminal.md)
- [dependency-graph.md](dependency-graph.md)
- [networking-and-rate-budgets.md](networking-and-rate-budgets.md)
- [logging-and-telemetry.md](logging-and-telemetry.md)
- [platform-and-managed-tooling.md](platform-and-managed-tooling.md)
- [platform-modrinth.md](platform-modrinth.md)
- [platform-curseforge.md](platform-curseforge.md)
- [testing-architecture.md](testing-architecture.md)
