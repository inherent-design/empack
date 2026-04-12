---
spec: cli-surface
status: ratified
created: 2026-04-08
updated: 2026-04-11
depends: [overview, types]
---

# CLI Surface

This file documents the clap surface defined in `application/cli.rs` and the root configuration flags defined in `application/config.rs`.

## Root Command

The root shape is:

```text
empack [ROOT OPTIONS] <COMMAND>
```

If no command is provided, empack prints a short banner and suggests `empack --help`.

## Root Options

These options come from `AppConfig`.

| Flag | Env var | Default | Meaning |
| --- | --- | --- | --- |
| `-w`, `--workdir <PATH>` | `EMPACK_WORKDIR` | current directory during validation | Working directory for project operations |
| `-j`, `--cpu-jobs <N>` | `EMPACK_CPU_JOBS` | `2` | Configured parallel job count for resource-aware work |
| `-t`, `--net-timeout <SECS>` | `EMPACK_NET_TIMEOUT` | `30` | HTTP timeout in seconds |
| `--modrinth-api-client-id <VALUE>` | `EMPACK_ID_MODRINTH` | *none* | Optional Modrinth client identifier |
| `--modrinth-api-client-key <VALUE>` | `EMPACK_KEY_MODRINTH` | *none* | Optional Modrinth API key |
| `--curseforge-api-client-key <VALUE>` | `EMPACK_KEY_CURSEFORGE` | built-in default key | CurseForge API key |
| `--log-level <N>` | `EMPACK_LOG_LEVEL` | `0` | Verbosity from error to trace |
| `--log-format <FMT>` | `EMPACK_LOG_FORMAT` | `text` | `text`, `json`, or `yaml` |
| `--log-output <DEST>` | `EMPACK_LOG_OUTPUT` | `stderr` | `stderr` or `stdout` |
| `-c`, `--color <MODE>` | `EMPACK_COLOR` | `auto` | Terminal capability intent: `auto`, `always`, `never` |
| `-y`, `--yes` | `EMPACK_YES` | `false` | Non-interactive defaults |
| `--dry-run` | `EMPACK_DRY_RUN` | `false` | Preview mode for supported commands |

Configuration precedence is defaults, `.env.local`, `.env`, environment variables, then CLI arguments.

## Exit Semantics

The current process exit contract is:

| Code | Meaning |
| --- | --- |
| `0` | Success |
| `1` | General runtime or subprocess failure |
| `2` | Usage, config, or project-state failure |
| `3` | Network, provider, or API failure |
| `4` | Not found or no results |
| `130` | Interrupted by Ctrl+C |

## Command List

| Command | Arguments | Purpose |
| --- | --- | --- |
| `requirements` | none | Check external tool availability |
| `version` | none | Print version and build metadata |
| `init` | `[DIR]` | Initialize or import a project |
| `sync` | none | Reconcile `empack.yml` with packwiz state |
| `build` | `<TARGET>...` | Build one or more distribution targets |
| `add` | `<MOD>...` | Add dependencies by query, URL, or direct download |
| `remove` | `<MOD>...` | Remove dependencies |
| `clean` | `[TARGET]...` | Clean build artifacts or cache |

`remove` also has the alias `rm`.

## Init Command

Form:

```text
empack init [DIR] [OPTIONS]
```

`DIR` is the target directory. It is independent from `--pack-name`.

| Flag | Short | Env var | Meaning |
| --- | --- | --- | --- |
| `--force` | `-f` | *none* | Overwrite existing project files |
| `--modloader <NAME>` | `-m` | `EMPACK_MODLOADER` | `neoforge`, `fabric`, `forge`, `quilt`, or `none` |
| `--mc-version <VERSION>` | *none* | `EMPACK_MC_VERSION` | Minecraft version |
| `--author <NAME>` | `-A` | `EMPACK_AUTHOR` | Author name |
| `--pack-name <NAME>` | `-n` | `EMPACK_NAME` | Display name |
| `--loader-version <VERSION>` | *none* | `EMPACK_LOADER_VERSION` | Loader version |
| `--pack-version <VERSION>` | *none* | `EMPACK_PACK_VERSION` | Pack version |
| `--datapack-folder <PATH>` | *none* | `EMPACK_DATAPACK_FOLDER` | Relative datapack folder |
| `--game-versions <V1,V2,...>` | *none* | `EMPACK_GAME_VERSIONS` | Additional accepted Minecraft versions |
| `--from <SOURCE>` | *none* | *none* | Import from a local archive or URL |

