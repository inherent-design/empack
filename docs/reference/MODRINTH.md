# Modrinth adapter

The engine uses Modrinth's v2 API through `ProviderCatalog`. The official
[API reference](https://docs.modrinth.com/api/) defines the wire protocol; this page
records empack's integration contract rather than duplicating the provider schema.

## Selection and identity

- Search returns bounded, ranked candidates. Ranking is not permission to select a project.
- Slugs, IDs and supported URLs resolve to a canonical Modrinth project identity.
- Exact version lookup verifies project ownership. Compatible selection checks game
  versions, loader, content kind and the chosen release-channel policy.
- Multi-file versions retain named file roles. Companion placement and optional
  participation require explicit choices when they cannot be inferred losslessly.
- Required dependency evidence participates in closure resolution. Missing evidence
  is unknown, not an empty dependency set.

## Integrity and transport

File identification uses the hash endpoint to nominate an exact version, then
checks its owner and original file assertions. Acquisition retains declared hashes
and sizes; computing a new SHA-256 does not replace provider evidence. Source URLs
are credential-free durable locators or execution-only values, as appropriate.

Transport enforces bounded bodies, cumulative deadlines, cancellation, retry and
rate policy. Authentication and upstream failures remain distinct from not-found.
Environment and per-file participation feed normalized build projections.

## Implementation and verification

- [Wire parsing](../../crates/empack-lib/src/engine/providers/modrinth.rs)
- [Transport](../../crates/empack-lib/src/engine/providers/transport.rs)
- [Provider contracts and fixtures](../../crates/empack-lib/src/engine/providers/tests.rs)
- [Live probe instructions](../testing.md)
- [Shared acquisition policy](../design/acquisition.md)
