# empack design

empack is a typed pack engine with verified, recoverable publication. These contracts
define v0.5.0-alpha.1. Ordinary CLI commands use the native engine; packwiz metadata
is an interchange format, not a requirement to install the packwiz executable.

| Contract | Scope |
| --- | --- |
| [Architecture](architecture.md) | Guarantees, module ownership and limits |
| [Policies](decisions.md) | Batch, integrity, preview and publication rules |
| [Model](model.md) | Identities, intent, exact resolution and documents |
| [Planning](planning.md) | Observations, conflicts, effects and authorization |
| [Acquisition](acquisition.md) | Providers, imports, verified bytes and storage |
| [Filesystem](filesystem.md) | Native roots, confinement and private staging |
| [Metadata and processes](backend.md) | Packwiz wire semantics and process ownership |
| [Builds](builds.md) | Projections, distributions, templates and runtimes |
| [Publication](publication.md) | Expected-old checks, journals and recovery |
| [Runtime](runtime.md) | Admission, cancellation, decisions and continuation |
| [API](api.md) | Engine and host interfaces, operation lifecycles |
| [Verification](verification.md) | Contract suites and failure injection |
| [Features](features.md) | Supported workflows and their required behavior |

The [CLI reference](../usage.md) describes commands and options. The
[testing guide](../testing.md) gives executable checks. Rust API documentation and
compiled examples are generated with `cargo doc --workspace --no-deps`.

Keep behavior and its limits here. Record run-specific evidence in CI and pull
requests, rather than retaining delivery ledgers or implementation journals.
