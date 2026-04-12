# empack usage guide

This file is the user-facing command reference for the empack CLI. For development setup, see [CONTRIBUTING.md](../CONTRIBUTING.md).

## Quick start

A typical workflow from project creation through build:

```bash
empack requirements
empack init my-pack --pack-name "My Pack" --modloader fabric --mc-version 1.21.1 --author "Your Name" -y
empack add sodium
empack sync
empack build all
```

## Root options

These options are defined on the root CLI and shape all command execution.

| Flag | Env var | Default | Meaning |
| --- | --- | --- | --- |
| `-w`, `--workdir <PATH>` | `EMPACK_WORKDIR` | current directory | Working directory for project operations |
| `-j`, `--cpu-jobs <N>` | `EMPACK_CPU_JOBS` | `2` | Configured parallel job count |
| `-t`, `--net-timeout <SECS>` | `EMPACK_NET_TIMEOUT` | `30` | HTTP timeout in seconds |
| `--modrinth-api-client-id <VALUE>` | `EMPACK_ID_MODRINTH` | *none* | Optional Modrinth client identifier |
| `--modrinth-api-client-key <VALUE>` | `EMPACK_KEY_MODRINTH` | *none* | Optional Modrinth API key |
| `--curseforge-api-client-key <VALUE>` | `EMPACK_KEY_CURSEFORGE` | built-in default key | CurseForge API key |
| `--log-level <N>` | `EMPACK_LOG_LEVEL` | `0` | Verbosity from error to trace |
| `--log-format <FMT>` | `EMPACK_LOG_FORMAT` | `text` | `text`, `json`, or `yaml` |
| `--log-output <DEST>` | `EMPACK_LOG_OUTPUT` | `stderr` | `stderr` or `stdout` |
| `-c`, `--color <MODE>` | `EMPACK_COLOR` | `auto` | `auto`, `always`, or `never` |
| `-y`, `--yes` | `EMPACK_YES` | `false` | Non-interactive defaults |
| `--dry-run` | `EMPACK_DRY_RUN` | `false` | Preview supported operations without changing files |

## Commands

### empack requirements

Check the required external tools and runtime support.

```bash
empack requirements
```

### empack version

Print version and build metadata.

```bash
empack version
```

### empack init

Create a new modpack project. The positional argument specifies the target directory; `--pack-name` sets the display name independently.

```bash
empack init my-pack \
  --pack-name "My Pack" \
  --modloader fabric \
  --mc-version 1.21.1 \
  --author "Your Name" \
  -y
```

Without arguments, empack initializes in the current directory and prompts for each field.

| Flag | Short | Env var | Description |
| --- | --- | --- | --- |
| `--pack-name` | `-n` | `EMPACK_NAME` | Modpack display name |
| `--modloader` | `-m` | `EMPACK_MODLOADER` | Mod loader: `neoforge`, `fabric`, `forge`, `quilt`, `none` |
| `--mc-version` | | `EMPACK_MC_VERSION` | Minecraft version |
| `--author` | `-A` | `EMPACK_AUTHOR` | Author name |
| `--loader-version` | | `EMPACK_LOADER_VERSION` | Loader version |
| `--pack-version` | | `EMPACK_PACK_VERSION` | Pack version string |
| `--from` | | | Import from a local file or URL (`.mrpack`, `.zip`) |
| `--datapack-folder` | | `EMPACK_DATAPACK_FOLDER` | Folder for datapacks relative to pack root |
| `--game-versions` | | `EMPACK_GAME_VERSIONS` | Additional accepted MC versions (comma-separated) |
| `--force` | `-f` | | Overwrite existing project files |

Use `--modloader none` for vanilla projects.

#### Importing modpacks

Import an existing modpack from a local archive or remote modpack URL:

```bash
empack init --from fabulously-optimized.mrpack my-pack
empack init --from https://cdn.modrinth.com/data/.../pack.mrpack my-pack --yes
empack init --from https://www.curseforge.com/minecraft/modpacks/... imported-pack
```

Current import sources:

- local `.mrpack`
- local `.zip`
- Modrinth modpack URLs
- CurseForge modpack URLs

`--dry-run` works for `init --from` and prints a resolve summary without writing files.

The `--force` flag overwrites existing project files:

```bash
empack init my-pack --force
```

### empack add

Add dependencies by name, URL, or direct download.

```bash
empack add sodium
empack add jei --platform curseforge
empack add complementary-reimagined --type shader
empack add polished-widgets --type datapack
empack add sodium --version-id 5QpJwx2J
empack add jei --platform curseforge --file-id 5101366
```

| Flag | Description |
| --- | --- |
| `--platform` | Preferred platform: `modrinth`, `curseforge`, or `both` |
| `--type` | Project type: `mod`, `datapack`, `resourcepack`, or `shader` |
| `--version-id` | Pin a Modrinth version ID |
| `--file-id` | Pin a CurseForge file ID |
| `--force` | Add projects even when version conflicts or duplicates exist |

Current add behavior:

