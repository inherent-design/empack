# CurseForge adapter

The engine uses CurseForge for Studios through `ProviderCatalog`. The official
[API reference](https://docs.curseforge.com/rest-api/) defines the wire protocol;
this page records empack's integration contract.

## Selection and identity

- Minecraft requests bind game ID 432. Project and file IDs are distinct typed values.
- Slug lookup requires exactly one matching project. Exact file lookup verifies both
  project ownership and game identity.
- Content classes normalize to mods, resource packs, shaders, datapacks and worlds.
  Provider modpack pages use the separate archive-selection path.
- Compatible selection checks game/loader requirements and release-channel policy.
  Explicit pins remain exact; sync does not silently upgrade them.
- Required dependency evidence participates in closure resolution. Missing dependency
  fields remain unknown evidence.

## Restricted content and integrity

An absent download URL is a manual-acquisition obligation, not an empty file or a
permission to guess a mirror. Continuation binds supplied bytes to the original
selection, declared size and hashes. MD5-only files can use explicit compatibility
policy; an internal SHA-256 does not strengthen that source evidence.

CurseForge fingerprints nominate candidates only. Identification verifies the
candidate's exact file assertions before accepting provider ownership. A fingerprint
collision cannot authorize a match.

Credentials stay in host configuration. Catalog access and the exact HTTPS
`edge.forgecdn.net:443` download origin use their configured credential rules;
redirects to other origins do not inherit the key. Authentication, rate limits and
provider failures remain distinct from not-found. See the shared
[acquisition contract](../design/acquisition.md) for bounds and redirect policy.

## Implementation and verification

- [Wire parsing](../../crates/empack-lib/src/engine/providers/curseforge.rs)
- [Transport](../../crates/empack-lib/src/engine/providers/transport.rs)
- [Identification](../../crates/empack-lib/src/engine/providers/identify.rs)
- [Provider contracts and fixtures](../../crates/empack-lib/src/engine/providers/tests.rs)
- [Live probe and restricted-import instructions](../testing.md)
