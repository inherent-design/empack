[![Build Status](https://img.shields.io/github/actions/workflow/status/inherent-design/empack/ci.yml?branch=main&style=flat)](https://github.com/inherent-design/empack/actions/workflows/ci.yml) [![Coverage](https://codecov.io/gh/inherent-design/empack/branch/main/graph/badge.svg)](https://codecov.io/gh/inherent-design/empack) [![License](https://img.shields.io/github/license/inherent-design/empack?style=flat)](LICENSE)

# empack

empack is a Rust CLI for Minecraft modpack lifecycle management. It wraps a packwiz-tx managed workspace with project initialization, dependency search across Modrinth and CurseForge, modpack import, dependency reconciliation, and build workflows.

## Getting Started

Check the host environment, initialize a project, add a dependency, then build all outputs:

```bash
empack requirements
empack init my-pack
cd my-pack
empack add sodium
empack build all
```

If a build stops on restricted CurseForge files, empack records continuation state and can resume with:

```bash
empack build --continue
```

In interactive terminals, empack can open direct CurseForge download URLs in the browser and wait up to 5 minutes for the files to appear before falling back to the manual `build --continue` flow.

Import an existing modpack from a local archive or a remote modpack URL:

```bash
empack init --from pack.mrpack my-imported-pack
empack init --from https://cdn.modrinth.com/data/.../pack.mrpack my-pack
```

`packwiz-tx` is auto-managed on first use. Override it with `EMPACK_PACKWIZ_BIN=/path/to/packwiz-tx` when needed.

## Commands

| Command | Purpose |
| --- | --- |
| `empack requirements` | Check `packwiz-tx`, Java, and archive support |
| `empack version` | Print version and build metadata |
| `empack init` | Create a project or import one with `--from` |
| `empack add` | Add dependencies by query, URL, or direct JAR/typed ZIP download |
| `empack sync` | Reconcile `empack.yml` with installed packwiz state |
| `empack build` | Build `mrpack`, `client`, `server`, `client-full`, `server-full`, or `all` |
| `empack remove` | Remove dependencies from the current project |
| `empack clean` | Clean build artifacts or cache data |

## Project Model

Each empack project consists of three parts:

- `empack.yml`: project configuration; mod list, loader version, Minecraft version, and build settings.
- `pack/`: managed packwiz workspace. empack reads and writes this directory.
- `dist/`: build artifact output. Contains mrpack archives and client/server distribution folders after a build.

Direct-download content that cannot be resolved to a platform project is tracked explicitly in `empack.yml` as a local dependency with a project-relative path and SHA-256 hash. `sync`, `remove`, and `build` all honor those tracked local entries.

Restricted CurseForge build steps may create internal continuation state when redistribution is blocked. The public recovery command is `empack build --continue`.
`empack clean` is non-destructive: it removes build artifacts and empack-managed cache data, but never removes `empack.yml` or `pack/`.

empack uses stable exit codes: `0` success, `1` general runtime/process failure, `2` usage/config/project-state failure, `3` network/provider/API failure, `4` not found/no results, `130` interrupt.

## Documentation

| Document | Description |
| --- | --- |
| [Usage Guide](docs/usage.md) | User-facing command reference |
| [Testing](docs/testing.md) | Test layers, prerequisites, and current counts |
| [Spec Overview](docs/specs/overview.md) | Technical spec index for current runtime behavior |
| [Contributing](CONTRIBUTING.md) | Development setup and workflow |
| [Provider API: Modrinth](docs/reference/MODRINTH.md) | Provider reference |
| [Provider API: CurseForge](docs/reference/CURSEFORGE.md) | Provider reference |

## License

[Apache 2.0](LICENSE)
