# v0.6.0-beta design

These documents define empack's authoring, distribution and installed-instance
contracts for v0.6.0-beta. Start with [usage](../usage.md) for commands and workflows.
Types in design examples name semantic boundaries; generated Rust API documentation
defines callable signatures.

| Contract | Contents |
| --- | --- |
| [Decisions](decisions.md) | Product scope and defaults |
| [Features](features.md) | Preserved capabilities and consumer obligations |
| [Architecture](architecture.md) | Crates, ports and authority |
| [Model](model.md) | Identities, authoring documents, release and instance values |
| [Planning](planning.md) | Read sets, grants and postconditions |
| [Instances](instances.md) | Three-way reconciliation, choices and launch eligibility |
| [Releases](releases.md) | Distribution, signatures and channels |
| [Consumers](builds.md) | Format projections and runtime recipes |
| [API](api.md) | Requests, outcomes, decisions and host wiring |
| [Acquisition](acquisition.md) | Provider resolution, imports, downloads and leases |
| [Filesystem](filesystem.md) | Native roots, path safety and private staging |
| [Publication](publication.md) | Durable application, recovery and retention |
| [Runtime](runtime.md) | Async ownership, cancellation and admission |
| [Verification](verification.md) | Evidence and conformance requirements |

Implementation tasks are confined to [TODO.md](../../TODO.md). Design pages contain
contracts, not progress records or migration ledgers.
