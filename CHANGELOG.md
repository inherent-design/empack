# Changelog

All notable changes to empack are documented in this file.


## v0.1.0-alpha.1 - 2026-03-29

### Features

- Implement loader-first architecture
- Bootstrap Rust project infrastructure
- Establish layered module architecture
- Implement comprehensive shared type system
- Implement domain-specific data structures
- Implement comprehensive configuration system
- Add stubs for runtime detection
- Add project and testing config
- Implement shared type system with errors
- Implement terminal capability detection
- Add networking and platform modules
- Switch CI to cargo binstall
- Add cross-platform tool detection + CLI commands
- Add display module for UI
- Add forge api support to v1 library
- Implement core mod management commands
- Complete session-based architecture
- Complete abstraction for external commands
- Add Apache 2.0 license
- **(api)** Implement DependencyGraph with transitive resolution
- Begin v4 internal refactor
- Add packwiz, integrations tests, builds
- **(test)** Extract test infrastructure helpers
- Publish default CF API key
- **(sync)** Add sync planning and execution logic for dependency managem
- **(init)** Expose non-interactive flags
- **(empack)** Make add/remove atomic by updating empack.yml directly
- Add terminal safety net for cursor restoration on interrupt
- Add 5-minute timeout to child process execution
- Add enforced state transition validation
- Add enforced state transition validation
- Validate init inputs from CLI flags against fetched version lists
- Add --loader-version CLI flag for fully unattended init
- Native server JAR download for all loaders, eliminate mrpack-install and curl
- Self-contained Fabric server via installer, fix Quilt srv.jar rename
- Download ServerStarterJar as srv.jar for NeoForge and Forge builds
- Tar fallback for distribution packaging, fix archive requirements reporting
- Two-phase search with incompatibility notice
- Vanilla (no modloader) init support
- Native archive operations, type-aware search, multi-type add
- --format flag for build, remove -j conflict, type-less direct ID adds
- Wave 2 fixes for alpha.1 release gate
- Cache-first download and with_pre_cached_jars helper
- Java installer side-effect hooks for MockProcessProvider
- MockSessionBuilder for cross-platform integration tests
- ArchiveProvider trait for testable archive operations
- Unify template system and wire into init
- Add release.yml template and remove empack CI dependency

### Bug Fixes

- Improve test reliability and workflow
- Windows BOOL import for v0.60
- Wrap GlobalMemoryStatusEx return in BOOL
- Compare BOOL to false instead of 0
- Implement proper Windows locale detection
- Minimal test and error clean-up
- NeoForge versioning; reduce version input
- **(versions)** Harden loader discovery and metadata resolution
- **(build)** Tighten build result contracts
- **(tests)** Align missing-installer assertions and pre-seed JAR for template test
- **(tests)** Align missing-installer assertions and pre-seed JAR for template test
- **(tests)** Update bootstrap invocation assertions for v1 pattern
- Resolve clippy violations and PR review findings for CI green
- Resolve clippy violations and PR review findings for CI green
- Remove redundant trim before split_whitespace
- Collapse nested if in discover_state to satisfy clippy
- Prevent state machine stuck on build pipeline failure
- Correct cycle detection return and clean version_overrides on remove
- Find_program respects custom_path for hermetic testing
- Deduplicate coverage test run in CI workflow
- Add EOF newlines to workflow files and document packwiz cwd rationale
- Query macOS page size at runtime via sysconf
- Handle Ctrl+C during dialoguer prompts as clean exit
- Use byte I/O for binary JAR file copying in build pipeline
- Address PR review findings for cursor guard and workdir
- Guard terminal escapes with is_terminal and unify interrupt cleanup
- Add biased; to tokio::select! to prevent race on clean exit
- Discriminate SIGINT from other EINTR in prompt interrupt handlers
- Write cursor escape sequences to both stdout and stderr
- Remove broken sigint_received() check from prompt interrupt handlers
- Use parent directory for git config lookup when target dir doesn't exist
- Kill child process on timeout and fix guard disarm ordering
- Platform-aware timeout message, deduplicate transition validation, add #[must_use] to guard
- Enforce layout validation for Cleaning marker transition
- Replace wildcard with explicit match arms in Clean transition
- Replace path unwrap calls with to_string_lossy in builds.rs
- Init without positional name now creates subdirectory
- Address PR review findings for init validation
- Cross-platform cache isolation for hermetic tests
- Add cfg(unix) gates for Windows compilation
- Cross-platform test paths and cfg-gate unix-only E2E tests
- Address PR #12 review findings (G2, G3, M1, M2)
- Prevent deadlock in process output handling with polling-based wait
- Remove dead build failure check and fix mock script arg matching
- Quilt version list, packwiz abstraction leak, sync display counter
- Marker content validation, unwrap cleanup, workspace dep, shadow removal
- Reject pinned project_id without explicit project_platform
- Address PR review findings across session, state, search, builds, config
- Sync orphan removal, state marker persistence, release cache
- Packwiz remove -y flag, add_dependency slug protection, search fallback, mock escaping
- Thread preferred_platform through add path, relax Modrinth ID check, hoist loop alloc
- Derive dep_key from packwiz .pw.toml filename, not user input
- Remove false-positive Modrinth ID heuristic, strengthen test assertions
- Serde-based YAML init, track planning-phase sync failures
- Complete non-interactive init, tighten packwiz file filter
- Preserve loader_version round-trip, deterministic config ordering
- Map neoforge to forge for mrpack-install, allow build retry after interruption
- Dry-run threading, clean detection, init cleanup, handle_remove state guard
- Binary file corruption in copy_dir_contents, dry-run clean ordering, hardcoded timeout
- Quilt loader_version passthrough, SHA1 test coverage, runtime guard
- Report cleanup failure in init error recovery
- Quilt positional loader arg, temp-mrpack cleanup, log omission, requirements update
- Clean up temp-mrpack-extract on build failure
- Archive EmptySource consistency, GzEncoder flush, docs, test quoting
- Retry with exponential backoff for server JAR downloads
- Resolve clippy warnings in session_mocks
- Address PR review findings
- Clean up orphaned .pw.toml after CF restricted mod detection
- Address second round of PR review findings
- Guard remove_file on packwiz remove failure
- Cross-platform packwiz cache directory resolution
- Route mods_dir existence check through session filesystem
- Route Go detection through session ProcessProvider
- Propagate errors from handle_remove
- Return Err from handle_init on existing project and empty loaders
- Address PR #17 review findings
- Resolve clippy type_complexity warning
- Address PR #18 review findings
- Address PR #18 review round 2
- Address PR #18 review round 3
- Address PR #19 alpha.1 release review findings
- Remove FABRIC_VERSION template alias; clean break to LOADER_VERSION
- Clean archive resolution and symlink safety
- Box BuildOrchestrator in StateTransition to fix large_enum_variant
- Create dist/ before writing server archive in release template
- Disable HTML escaping, gitignore archives, align PR triggers

