[![PR CI](https://img.shields.io/github/actions/workflow/status/inherent-design/empack/pr-ci.yml?branch=dev&style=flat)](https://github.com/inherent-design/empack/actions/workflows/pr-ci.yml) [![License](https://img.shields.io/github/license/inherent-design/empack?style=flat)](LICENSE)

# empack

empack manages Minecraft packs across Modrinth, CurseForge and local content.
Its native engine reads and writes packwiz metadata and produces verified distributions;
the CLI does not require the packwiz-tx executable.

The development target is **v0.5.0-alpha.1**: a typed pack engine that separates
intent, exact resolution, observation, staging, verification and recoverable
publication. The [design](docs/design/README.md) is the target contract; the
[implementation ledger](docs/design/implementation.md) records what has landed.
The full engine is not implemented yet.

## Development status

The target architecture replaces the experimental implementation. Old formats
and flags are not compatibility obligations. The [command contract](docs/usage.md)
describes the intended workflows; the implementation ledger identifies what is
available. Useful provider integrations and pack fixtures remain inputs to the rewrite.

## Design and implementation

| Document | Purpose |
| --- | --- |
| [Target design](docs/design/README.md) | Guarantees, domain model, ports and publication lifecycle |
| [Decisions](docs/design/decisions.md) | Accepted policy and verified implementation qualifications |
| [Implementation ledger](docs/design/implementation.md) | Landed work and remaining gates |
| [Feature requirements](docs/design/parity.md) | Intended pack-management capabilities |
| [CLI contract](docs/usage.md) | Target operations and outcomes |
| [Verification](docs/testing.md) | Contract suites and native failure tests |
| [Contributing](CONTRIBUTING.md) | Build and review workflow |

## License

[Apache 2.0](LICENSE)
