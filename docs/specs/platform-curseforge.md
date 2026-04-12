---
spec: platform-curseforge
status: partial
created: 2026-04-04
updated: 2026-04-11
depends: [overview, types, search-and-resolution, import-pipeline]
---

# CurseForge Platform Contracts

API base: `https://api.curseforge.com`

All response field names are camelCase. All endpoints require `x-api-key` header. All responses wrap in `{"data": ...}`. List endpoints add a `pagination` object.

## Class Taxonomy (Minecraft, gameId 432)

Relevant live class IDs used by empack:

| classId | Name | URL slug |
| --- | --- | --- |
| 5 | Bukkit Plugins | bukkit-plugins |
| 6 | Mods | mc-mods |
| 12 | Resource Packs | texture-packs |
| 17 | Worlds | worlds |
| 4471 | Modpacks | modpacks |
| 4546 | Customization | customization |
| 4559 | Addons | mc-addons |
| 6552 | Shaders | shaders |
| 6945 | Data Packs | data-packs |

empack ProjectType mapping:

| classId | ProjectType | Notes |
| --- | --- | --- |
| 6 | Mod | |
| 5 | Mod | Bukkit Plugins; no Plugin variant |
| 12 | ResourcePack | |
| 17 | Datapack | Helper fallback for worlds and some datapack-like content |
| 6552 | Shader | Observed during live metadata resolution |
| 6945 | Datapack | Data pack class used by import classification |

Current helper caveats:

- `ProjectType::Datapack` still maps back to class ID `17`.
- `ProjectType::Shader` still maps back to class ID `6`.
- import resolution can still recover `6945` and `6552` from live API responses.

## Endpoints Used

### GET /v1/mods/{modId}

Used by import manifest resolution.

Response: `{"data": {mod object}}`.

Relevant fields:

- `id`
- `name`
- `slug`
- `classId`

### GET /v1/mods/search

Used by search and direct slug resolution.

Required parameter: `gameId` (integer, 432 for Minecraft).

Optional: `classId`, `searchFilter`, `slug`, `pageSize` (max 50), `modLoaderType` (0=Any, 1=Forge, 4=Fabric, 5=Quilt, 6=NeoForge).

Current empack usage:

- direct slug lookup for CurseForge project URLs
- search with type and loader hints

### POST /v1/mods/files

Used by import resolution to map CurseForge CDN file IDs back to project IDs in batches.

### POST /v1/fingerprints

Match files by Murmur2 fingerprint. Used by `ApiJarResolver::query_curseforge`.

Request body: `{"fingerprints": [int64]}`.

Response: `{"data": {"exactMatches": [...], "exactFingerprints": [...], "installedFingerprints": [...], "unmatchedFingerprints": [...]}}`.

Each exact match: `{"id": <mod_id>, "file": {file object}, "latestFiles": [...]}`.

Properties:
- `exactMatches[].id` is the **mod ID** (project ID), not the fingerprint.
- `exactMatches[].file.id` is the **file ID**.
- `exactMatches[].file.modId` echoes the mod ID.
- `exactMatches[].file.displayName` is the human-readable file name.
- Murmur2 hash must be computed with seed 1 after stripping whitespace bytes (0x09 TAB, 0x0A LF, 0x0D CR, 0x20 SPACE).

## Manifest Format (modpack zip)

`manifest.json` at the archive root.

| Field | Type | Description |
| --- | --- | --- |
| `minecraft.version` | string | Minecraft version |
| `minecraft.modLoaders[]` | array | `{id: "forge-47.2.0", primary: true}` |
| `manifestType` | string | Always "minecraftModpack" |
| `manifestVersion` | integer | Always 1 |
| `name` | string | Modpack name |
| `version` | string | Modpack version |
| `author` | string | Modpack author |
| `files[]` | array | `{projectID: int, fileID: int, required: bool}` |
| `overrides` | string | Override directory name (default "overrides") |

Properties:
- `overrides` is a real manifest field (unlike Modrinth where it is an archive convention).
- Loader ID format is `{type}-{version}` (e.g., `fabric-0.16.0`). Split on first `-` to extract type and version.
- `files[].projectID` and `files[].fileID` use uppercase `ID` suffix (unlike the API which uses camelCase `modId`, `id`).

## Restricted Download Implications

CurseForge restricted download handling is build-time behavior, not import-time manifest behavior.

Current build path:

- packwiz-installer reports restricted files in stderr
- empack parses that output into `RestrictedModInfo`
- the user downloads files manually
- empack can scan `--downloads-dir` or `~/Downloads` and retry the build
