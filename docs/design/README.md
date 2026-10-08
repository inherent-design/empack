# empack v0.5 target

empack is a typed pack engine with verified, recoverable publication. This design
defines the implementation contract for v0.5.0-alpha.1. The
[delivery ledger](implementation.md) records implemented routes, revision-specific
validation and the limits of that evidence.

The source is the user-supplied `empack.md`, prepared 2026-10-04 against empack
`50c121f` and Playground `dc6846a`. Its SHA-256 is `bfccf5976bdc6848a5516a6c125c9d56f6c15aba1fcf9c355b762c374e444ffe`.
The numbered sections retain that document's vocabulary and requirements. Original
source references remain linked next to their claims. Reviewed clarifications and
policy choices live in [decisions](decisions.md).

| Contract | Scope |
| --- | --- |
| [Architecture](architecture.md) | Source sections 1, 2, 3 |
| [Model](model.md) | Source sections 4, 5, 6 |
| [Planning](planning.md) | Source sections 7, 8 |
| [Acquisition](acquisition.md) | Source sections 9, 10 |
| [Filesystem](filesystem.md) | Source sections 11 |
| [Backend](backend.md) | Source sections 12 |
| [Builds](builds.md) | Source sections 13 |
| [Publication](publication.md) | Source sections 14 |
| [Runtime](runtime.md) | Source sections 15, 16 |
| [Api](api.md) | Source sections 17, 18 |
| [Verification](verification.md) | Source sections 19, 20 |
| [Implementation order](implementation-order.md) | Source sections 21, 22, 23 |

[Implementation status](implementation.md) tracks delivery of the target and the
[feature requirements](parity.md) record useful capabilities. Old specifications
and compatibility guides are removed; Git history retains their evidence.

The engine now includes the semantic core, snapshots, resolution locks, staged
execution, inventory verification and journaled publication. Ordinary CLI commands
use the engine. The legacy project library and executable bootstrap are removed.
The routing table connects preserved capabilities to their acceptance evidence;
dispatch wiring alone does not establish parity. The ledger also records explicit
runtime and publication limits.
