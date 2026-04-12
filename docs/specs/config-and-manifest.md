---
spec: config-and-manifest
status: partial
created: 2026-04-08
updated: 2026-04-11
depends: [overview, types]
---

# Config and Manifest

empack keeps user intent in `empack.yml` and packwiz runtime state in `pack/pack.toml`.

## File Roles

| File | Role |
| --- | --- |
| `empack.yml` | empack-owned project intent, dependency declarations, optional metadata overrides |
| `pack/pack.toml` | packwiz-owned pack metadata and options |
| `pack/*.pw.toml` | installed dependency records managed by packwiz |

## empack.yml Shape

The top-level shape is:

```yaml
empack:
  dependencies: {}
  minecraft_version: "1.21.1"
  loader: fabric
  loader_version: "0.16.0"
  datapack_folder: "datapacks"
  acceptable_game_versions:
    - "1.21"
    - "1.21.1"
  name: "Example Pack"
  author: "Example Author"
  version: "1.0.0"
```

### Project fields

| Field | Type | Meaning |
| --- | --- | --- |
| `dependencies` | `BTreeMap<String, DependencyEntry>` | Dependency map keyed by slug |
| `minecraft_version` | `Option<String>` | Target Minecraft version |
| `loader` | `Option<ModLoader>` | Target loader |
| `loader_version` | `Option<String>` | Loader version |
| `datapack_folder` | `Option<String>` | Relative datapack install path |
| `acceptable_game_versions` | `Option<Vec<String>>` | Additional acceptable versions for resolution |
| `name` | `Option<String>` | Pack display name |
| `author` | `Option<String>` | Pack author |
| `version` | `Option<String>` | Pack version |

### Dependency map

Each dependency key is the canonical slug used by empack and packwiz file naming.

The value is an untagged union:

- `DependencyEntry::Resolved(DependencyRecord)`
- `DependencyEntry::Local(LocalDependencyRecord)`
- `DependencyEntry::Search(DependencySearch)`

Resolved entries are current-state declarations. Search entries are deferred intent that sync resolves before building the `ProjectPlan`.
Local entries are first-class tracked project files that stay outside packwiz metadata.

### LocalDependencyRecord

Tracked local content uses this shape:

```yaml
example-pack:
  status: local
  title: Example Pack
  type: resourcepack
  path: pack/resourcepacks/example-pack.zip
  source_url: https://example.com/example-pack.zip
  sha256: <hex>
```

Current field rules:

- `status` is always `local`
- `path` is always project-relative
- `sha256` is required for URL-downloaded local content
- `source_url` is optional metadata for provenance

## pack.toml Fallback Rules

`ConfigManager::create_project_plan()` loads `empack.yml`, then optionally loads `pack/pack.toml`.

Fallback order:

| Field | Preferred source | Fallback source |
| --- | --- | --- |
| `name` | `empack.yml` | `pack.toml` |
| `author` | `empack.yml` | `pack.toml` |
| `version` | `empack.yml` | `pack.toml` |
| `minecraft_version` | `empack.yml` | `pack.toml [versions.minecraft]` |
| `loader` | `empack.yml` | inferred from `pack.toml [versions]` keys |
| `loader_version` | `empack.yml` | loader-specific key in `pack.toml [versions]` |

Loader inference currently checks `fabric`, `forge`, `quilt`, and `neoforge` keys.

## pack.toml Options Wiring

`write_pack_toml_options()` merges empack-owned options into the `[options]` table in `pack.toml`.

| empack field | pack.toml field |
| --- | --- |
| `datapack_folder` | `options.datapack-folder` |
| `acceptable_game_versions` | `options.acceptable-game-versions` |

Current behavior:

- If both values are absent, the function does nothing.
- If `[options]` does not exist, it is created.
- The file is parsed and re-serialized through the `toml` crate.
- Existing comments and formatting are not preserved.

## ProjectPlan

`ProjectPlan` is the resolved operational view used by sync and add workflows.

Fields:

| Field | Meaning |
| --- | --- |
| `name`, `author`, `version` | Effective metadata after fallback |
| `minecraft_version`, `loader`, `loader_version` | Effective runtime target |
| `dependencies` | Operational `ProjectSpec` records for resolved and tracked local dependencies |

Each `ProjectSpec` carries the dependency key, search query, type, version target, optional loader, and a `DependencySource`:

- `Platform { project_id, project_platform, version_pin }`
- `Local { path, source_url, sha256 }`

## Consistency Checks

`validate_consistency()` compares `empack.yml` against `pack.toml` when `pack.toml` exists.

Current checks cover:

- Minecraft version mismatch
- Loader mismatch

This validation produces warnings for refresh and sync paths. It does not silently rewrite either file.

## Sync Invariants

Current sync rules depend on the config model:

- `empack.yml` is the source of dependency intent.
- `pack.toml` is the source of packwiz pack metadata.
- Search entries must resolve to canonical platform records before they enter the operational `ProjectPlan`.
- Local entries enter the operational `ProjectPlan` directly without packwiz resolution.
- Resolved dependency keys are compared against installed `.pw.toml` filename stems.
- `clean` preserves `empack.yml` and `pack/`.
- Destructive reset of `empack.yml` and `pack/` is an explicit init rollback / `init --force` path, not a normal state-machine clean transition.
