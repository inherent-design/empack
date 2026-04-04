---
spec: overview
status: draft
created: 2026-04-04
updated: 2026-04-04
depends: []
---

# System Overview

empack is a Rust CLI for Minecraft modpack lifecycle management. It wraps packwiz as the pack management layer and adds project initialization, mod discovery across Modrinth and CurseForge, dependency reconciliation, modpack import, and build/export workflows.

## Crate Structure

| Crate | Role |
|-------|------|
| `empack` | CLI entry point (clap) |
| `empack-lib` | Application logic, resolver, build system |
| `empack-tests` | Workflow and integration tests |

## Command Surface

| Command | Purpose |
|---------|---------|
| `init` | Create a new modpack project; `--from` imports an existing modpack |
| `add` | Add mods by name, URL, or direct download |
| `remove` | Remove mods from the project |
| `sync` | Reconcile empack.yml with packwiz state |
| `build` | Export to mrpack, client, server, or full distributions |
| `clean` | Remove build artifacts |
| `requirements` | Check external tool dependencies |
| `version` | Print version |

## Module Graph

```
primitives/         Type definitions (ProjectType, ProjectPlatform, BuildTarget, PackState)
    |
empack/             Domain logic
    ├── content.rs      URL classification, JAR identification
    ├── import.rs       Modpack import pipeline (parsers, resolver, executor)
    ├── config.rs       empack.yml schema, DependencyRecord
    ├── search.rs       Platform search, ProjectResolver
    ├── state.rs        PackState machine, transitions
    ├── builds.rs       Build orchestrator
    ├── templates.rs    Handlebars template engine
    ├── versions.rs     Loader version fetcher
    ├── parsing.rs      ModLoader, packwiz format parsing
    ├── archive.rs      Archive format handling
    └── fuzzy.rs        Fuzzy matching
    |
application/        CLI layer
    ├── cli.rs          Clap definitions
    ├── commands.rs     Command handlers
    ├── sync.rs         Sync plan, add contract, DependencySource
    ├── session.rs      Session trait, provider traits, DI
    └── loader.rs       Configuration loading
```

## Key Architectural Decisions

- packwiz is the pack management layer; empack does not implement TOML metadata management directly.
- Session-based dependency injection with 8 provider traits. All side effects go through providers.
- empack.yml is the project manifest; pack.toml is packwiz's manifest. Both must stay in sync.
- No git support (packwiz has zero git integration; download.url must be HTTP/HTTPS).
- All empack/ modules are flat .rs files. Test companions use `include!` (e.g., `search.test.rs`).

## Cross-References

- [session-providers.md](session-providers.md): provider traits and DI pattern
- [platform-modrinth.md](platform-modrinth.md): Modrinth API contracts and mrpack format
- [platform-curseforge.md](platform-curseforge.md): CurseForge API contracts and manifest format
- [types.md](types.md): shared type definitions
- [state-machine.md](state-machine.md): PackState transitions
