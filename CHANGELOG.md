# Changelog

All notable changes to empack are documented in this file.


## v0.4.0-alpha.4 - 2026-04-12

### Features

- Stabilize cli contracts and tracked local deps

### Bug Fixes

- Preserve reported restricted download paths
- Tighten exit and restricted parser heuristics
- Align zip type exit handling and build validation
- Suppress duplicate process error output
- Classify local dependency validation as usage
- Harden restricted parsing and local removal
- Bound subprocess marker cleanup
- Guard local key collisions and empty urls
- Guard clean and restricted build edges
- Harden local dependency validation

### Testing

- Backfill reviewer lint and coverage pass
- Stabilize interactive browser opener e2e
- Align init e2e with display error output

### Documentation

- Consolidate specs and drop stale audit

### CI/CD

- Split branch, pr, and post-merge workflows

### Maintenance

- Update changelog for 0.4.0-alpha.3
- **(deps)** Update rust crate sha2 to 0.11

## v0.4.0-alpha.3 - 2026-04-10

### Bug Fixes

- Make clean non-destructive
- Recover interrupted clean state

### Maintenance

- Update changelog for 0.4.0-alpha.2

## v0.4.0-alpha.2 - 2026-04-10

### Features

- Land smoke continuation and coverage hardening

### Bug Fixes

- Harden windows e2e and cleanup
- Repair session command resolution build
- Tighten continue build cli contract
- Dedupe post-continue restricted counts
- Copy binary build template assets

### Testing

- Relax windows path casing assertion
- Stabilize direct download wrapper failures

### Documentation

- Regenerate specs and align runtime docs

### Maintenance

- Update changelog for 0.4.0-alpha.1

## v0.4.0-alpha.1 - 2026-04-09

### Features

- Add telemetry instrumentation with tracing-chrome and OTLP
- Add wide structured events at command completion
- Managed packwiz-tx binary auto-download
- Wire --no-refresh into import and sync pipelines
- Add adaptive resolve pacing and live output

### Bug Fixes

- Per-layer filtering and Chrome flush in telemetry
- Dep-key regression, semaphore check, filter dedup
- Stderr TTY detection and telemetry clippy in CI
- CF key guard on cobblemon E2E, codecov PR comments, stem mismatch warn
- Graceful skip for telemetry trace test without feature
- Relax live import pw.toml count assertions
- Stop swallowing import failures; add retry with backoff
- Increase retry backoff for Modrinth 429 rate limits
- Address P2 review findings in import pipeline
- Lowercase pw.toml stems in diagnostic check to match derive_dep_key
- Include custom datapack folder in pw.toml stem scan
- Wire managed packwiz-tx binary path to all execution callsites
- Replace reqwest::blocking with curl for packwiz-tx download
- Use managed resolver for has_packwiz; add PATH lookup tier
- Only use --offline when pre-resolved metadata is complete
- Use PACKWIZ_TX_VERSION constant in requirements E2E assertion
- Verify packwiz-tx binary works instead of comparing path strings
- Log packwiz-tx resolution errors; silence curl progress
- Check packwiz refresh exit code after batch operations
- Satisfy clippy in streaming process loop
- Tighten live issue filtering and CF pacing
- Harden rate budget rollover accounting
- Stage managed packwiz binary for linux
- Preserve future budget reservations
- Stabilize cross-platform test and packwiz flows
- Restore shared dotenv loading
- Break smoke pty loop on eof
- Gate legacy loader boundaries
- Harden runtime edge paths
- Tighten release hardening cleanup

### Testing

- **(e2e)** Add live modpack import and interactive tests
- **(e2e)** Add telemetry chrome trace verification
- **(e2e)** Verify packwiz-tx managed binary in requirements output
- Expand rate budget coverage
- Expand runtime coverage across slices
- Expand runtime coverage
- Harden platform-specific CI coverage
- Harden CI platform env coverage
- Stabilize interactive ci flow
- Split interactive pty and input coverage
- Tighten env and import e2e guards
- Tighten final review coverage fixes
- Scope cli env cleanup in loader tests
- Guard cache env overrides
- Raise headroom for live import builds
- Tighten final greptile cleanup

### Performance

- Concurrent mod resolution in import pipeline
- Batch filesystem scans in import content loop
- Use --offline flag to skip packwiz API calls in batch imports

