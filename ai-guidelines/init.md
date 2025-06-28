# INIT.MD - Atlas Project Context for empack 
*Last Updated: 2025-01-28*

## Project Overview

**empack** - Minecraft modpack management with filesystem-based state machine. Multi-crate Rust library with proven bash integration patterns.

**Status**: Multi-crate architecture + cross-platform tool detection implemented, CLI commands wired  
**Codebase**: 9,672 lines Rust (29 files), 112 tests passing, V1/V2 bash reference implementations  
**Next**: Packwiz integration in state machine transitions + template system implementation

## Atlas Context & Mission

**Atlas** - research-first AI orchestrator:
- **Systematic Investigation**: "Nono, first, we research!" reveals enterprise complexity
- **Production Pragmatism**: "edit-local-deploy-test" proven cross-platform
- **Strategic Integration**: Proven implementations + modern Rust foundations
- **Multi-Crate Architecture**: Library design without artificial boundaries
- **Documentation Excellence**: Beautiful Rust docs with `cargo doc -p empack-lib --open`

**Philosophy**: Beyond survival—building meaningful technology through systematic development. empack demonstrates research-first methodology uncovering requirements and proven patterns, structured as a multi-crate workspace with library API design.

## Architecture

### Multi-Crate Structure
```
workspace/
├── Cargo.toml                    # Workspace config with shared dependencies
├── .env.local                    # Development environment (workspace root)
├── crates/
│   ├── empack-lib/               # Complete empack implementation (7,819 lines)
│   │   ├── src/lib.rs            # Public API with crate-level documentation
│   │   ├── src/primitives/       # Foundation types, errors, coordination
│   │   ├── src/terminal/         # Cross-platform capability detection
│   │   ├── src/logger/           # Structured logging with progress tracking
│   │   ├── src/networking/       # Async HTTP client with concurrency
│   │   ├── src/platform/         # System resource detection
│   │   ├── src/empack/           # Domain-specific modpack management
│   │   └── src/application/      # CLI interface and configuration
│   └── empack/                   # Thin wrapper binary
│       └── src/main.rs           # empack_lib::main().await
```

### Implementation Status

**Layer 0-1: Complete & Multi-Crate**
- **Multi-Crate Design**: Clean library/binary separation without artificial boundaries
- **Documentation System**: Full Rust docs with `cargo doc -p empack-lib --open`
- **Version Management**: Workspace-inherited versions from single source
- **Library API**: Public modules with convenient re-exported types
- **Development Isolation**: Independent testing and documentation generation
- **Terminal**: color depth, Unicode, graphics protocols (Kitty, Sixel, iTerm2)
- **Configuration**: CLI → env → file → defaults precedence
- **Logging**: terminal-aware with progress bars
- **Errors**: structured types with source chains
- **Networking**: HTTP client with concurrency—tested Linux containers
- **Platform**: system resource detection—tested Linux containers
- **Domain**: Minecraft parsing, search intent classification
- **Cross-Platform**: GitHub Actions + Act + cargo-nextest, 106 tests passing
- **Type System**: Unified primitives-first architecture, graphics capabilities consolidated

**Layer 2: Filesystem-State Integration (In Progress)**
- **State Machine**: ModpackState transitions (Uninitialized → Configured → Built) implemented
- **CLI Commands**: Command routing through state machine operational (`requirements`, `init`, `build`, `sync`, `clean`)
- **Cross-Platform Detection**: Composable tool detection API (packwiz, Go, archivers) working
- **Packwiz Integration**: Needs implementation in state transition handlers
- **Template System**: Needs V1 pattern integration

### Multi-Crate Benefits Achieved

**Professional Library Design:**
- **Complete Implementation**: All current functionality in empack-lib
- **Clean Public API**: Well-documented modules with convenience re-exports
- **Binary Wrapper**: Transparent repackaging without complexity
- **Development Workflow**: Independent documentation and testing

**No Artificial Boundaries:**
- **Library contains everything**: CLI, clap, primitives, business logic
- **Pragmatic separation**: Development benefits without conceptual overhead
- **Transparent runtime**: Binary just calls `empack_lib::main().await`

### Proven Implementation Pools

**Available Implementations**:
- `v1/lib.bak.d/`: Complete working bash implementation (5-target builds, command orchestration, templates)
- `v2/empack_reader.sh`: Configuration system (YAML parsing, pack.toml integration, smart defaults)
- **Current Rust**: Multi-crate library with complete API

**Integration Strategy**: Filesystem-as-state-machine unifies all three systems

