---
spec: build-and-distribution
status: partial
created: 2026-04-08
updated: 2026-04-11
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

1. Validate tracked local dependencies from the current `ProjectPlan`.
2. Prepare the build environment under `dist/`.
3. Resolve cached paths for `packwiz-installer-bootstrap.jar` and `packwiz-installer.jar`.
4. Execute targets in the requested order.
5. Remove the temporary mrpack extraction directory if it exists.
6. Complete the marker guard on success.

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

Current template directory layout:

- `templates/common` applies to `client`, `server`, `client-full`, and `server-full`
- `templates/client` applies to `client` and `client-full`
- `templates/server` applies to `server` and `server-full`
- `mrpack` does not consume build templates

Current file behavior:

- files ending in `.template` are rendered through the template engine and written without the `.template` suffix
- non-`.template` files are rendered in place when they are valid UTF-8 text
- non-`.template` files that are not valid UTF-8 are copied byte-for-byte

## Runtime Assets

The command layer ensures required jars exist in cache before build execution:

| Asset | Needed for |
| --- | --- |
| `packwiz-installer-bootstrap.jar` | `client`, `server`, `client-full`, `server-full` |
| `packwiz-installer.jar` | `client-full`, `server-full` |

These files are cached under the empack cache root.

## Tracked Local Dependencies

Current build behavior for `DependencySource::Local` is explicit:

- every build path validates local file presence and SHA-256 hash before starting
- validation failure is a project-state/config error, not a silent omission
- `mrpack` export is currently blocked when any tracked local dependency remains in the project plan
- non-`mrpack` targets may proceed only after local dependency validation passes

## Restricted CurseForge Downloads

Restricted download handling is part of the current build pipeline for both:

- full-distribution installer flows
- `mrpack` export failures that report manual CurseForge downloads

The public recovery command is `empack build --continue`.

Current behavior:

- packwiz-installer output is parsed for restricted mod records
- packwiz `mr export` manual-download output is also parsed into restricted mod records
- continuation state is persisted internally when a fresh full build is blocked
- the same continuation state is reused when `mrpack` export is blocked on manual downloads
- records keep every destination path; user-facing display is deduplicated by download URL
- the command prints the download URL, managed cache path, and destination path for each unique restricted download
- fresh builds scan for matching filenames in this order:
  - empack-managed restricted-build cache
  - `--downloads-dir`
  - `~/Downloads`
  - recorded parent directories of the pending destination paths
- matching files found outside the cache are imported into the managed cache
- if every required file is cached, empack reuses the same continuation path as `build --continue`
- `build --continue` restores cached files into the recorded destination paths and reruns the original targets in continuation mode
- continuation mode skips the initial clean for `client-full` and `server-full`
- `build --continue` is parse-time incompatible with positional targets, `--clean`, and `--format`
- if files are still missing and the terminal is interactive, empack can offer to open direct CurseForge `/download/{file-id}` URLs in the browser
- after opening those URLs, empack waits up to 5 minutes for files to appear in the watched directories and continues automatically if they all arrive
- empack does not directly fetch restricted CurseForge download URLs itself

If restricted files remain missing, the command exits with an error after printing the managed cache location and `empack build --continue`.

The pending continuation file and exact cache layout are internal implementation details, not documented user-facing contracts.

## Clean Behavior

Build cleanup has two layers:

- `build --clean` removes prior build artifacts before starting a new build
- `empack clean builds` removes build artifacts without starting a new build
- `empack clean cache` removes empack-managed cache data without touching project source files
- `empack clean all` removes both build artifacts and empack-managed cache data

`build --clean` also clears any pending restricted-build continuation state before rebuilding.

State-machine cleanup for `Configured` projects is documented in [state-machine.md](state-machine.md).
