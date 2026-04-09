---
spec: build-and-distribution
status: draft
created: 2026-04-08
updated: 2026-04-08
depends: [overview, types, config-and-manifest]
---

# Build and Distribution

`empack build` produces artifacts under `dist/` through `BuildOrchestrator`.

## Target Set

| Target | Output class | Notes |
| --- | --- | --- |
| `mrpack` | Modrinth archive | Uses packwiz export flow |
| `client` | Bootstrapped client distribution | Uses packwiz-installer-bootstrap |
| `server` | Bootstrapped server distribution | Adds server runtime assets and templates |
| `client-full` | Full client package | Non-redistributable; can surface restricted CurseForge downloads |
| `server-full` | Full server package | Non-redistributable; can surface restricted CurseForge downloads |

The CLI meta-target `all` expands to all five targets.

## Build Pipeline

`BuildOrchestrator::execute_build_pipeline()` wraps the build with a `Building` marker guard.

Current pipeline structure:

1. Prepare the build environment under `dist/`.
2. Resolve cached paths for `packwiz-installer-bootstrap.jar` and `packwiz-installer.jar`.
3. Execute targets in the requested order.
4. Remove the temporary mrpack extraction directory if it exists.
5. Complete the marker guard on success.

If the build exits early, the marker remains and the next state discovery reports `Interrupted`.

## Archive Formats

Supported archive formats come from `empack/archive.rs`.

| Format | CLI value | Extension |
| --- | --- | --- |
| `Zip` | `zip` | `zip` |
| `TarGz` | `tar.gz` | `tar.gz` |
| `SevenZ` | `7z` | `7z` |

Current default is always `zip`. There is no platform-specific default switching in the live CLI.

## Template Processing

The build system uses `TemplateEngine` for both initialization scaffolding and build-time rendering.

Embedded template names include:

- `gitignore`
- `packwizignore`
- `instance.cfg`
- `install_pack.sh`
- `server.properties`
- `validate.yml`
- `release.yml`

Build-time template processing loads variables from `pack/pack.toml` and then renders user templates from the project template directories into target output trees.

## Runtime Assets

The command layer ensures required jars exist in cache before build execution:

| Asset | Needed for |
| --- | --- |
| `packwiz-installer-bootstrap.jar` | `client`, `server`, `client-full`, `server-full` |
| `packwiz-installer.jar` | `client-full`, `server-full` |

These files are cached under the empack cache root.

## Restricted CurseForge Downloads

Restricted download handling is part of the current build pipeline for full distributions.

Current behavior:

- packwiz-installer output is parsed for restricted mod records
- records are deduplicated by download URL across targets
- the command prints the download URL and destination path for each restricted mod
- `--downloads-dir` or `~/Downloads` is scanned for matching filenames
- if every required file is found and copied into place, empack re-runs the build automatically
- if files are still missing and the terminal is interactive, empack can offer to open the URLs in a browser

If restricted files remain missing, the command exits with an error after printing instructions.

## Clean Behavior

Build cleanup has two layers:

- `build --clean` removes prior build artifacts before starting a new build
- `empack clean builds` removes build artifacts without starting a new build

State-machine cleanup for `Configured` projects is documented in [state-machine.md](state-machine.md).