Current command rules:

- `--from` accepts a local `.mrpack`, a local `.zip`, a Modrinth modpack URL, or a CurseForge modpack URL.
- `--dry-run` works for both plain init and import init.
- `--yes` without `--modloader` is rejected for plain init.
- `--modloader none` is the vanilla path.

## Sync Command

Form:

```text
empack sync
```

`sync` has no command-specific flags in the current CLI.

## Build Command

Form:

```text
empack build <TARGET>... [OPTIONS]
empack build --continue [OPTIONS]
```

| Flag | Short | Env var | Default | Meaning |
| --- | --- | --- | --- | --- |
| `--continue` | *none* | *none* | `false` | Resume a previously blocked restricted-mod build from persisted state |
| `--clean` | `-c` | *none* | `false` | Remove previous build artifacts before building |
| `--format <FMT>` | *none* | *none* | `zip` | Archive format for distribution packages |
| `--downloads-dir <PATH>` | *none* | `EMPACK_DOWNLOADS_DIR` | `~/Downloads` fallback | Directory scanned for restricted CurseForge downloads |

### Build targets

| Value | Meaning |
| --- | --- |
| `mrpack` | Build the Modrinth pack archive |
| `client` | Build the bootstrapped client distribution |
| `server` | Build the bootstrapped server distribution |
| `client-full` | Build the full client package |
| `server-full` | Build the full server package |
| `all` | Expand to all five targets |

### Archive formats

| Value | Meaning |
| --- | --- |
| `zip` | Zip archive |
| `tar.gz` | Gzip-compressed tar archive |
| `7z` | 7z archive |

### Build command rules

- `build --continue` resumes the original targets and archive format from persisted state.
- `build --continue` is incompatible with positional targets.
- `build --continue` is incompatible with `--clean`.
- `build --continue` is incompatible with `--format`.
- `--downloads-dir` is used in both fresh and continuation flows as an auxiliary search path for manually downloaded restricted files.
- all build entry paths validate tracked local dependency paths and SHA-256 hashes before build work starts
- `mrpack` is rejected when the current `ProjectPlan` still contains tracked local dependencies
- Fresh and continued restricted builds search for matching files in the managed cache first, then `--downloads-dir`, then `~/Downloads`, then the recorded parent directories of the pending destination paths.
- If the terminal is interactive and `--yes` is not set, the command can offer to open direct CurseForge `/download/{file-id}` URLs in the browser and wait up to 5 minutes for files to appear before falling back to manual continuation.

## Add Command

Form:

```text
empack add <MOD>... [OPTIONS]
```

| Flag | Meaning |
| --- | --- |
| `--platform <VALUE>` | Preferred platform: `modrinth`, `curseforge`, or `both` |
| `--type <VALUE>` | Project type: `mod`, `datapack`, `resourcepack`, `shader` |
| `--version-id <ID>` | Pin a Modrinth version ID |
| `--file-id <ID>` | Pin a CurseForge file ID |
| `--force` | Reinstall or keep going through duplicate and conflict cases |

Current command rules:

- Plain search defaults to Modrinth-first resolution.
- `--platform both` removes the preference and keeps the default search order.
- Modrinth project URLs become direct Modrinth project IDs.
- CurseForge project URLs resolve by slug through the CurseForge API.
- Direct `.jar` URLs are supported for mods.
- Unidentified direct `.jar` downloads become tracked local mod dependencies.
- Direct `.zip` URLs are accepted only with `--type resourcepack`, `--type shader`, or `--type datapack`.
- Direct non-`.zip` non-`.jar` URLs are rejected explicitly.
- If both `--version-id` and `--file-id` are provided, the chosen pin depends on the resolved direct platform. CurseForge direct paths prefer `file-id`. Other paths prefer `version-id`.

## Remove Command

Form:

```text
empack remove <MOD>... [OPTIONS]
empack rm <MOD>... [OPTIONS]
```

| Flag | Short | Meaning |
| --- | --- | --- |
| `--deps` | `-d` | Also remove dependencies that are no longer needed |

## Clean Command

Form:

```text
empack clean [TARGET]...
```

| Value | Meaning |
| --- | --- |
| `builds` | Remove build artifacts from `dist/` |
| `cache` | Remove cache data |
| `all` | Clean both builds and cache |

If no clean target is provided, the command treats the request as `builds`.

`clean` never removes project metadata such as `empack.yml` or `pack/`.
`clean cache` removes empack-managed cache data under the cache root plus staged managed binaries in the system temp area.
