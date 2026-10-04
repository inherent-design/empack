# Workflow contracts

Empack keeps its session/provider architecture. This revision adds explicit boundaries between intent, observed state, execution and verification. It does not introduce a scheduler or a transaction database.

## Dependency reconciliation

The manifest records explicit roots. An installed snapshot records provider, project ID, content type, installed version and metadata key. The planner matches identities rather than filenames, retains unlisted installed content, and emits an install when an explicit pin differs. Duplicate identities and conflicting keys fail before execution. Unpinned entries preserve their installed version.

After execution, the live adapter observes the project again and requires the remaining plan to be empty. A backend success exit alone is insufficient. Explicit add pins pass through resolution into persistence. Search aliases do not create an add/remove pair.

Direct additions distinguish a `ProjectSelector` from a canonical provider identity. The resolver uses the provider's exact project endpoint, discovers the content type, and checks that an explicit version belongs to that project. Slugs and URLs never become persisted project IDs. After installation, the live adapter requires exactly one matching identity and requested pin, then uses that installation's metadata key for publication. A backend success without matching metadata fails the command. Sync uses already recorded canonical identities without repeating selector lookup. CurseForge shaders and world archives receive explicit backend folders. World archives have their own `world` type under `pack/saves/`; they are not datapacks and are not automatically expanded into playable worlds. Datapacks use CurseForge class 6945, shaders 6552 and worlds 17.

The lookup contract follows the provider's [project](https://docs.modrinth.com/api/operations/getproject/) and [version](https://docs.modrinth.com/api/operations/getversion/) endpoints. Search remains a separate operation with its existing matching rules.

Removal has its own read-only plan. An exact manifest key selects that logical record; otherwise a unique title or installed filename can select it. Provider identities connect intent to the observed metadata filename and version. Ambiguous identities and unknown selectors fail before any removal begins. Local paths are validated across the whole batch. Execution checks that the observed target has not changed, removes that filename, verifies its absence, then publishes removal of the exact manifest record. An already absent installation permits intent cleanup without deleting an unrelated same-name file. Backend success without removal retains the manifest and fails the command.

Retention is conservative because packwiz metadata does not establish a complete dependency graph. Automatic orphan removal remains unavailable. A future closure model needs provider-qualified edges, provenance and completeness evidence before it may authorize deletion. Provider/project replacement still requires explicit removal.

## Mutation outcomes

Tracked local files use a validated path under `pack/`. Existing destinations must be regular files, and ancestor checks apply before writes, builds and removal. Removing a file record never authorizes recursive directory deletion.

Project locks serialize live empack mutations; atomic replacement protects individual documents. Backend installation and manifest publication remain separate effects. `InstalledButUnrecorded` reports an installation whose manifest update failed, with recovery guidance. Commands return failure rather than counting that dependency as fully added.

A future recovery journal should record operation identity, preconditions, completed effects and pending publication. It should support reconciliation after interruption before promising rollback. This revision does not claim a transaction across packwiz and empack files.

## Content and build freshness

Common imported content belongs in `pack/`. Client and server layers remain in `overrides/client/` and `overrides/server/`. Each distribution stages common content followed by its own layer. Mrpack exports preserve all three environments and their precedence.

Production target planning inserts one fresh mrpack export before lightweight client/server targets. A file left by a previous invocation is not evidence of freshness. Tracked local files under `pack/` use the same export path after SHA-256 and ancestor validation; the CLI no longer rejects this supported packwiz operation. Archive parsing is file-backed and bounds compressed input, manifests, entries and declared extraction size. Import destinations use the same ancestor validation as other confined workflows.

## Verified remote files and requirements

`UrlDependencyRecord` carries destination, download alternatives, source digests, byte size and environment without a provider identity. Import verifies these bytes before creating the project. The persisted record produces packwiz metadata with the declared filename, side and optional flag. Sync restores missing metadata through the same verification path. Build verifies alternatives again, switches to a working URL only after checking source digests, and retains all alternatives in the exported manifest. Observed hashes of those verified bytes establish export integrity even when the backend emits a different supported hash algorithm. An export must contain at least one matching strong digest. Removal validates the exact metadata path and current content before deletion.

Provider-backed imports persist their environment requirements too. Sync reapplies those requirements after installation and repairs missing optional metadata. Packwiz can represent an optional file on one or both supported sides. A file that is required on one side and optional on the other cannot be represented by a single packwiz record; import rejects that conversion before initialization. Optional embedded files are also rejected rather than converted to unconditional overrides.

Mrpack exports that need preserved URL or optional records disable packwiz's hosting-domain restriction. The format permits direct URLs, while modrinth.com upload rules may restrict their hosts. This keeps format semantics intact; it does not promise eligibility for publication on every hosting service. Optional files that require a restricted-provider download mode cannot be exported losslessly by the backend; mrpack export rejects them and directs the user to a full distribution or an explicit requirement change. URL exports are checked for the declared destination, environment and size because a successful backend exit can still omit a failed download.

## Restricted download identity

Continuation records bind requests to strong digests from installed metadata where available. Candidate discovery enumerates each search directory once and caches metadata and digests for that pass. Duplicate paths are scanned once. Unreadable automatic candidates are skipped; explicitly selected unreadable files still fail with an error. Cache entries are checked again after copying and before staging. A renamed file can satisfy only the identity its bytes establish. Without a supported digest, the user must associate a file explicitly or place it at the printed cache destination; extension and recency are not evidence of identity.

Fingerprints include pack content, side layers and templates. A stale continuation is a read-only classification, including during preview. It preserves recovery evidence and requires a fresh build. Filesystem ancestor checks constrain every saved destination.

## Verification boundary

Planner tests cover identity and pin decisions. Live temporary-filesystem tests cover publication failures, symlink confinement, content-layer round trips and stale exports. CLI smoke tests combine add and repeated sync against a deterministic backend. Strict live tests exercise external tooling separately. These checks do not establish Minecraft runtime compatibility or complete cooperative cancellation of synchronous filesystem work.

The environment precedence follows the [Modrinth format specification](https://support.modrinth.com/en/articles/8802351-modrinth-modpack-format-mrpack): each side layer is applied after common overrides. Installed identities and digest fields come from the [packwiz metadata contract](https://packwiz.infra.link/reference/pack-format/mod-toml/). Empack's automatic restricted-file association accepts SHA digests; files with only MD5 or Murmur2 metadata require explicit selection.
