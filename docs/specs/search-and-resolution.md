---
spec: search-and-resolution
status: partial
created: 2026-04-08
updated: 2026-04-11
depends: [overview, types]
---

# Search and Resolution

empack resolves add and sync inputs through URL classification, platform preference handling, tiered type search, and hash-based file identification.

## Search Inputs

`ProjectResolver::resolve_project()` takes:

| Input | Meaning |
| --- | --- |
| `title` | Search query |
| `project_type` | Optional explicit type |
| `minecraft_version` | Optional version filter |
| `mod_loader` | Optional loader filter |
| `preferred_platform` | Optional platform preference |

The result is `ProjectInfo` with `platform`, `project_id`, `title`, `downloads`, `confidence`, and `project_type`.

## Platform Order

Platform preference affects ordering, not schema shape.

| Preference | Current search order |
| --- | --- |
| `None` | Modrinth first, then CurseForge |
| `Modrinth` | Modrinth first |
| `CurseForge` | CurseForge first |

CLI `--platform both` maps to `None`, which preserves the default Modrinth-first order.

## Type-Aware Search

If the caller provides a type, the resolver searches only that type.

If the caller omits a type, the resolver tries these tiers in order:

1. `mod`
2. `resourcepack`
3. `shader`
4. `datapack`

This tiered search comes from the current search redesign and is the live behavior on 2026-04-08.

## URL Classification

`classify_url()` recognizes these inputs:

| Variant | Examples |
| --- | --- |
| `ModrinthModpack` | `modrinth.com/modpack/<slug>` and version URLs |
| `ModrinthProject` | `modrinth.com/mod`, `plugin`, `resourcepack`, `datapack`, `shader` |
| `CurseForgeModpack` | `curseforge.com/minecraft/modpacks/<slug>` |
| `CurseForgeProject` | `curseforge.com/minecraft/mc-mods/<slug>` |
| `DirectDownload` | direct URLs with a filename extension |

Current command behavior built on top of `UrlKind`:

- Modrinth project URLs become direct Modrinth project IDs.
- CurseForge project URLs resolve by slug through the CurseForge API.
- Direct `.jar` URLs download and attempt JAR identification.
- Identified `.jar` URLs become resolved platform dependencies.
- Unidentified `.jar` URLs become tracked local mod dependencies.
- Direct `.zip` URLs are accepted by `add` only when paired with `--type resourcepack`, `shader`, or `datapack`.
- Direct non-`.zip` non-`.jar` URLs are classified as direct downloads and then rejected explicitly by `add`.

## Search Contracts

`resolve_add_contract()` is the shared path for add and sync resolution.

It resolves one of two cases:

| Case | Behavior |
| --- | --- |
| Direct platform record | Use provided project ID and optional version pin |
| Search path | Call `ProjectResolver`, then build packwiz add commands from the result |

Packwiz add command planning is platform-specific:

| Platform | Base command |
| --- | --- |
| Modrinth | `packwiz-tx modrinth add --project-id <ID>` |
| CurseForge | `packwiz-tx curseforge add --addon-id <ID>` |

Optional version pins map to `--version-id` for Modrinth and `--file-id` for CurseForge.

## JAR Identification

`ApiJarResolver` identifies local JARs before add or import fallback handling.

Resolution order:

1. Modrinth by SHA1 through `GET /v2/version_file/{sha1}?algorithm=sha1`
2. CurseForge by Murmur2 fingerprint through `POST /v1/fingerprints`
3. `Unidentified`

Current fingerprint details:

- Modrinth lookup uses SHA1 only.
- CurseForge lookup strips whitespace bytes before hashing.
- CurseForge lookup needs an API key.

## Local Direct-Download Outcomes

When direct downloads do not resolve to a platform record, the add path writes tracked local content into the project tree:

| Type | Destination |
| --- | --- |
| `mod` | `pack/mods/` |
| `resourcepack` | `pack/resourcepacks/` |
| `shader` | `pack/shaderpacks/` |
| `datapack` | `pack/<datapack_folder>/` |

If a datapack direct download is the first datapack content in the project and `datapack_folder` is unset, empack initializes it to `datapacks` in both `empack.yml` and `pack.toml [options]`.

## Error Shape

Search and resolution can fail with:

- no results
- low confidence
- incompatible loader or version support
- missing API key
- request or JSON errors

These failures surface as command errors. They are not converted into speculative fallback records.
