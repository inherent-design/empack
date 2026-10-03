# Workflow contracts

Empack keeps its session/provider architecture. This revision adds explicit boundaries between intent, observed state, execution and verification. It does not introduce a scheduler or a transaction database.

## Dependency reconciliation

The manifest records explicit roots. An installed snapshot records provider, project ID, content type, installed version and metadata key. The planner matches identities rather than filenames, retains unlisted installed content, and emits an install when an explicit pin differs. Duplicate identities and conflicting keys fail before execution. Unpinned entries preserve their installed version.

After execution, the live adapter observes the project again and requires the remaining plan to be empty. A backend success exit alone is insufficient. Explicit add pins pass through resolution into persistence. Search aliases do not create an add/remove pair.

Retention is conservative because packwiz metadata does not establish a complete dependency graph. Automatic orphan removal remains unavailable. A future closure model needs provider-qualified edges, provenance and completeness evidence before it may authorize deletion. Provider/project replacement still requires explicit removal.

## Mutation outcomes

Tracked local files use a validated path under `pack/`. Existing destinations must be regular files, and ancestor checks apply before writes, builds and removal. Removing a file record never authorizes recursive directory deletion.

Project locks serialize live empack mutations; atomic replacement protects individual documents. Backend installation and manifest publication remain separate effects. `InstalledButUnrecorded` reports an installation whose manifest update failed, with recovery guidance. Commands return failure rather than counting that dependency as fully added.

A future recovery journal should record operation identity, preconditions, completed effects and pending publication. It should support reconciliation after interruption before promising rollback. This revision does not claim a transaction across packwiz and empack files.

## Content and build freshness

Common imported content belongs in `pack/`. Client and server layers remain in `overrides/client/` and `overrides/server/`. Each distribution stages common content followed by its own layer. Mrpack exports preserve all three environments and their precedence.

Production target planning inserts one fresh mrpack export before lightweight client/server targets. A file left by a previous invocation is not evidence of freshness. Tracked local files under `pack/` use the same export path after SHA-256 and ancestor validation; the CLI no longer rejects this supported packwiz operation. Archive parsing is file-backed and bounds compressed input, manifests, entries and declared extraction size. Import destinations use the same ancestor validation as other confined workflows.

## Restricted download identity

Continuation records bind requests to strong digests from installed metadata where available. Candidate discovery compares content, and cache entries are checked again before staging. A renamed file can satisfy only the identity its bytes establish. Without a supported digest, the user must associate a file explicitly or place it at the printed cache destination; extension and recency are not evidence of identity.

Fingerprints include pack content, side layers and templates. A stale continuation is a read-only classification, including during preview. It preserves recovery evidence and requires a fresh build. Filesystem ancestor checks constrain every saved destination.

## Verification boundary

Planner tests cover identity and pin decisions. Live temporary-filesystem tests cover publication failures, symlink confinement, content-layer round trips and stale exports. CLI smoke tests combine add and repeated sync against a deterministic backend. Strict live tests exercise external tooling separately. These checks do not establish Minecraft runtime compatibility or complete cooperative cancellation of synchronous filesystem work.

The environment precedence follows the [Modrinth format specification](https://support.modrinth.com/en/articles/8802351-modrinth-modpack-format-mrpack): each side layer is applied after common overrides. Installed identities and digest fields come from the [packwiz metadata contract](https://packwiz.infra.link/reference/pack-format/mod-toml/). Empack's automatic restricted-file association accepts SHA digests; files with only MD5 or Murmur2 metadata require explicit selection.
