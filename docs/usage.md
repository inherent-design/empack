# empack usage guide

This is the command reference for the empack CLI. For development setup, see [CONTRIBUTING.md](../CONTRIBUTING.md).

## Quick start

A typical workflow from project creation through build:

```bash
empack requirements
empack init my-pack --pack-name "My Pack" --modloader fabric --mc-version 1.21.1 --author "Your Name" -y
empack add sodium
empack sync
empack build mrpack
```

## Commands

### empack requirements

Check availability of external tools (packwiz, java). Run this before first use to verify the host environment.

```bash
empack requirements
```

### empack version

Print version information.

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
| `--pack-name` | `-n` | `EMPACK_NAME` | Modpack display name (default: directory basename) |
| `--modloader` | `-m` | `EMPACK_MODLOADER` | Mod loader: `neoforge`, `fabric`, `forge`, `quilt`, `none` |
| `--mc-version` | | `EMPACK_MC_VERSION` | Minecraft version |
| `--author` | `-A` | `EMPACK_AUTHOR` | Author name |
| `--loader-version` | | `EMPACK_LOADER_VERSION` | Loader version (e.g. `0.15.0` for Fabric) |
| `--pack-version` | | `EMPACK_PACK_VERSION` | Pack version string (e.g. `1.0.0`) |
| `--from` | | | Import from a local file or URL (`.mrpack`, `.zip`) |
| `--datapack-folder` | | `EMPACK_DATAPACK_FOLDER` | Folder for datapacks relative to pack root |
| `--game-versions` | | `EMPACK_GAME_VERSIONS` | Additional accepted MC versions (comma-separated) |
| `--force` | `-f` | | Overwrite existing project files |

Use `--modloader none` for vanilla (no mod loader) projects. When vanilla is selected, loader version prompts are skipped and no loader metadata is written to `empack.yml`.

#### Importing modpacks

Import an existing modpack from a Modrinth `.mrpack` or CurseForge `.zip` archive:

```bash
empack init --from fabulously-optimized.mrpack my-pack
empack init --from https://cdn.modrinth.com/data/.../pack.mrpack my-pack --yes
```

The import pipeline parses the manifest, resolves platform references via Modrinth and CurseForge APIs, copies overrides, and auto-detects the datapack folder strategy (Paxi, Open Loader, or root datapacks).

The `--force` flag overwrites existing project files:

```bash
empack init my-pack --force
```

### empack add

Add mods by name, URL, or project ID. empack searches Modrinth and CurseForge by default.

```bash
empack add sodium
empack add jei --platform curseforge
empack add sodium --force
empack add complementary-reimagined --type shader
empack add faithful --type resourcepack
```

| Flag | Description |
| --- | --- |
| `--platform` | Restrict search to `modrinth`, `curseforge`, or `both` (default) |
| `--type` | Project type: `mod`, `resourcepack`, `shader`. Skips tiered type guessing when specified |
| `--force` | Add projects even when version conflicts or duplicates exist |

### empack sync

Reconcile declared dependencies in `empack.yml` with the installed pack state.

```bash
empack sync
empack sync --dry-run
```

### empack build

Produce build artifacts under `dist/`. Available targets: `mrpack`, `client`, `server`, `client-full`, `server-full`, `all`.

```bash
empack build mrpack
empack build --clean all
empack build -j 4 all
```

The `--clean` flag removes previous build outputs before building. The `-j` flag controls the number of parallel build processes.

### empack remove

Remove mods from the current project. Alias: `rm`.

```bash
empack remove sodium
empack remove sodium --deps
empack rm sodium
```

The `--deps` flag offers to clean up orphaned dependencies.

### empack clean

Remove build outputs from the `dist/` directory. Project configuration and pack metadata are not affected. Available targets: `builds`, `cache`, `all`.

```bash
empack clean builds
```

## Global flags

These flags apply to all commands:

| Flag | Env var | Default | Description |
| --- | --- | --- | --- |
| `-y`, `--yes` | `EMPACK_YES` | `false` | Skip prompts and use defaults |
| `--dry-run` | `EMPACK_DRY_RUN` | `false` | Preview operations without executing |
| `-w`, `--workdir <PATH>` | `EMPACK_WORKDIR` | current directory | Working directory for modpack operations |
| `-j`, `--cpu-jobs <N>` | `EMPACK_CPU_JOBS` | `2` | Number of parallel API requests |
| `-t`, `--net-timeout <SECS>` | `EMPACK_NET_TIMEOUT` | `30` | API timeout in seconds |
| `-c`, `--color <MODE>` | `EMPACK_COLOR` | `auto` | Color output: `auto`, `always`, `never` |
| `--log-level <N>` | `EMPACK_LOG_LEVEL` | `0` | Verbosity: 0=error, 1=warn, 2=info, 3=debug, 4=trace |
| `--log-format <FMT>` | `EMPACK_LOG_FORMAT` | `text` | Output format: `text`, `json`, `yaml` |
| `--log-output <DEST>` | `EMPACK_LOG_OUTPUT` | `stderr` | Log destination: `stderr`, `stdout` |

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

## Project model

- `empack.yml`: project configuration (declared dependencies, metadata, build settings)
- `pack/`: managed packwiz workspace (tracks installed state)
- `dist/`: build artifact output
