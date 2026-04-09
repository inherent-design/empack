---
spec: session-providers
status: draft
created: 2026-04-04
updated: 2026-04-08
depends: [overview]
---

# Session and Provider Architecture

Commands operate on `&dyn Session`. The session owns runtime seams for filesystem, networking, process execution, prompting, display, packwiz integration, archive handling, and state access.

## Session Trait

`Session` accessors are the command layer's only supported path to side effects.

| Accessor | Provider | Responsibility |
| --- | --- | --- |
| `display()` | DisplayProvider | Terminal output, progress bars |
| `filesystem()` | FileSystemProvider | File I/O, config manager |
| `network()` | NetworkProvider | HTTP client, project resolver, host rate budgets |
| `process()` | ProcessProvider | External command execution |
| `config()` | ConfigProvider | App configuration |
| `interactive()` | InteractiveProvider | User prompts (dialoguer) |
| `terminal()` | TerminalCapabilities | Terminal size, color detection |
| `archive()` | ArchiveProvider | Zip/tar creation and extraction |
| `packwiz()` | PackwizOps | Packwiz CLI operations |
| `state()` | PackStateManager | State machine queries |
| `packwiz_bin()` | `&str` | Resolved `packwiz-tx` binary path |

## Provider Traits

### FileSystemProvider

Methods cover current directory lookup, text and binary reads and writes, existence checks, directory creation, file listing, build artifact detection, and file or directory removal.

`config_manager(workdir)` is the bridge from raw filesystem access to manifest and pack metadata operations.

### NetworkProvider

`NetworkProvider` exposes three concerns:

- `http_client()` returns a cloned `reqwest::Client`
- `project_resolver()` builds a `ProjectResolverTrait`
- `rate_budgets()` exposes the shared `HostBudgetRegistry`

The live provider owns an `HttpCache`, a `RateLimiterManager`, and the shared rate-budget registry used by import and search flows.

### ProcessProvider

`ProcessProvider` supports:

- `execute()`
- `execute_streaming()`
- `find_program()`

`execute_process_with_live_issues()` wraps `execute_streaming()` with an `IssueStreamObserver` that forwards warning and error-like subprocess lines to the display layer while the command is still running.

### ConfigProvider

`ConfigProvider` is a thin accessor around `AppConfig`.

### ArchiveProvider

`ArchiveProvider` covers zip extraction and archive creation for `zip`, `tar.gz`, and `7z`.

### InteractiveProvider

`InteractiveProvider` supports:

- `text_input()`
- `confirm()`
- `select()`
- `fuzzy_select()`

The live implementation short-circuits to defaults in `--yes` mode or when stdin and stdout are not TTYs. Ctrl+C handling restores the cursor, flushes telemetry, removes the state marker when possible, and exits with status `130`.

## Session Construction

`CommandSession::new_async()` is the standard live entry point.

Construction steps:

1. Resolve `packwiz_bin_path` with `platform::packwiz_bin::resolve_packwiz_binary()`.
2. Detect terminal capabilities from `AppConfig.color`.
3. Initialize display and logger systems.
4. Create live filesystem, network, process, config, and interactive providers.
5. Expose packwiz operations through `LivePackwizOps`, bound to the resolved binary path.

If managed binary resolution fails, the session logs a warning and falls back to the bare `packwiz-tx` program name for PATH lookup.

## Mock Infrastructure

| Mock | Backing | Capabilities |
| --- | --- | --- |
| MockFileSystemProvider | In-memory HashMap | Deferred files on create_dir_all |
| MockNetworkProvider | Canned clients and resolver hooks | Shared test budgets and resolver injection |
| MockProcessProvider | Pre-registered arg-to-result map | Side effects: materialize .pw.toml, mrpack exports, Java installer |
| MockArchiveProvider | Spy vectors | Records create/extract calls |
| MockInteractiveProvider | VecDeque queue + fallback | Pre-programmed prompt responses |
| MockPackwizOps | Structured packwiz behavior | Init, refresh, installed-mod discovery, jar cache paths |

### Session types

| Type | Role |
| --- | --- |
| `MockCommandSession` | Fully in-memory default test session |
| `MockSessionBuilder` | Builder API for mock session composition |
| `CommandSession::new_with_providers()` | Mixed provider injection for test-utils builds |

`new_with_providers()` is useful for mixed live and mock tests, but it still hardcodes `LiveArchiveProvider`.

## E2E Boundary

Subprocess E2E tests bypass the session injection layer and execute the compiled `empack` binary through `assert_cmd` and `expectrl`. That path validates CLI parsing, session construction, logger setup, process exit behavior, and live tool integration together.

| Component | Purpose |
| --- | --- |
| `TestProject` | Isolated TempDir + `cmd()` builder with NO_COLOR |
| `empack_bin()` | Binary resolution: EMPACK_E2E_BIN, llvm-cov, debug, release, then PATH |
| `empack_assert_cmd()` | assert_cmd Command from resolved binary |
| `skip_if_no_packwiz!()` | Skip macros for missing prerequisites (chained) |

### Abstraction Gaps

- `CommandSession` stores `LiveArchiveProvider` directly. Mixed-provider construction cannot replace it today.
- Display is initialized through `LiveDisplayProvider` and the global display singleton. Unit tests do not capture the same output path as subprocess E2E.
- Interrupt handling exits the process directly with status `130`.
- Some import helpers still open local archives with `std::fs::File`, which bypasses `FileSystemProvider`.
