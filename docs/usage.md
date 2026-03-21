# empack usage

## Scope

This guide describes the current Rust CLI. The Bash implementations in `v1/` and `v2/` are historical reference only. For project structure and repository layout, see [`../README.md`](../README.md).

## Command overview

| Command | Purpose |
| --- | --- |
| `empack requirements` | Check external tool availability |
| `empack version` | Print version information |
| `empack init` | Initialize a project or complete partial setup |
| `empack add` | Add projects by name, URL, or project ID |
| `empack sync` | Reconcile declared dependencies with installed state |
| `empack build` | Produce `mrpack` and other build targets |
| `empack remove` | Remove projects from the current modpack (alias: `rm`) |
| `empack clean` | Remove build outputs |

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

Configuration precedence (highest to lowest): CLI flags, environment variables, `.env` file, defaults.

Standard color environment variables are also respected:

| Variable | Effect |
| --- | --- |
| `NO_COLOR` | Any non-empty value disables color output |
| `FORCE_COLOR` | `0`/`false` disables, `1`/`2`/`3`/`true` enables color |
| `CLICOLOR` | `0` disables color (BSD/macOS convention) |
| `CI` | Any value disables color and interactive features |

API keys:

| Variable | Purpose |
| --- | --- |
| `EMPACK_KEY_CURSEFORGE` | CurseForge API key (has a built-in default) |
| `EMPACK_KEY_MODRINTH` | Modrinth API key (optional) |
| `EMPACK_ID_MODRINTH` | Modrinth API client ID (optional) |

## Typical workflow

### Local prerequisites

```bash
empack requirements
```

### Project initialization

```bash
empack init my-pack \
  --pack-name "My Pack" \
  --modloader fabric \
  --mc-version 1.21.1 \
  --author "Your Name" \
  -y
```

The `--force` flag overwrites existing project files:

```bash
empack init my-pack --force
```

### Adding dependencies

By name:

```bash
empack add sodium
```

With a platform preference:

```bash
empack add jei --platform curseforge
```

The `--platform` flag accepts `modrinth`, `curseforge`, or `both`.

The `--force` flag adds projects even if version conflicts exist:

```bash
empack add sodium --force
```

### Reconciling declared and installed state

```bash
empack sync
```

Preview changes without applying them:

```bash
empack sync --dry-run
```

### Building artifacts

Build a single target:

```bash
empack build mrpack
```

Build all targets after cleaning previous outputs:

```bash
empack build --clean all
```

Control parallel build processes with `--jobs`:

```bash
empack build -j 4 all
```

Build outputs appear under the project-local `dist/` directory.

### Removing dependencies

```bash
empack remove sodium
```

Remove a project and offer orphan dependency cleanup:

```bash
empack remove sodium --deps
```

The `rm` alias is equivalent:

```bash
empack rm sodium
```

### Cleaning build artifacts

```bash
empack clean builds
```

`clean` targets the artifact tree under `dist/`, leaving `empack.yml` and pack metadata intact.

## Project model

- `empack.yml`: declared project configuration
- `pack/`: managed `packwiz` workspace
- `dist/`: build artifact root

## Remaining gaps

Broader remove behavior beyond targeted command tests is not yet promoted into the main verification matrix.
