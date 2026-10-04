---
spec: config-and-manifest
status: partial
created: 2026-04-08
updated: 2026-10-02
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
| `dependencies` | `BTreeMap<String, DependencyEntry>` | Dependency map keyed by user-facing label |
| `minecraft_version` | `Option<String>` | Target Minecraft version |
| `loader` | `Option<ModLoader>` | Target loader |
| `loader_version` | `Option<String>` | Loader version |
| `datapack_folder` | `Option<String>` | Relative datapack install path |
| `acceptable_game_versions` | `Option<Vec<String>>` | Additional acceptable versions for resolution |
| `name` | `Option<String>` | Pack display name |
| `author` | `Option<String>` | Pack author |
| `version` | `Option<String>` | Pack version |

### Dependency map

Dependency keys are user-facing labels. Resolved dependencies are reconciled by provider, project ID and content type, independently of packwiz filenames.

Deserialization dispatches on explicit status and rejects malformed intent. Supported variants are:

- `DependencyEntry::Resolved(DependencyRecord)`
- `DependencyEntry::Local(LocalDependencyRecord)`
- `DependencyEntry::Url(UrlDependencyRecord)`
- `DependencyEntry::Search(DependencySearch)`

Resolved entries are current-state declarations. Search entries are deferred intent that sync resolves before building the `ProjectPlan`.
Local entries are tracked project files that stay outside packwiz metadata. URL entries preserve provider-free remote files with packwiz download metadata. Imported resolved records may carry an `environment` object with client and server requirements; sync reapplies it after a pin change or metadata drift.

### UrlDependencyRecord

A `status: url` record contains `title`, `type`, a pack-relative `destination`, `downloads`, `hashes`, optional byte `size`, and `env.client` / `env.server`. Each environment is `required`, `optional`, `unsupported`, or `unknown`. The source must declare at least one valid SHA-1, SHA-256 or SHA-512 digest. All supported declared digests are verified before initial publication or reconstruction. Unknown fields are rejected.

The metadata file is stored next to the declared destination as `<filename>.pw.toml`. The download URL's basename does not select the installed filename. Sync validates existing metadata against the record and can restore missing metadata after verifying the source again. Divergent metadata fails validation. Removal deletes that validated metadata file and refreshes the index before removing intent.

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
- Dry-run sync builds the operational plan from in-memory resolved intent without persisting search resolutions.
- Search resolution and planning failures make sync fail, even when other actions succeed. The first resolution error remains in the error chain for exit-code classification.
- Local entries enter the operational `ProjectPlan` directly without packwiz resolution.
- Resolved entries match an installed snapshot by provider, project ID and content type. Manifest keys are labels and may differ from installed filenames. Duplicate identities and conflicting installed keys fail before mutation. Explicit pin drift schedules a reinstall for the same identity; unpinned entries keep the installed version. Provider/project replacement still requires explicit removal.
- Sync preserves unlisted installations because root intent does not describe required dependency edges. Deleting a manifest entry does not authorize deleting its installation. Use explicit `remove` for removal.
- Successful backend execution is followed by a fresh installed-state check. Missing identities or unsatisfied pins fail the command even when the subprocess returned success.
- `clean` preserves `empack.yml` and `pack/`.
- Destructive reset of `empack.yml` and `pack/` is an explicit init rollback / `init --force` path, not a normal state-machine clean transition.

## Initialization Preview

Forced initialization defers the existing-project reset until input resolution and validation succeed, the user confirms ordinary initialization, and dry-run has returned. Declining ordinary initialization or previewing either initialization path preserves the project tree. Execution after that point remains a destructive reset; it does not provide rollback if replacement initialization fails.

Dependency entries with `status: resolved` or `status: local` must satisfy that record schema. Unknown fields and invalid status values are errors. Search entries omit `status` and accept only `title`, `type`, and `platform`; a malformed explicit record never falls back to a search.

Sync resolves search entries in memory in both execution and preview modes. Persistence starts after planning and installed-intent validation. Successful planning does not provide a transaction across packwiz and the manifest; provider/project replacement remains unsupported; pin changes for an unchanged identity are reconciled.

Explicit `add --version-id` and `add --file-id` requests retain their pin in the
resolved record. Unpinned requests remain unpinned. Backend installation and
manifest publication are separate steps: if publication fails, add and import
return an incomplete-operation error naming the installed dependency and the
manifest that needs repair. They do not report full success or imply rollback.

World archives use `type: world` and live under `pack/saves/`. CurseForge class 17 identifies worlds; datapacks use class 6945. Import preserves the downloaded world archive without automatically extracting it. Direct local ZIP additions also accept `--type world`.