- `--platform both` keeps the default Modrinth-first order.
- Modrinth and CurseForge project URLs are resolved through platform-specific paths.
- Direct `.jar` URLs stay supported for mods.
- Unidentified direct `.jar` downloads are now tracked as local mod dependencies in `empack.yml` instead of being left unmanaged.
- Direct `.zip` URLs are supported for `resourcepack`, `shader`, and `datapack`, but they require `--type`.
- Arbitrary non-`.zip` non-`.jar` direct downloads are rejected.

Tracked local dependencies are written into `empack.yml` with `status: local`:

```yaml
dependencies:
  example-pack:
    status: local
    title: Example Pack
    type: resourcepack
    path: pack/resourcepacks/example-pack.zip
    source_url: https://example.com/example-pack.zip
    sha256: <hex>
```

### empack sync

Reconcile declared dependencies in `empack.yml` with the installed pack state.

```bash
empack sync
empack sync --dry-run
```

Current sync behavior:

- resolved platform dependencies still reconcile through packwiz
- tracked local dependencies are validated in place and are not passed to packwiz
- missing local files or hash drift fail normal sync
- `--dry-run` reports local dependency drift without mutating the project

### empack build

Produce build artifacts under `dist/`.

```bash
empack build mrpack
empack build all --clean
empack build client-full --downloads-dir ~/Downloads
empack build --continue
empack build server --format tar.gz
```

Available targets:

- `mrpack`
- `client`
- `server`
- `client-full`
- `server-full`
- `all`

Build options:

| Flag | Description |
| --- | --- |
| `--continue` | Resume a previously blocked restricted-mod build from persisted state |
| `--clean` | Remove previous build outputs before building |
| `--format` | Output archive format: `zip`, `tar.gz`, `7z` |
| `--downloads-dir` | Directory scanned for manually downloaded restricted CurseForge files |

`--continue` resumes the original full-build targets and archive format from persisted state. It must be used without positional targets, without `--clean`, and without `--format`.

If restricted CurseForge files are missing during a build:

- empack records pending continuation state internally
- empack scans the managed restricted-build cache first
- empack then scans `--downloads-dir` if provided
- empack finally scans `~/Downloads`
- empack also scans the recorded destination parent directories for matching files, including the packwiz import cache path used by `mrpack` export failures
- any matching files found outside the cache are imported into the managed cache
- if all required files are cached, empack reuses the same continuation path as `empack build --continue`
- if files are still missing, empack prints download URLs, the managed cache location, and the `empack build --continue` instruction

When the terminal is interactive and `--yes` is not set, empack can optionally:

- open direct CurseForge `/download/{file-id}` URLs in the default browser
- wait up to 5 minutes for the files to appear in the watched download locations
- continue automatically once every required file is cached

empack does not fetch restricted CurseForge files directly. The browser-open step is an aid, not a separate download client inside empack.

Tracked local dependency behavior:

- every build validates tracked local dependency paths and SHA-256 hashes before build work starts
- missing or mismatched local files are treated as project-state/config failures
- `mrpack` exports currently reject tracked local dependencies instead of omitting them silently

### empack remove

Remove mods from the current project. Alias: `rm`.

```bash
empack remove sodium
empack remove sodium --deps
empack rm sodium
```

The `--deps` flag offers to clean up orphaned dependencies.

When a dependency is tracked as `status: local`, `empack remove` deletes the recorded file if it still exists and then removes the entry from `empack.yml`.

- If the tracked file is already missing, empack warns and still attempts to remove the config entry.
- If the file is removed but `empack.yml` cannot be updated, the command fails and tells you to fix the write error and rerun `empack remove <name>`.

### empack clean

Clean build outputs or cache data.

```bash
empack clean
empack clean builds
empack clean cache
empack clean all
```

Clean targets:

- `builds`
- `cache`
- `all`

If no target is provided, empack cleans `builds`.

## Exit Codes

empack uses a stable process exit contract:

- `0`: success
- `1`: general runtime or subprocess failure
- `2`: usage, config, or project-state failure
- `3`: network, provider, or API failure
- `4`: not found or no results
- `130`: interrupted by Ctrl+C

`clean` never removes project metadata such as `empack.yml` or `pack/`.

## Environment variables

### Configuration precedence

CLI flags > environment variables > `.env.local` > `.env` > defaults.

### Color control

Standard color environment variables are respected:

| Variable | Effect |
| --- | --- |
| `NO_COLOR` | Any non-empty value disables color output |
| `FORCE_COLOR` | `0`/`false` disables, `1`/`2`/`3`/`true` enables color |
| `CLICOLOR` | `0` disables color (BSD/macOS convention) |
| `CI` | Any value disables color and interactive features |

### API keys

| Variable | Purpose |
| --- | --- |
| `EMPACK_KEY_CURSEFORGE` | CurseForge API key (has a built-in default) |
| `EMPACK_KEY_MODRINTH` | Modrinth API key (optional) |
| `EMPACK_ID_MODRINTH` | Modrinth API client ID (optional) |
| `EMPACK_PACKWIZ_BIN` | Override the `packwiz-tx` binary path |
| `EMPACK_DOWNLOADS_DIR` | Default downloads directory for restricted file scanning |

## Project model

- `empack.yml`: project configuration (declared dependencies, metadata, build settings)
- `pack/`: managed packwiz workspace
- `dist/`: build artifact output
