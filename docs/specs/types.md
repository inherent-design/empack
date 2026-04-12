---
spec: types
status: ratified
created: 2026-04-04
updated: 2026-04-11
depends: [overview]
---

# Shared Type Definitions

Shared runtime types live in `primitives/empack.rs` and `primitives/project_platform.rs`. Domain-specific types stay in their owning modules under `empack/`.

## Project Platform

`ProjectPlatform` identifies the remote host used for search, resolution, and packwiz add commands.

| Variant | Display value | API key required |
| --- | --- | --- |
| `Modrinth` | `modrinth` | No |
| `CurseForge` | `curseforge` | Yes |

The helper methods on `ProjectPlatform` expose API base URLs and conservative platform defaults. Runtime pacing is documented in [networking-and-rate-budgets.md](networking-and-rate-budgets.md).

## Project Type

`ProjectType` drives search facets, destination folders, and packwiz add routing.

| Variant | Modrinth facet | `uses_loader_facet()` | `curseforge_class_id()` |
| --- | --- | --- | --- |
| `Mod` | `mod` | `true` | `6` |
| `Datapack` | `datapack` | `false` | `17` |
| `ResourcePack` | `resourcepack` | `false` | `12` |
| `Shader` | `shader` | `false` | `6` |

`curseforge_class_id()` is a current helper, not a full taxonomy. Import resolution can still classify live CurseForge responses as `6945` for datapacks or `6552` for shaders when those class IDs are returned by the API.

## Build Target

`BuildTarget` models concrete output targets.

| Variant | Display value | Purpose |
| --- | --- | --- |
| `Mrpack` | `mrpack` | Modrinth-compatible pack archive |
| `Client` | `client` | Bootstrapped client distribution |
| `Server` | `server` | Bootstrapped server distribution |
| `ClientFull` | `client-full` | Non-redistributable client package with embedded content |
| `ServerFull` | `server-full` | Non-redistributable server package with embedded content |

User-facing `all` expansion happens in `application/commands.rs` and resolves to all five targets. `BuildTarget::expand_all()` remains a narrower helper that returns only `mrpack`, `client`, and `server`.

## Pack State

`PackState` describes filesystem-observed lifecycle state.

| Variant | Meaning |
| --- | --- |
| `Uninitialized` | No usable empack project state exists |
| `Configured` | Both `empack.yml` and `pack/pack.toml` exist |
| `Built` | Canonical artifacts exist under `dist/` |
| `Building` | Internal in-progress marker state |
| `Cleaning` | Internal in-progress marker state |
| `Interrupted { was }` | A previous building or cleaning operation left a marker behind |

Transition identity types live beside `PackState`:

- `TransitionKind`: `Initialize`, `RefreshIndex`, `Build`, `Clean`
- `MarkerKind`: `Building`, `Cleaning`
- `InitializationConfig`: `name`, `author`, `version`, `modloader`, `mc_version`, `loader_version`

See [state-machine.md](state-machine.md) for transition legality and discovery rules.

Current cleanup semantics:

- `TransitionKind::Clean` is non-destructive and idempotent.
- `clean` removes build artifacts and marker state when present.
- `clean` does not remove `empack.yml` or `pack/`.

## Mod Loader

`ModLoader` lives in `empack/parsing.rs`.

| Variant | Serialized value |
| --- | --- |
| `NeoForge` | `neoforge` |
| `Fabric` | `fabric` |
| `Quilt` | `quilt` |
| `Forge` | `forge` |

`ModLoader::parse_from_platform_id()` accepts both pack-style and platform-style identifiers such as `fabric-loader`, `quilt-loader`, and `fabric-0.16.0`.

## Dependency Entry Union

`empack.yml` dependencies are an untagged union keyed by slug.

### Resolved entry

`DependencyRecord` fields:

| Field | Type | Meaning |
| --- | --- | --- |
| `status` | `DependencyStatus` | Always `resolved` today |
| `title` | `String` | Human-readable title |
| `platform` | `ProjectPlatform` | Canonical source platform |
| `project_id` | `String` | Canonical project identifier |
| `type` | `ProjectType` | Content type, default `mod` |
| `version` | `Option<String>` | Optional pinned Modrinth version ID or CurseForge file ID |

### Search entry

`DependencySearch` fields:

| Field | Type | Meaning |
| --- | --- | --- |
| `title` | `String` | Search query to resolve on sync |
| `type` | `Option<ProjectType>` | Optional type filter |
| `platform` | `Option<ProjectPlatform>` | Optional preferred platform |

### Local entry

`LocalDependencyRecord` fields:

| Field | Type | Meaning |
| --- | --- | --- |
| `status` | `DependencyStatus` | Always `local` |
| `title` | `String` | Human-readable title |
| `type` | `ProjectType` | Content type |
| `path` | `String` | Project-relative destination path |
| `source_url` | `Option<String>` | Optional provenance URL |
| `sha256` | `String` | Required integrity hash |

`ConfigManager::create_project_plan()` includes resolved and local entries in the operational `ProjectPlan`. Search entries remain declarative intent until sync or add resolves them.

## Content and Import Types

### URL classification

`UrlKind` classifies user input before add or import handling.

| Variant | Fields |
| --- | --- |
| `ModrinthModpack` | `slug`, optional `version` |
| `ModrinthProject` | `slug` |
| `CurseForgeModpack` | `slug` |
| `CurseForgeProject` | `slug` |
| `DirectDownload` | `url`, `extension` |

### JAR identity

`JarIdentity` is the result of hash-based lookup for a downloaded JAR.

| Variant | Fields |
| --- | --- |
| `Modrinth` | `project_id`, `version_id`, `title` |
| `CurseForge` | `project_id`, `file_id`, `title` |
| `Unidentified` | *none* |

### Side and override metadata

Import flow types in `empack/content.rs` and `empack/import.rs` carry side and override metadata:

- `SideRequirement`: required, optional, unsupported
- `SideEnv`: client and server side requirements
- `OverrideSide`: both, client only, server only
- `OverrideCategory`: config, script, resource pack, shader pack, data pack, world, server config, client config, mod data, other