### Refactoring

- Simplify terminal detection; delegate to console crate
- Migrate from packwiz to packwiz-tx
- Rename modpack-survey.py to import-smoke-test.py

### CI/CD

- Add telemetry feature check to lint job
- Deduplicate PR+push runs and bump test timeout
- Fix codecov comments, prettier coverage summary
- Make patch coverage informational, not blocking
- Enable telemetry feature in coverage pipeline
- Unify push and pr concurrency keys
- Restore original concurrency key

### Maintenance

- Update changelog for 0.3.0-alpha.3
- Remove packwiz-tx from mise; will be managed binary
- Use mise depends for binary build before test/e2e tasks
- Gitignore python cache

### Other

- Revert "ci: make patch coverage informational, not blocking"

This reverts commit cbf232a86604d156fd2ed6584aba41cb70b6c531
- Format fake packwiz helper

## v0.3.0-alpha.3 - 2026-04-06

### Features

- **(test)** Scaffold live E2E harness with assert_cmd and expectrl
- **(test)** Add TestProject, skip macros, and empack_cmd builder
- **(test)** Enable E2E coverage via instrumented binary resolution
- Add modpack survey script for import compatibility testing
- **(config)** Add datapack_folder and acceptable_game_versions to empack.yml
- **(init)** Add --datapack-folder and --game-versions CLI flags
- **(import)** Auto-detect datapack folder and route CF datapacks
- **(build)** Parse packwiz-installer output for CF restricted mods
- **(build)** Add --downloads-dir flag and interactive browser open for restricted mods
- **(build)** Scan downloads dir for restricted mod files and auto-place
- Implement CurseForge URL modpack download; add sync progress bar

### Bug Fixes

- Return Err on error conditions instead of Ok(())
- **(test)** Reduce MockNetworkProvider HTTP timeout from 30s to 1ms
- Windows binary discovery, CI build step, clippy, version string
- **(ci)** Build binary before all tests, add sudo for macOS packwiz
- **(ci)** Remove cargo tools from mise, use taiki-e for nextest/llvm-cov
- **(ci)** Add Go for packwiz build, build binary before all tests
- **(test)** Gate HermeticSessionBuilder test on unix
- Exclude E2E from test task to avoid double-run; add exit code assertion
- Gate live API test, remove redundant build/env steps
- **(ci)** Build instrumented empack binary before coverage tests
- **(ci)** Use show-env to build instrumented binary for coverage
- **(ci)** Use eval instead of bash process substitution for sh compat
- **(ci)** Plain cargo build before llvm-cov nextest
- Use ProcessOutput::error_output() for packwiz error reporting
- **(import)** Resolve mrpack platform refs via SHA1 and CurseForge batch lookup
- Remove unused variables in modpack survey script
- Validate both ForgeCD URL segments as numeric; read CF key from env
- **(import)** Detect CF datapacks (classId 6945) in detect_datapack_folder
- Gate datapack folder prompt on --yes; short-circuit write_pack_toml_options when both params are None
- **(import)** Detect datapack folder before writing empack.yml
- Point badges at main branch
- **(ci)** Commit changelog to main instead of dev on release
- CF restricted mod detection now queries API instead of .pw.toml
- Use filesystem abstraction for copy, check retry results, scan-ahead parser
- Restricted parser, --type datapack, --dry-run init, net_timeout, progress bars
- **(build)** Pass restricted mod results through pipeline instead of dropping
- Deduplicate restricted mods across targets; guard empty URL; remove duplicate doc comment
- Address PR review findings
- **(ci)** Instrument empack binary for E2E coverage
- **(ci)** Add --no-clean to coverage nextest invocation
- **(ci)** Use hardcoded CF key for E2E tests

### Testing

- **(e2e)** Add init and build subprocess tests
- **(e2e)** Add subprocess tests for add command and interactive init
- **(e2e)** Add codegen matrix tests via macros
- Delete test files replaced by E2E subprocess tests
- Strengthen weak assertions; update testing docs
- Add datapack folder detection and CLI flag tests
- **(e2e)** Add import+build lifecycle tests; writing guidelines pass
- Add unit tests for parse_installer_restricted_output and format_empack_yml
- Dead test cleanup and codegen matrix expansion
- Consolidate build tests and add E2E coverage
- Add 110 unit tests for coverage gaps