### Testing

- Complete migration to separate test files
- Coverage and error fixes
- **(commands)** Format test code and improve mock setup
- Improve hermetic E2E testing with mock HTTP client and packwiz ass
- **(commands)** Improve init command tests with comprehensive assertions
- **(sync)** Improve packwiz command assertions to handle multiple comman
- **(state)** Refactor malformed yaml test to use mock filesystem provide
- **(commands)** Update packwiz add command assertions to match new argum
- **(fixtures)** Add workflow project fixture and improve mock invocation
- **(sync)** Add comprehensive tests for sync plan building and contract
- **(application)** Tighten add and sync contract coverage
- **(mocks)** Add mrpack export side effects to process provider
- **(builds)** Validate artifact existence and isolate client-full builds
- **(contracts)** Tighten build contracts and close E2E workflow gaps
- **(coverage)** Add workflow for code coverage reporting with codecov
- Strengthen vacuous command tests with real assertions
- Remove phantom validation tests that exercise no real code
- Add missing packwiz assertions to build tests
- Consolidate duplicate build tests and add assertion messages
- Add 89 tests covering CLI input resolution, sync invariants, config serde, search error paths
- Remove orphaned and vacuous tests, strengthen weak assertions
- Queue-based MockInteractiveProvider for multi-step flows
- Wave 1 test creation for alpha.1 release gate
- Expanded hermetic matrix for init, add, and dry-run
- Add review fix coverage and update testing docs
- Remove vacuous integration tests
- Build matrix coverage for NeoForge, Quilt, Vanilla, Fabric client
- Migrate 8 init tests to cross-platform mock sessions
- Migrate 7 add/remove/sync tests to cross-platform mock sessions
- Migrate 18 build tests to cross-platform mock sessions
- Migrate 2 lifecycle tests to cross-platform mock sessions
- Verify release.yml registration and init scaffolding

### Documentation

- Add contributing guidelines for empack
- Add AI development guidelines
- Add commit style guidlines
- Update architectural direction
- Establish semantic style guidelines
- Update context with state machine + CLI implementation
- Checkpoint project documentation
- Remove old audits and organize
- Track AI cross-talk logs- premonition of what's to come
- Remove unused ai-guidelines
- Add comprehensive README with project overview and verification ma
- Reorganize README with quick-start section and improved documentat
- Clarify current source-of-truth guides
- Move architectural decision record to docs directory
- Reconcile spec and docs with completed wave checkpoints
- Reconcile spec and docs with completed wave checkpoints
- Add comprehensive architecture visualization (Excalidraw)
- Rebuild architecture visualization with proper Excalidraw structure
- Document rationale for pre-session current_dir usage in config validation
- Update stale test counts and consolidate cross-file duplication
- Restructure all markdown for progressive disclosure and deduplication
- Rewrite user-facing documentation in cfgate reference style
- Move ADR to references
- Update prerequisites and testing docs for unified srv.jar strategy
- Clarify sync usage in quickstart
- Update testing.md to reflect alpha.1 release candidate state