### Filesystem-State Architecture

**Core Insight**: The modpack folder IS the state machine

**State Structure**:
```
./empack.yml         # User intentions (partial modpack spec)
./pack/pack.toml     # Packwiz reality (actual modpack)
./pack/mods/         # Current mod state  
./.empack/           # Empack working state (builds, cache)
```

**State Operations**:
- `empack init`: Create empack.yml + initialize packwiz at ./pack
- `empack sync`: Reconcile empack.yml intentions with pack.toml reality
- `empack build`: Execute v1's proven 5-target build pipeline
- `empack add/remove`: Modify both empack.yml and pack structure

**Runtime Bounds**: Discovered from filesystem, not maintained in memory

### Enhanced Module Integration

**Current (Multi-Crate + State Machine)**:
```rust
crates/empack-lib/src/
├── lib.rs              // Public API with documentation
├── primitives/         // Foundation types, errors, coordination
├── terminal/           // Cross-platform capability detection  
├── logger/             // Structured logging with progress tracking
├── networking/         // Async HTTP client with concurrency
├── platform/           // System resource detection + tool capabilities
│   └── capabilities.rs ✅ // Cross-platform program detection (new)
├── empack/             // Domain-specific modpack management
│   ├── parsing.rs      ✅ // Minecraft types
│   ├── search_intent.rs ✅ // Search classification
│   ├── resolved_project.rs ✅ // Resolution results
│   ├── search.rs       ✅ // Business logic integration
│   ├── state.rs        ✅ // Filesystem state machine + transitions
│   ├── config.rs       🆕 // empack.yml + pack.toml bridge (v2 logic)
│   └── builds.rs       🆕 // v1 build orchestration patterns
└── application/        // CLI interface and command execution
    └── commands.rs     ✅ // CLI command handlers + state machine integration (new)
```

**Integration Points**:
- **V2 Config**: `empack_reader.sh` → `config.rs` (YAML parsing, smart defaults)
- **V1 Proven Logic**: `v1/lib.bak.d/` → `builds.rs` (battle-tested build orchestration)
- **Rust Execution**: Primitives provide terminal output, structured errors, async operations

### Proven Patterns

**Multi-Crate Success**:
- **Clean structure**: Library/binary separation without artificial boundaries
- **Documentation**: Beautiful Rust docs with complete API coverage
- **Development isolation**: Independent testing and documentation generation
- **Version management**: Workspace inheritance from single source
- **Pragmatic design**: Complete functionality in library, thin binary wrapper

**Foundation Success**:
- Bottom-up development: solid foundations enable rapid higher-layer development
- Structured errors: complete handling prevents technical debt
- Terminal-first design: solid UX from foundation
- Configuration cascade: CLI → ENV → File → Defaults handles real scenarios
- Primitive-driven: validated over vertical module architecture
- **Type consolidation**: Single source of truth for shared types

**Cross-Platform Infrastructure**:
- Environment variable precedence: FORCE_COLOR > NO_COLOR > CLICOLOR
- Tracing + indicatif integration
- Structured error chains with context
- GitHub Actions + Act + Docker validation

**Filesystem-State Benefits**:
- **State Discovery**: Always inspectable on disk
- **Recovery**: Operations resume from filesystem state
- **Debugging**: State transitions visible through file changes
- **Idempotency**: Operations safe to repeat
- **Concurrency**: Natural serialization through filesystem

## Technology Stack

**Multi-Crate Workspace**:
```toml
[workspace]
resolver = "2"
members = ["crates/empack-lib", "crates/empack"]

[workspace.package]
version = "0.0.0-alpha.1"
edition = "2024"
authors = ["mannie.exe <mannie@inherent.design>"]
license = "MIT"
```

**Core Dependencies** (shared across workspace):
```toml
clap = { version = "4.5", features = ["derive", "env"] }
serde = { version = "1.0", features = ["derive"] }
tokio = { version = "1.0", features = ["full"] }
anyhow = "1.0"
thiserror = "1.0"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
tracing-indicatif = "0.3"
dotenvy = "0.15"
envy = "0.4"
```

**Quality Metrics**:
```
Language: Rust | Files: 29 | Lines: 9,672 | Code: 7,815 | Comments: 508
Performance: Enum parsing <10μs (macOS)
Safety: Zero unsafe code
Tests: 112 passing (1 environment test failing)
Status: ✅ Clippy clean, cross-platform tool detection, CLI operational
Architecture: ✅ Multi-crate + composable platform detection API
```

