# Feature requirements

The target retains useful pack-management capabilities, not old spellings,
formats or broken semantics. Existing fixtures provide examples to verify where
applicable. Target-only requests become available when their implementations land.

| Baseline feature | Disposition | Target owner | Evidence to preserve |
| --- | --- | --- | --- |
| init, forced init, metadata/runtime overrides | Preserve | Initialize request and publisher | Decline/preview/failure retains original tree |
| Local/remote mrpack and CurseForge ZIP | Preserve | Import adapter and acquisition | Side layers, requirements, bytes, identity and preflight failure |
| Detected packwiz-directory import | Explicitly unsupported today | Capability report | Recognition must not imply support |
| Provider search, order, exact ID, slug, URL, pin | Preserve | Selector/catalog and typed resolution | Equivalent identity; pin ownership; resource/shader/world types |
| Direct JAR, typed ZIP, recognized/unidentified local content | Preserve | Probe and normalized add | Bounded acquisition, provenance, safe placement and repeated sync |
| Mods, resources, shaders, datapacks, worlds | Preserve | Content kind and placement | Type-specific folders and provider capability errors |
| Aliases, title/stem removal, local file removal | Preserve | Shared identity planner | Collision, ambiguous selection, wrong-kind and link fixtures |
| Membership-only sync and mutable build inputs | Bug to remove | Lock, snapshot and planner | Exact selection, preserved transitive content and source conflicts |
| Unpinned installed-version retention | Preserve | Exact lock and explicit update | Sync does not upgrade content |
| Common/client/server layers and optional files | Preserve | Requirements and projections | Explicit precedence; reject lossy conversion |
| mrpack, client, server, full variants, all | Preserve | Build planner and inventory | Fresh shared prerequisites; complete semantic artifacts |
| ZIP, TAR.GZ, 7z and binary templates | Preserve | Writers/readers and renderer | Byte inventory, escaped scripts and binary copying |
| Loader families and historical accepted variants | Preserve | Runtime adapters | Existing loader and game-version matrix |
| Datapack-folder inference and backend options | Preserve | Layout proposal and backend recipe | Inference evidence and explicit override precedence |
| Browser/download scans/manual association/continue | Preserve | Decisions and acquisition resume | Digest association, stale state remains read-only |
| Clean builds/cache/all | Preserve with scoped plans | Publisher and cache maintenance | Ownership, leases and recovery data survive unrelated cleanup |
| Managed/external tooling, process limits | Preserve | Tool resolver and process port | Provenance, bounded probes, descendant retirement |
| Workdir, CLI/env/dotenv, interactive/headless, logs/exits | Preserve | Host adapters | Current smoke fixtures and stable exit classes |
| Default best-effort batch publication | Replace | AllRequested policy | No publication if any requested group fails |
| Explicit independent partial progress | Preserve behind explicit policy | Group planner and partial receipt | Failed groups retain original intent/content |
| Update/adopt engine requests | New target | Request planner and codec | No CLI support claim before implementation |
| Arbitrary tree deletion and automatic orphan inference | Bug to remove / remain refused | Managed change plan | Only justified managed changes; incomplete evidence retains content |

Each implemented feature must name its contract tests in the implementation ledger.
CLI and document formats may change directly. Remove a feature only through an
explicit design decision, not accidentally while replacing an implementation.
