---
spec: session-providers
status: partial
created: 2026-04-04
updated: 2026-10-02
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
In the standard async construction path, the HTTP cache is loaded from disk under the empack cache root and then persisted on cache mutation.

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

The live implementation short-circuits to defaults in `--yes` mode or when stdin and stdout are not TTYs. Interrupted prompts return a typed cancellation error. Providers never terminate the host process or remove interruption markers.

The executable owns the signal listener, logger lifecycle, cursor restoration, and exit status. Each live command receives its own cancellation token. Child exit and asynchronous pipe drainage share a deadline. Unix children run in a process group. Windows children start suspended, join an owned kill-on-close job, and resume only after registration succeeds. The host must permit nested job registration; otherwise the command returns an error before child code executes. The job retains ownership of descendants after their ancestors exit. Cleanup closes readers and terminates owned children. The synchronous provider interface waits for its process worker to finish before returning. Output is limited to 16 MiB per stream.

## Session Construction

`CommandSession::new_async()` is the standard live session entry point.
The executable loads dotenv before parsing CLI arguments; `run_with_config()`
validates configuration and makes the working directory absolute before entering
the main loop and constructing the session.

Construction steps:

1. Allocate an empty cache for the packwiz binary path.
2. Detect terminal capabilities from `AppConfig.color`.
3. Initialize display access; the executable initializes logging.
4. Create live filesystem, network, process, config, archive, and interactive providers.
5. Load the live HTTP cache from `<cache_root>/http`.
6. Expose packwiz operations through `LivePackwizOps`. Resolve and cache the binary path when execution first requires it; inventory reads and version display do not bootstrap tools.

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
| `CommandSession::new_with_providers()` | Mixed provider injection for test-utils builds, including archive provider replacement |

`new_with_providers()` is useful for mixed live and mock tests when only some seams need replacement.

Current test scaffolding no longer treats archive, packwiz, or state access as intentionally unimplemented seams. The shared test session path exposes those providers so command tests can exercise the same contract surface as live code.

## E2E Boundary

Subprocess E2E tests bypass the session injection layer and execute the compiled `empack` binary through `assert_cmd` and targeted `expectrl` PTY coverage. That path validates CLI parsing, session construction, logger setup, process exit behavior, and live tool integration together.

| Component | Purpose |
| --- | --- |
| `TestProject` | Isolated TempDir + `cmd()` builder with NO_COLOR |
| `empack_bin()` | Binary resolution: EMPACK_E2E_BIN, active coverage llvm-cov, debug, release, then PATH |
| `empack_assert_cmd()` | assert_cmd Command from resolved binary |
| `skip_if_no_packwiz!()` | Chained prerequisite checks; fail when EMPACK_E2E_STRICT is set |

Current PTY scope is intentionally limited:

- one active interactive `init` PTY test validates resulting project state
- one prompt-sequence PTY test remains manual-only
- one active restricted-build PTY test validates browser-confirm reachability and persisted pending state
- one Unix-only PTY test validates browser-opener invocation through a fake platform opener
- one Unix-only PTY test validates successful restricted-build auto-continue after a watched manual download appears
- injected session-provider tests still cover browser confirmation semantics without relying on raw PTY prompt matching

### Abstraction Gaps

- Display is initialized through `LiveDisplayProvider` and the global display singleton. Unit tests do not capture the same output path as subprocess E2E.
- Display error suppression still uses a process-global flag; command outcomes are not yet a complete structured reporting API.
- `LiveFileSystemProvider` is path-transparent. Filesystem safety guarantees come from command/workflow constraints, not from a sandboxed provider boundary.
- Public compatibility wrappers such as `parse_curseforge_zip(path)` and `parse_modrinth_mrpack(path)` still expose path-based parsing APIs, even though live command paths now read archive bytes through `FileSystemProvider`.

## Document Publication and Mutation Locks

`write_atomic()` publishes manifests, state markers, and continuation records through a same-directory temporary file. The live implementation preserves existing permissions, refuses read-only files and symlinks, syncs file contents, replaces the destination, and syncs the parent directory on Unix. A post-publication durability error explicitly reports that publication already occurred.

Live mutating commands hold an OS-backed project lock through execution and cleanup. Initialization locks its resolved target directory. Locks use canonical project paths and persistent files under the application data directory, separate from cleanable caches. Dry-run creates no project lock or project files. Provider implementations used outside the live command path must supply their own mutation ownership; the mock provider uses an in-memory no-op guard.

These contracts prevent concurrent empack command mutations and partial document replacement. They do not provide rollback across packwiz, the manifest, and generated artifacts, or coordinate arbitrary external editors. Journaled multi-file recovery remains future work.

`PackwizOps::installed_snapshot` exposes observed provider identity, version and
metadata filename for reconciliation. Filename membership alone is insufficient
for the live planner. `verify_reconciled` reads a new snapshot after execution.
The legacy mock models declared installed names and records subprocess calls;
CLI smoke tests with real files establish alias matching, required-content
retention, pin convergence and detection of false backend success.
