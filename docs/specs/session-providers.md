---
spec: session-providers
status: draft
created: 2026-04-04
updated: 2026-04-04
depends: [overview]
---

# Session and Provider Architecture

All side effects go through typed provider traits. Provider selection happens at session construction. Tests swap live providers for mocks.

## Session Trait

The `Session` trait (session.rs) is the root accessor for all providers. Command handlers accept `&dyn Session`.

| Accessor | Provider | Responsibility |
|----------|----------|---------------|
| `display()` | DisplayProvider | Terminal output, progress bars |
| `filesystem()` | FileSystemProvider | File I/O, config manager |
| `network()` | NetworkProvider | HTTP client, project resolver |
| `process()` | ProcessProvider | External command execution |
| `config()` | ConfigProvider | App configuration |
| `interactive()` | InteractiveProvider | User prompts (dialoguer) |
| `terminal()` | TerminalCapabilities | Terminal size, color detection |
| `archive()` | ArchiveProvider | Zip/tar creation and extraction |
| `packwiz()` | PackwizOps | Packwiz CLI operations |
| `state()` | PackStateManager | State machine queries |

## Provider Traits

### FileSystemProvider (14 methods)

`current_dir`, `config_manager`, `read_to_string`, `read_bytes`, `write_file`, `write_bytes`, `exists`, `metadata_exists`, `is_directory`, `create_dir_all`, `get_file_list`, `has_build_artifacts`, `remove_file`, `remove_dir_all`.

### NetworkProvider (2 methods)

`http_client() -> reqwest::Client`, `project_resolver(client, curseforge_api_key) -> Box<dyn ProjectResolverTrait>`.

### ProcessProvider (2 methods)

`execute(command, args, working_dir) -> ProcessOutput`, `find_program(program) -> Option<String>`.

### ArchiveProvider (2 methods)

`extract_zip(archive_path, dest_dir)`, `create_archive(source_dir, dest_path, format)`.

### InteractiveProvider (4 methods)

`text_input`, `confirm`, `select`, `fuzzy_select`.

## Mock Infrastructure

| Mock | Backing | Capabilities |
|------|---------|-------------|
| MockFileSystemProvider | In-memory HashMap | Deferred files on create_dir_all |
| MockNetworkProvider | Canned responses HashMap | Failing or mapped HTTP client |
| MockProcessProvider | Pre-registered arg→result map | Side effects: materialize .pw.toml, mrpack exports, Java installer |
| MockArchiveProvider | Spy vectors | Records create/extract calls |
| MockInteractiveProvider | VecDeque queue + fallback | Pre-programmed prompt responses |

### Session Builders

| Builder | Providers | Cross-platform |
|---------|-----------|---------------|
| MockSessionBuilder | All mocks | Yes |
| HermeticSessionBuilder | Real FS + real process + mock network | Unix only (shell scripts) |
| CommandSession::new_with_providers | Mixed (real FS + mock process typical) | Yes, but ArchiveProvider hardcoded to live |

### Abstraction Gaps

- `LiveArchiveProvider` is hardcoded in `CommandSession`; cannot inject `MockArchiveProvider` through `new_with_providers`.
- `LiveDisplayProvider` is not pluggable; stdout/stderr not capturable in tests.
- `std::process::exit(130)` on Ctrl+C; interrupt recovery not testable.
- Some import functions use `std::fs::File::open` directly, bypassing `FileSystemProvider`.
