---
spec: platform-modrinth
status: draft
created: 2026-04-04
updated: 2026-04-04
depends: [overview]
---

# Modrinth Platform Contracts

API base: `https://api.modrinth.com/v2`

All field names are snake_case. Authentication is optional for read-only endpoints. A `User-Agent` header is effectively required.

## Endpoints Used

### GET /v2/search

Search for projects. Used by `empack add <query>`.

Query parameters: `query` (string), `facets` (JSON-encoded 2D array), `index` (sort), `offset`, `limit` (1 to 100, default 10).

Facets format: `[["project_type:mod"],["versions:1.20.1"]]`. Outer array is AND; inner arrays are OR.

Response fields on each hit: `project_id` (not `id`), `slug`, `title`, `project_type`, `downloads`, `author` (string, not `author_id`).

### GET /v2/project/{id|slug}

Get project metadata. Used by `resolve_modrinth_project` during import.

Key response fields: `id` (project ID), `title`, `project_type` ("mod", "modpack", "resourcepack", "shader").

### GET /v2/project/{id|slug}/version

List versions. Used by `download_modrinth_modpack` for init --from.

Query parameters: `loaders` (JSON array), `game_versions` (JSON array), `featured` (boolean).

Returns an array of Version objects. Each version has: `id` (the version's own ID), `project_id`, `name`, `version_number`, `files[]`.

Each file has: `hashes` (object with `sha1`, `sha512` keys), `url`, `filename`, `primary` (boolean), `size`.

### GET /v2/version_file/{hash}

Look up a version by file hash. Used by `ApiJarResolver::query_modrinth`.

Query parameter: `algorithm` (default `sha1`; accepts `sha1`, `sha512`).

Response is a Version object. The version's own ID is the `id` field at top level. The field name `version_id` only appears inside `dependencies[]` objects.

Properties:
- Omitting `?algorithm=sha1` when sending a 40-char hex SHA-1 may work (backend infers from length) but the parameter should be explicit.
- SHA-256 is not supported.
- Non-matching hashes return 404.

## mrpack Format

The `.mrpack` file is a zip archive. The manifest is `modrinth.index.json` at the archive root.

### modrinth.index.json fields

| Field | Type | Description |
|-------|------|-------------|
| `formatVersion` | integer | Always 1 |
| `game` | string | Always "minecraft" |
| `name` | string | Modpack display name |
| `versionId` | string | Modpack version |
| `summary` | string (optional) | Short description |
| `dependencies` | object | Keys: `minecraft`, `forge`, `neoforge`, `fabric-loader`, `quilt-loader` |
| `files` | array | Content entries (see below) |

### files[] entry fields

| Field | Type | Description |
|-------|------|-------------|
| `path` | string | Destination relative to .minecraft |
| `hashes` | object | Must contain `sha1` and `sha512` |
| `downloads` | array of strings | HTTPS CDN URLs |
| `env` | object (optional) | `{client, server}` each `"required"`, `"optional"`, `"unsupported"` |
| `fileSize` | integer | **camelCase**; file size in bytes |
| `projectId` | string (optional) | **camelCase**; present in some real modpacks, not guaranteed by spec |

### Override directories

Override directory names are hardcoded archive conventions, not JSON fields:

| Directory | Side | Applied |
|-----------|------|---------|
| `overrides/` | Both | First |
| `client-overrides/` | Client only | After overrides/, overwrites |
| `server-overrides/` | Server only | After overrides/, overwrites |

The JSON index does not reference these directories. Parsers must use the convention names as defaults.

## URL Patterns

| URL pattern | Classification |
|-------------|---------------|
| `modrinth.com/modpack/{slug}` | ModrinthModpack |
| `modrinth.com/modpack/{slug}/version/{version}` | ModrinthModpack with version |
| `modrinth.com/mod/{slug}` | ModrinthProject |
| `modrinth.com/plugin/{slug}` | ModrinthProject |
| `modrinth.com/resourcepack/{slug}` | ModrinthProject |
| `modrinth.com/datapack/{slug}` | ModrinthProject |
| `modrinth.com/shader/{slug}` | ModrinthProject |
