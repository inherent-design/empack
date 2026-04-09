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

Add dependencies by name, URL, or direct download JAR.

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
- Direct download URLs are supported only for `.jar` files.
- Non-JAR direct download URLs are rejected.

### empack sync

Reconcile declared dependencies in `empack.yml` with the installed pack state.

```bash
empack sync
empack sync --dry-run
```

### empack build

Produce build artifacts under `dist/`.

```bash
empack build mrpack
empack build all --clean
empack build client-full --downloads-dir ~/Downloads
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
| `--clean` | Remove previous build outputs before building |
| `--format` | Output archive format: `zip`, `tar.gz`, `7z` |
| `--downloads-dir` | Directory scanned for manually downloaded restricted CurseForge files |

If restricted CurseForge files are missing, empack prints download URLs and can retry the build automatically after files are placed.

### empack remove

Remove mods from the current project. Alias: `rm`.

```bash
empack remove sodium
empack remove sodium --deps
empack rm sodium
```

The `--deps` flag offers to clean up orphaned dependencies.

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

## Environment variables

### Configuration precedence

CLI flags > environment variables > `.env` file > defaults.

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