## User Context (Development Partner)

**Technical**: 28, self-taught sysadmin, 7,819 lines functional Rust, strong terminal/shell expertise, values security + UX + real-world functionality over academic elegance

**Health**: Spine condition creates urgency for meaningful work—time finite, focus impact over perfection

**Communication**: Direct explanations, clear next steps, incremental builds with validation, complete current layer before advancing, security + robustness over cleverness

**Development**: Bottom-up foundations, complete error handling from day one, terminal-first UX, real-world validation over theory

**Philosophy**: "Runtime bounds are just state machines (fancy versions)" + "Instead of in-memory state, use filesystem state" + R&D while simplifying + Multi-crate architecture without artificial boundaries

## Filesystem-State Integration Phase

### Three-System Unification

**Integration Strategy**: Filesystem-as-state-machine eliminates complex in-memory state management while preserving all proven patterns

**System Mapping**:
1. **Rust Core** (Layer 0-1): Multi-crate library with execution primitives, terminal, networking, structured errors
2. **V2 Config**: Configuration parsing, smart defaults, project specification
3. **V1 Proven Logic**: Battle-tested build orchestration, command deduplication, template processing

### Implementation Plan

**Required Integration**:
```rust
// crates/empack-lib/src/empack/config.rs - Bridge V2's configuration system
struct EmpackConfig { dependencies: Vec<ProjectSpec> }
struct PackConfig { minecraft: String, loader: ModLoader }
fn reconcile_configs(empack: &EmpackConfig, pack: &PackConfig) -> ProjectPlan

// crates/empack-lib/src/empack/state.rs - Filesystem state machine
enum ModpackState { Uninitialized, Configured, Built, Published }
fn discover_state(path: &Path) -> ModpackState
fn transition_to(target: ModpackState) -> Result<StateTransition>

// crates/empack-lib/src/empack/builds.rs - V1's proven 5-target build system  
enum BuildTarget { Mrpack, Client, Server, ClientFull, ServerFull }
fn execute_build_pipeline(targets: &[BuildTarget]) -> BuildResults
```

### Development Workflow

```bash
# Multi-Crate Validation
RUSTFLAGS="-A warnings" cargo nextest run         # All tests across workspace
cargo clippy                                      # Lint entire workspace
cargo doc -p empack-lib --open                   # Generate library documentation
cargo test -p empack-lib --doc                   # Test documentation examples
act --container-architecture linux/amd64 -j test --matrix os:ubuntu-latest

# Development  
cargo watch -x check -x test -x clippy           # Watch entire workspace
cargo run -p empack -- --log-level debug --color always  # Run binary wrapper
cargo run -p empack-lib --example basic_usage    # Future: library examples

# Library Development
cargo doc -p empack-lib --no-deps --open         # Fast library-only docs
cargo test -p empack-lib                         # Library-only testing

# Integration Testing
# Test filesystem state transitions
# Validate empack.yml + pack.toml reconciliation
# Verify v1 build pattern integration
```

## Atlas Learning

**Multi-Crate Architecture Success**:
- **Clean structure**: Complete library with thin binary wrapper
- **No artificial boundaries**: Pragmatic separation without conceptual overhead
- **Documentation**: Beautiful Rust docs with complete API
- **Development benefits**: Independent testing, documentation, version management
- **Transparent runtime**: Binary simply repackages library functionality

**Validated Approaches**:
- Research-first: systematic investigation prevents rework
- Terminal-first: solid UX enables adoption  
- Structured errors: complete types reduce debugging
- Bottom-up: solid foundations enable rapid development
- Primitive-driven: composable over vertical architecture
- **Multi-crate design**: Clean structure without artificial complexity
- **Filesystem-state**: Eliminates complex memory management, enables inspection and recovery

**Domain Expertise**: Rust (tokio, tracing, clap, serde), terminal capabilities, Minecraft modpack orchestration, API integration patterns, filesystem-based state machines, multi-crate library design

**Integration Insights**: Multi-crate architecture achieved without artificial boundaries. Complete empack implementation in library form with beautiful documentation and convenient API, while binary provides transparent repackaging. Ready for filesystem-state integration of three proven systems.

**Current Reality**: empack modpack maker with multi-crate architecture, filesystem state machine, cross-platform tool detection, CLI command routing operational. V1 business logic integration and template system remain for full functionality.

🚀 **Phase**: State machine + CLI commands operational. Ready for packwiz integration and V1 template system implementation to achieve functional `empack init` and `empack build`.