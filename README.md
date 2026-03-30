[![Build Status](https://img.shields.io/github/actions/workflow/status/inherent-design/empack/ci.yml?branch=dev&style=flat)](https://github.com/inherent-design/empack/actions/workflows/ci.yml) [![License](https://img.shields.io/github/license/inherent-design/empack?style=flat)](LICENSE)

# empack

empack is a Rust CLI for Minecraft modpack management. It handles project initialization, mod discovery across Modrinth and CurseForge, dependency reconciliation, and build/export workflows. Build targets include mrpack archives and full client/server distributions. empack uses [packwiz](https://github.com/packwiz/packwiz) as its underlying pack management layer.

## Getting Started

Verify that external tools are available, then scaffold a new project:

```bash
empack requirements          # check for packwiz, java, etc.
empack init my-pack          # create a new modpack project
cd my-pack
empack add sodium            # search and add a mod
empack build all             # produce mrpack, client, and server artifacts
```

See [Usage Guide](docs/usage.md) for the full command reference, flags, and environment variables.

## Commands

| Command               | Purpose                                              |
| --------------------- | ---------------------------------------------------- |
| `empack requirements` | Check external tool availability                     |
| `empack version`      | Print version information                            |
| `empack init`         | Create or complete a modpack project                 |
| `empack add`          | Add mods by name, URL, or project ID                 |
| `empack sync`         | Reconcile declared dependencies with installed state |
| `empack build`        | Build mrpack, client, server, or all targets         |
| `empack remove`       | Remove mods from the current project (alias: `rm`)   |
| `empack clean`        | Remove build outputs from `dist/`                    |

## Project Model

Each empack project consists of three parts:

- `empack.yml`: project configuration; mod list, loader version, Minecraft version, and build settings.
- `pack/`: managed packwiz workspace. empack reads and writes this directory; manual edits are overwritten on sync.
- `dist/`: build artifact output. Contains mrpack archives and client/server distribution folders after a build.

## Documentation

| Document                                                 | Description                                         |
| -------------------------------------------------------- | --------------------------------------------------- |
| [Usage Guide](docs/usage.md)                             | Command reference, flags, and environment variables |
| [Testing](docs/testing.md)                               | Test strategy, verification matrix, VCR fixtures    |
| [Contributing](CONTRIBUTING.md)                          | Development setup and workflow                      |
| [Provider API: Modrinth](docs/reference/MODRINTH.md)     | Modrinth API reference                              |
| [Provider API: CurseForge](docs/reference/CURSEFORGE.md) | CurseForge API reference                            |
| [Changelog](CHANGELOG.md)                                | Release history                                     |

## Project Structure

```
empack/
  crates/
    empack/              CLI entry point
    empack-lib/          Application logic, resolver, and build system
    empack-tests/        Workflow and integration tests
  docs/
    usage.md             Command reference
    testing.md           Test strategy and verification
    reference/           Provider API documentation
  v1/, v2/               Historical Bash implementations (reference only)
```

## Development

```bash
cargo build --workspace
cargo nextest run -p empack-lib --features test-utils
cargo nextest run -p empack-tests
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for full development setup, testing strategy, and contribution guidelines.

## License

[Apache 2.0](LICENSE)
