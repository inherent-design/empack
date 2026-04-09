---
spec: platform-modrinth
status: draft
created: 2026-04-04
updated: 2026-04-08
depends: [overview, types, search-and-resolution, import-pipeline]
---

# Modrinth Platform Contracts

API base: `https://api.modrinth.com/v2`

All response field names used by empack are snake_case.

## Endpoints Used

### GET /v2/search

Used by project search and add resolution.

Current query features used by empack:

- `query`
- `facets`
- version and loader filtering through facets
- bounded pagination

Relevant hit fields:

| Field | Meaning |
| --- | --- |
| `project_id` | canonical project ID |
| `slug` | stable slug |
| `title` | display title |
| `project_type` | `mod`, `resourcepack`, `shader`, `datapack` |
| `downloads` | popularity signal used in resolution |
| `categories` | loader and ecosystem metadata |
| `versions` | supported Minecraft versions |

### GET /v2/project/{id|slug}

Used by import manifest resolution.

Relevant response fields:

- `id`
- `slug`
- `title`
- `project_type`

### GET /v2/project/{id|slug}/version

Used by remote modpack download for `init --from`.

Relevant file fields include:

- `id`
- `project_id`
- `name`
- `version_number`
- `files[]`

empack uses version filtering to select the requested or best-match mrpack file.

### GET /v2/version_file/{hash}

Used in two places:

- `ApiJarResolver::query_modrinth()` for downloaded JAR identification
- import resolution when a Modrinth file ID must be backfilled from SHA1

Current request form:

```text
GET /v2/version_file/{sha1}?algorithm=sha1
```

Relevant response fields:

- `project_id`
- top-level `id` as the version ID
- `name`

## mrpack Format

A `.mrpack` file is a zip archive with `modrinth.index.json` at the root.

### modrinth.index.json fields

| Field | Type | Description |
| --- | --- | --- |
| `formatVersion` | integer | Always 1 |
| `game` | string | Always "minecraft" |
| `name` | string | Modpack display name |
| `versionId` | string | Modpack version |
| `summary` | string (optional) | Short description |
| `dependencies` | object | Keys: `minecraft`, `forge`, `neoforge`, `fabric-loader`, `quilt-loader` |
| `files` | array | Content entries (see below) |

### files[] entry fields

| Field | Type | Description |
| --- | --- | --- |
| `path` | string | Destination relative to .minecraft |
| `hashes` | object | Must contain `sha1` and `sha512` |
| `downloads` | array of strings | HTTPS CDN URLs |
| `env` | object (optional) | `{client, server}` each `"required"`, `"optional"`, `"unsupported"` |
| `fileSize` | integer | **camelCase**; file size in bytes |
| `projectId` | string (optional) | **camelCase**; present in some real modpacks, not guaranteed by spec |

### Override directories

Override directory names are hardcoded archive conventions, not JSON fields:

| Directory | Side | Applied |
| --- | --- | --- |
| `overrides/` | Both | First |
| `client-overrides/` | Client only | After overrides/, overwrites |
| `server-overrides/` | Server only | After overrides/, overwrites |

The JSON index does not reference these directories. Parsers must use the convention names as defaults.

## URL Patterns

| URL pattern | Classification |
| --- | --- |
| `modrinth.com/modpack/{slug}` | ModrinthModpack |
| `modrinth.com/modpack/{slug}/version/{version}` | ModrinthModpack with version |
| `modrinth.com/mod/{slug}` | ModrinthProject |
| `modrinth.com/plugin/{slug}` | ModrinthProject |
| `modrinth.com/resourcepack/{slug}` | ModrinthProject |
| `modrinth.com/datapack/{slug}` | ModrinthProject |
| `modrinth.com/shader/{slug}` | ModrinthProject |

## Current empack Implications

- Modrinth is the default first platform for search when no preference is provided.
- `ProjectType::modrinth_facet_name()` includes `datapack` as a first-class facet.
- Import can resolve project IDs, slugs, names, types, and version IDs from live Modrinth metadata.
- JAR identification depends on SHA1 lookup, not URL pattern guessing.
