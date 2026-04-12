---
spec: platform-and-managed-tooling
status: partial
created: 2026-04-08
updated: 2026-04-11
depends: [overview]
---

# Platform and Managed Tooling

empack centralizes OS-specific helpers and managed `packwiz-tx` behavior under `platform/`.

## Platform Helpers

| Function | Purpose |
| --- | --- |
| `browser_open_command()` | Return the OS-appropriate browser launch command |
| `home_dir()` | Resolve the user's home directory |
| `config_dir()` | Resolve the empack config directory |
| `data_dir()` | Resolve the empack data directory |
| `cache_root()` | Resolve the empack cache directory |

Current browser commands:

- macOS: `open`
- Linux: `xdg-open`
- Windows: `cmd /c start ""`

## Cache Root Resolution

`cache_root()` resolves in this order:

1. `EMPACK_CACHE_DIR`
2. platform-standard `ProjectDirs` cache location
3. temp directory fallback

Managed jars and managed binaries both live under this cache root.

Current cache layout:

- `<cache_root>/bin/` for managed `packwiz-tx`
- `<cache_root>/jars/` for managed build jars
- `<cache_root>/restricted-builds/` for restricted-download continuation cache
- `<cache_root>/versions/` for persisted loader/version cache JSON
- `<cache_root>/http/` for persisted HTTP cache
- `<temp>/empack-bin/` for staged managed binaries used when cache paths are not executable

## System Resources

`SystemResources::detect()` collects:

- logical CPU core count
- total memory
- available memory
- derived memory pressure

`calculate_optimal_jobs()` derives a bounded job count from those signals. This logic is available to networking and build-adjacent code, even though not every CLI path consumes it yet.

## Managed packwiz-tx Resolution

`platform/packwiz_bin.rs` is the current source of truth for `packwiz-tx` discovery.

Resolution order:

1. `EMPACK_PACKWIZ_BIN`
2. PATH lookup for `packwiz-tx`
3. managed cached binary under the empack cache root

Current pinned values in code:

| Constant | Value |
| --- | --- |
| `PACKWIZ_TX_REQUIREMENT` | `>=0.2.0, <0.3.0` |
| `PACKWIZ_TX_VERSION` | `v0.2.0` |

Managed downloads come from GitHub releases for `mannie-exe/packwiz-tx`.

## Managed Download Behavior

Current managed install behavior:

- cache path: `<cache_root>/bin/packwiz-tx-v0.2.0/`
- install lock directory: `.install.lock`
- download tool: `curl`
- archive format: platform-specific release tarball
- Unix executable bit is set after extraction
- a failed rename falls back to copy

If the cached binary exists and is executable, empack reuses it without downloading again.

## Execution Staging

Some systems mount cache directories with `noexec`.

Current behavior:

- probe the cached binary with `--help`
- if the probe fails, copy the binary into the system temp directory
- probe the staged copy
- use the staged copy if it runs successfully

This staging behavior is part of the supported runtime path, not a test-only fallback.
`empack clean cache` removes both the cache-root data and the staged temp binary directory.

## Session Integration

`CommandSession` resolves the packwiz binary once at session construction and exposes the result through `Session::packwiz_bin()`.

Callers that execute packwiz commands should use that accessor instead of the bare `PACKWIZ_BIN` constant.
