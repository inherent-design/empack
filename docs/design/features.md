# Feature contracts

Every supported workflow uses normalized intent, verified effects and recoverable
publication. This table identifies its owner and required behavioral evidence.
Packwiz-directory import is recognized but unsupported; recognition is not a
capability claim.

| Workflow | Owner | Required behavior |
| --- | --- | --- |
| init, forced init, metadata/runtime overrides | Initialize request and publisher | Decline/preview/failure retains original tree |
| Local/remote mrpack and CurseForge ZIP | Import adapter and acquisition | Side layers, requirements, bytes, identity and preflight failure |
| Unsupported packwiz-directory import | Capability report | Recognition must not imply support |
| Provider search, order, exact ID, slug, URL, pin | Selector/catalog and typed resolution | Equivalent identity; pin ownership; resource/shader/world types |
| Direct JAR, typed ZIP, recognized/unidentified local content | Probe and normalized add | Bounded acquisition, provenance, safe placement and repeated sync |
| Mods, resources, shaders, datapacks, worlds | Content kind and placement | Type-specific folders and provider capability errors |
| Aliases, title/stem removal, local files and untracked installed metadata | Shared identity planner | Collision, ambiguous selection, observed-only ownership, wrong-kind and link fixtures |
| Exact synchronization and captured build inputs | Lock, snapshot and planner | Exact selection, preserved transitive content and source conflicts |
| Unpinned installed-version retention | Exact lock and explicit update | Sync does not upgrade content |
| Common/client/server layers and optional files | Requirements and projections | Explicit precedence; reject lossy conversion |
| mrpack, client, server, full variants, all | Build planner and inventory | Fresh shared prerequisites; complete semantic artifacts |
| ZIP, TAR.GZ, 7z and binary templates | Writers/readers and renderer | Byte inventory, escaped scripts and binary copying |
| Loader families and historical accepted variants | Runtime adapters | Existing loader and game-version matrix |
| Datapack-folder inference and backend options | Layout proposal and backend recipe | Inference evidence and explicit override precedence |
| Browser/download scans/manual association/continue | Decisions and acquisition resume | Digest association, stale state remains read-only |
| Clean builds/cache/all | Publisher and cache maintenance | Ownership, leases and recovery data survive unrelated cleanup |
| Runtime assets, external installers and process limits | Verified runtime acquisition and host process port | Original assertions, bounded execution and descendant retirement; ordinary commands need no packwiz executable |
| Workdir, CLI/env/dotenv, interactive/headless, logs/exits | Host adapters | Current smoke fixtures and stable exit classes |
| Default all-requested batches | AllRequested policy | No publication if any requested group fails |
| Explicit independent partial progress | Group planner and partial receipt | Failed groups retain original intent/content |
| Update and adoption | Request planner and codec | Exact selections, observed content verification and command composition |
| Conservative deletion and orphan retention | Managed change plan | Only justified managed changes; incomplete evidence retains content |

The [testing guide](../testing.md) maps verification commands to maintained suites.
Feature changes must preserve these outcomes or explicitly revise the contract.
