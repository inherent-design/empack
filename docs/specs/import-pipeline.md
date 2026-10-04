---
spec: import-pipeline
status: partial
created: 2026-04-08
updated: 2026-04-11
depends: [overview, types, config-and-manifest, search-and-resolution]
---

# Import Pipeline

`init --from` imports a Modrinth or CurseForge modpack archive into an empack project.

## Source Detection

Local source detection accepts:

| Source kind | Detection rule |
| --- | --- |
| `ModrinthMrpack` | local file with `.mrpack` extension |
| `CurseForgeZip` | local file with `.zip` extension |
| `PackwizDirectory` | local directory containing `pack.toml` |

Current command behavior:

- `PackwizDirectory` is detected but rejected with an explicit "not yet implemented" error.
- Existing empack projects are rejected as `AlreadyEmpackProject`.
- Unrecognized local paths fail before any import work starts.

Remote source handling accepts:

- Modrinth modpack URLs
- CurseForge modpack URLs

Remote import first downloads the archive through the session filesystem, then parses it as a local archive.

## Parse Phase

### CurseForge archives

`parse_curseforge_zip()` expects `manifest.json` at the archive root.

Parsers use seekable `FileSystemProvider` readers. The live reader is file-backed, and the path-based entry point uses the same parser. Compressed input is limited to 2 GiB before ZIP parsing; textual manifests are limited to 16 MiB.

Parsed data includes:

- pack identity
- runtime target from `minecraft.version` and `minecraft.modLoaders[]`
- `files[]` entries as platform references
- overrides from the manifest `overrides` directory

### Modrinth archives

`parse_modrinth_mrpack()` expects `modrinth.index.json` at the archive root.

Parsers use seekable `FileSystemProvider` readers. The live reader is file-backed, and the path-based entry point uses the same parser. Compressed input is limited to 2 GiB before ZIP parsing; textual manifests are limited to 16 MiB.

Parsed data includes:

- pack identity
- runtime target from `dependencies`
- `files[]` entries as platform references or embedded JARs
- overrides from archive conventions such as `overrides/`, `client-overrides/`, and `server-overrides/`

## Resolve Phase

`resolve_manifest()` converts the parsed manifest into a `ResolvedManifest`.

Current behavior:

- concurrent resolution of platform references
- shared host rate budgets for Modrinth and CurseForge
- Modrinth file ID backfill from SHA1 when missing
- CurseForge batch file lookup when a file ID must be resolved from CDN structure
- warning collection instead of early abort for per-entry failures

Resolved platform references may gain:

- `resolved_name`
- `resolved_slug`
- `resolved_type`
- `cf_class_id`
- `file_id`

Embedded JARs remain passthrough items and are handled later.

## Datapack Folder Detection

If the caller does not pass `--datapack-folder`, import attempts to infer one from the manifest.

Current detection order:

1. `config/paxi/datapacks`
2. `config/openloader/data`
3. `datapacks/` zip overrides
4. platform references targeting `datapacks/`
5. CurseForge class ID `6945` on resolved content

If none of those signals exist, no datapack folder is written.

## Execute Phase

`execute_import()` applies the resolved manifest to a target directory.

High-level steps:

1. Create the target directory.
2. Write `empack.yml` from import metadata and CLI overrides.
3. Run the normal initialization transition through `PackStateManager`.
4. Write packwiz `[options]` when datapack folder or acceptable game versions are present.
5. Refresh packwiz after writing those options.
6. Add resolved platform references through packwiz.
7. Extract or copy embedded and override content.
8. Update import statistics.

Platform add batching uses `--no-refresh` behavior when more than one content entry is being added, then refreshes as needed after the batch.

## Override Handling

Override files are classified into categories such as config, resource pack, shader pack, data pack, world, server config, client config, and other.

Current side handling supports:

- both
- client only
- server only

Overrides are copied into the project after initialization. They can supersede unresolved platform references by basename.
Execution reuses one file-backed archive reader for embedded and override content.

## Failure Semantics

Import tracks:

- `platform_referenced`
- `platform_failed`
- `platform_skipped`
- `embedded_jars_identified`
- `embedded_jars_unidentified`
- `overrides_copied`
- `warnings`

Current command outcome rules:

- skipped platform items are reported but do not fail the command
- failed platform adds are reported and make `init --from` return an error after the summary
- warnings are surfaced during resolve and execution
- `--dry-run` stops after resolve and summary

An import is considered incomplete if any platform references fail to add.

## Archive Resource Limits

Import execution opens one seekable archive reader and reuses it for embedded content and overrides. The live reader is file-backed; mocks use an in-memory cursor. Archive metadata is checked before execution creates the target project. Imports accept at most 100,000 entries, 512 MiB per uncompressed entry, and 4 GiB of declared uncompressed content. Extraction also enforces the per-entry limit while reading. Individual output files still pass through the filesystem provider as byte buffers.

Archive destinations use the workflow output validator before extraction. The
live filesystem rejects symlinked ancestors even when the final file does not
exist. A failed manifest publication after installation is a command failure;
its diagnostic identifies the partial installation and required reconciliation.

Environment distinctions persist in the project layout. Common overrides go to
`pack/`; client-only files go to `overrides/client/`; server-only files go to
`overrides/server/`. Embedded files follow their declared environment too. The
same relative destination can exist in all three layers without overwriting
another layer during import. Forced initialization resets these managed content
directories only after its execution checks.

Remote acquisition uses a temporary file and enforces the 2 GiB compressed-input limit while receiving bytes. It rejects an excessive Content-Length before receiving the body and counts streamed chunks even when the header is absent. Cancellation interrupts stalled header or body waits. Failed acquisition drops the temporary file; completed acquisition publishes through the filesystem provider's streamed atomic replacement. Production publication does not buffer the file in memory. In-memory test providers may buffer their fixtures.

### Provider-free files and optional requirements

Unknown-host downloads become `ContentEntry::UrlFile` and persist as `status: url`; they do not pass through `packwiz url add` or acquire an empty Modrinth ID. The record retains every download alternative, the archive's expected digests and size, its destination, and its environment. Acquisition verifies supported source hashes before project initialization. Metadata publication uses the verified alternative without replacing the source digest with a digest of arbitrary replacement bytes.

Provider imports persist an optional `environment` in their resolved manifest record. After the backend writes metadata, empack applies the declared side and optional flag, then refreshes the index. Sync restores these requirements after pin changes. Required/optional combinations that differ between supported sides, and optional embedded files, fail preflight because the current packwiz representation cannot preserve them. The tests inspect generated metadata and a real backend re-export.
