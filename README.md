[![PR CI](https://img.shields.io/github/actions/workflow/status/inherent-design/empack/pr-ci.yml?branch=dev&style=flat)](https://github.com/inherent-design/empack/actions/workflows/pr-ci.yml) [![License](https://img.shields.io/github/license/inherent-design/empack?style=flat)](LICENSE)

# empack

empack manages Minecraft packs across Modrinth, CurseForge and local content.
Its native engine reads and writes packwiz metadata and produces verified distributions;
the CLI does not require the packwiz-tx executable.

The development target is **v0.5.0-alpha.1**: a typed pack engine that separates
intent, exact resolution, observation, staging, verification and recoverable
publication. The [design](docs/design/README.md) defines its contracts and
[operational limits](docs/design/architecture.md#operational-limits).

## Development status

The package remains an alpha. The
[command contract](docs/usage.md) describes available workflows; the
[verification guide](docs/testing.md) explains how their behavior is tested.

## Design and implementation

| Document | Purpose |
| --- | --- |
| [Target design](docs/design/README.md) | Guarantees, domain model, ports and publication lifecycle |
| [Decisions](docs/design/decisions.md) | Accepted policy and verified implementation qualifications |
| [Feature requirements](docs/design/features.md) | Preserved pack-management capabilities |
| [CLI contract](docs/usage.md) | Available operations and outcomes |
| [Verification](docs/testing.md) | Contract suites and native failure tests |
| [Contributing](CONTRIBUTING.md) | Build and review workflow |

## License

[Apache 2.0](LICENSE)
