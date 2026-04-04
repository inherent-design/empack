---
spec: types
status: draft
created: 2026-04-04
updated: 2026-04-04
depends: [overview]
---

# Shared Type Definitions

Types live in `primitives/empack.rs`. Domain types live in their respective `empack/` modules. No parallel type hierarchies; extend existing enums rather than creating new ones.

## ProjectType

4 variants. Covers all content types that have distinct behavior in empack's sync/build pipeline.

| Variant | Modrinth project_type | CurseForge classId(s) |
|---------|----------------------|----------------------|
| Mod | "mod" | 6, 5 (Bukkit Plugins) |
| Datapack | *none* | 17 (Worlds), 6945 (Data Packs) |
| ResourcePack | "resourcepack" | 12 |
| Shader | "shader" | 6552 |

Properties:
- Adding variants requires updating 10+ exhaustive match sites.
- Content types outside these 4 (Customization, Addons) map to `Mod` as the default.
- CurseForge classId 17 is Worlds, not Data Packs. This is a known approximation.

## ProjectPlatform

2 variants: `Modrinth`, `CurseForge`.

Properties:
- CurseForge requires an API key; Modrinth does not (for read-only).
- CurseForge rate limits are undocumented and aggressive on the free tier.

## BuildTarget

5 variants: `Mrpack`, `Client`, `Server`, `ClientFull`, `ServerFull`.

## PackState

4 states: `Uninitialized`, `Initialized`, `Configured`, `Built`.

Transitions defined in `state.rs`. Only forward transitions are allowed except `Built` can transition back on clean.

## ModLoader

4 variants in `parsing.rs`: `Fabric`, `Forge`, `NeoForge`, `Quilt`.

Also exists in `versions.rs` (with `From<parsing::ModLoader>` conversion). The two enums should eventually unify.

Platform-specific ID parsing:
- CurseForge loader IDs: `{type}-{version}` (e.g., `fabric-0.16.0`)
- Modrinth dependency keys: `fabric-loader`, `quilt-loader`, `forge`, `neoforge`

## DependencySource (sync.rs)

Tagged union for dependency origin:

| Variant | Fields | Behavior in sync |
|---------|--------|-----------------|
| Platform | project_id, project_platform, version_pin | Resolved via packwiz add |
| Local | path, hash | Skipped; already on disk |

## Content Types (content.rs)

### UrlKind

Classification of user-supplied URLs. See [platform-modrinth.md](platform-modrinth.md) and [platform-curseforge.md](platform-curseforge.md) for URL patterns.

| Variant | Fields |
|---------|--------|
| ModrinthModpack | slug, version (optional) |
| ModrinthProject | slug |
| CurseForgeModpack | slug |
| CurseForgeProject | slug |
| DirectDownload | url, extension |

### JarIdentity

Result of JAR identification via hash lookup.

| Variant | Fields | Source |
|---------|--------|--------|
| Modrinth | project_id, version_id, title | GET /v2/version_file/{sha1} |
| CurseForge | project_id (u64), file_id (u64), title | POST /v1/fingerprints |
| Unidentified | *none* | Both lookups failed |
