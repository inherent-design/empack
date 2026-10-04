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
4. Expand production prerequisites, placing `mrpack` before light client/server targets, then execute each target once.
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
- when pack metadata supplies a SHA-1, SHA-256 or SHA-512 digest, fresh builds scan for matching content in this order:
  - empack-managed restricted-build cache
  - `--downloads-dir`
  - `~/Downloads`
  - recorded parent directories of the pending destination paths
- matching content is stored under its digest and checked again before staging
- without a supported digest, empack does not guess from filenames, timestamps or extensions; use `empack build --continue --associate-download FILENAME=PATH`, or place the selected file at the printed cache path
- explicit associations validate all selections before copying and respect dry-run
- saved fingerprints cover the manifest, pack files, side layers and templates; changed inputs retain the stale record and require a fresh build
- continuation records from before content fingerprints were introduced require a fresh build
- if every required file is cached, empack reuses the same continuation path as `build --continue`
- `build --continue` restores cached files into the recorded destination paths and reruns the original targets in continuation mode
- continuation mode skips the initial clean for `client-full` and `server-full`
- `build --continue` is parse-time incompatible with positional targets, `--clean`, and `--format`
- if files are still missing and the terminal is interactive, empack can offer to open direct CurseForge `/download/{file-id}` URLs in the browser
- after opening those URLs, empack waits up to 5 minutes for verified content or explicitly staged cache files and continues automatically if all requests are satisfied
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

## Template Data

Initialization installs client and server templates with their placeholders intact. Builds render them using current pack metadata. The embedded Bash installer uses `shell_quote` for metadata assignments and `printf` for display, so names and versions remain literal data. Existing project templates are user-owned; update old installer templates to use `{{shell_quote NAME}}` and `{{shell_quote VERSION}}` in shell assignments before distributing them. Raw metadata does not belong in shell source or generated comments.

Loading a saved continuation is read-only. Stale state returns an error and
remains available for inspection, including during a preview. A fresh build or
explicit cleanup can replace or clear the saved state.

## Content layers and freshness

A build invocation refreshes its inputs and produces one fresh mrpack export when
needed. An existing archive with the same name and version is not a cache hit.
Reusing an orchestrator for another invocation resets its input and export state.
Prerequisite export failures stop dependent targets and retain restricted-download
continuation information.

Common content lives in `pack/`. Optional `overrides/client/` and
`overrides/server/` directories contain environment-specific content. Each target
copies common content into its bundled pack, applies the selected side, and
refreshes that staged pack index before installation. Client-full uses the
installer's client side; server-full uses its server side. Light distributions
apply common export overrides followed by their matching side overrides.

Mrpack exports retain both side layers under `client-overrides/` and
`server-overrides/`, using streamed ZIP replacement. This follows Modrinth's
[override precedence](https://support.modrinth.com/en/articles/8802351-modrinth-modpack-format-mrpack).
Project templates remain distribution scaffolding and do not store imported
side-specific game content.