### Documentation

- Update specs and bootstrap for live E2E harness
- Update CONTRIBUTING.md for mise-based workflow
- Remove stale v1/v2 reference from project structure

### Refactoring

- Inline mise tasks, add packwiz/nextest to tools, use mise-action in CI
- **(test)** Remove HermeticSessionBuilder and dead infrastructure
- Deduplicate format_empack_yml; guard empty CF project_id in resolve
- Remove broken CF restricted mod pre-flight scan
- Collapse command handler params into Args structs
- Delete dead code modules

### CI/CD

- Unify CI workflows; add cross-platform E2E and coverage
- **(release)** Generate and commit full changelog on release
- Add Codecov integration and fix coverage summary formatting

### Maintenance

- Move archives to mannie-exe/empack-archive
- **(deps)** Update sha1 0.11, sha2 0.11, serde-saphyr 0.23, actions v5
- Remove unused sha2 dep; unify hex encoding via content::hex
- **(deps)** Update github artifact actions
- Set workspace version to 0.0.0-dev; inject from tag at release time
- Gitignore lcov.info coverage artifact

## v0.2.0-alpha.2 - 2026-04-05

### Features

- V0.2.0-alpha.1 release

### Bug Fixes

- **(import)** Correct packwiz flags in add_platform_ref
- **(cli)** Rename --from-source flag to --from
- **(sync)** Derive dep keys from packwiz .pw.toml filenames
- Require .pw suffix in toml scan; correct doc inconsistencies
- **(import)** Use project-id and version-id for Modrinth packwiz add

### Documentation

- Rewrite testing.md for two-tier test architecture
- Update CONTRIBUTING.md for two-tier test architecture

### Maintenance

- Bump workspace version to 0.2.0-alpha.1

### Other

- Add renovate.json

## v0.2.0-alpha.1 - 2026-04-04

### Features

- **(content)** Add UrlKind classifier, JarResolver, and side types
- **(import)** Add modpack import pipeline (Track A)
- **(add)** Version pin flags, URL-based add, sync contract evolution

### Bug Fixes

- Resolve single-element loop clippy warning in template installer
- Address review findings across import, content, and add pipelines
- Zip slip, version ID lookup, batch dedup, dead code, path classifiers
- **(import)** Correct CurseForge project resolution endpoint and auth
- **(content)** Correct CurseForge fingerprint endpoint in ApiJarResolver
- Modrinth version-file algorithm param and mrpack override extraction
- Correct CurseForge classId mappings and add Modrinth URL patterns
- **(content)** Use file.id for CurseForge file ID, not fingerprint hash
- **(import)** Write DependencyRecord for all imported platform refs
- **(import)** Add fileSize serde rename; expand VCR for v0.2 endpoints
- **(import)** Add versionId serde rename for mrpack manifest

### Testing

- Add URL classification tests and fix clippy warning
- Harden v0.2.0-alpha.1 test coverage
- Record v0.2 VCR cassettes from live APIs
- Add API contract tests and fix mrpack fixture field names

### Documentation

- Remove 'empack sync' from README.md start
- Remove stale/historical ADR
- Add doc comments, scoped commits, agent guidelines, changelog to CONTRIBUTING
- Add behavioral spec decomposition for empack

### Maintenance

- Comment+docs clean-up
- Remove obvious comments in import executor
- Remove comments that restate what the code says

## v0.1.0-alpha.4 - 2026-03-30

### Bug Fixes

- Rename Init positional field to dir, filter dot path defaults
- Resolve actual directory basename for dot-path positional args

### Refactoring

- Separate directory resolution from pack name in init

## v0.1.0-alpha.3 - 2026-03-29

### Bug Fixes

- Move template scaffolding after state transition in init

### Maintenance

- Update changelog

## v0.1.0-alpha.2 - 2026-03-29

### Bug Fixes

- Gate locale tests on unix, remove stale DimensionSource variants
- Real terminal size detection, remove empty capabilities module

### Refactoring

- Remove terminal probing, enforce NO_COLOR, capability-driven display
- Consolidate platform module, remove dead capabilities
- Display tests, un-ignore terminal tests, break circular dep

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
- Add changelog and reference in README

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
- Use macos-26 and macos-26-intel runners

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