### Performance

- Cache TemplateEngine on BuildOrchestrator

### Refactoring

- Archive prototypes and preserve history
- Migrate config to structured errors
- Migrate to layered module architecture
- Multi-crate workspace
- Integrate state machine with business logic
- Implement session-based DI with provider pattern (Phase 0 complete)
- **(state)** Preserve source errors in StateError variants
- **(commands)** Consolidate templates and simplify conversions
- **(env)** Standardize to EMPACK_* prefix
- **(sync)** Extract add resolution logic into dedicated function
- **(add)** Extract resolution contract and improve state validation
- **(commands)** Add state validation for add and sync commands
- Remove dormant state, dead methods, and stale feature gates
- **(versions)** Replace hand-rolled version ordering with semver crate
- Improve error handling and add binary file support to filesyst
- Improve error handling and add binary file support to filesystem provider
- Extract packwiz methods from FileSystemProvider into PackwizOps trait
- Extract packwiz-specific methods from ProcessProvider into free functions
- Make Display::init idempotent and remove panic from Display::global
- Remove eprintln bypass in state.rs and surface warnings through return values
- Harden state machine with marker files, efficient discovery, and safe Session::state
- Remove expect() panics and route git config through ProcessProvider
- Route templates.rs through FileSystemProvider
- Replace eprintln in versions.rs with tracing::warn, route dependency_graph through FileSystemProvider
- Add find_program to ProcessProvider, harden packwiz ops and state recursion
- Unify cache directories under ProjectDirs
- Remove installer/ directory, use cache-only JAR resolution
- Replace Unix-only mock paths with cross-platform mock_root()
- Strip decorative emoji from user-facing command output
- Route summary symbols through terminal primitives
- Update init success message with add/rm workflow guidance
- Defer init side-effects past confirmation (ops-as-values)
- Batch add/remove operations before executing side-effects
- Consolidate MultiProgress ownership and add session Drop impl
- Add RAII StateMarkerGuard for build/clean pipelines
- Split marker transitions into pub(crate) MarkerKind
- Extract From impl for ModLoader conversion, fix test metadata
- Replace stringly-typed dep schema with DependencyEntry enum
- Dead code removal, fuzzy extraction, networking wiring, pagination (#13)
- Shared HTTP client and block_in_place for build downloads
- Platform module cleanup and Session terminal wiring
- Extract download_to_cache with retry for JAR downloads
- Extract fetch_loader_versions to collapse 4 duplicate arms
- Purge v1/v2 backward compatibility references

### Infrastructure

- **(deps)** Modernize workspace dependency surface
- **(empack-lib)** Add semver dependency

### CI/CD

- Add cross-platform testing with Windows support
- Add comprehensive CI workflow for Rust project
- Add release workflow for multi-platform binary builds and GitHub rel
- Add -- -D warnings to clippy, restrict build-check to Linux targets
- Add pull_request trigger to coverage, improve error handling and cod
- Remove dev from push trigger to prevent duplicate CI runs

### Maintenance

- Clean up development artifacts
- Add intent workspace configuration with build, test, and check sc
- Isolate cleanup-only churn and test alignment
- **(mise)** Add task configuration for build, test, and development wor
- **(build)** Add PowerShell build script for Windows development
- **(build)** Add shell build script for Unix-like systems
- **(build)** Consolidate run and check tasks to use unified build and c
- **(build)** Add remaining task scripts and git-cliff config
- Fix runtime failures and CI review findings
- Pin Rust 1.94.0 via rust-toolchain.toml
- Remove dead SIGINT_RECEIVED flag and related functions
- Remove unused CursorGuard and dead code from cursor module
- Simplify changelog header formatting in cliff.toml
- Update LICENSE
- Delete Excalidraw file
- **(gitignore)** Untrack Intent
- Remove stale curl mock from lifecycle_forge_full test
- Cargo fmt + collapse nested if for clippy
- Cargo fmt
- Cargo fmt
- Cargo fmt
- Update embedded templates for alpha.1

### Other

- **(networking)** Format trace macro call for readability
- Auto-sync empack.yml after empack add and empack remove

- handle_add: call handle_sync after successful add summary
- handle_remove: call handle_sync after successful remove summary
- cli.rs: add #[command(alias = "rm")] on the Remove variant

Agent-Id: agent-dc2bd0ba-cbd5-4173-bf88-2ea0d70a4c47
- Reformat code for improved readability
