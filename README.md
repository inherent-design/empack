[![PR CI](https://img.shields.io/github/actions/workflow/status/inherent-design/empack/pr-ci.yml?branch=dev&style=flat)](https://github.com/inherent-design/empack/actions/workflows/pr-ci.yml) [![License](https://img.shields.io/github/license/inherent-design/empack?style=flat)](LICENSE)

# empack

empack manages Minecraft packs across Modrinth, CurseForge and local content,
using packwiz as its backend.

The development target is **v0.5.0-alpha.1**: a typed pack engine that separates
intent, exact resolution, observation, staging, verification and recoverable
publication. The [design](docs/design/README.md) is the target contract; the
[implementation ledger](docs/design/implementation.md) records what has landed.
The full engine is not implemented yet.

## Existing CLI

```bash
empack requirements
empack init my-pack
cd my-pack
empack add sodium
empack build all
```

Import with `empack init --from pack.mrpack my-pack`. Continue a restricted-download
build with `empack build --continue`. The managed backend is resolved when needed;
`EMPACK_PACKWIZ_BIN` selects an external binary.

The [compatibility guide](docs/compatibility/usage-0.4.md) documents current commands,
flags, configuration and prerequisites. Planned update/adopt/migrate requests and
the public `Engine` sketches are not yet available commands or library APIs.

## Design and migration

| Document | Purpose |
| --- | --- |
| [Target design](docs/design/README.md) | Guarantees, domain model, ports and publication lifecycle |
| [Decisions](docs/design/decisions.md) | Accepted policy and verified implementation qualifications |
| [Implementation ledger](docs/design/implementation.md) | Landed work and remaining gates |
| [Feature parity](docs/design/parity.md) | Existing features the refactor must preserve |
| [CLI migration](docs/usage.md) | Compatibility surface and target requests |
| [Verification](docs/testing.md) | Contract suites and native failure tests |
| [Contributing](CONTRIBUTING.md) | Build and review workflow |

## License

[Apache 2.0](LICENSE)
