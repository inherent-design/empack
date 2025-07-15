# Empack Architectural Decision Record: Session-Scoped Dependency Injection Pattern

**Date**: 2025-01-14  
**Status**: APPROVED  
**Participants**: Atlas-Claude, Atlas-Gemini, Mannie  

## Executive Summary

Following comprehensive architectural analysis of the DisplayProvider compilation crisis and the broader empack ecosystem, we have identified a fundamental impedance mismatch between our desired trait-based architecture and Rust's ownership model. This document formalizes our commitment to the **Session-Scoped Dependency Injection Pattern** as the foundational architecture for resolving the immediate DisplayProvider issues while establishing a robust foundation for completing the remaining feature implementation.

## Current State Assessment

### Infrastructure Maturity (85% Complete)
- **Strengths**: Robust CLI parsing (clap), configuration management (empack.yml + pack.toml), logging infrastructure, networking layer, terminal capabilities detection
- **Quality**: Well-structured modular architecture with clean separation of concerns
- **Testing**: Recent test migration to `.test.rs` files completed successfully, establishing strong testing foundation

### The DisplayProvider Crisis (Critical Blocker)
**Immediate Issue**: The DisplayProvider trait implementation has encountered six compilation errors due to lifetime mismatches between our `Box<dyn ProgressTracker>` return types and the `indicatif` library's borrowing requirements.

**Root Cause**: Fundamental impedance mismatch between:
- **Our Goal**: Trait-based dependency injection with owned trait objects (`Box<dyn Trait>`)
- **Rust Reality**: `indicatif::MultiProgress` requires borrowed references with explicit lifetimes
- **Library Constraint**: `ProgressBar` instances must borrow from their parent `MultiProgress` object

**Architectural Significance**: This is not a bug but a design crisis revealing the collision between our v1 "ambient state" model and Rust's "explicit ownership" requirements.

### Feature Implementation Gap (40% Complete)
**Fully Implemented**:
- `empack init`: Complete state machine-driven project initialization
- `empack build`: Functional for mrpack and client targets
- `empack clean`: Build artifact cleaning operational
- `empack requirements`: Dependency checking system

**Blocked by Architecture**:
- All user-facing commands requiring progress indication or interactive prompts
- Testing infrastructure for any command using DisplayProvider
- Further development until ownership model is resolved

**Missing Entirely**:
- `empack version`: Present in V1, absent in Rust
- Mod search & resolution: V2 shows sophisticated Modrinth/CurseForge integration
- Dependency resolution: No conflict detection or version constraint handling
- empack.yml dependency processing: No translation to packwiz commands

### Architectural Health (Crisis State)
**Critical Architecture Debt**:
- **Ownership Model Undefined**: No clear pattern for managing state that must be shared across multiple components
- **Lifetime Management Unclear**: No established pattern for handling borrowed resources within trait abstractions
- **Session Boundaries Implicit**: Command execution lifecycle not explicitly modeled in the type system

## Architectural Analysis: The Great Contention

### The Impedance Mismatch Defined

**The v1 Model: Ambient State Architecture**
The Bash implementation operated on an "ambient state" model where global environment variables (`EMPACK_*`) created a shared information space accessible from any execution context. This enabled loose coupling between components while maintaining system-wide coordination.

**The Rust Model: Explicit Ownership Architecture**
Rust enforces explicit ownership where every piece of data must have a clear owner, borrowers must be explicitly scoped, and the compiler validates all references remain valid. This trades ambient accessibility for guaranteed memory safety.

**The Collision Point**: The DisplayProvider crisis represents the first irreconcilable collision between these models. `Box<dyn ProgressTracker>` implies `'static` ownership while `indicatif::ProgressBar` requires borrowing from a parent `MultiProgress`.

### The Soul of empack: Immutable Principles

Through analysis of the complete historical arc, two immutable principles constitute the "soul" of empack:

**Principle 1: Runtime Boundary Enforcement**
The strict separation between pre-init and post-init states prevents destructive operations and ensures operations only occur in valid contexts. This must survive translation to Rust through the type system.

**Principle 2: Progressive Disclosure of Complexity**
The "Loader-First Auto-Fill" philosophy where systems are maximally intelligent by default while allowing expert access through progressive disclosure. This should inform API design throughout the Rust implementation.

### Historical Context: V1/V2 Sophistication

**V1 Implementation Excellence**:
- **Runtime Boundary Architecture**: Clean pre-init/post-init phase separation
- **Command Registry**: Sophisticated five-array system with validation
- **API Integration**: Multi-modloader version resolution with fallbacks
- **Template System**: Static vs dynamic template processing
- **Interactive Modes**: Three initialization modes with progressive disclosure

**V2 Implementation Excellence**:
- **Multi-Platform Search**: `search_modrinth.sh` + `search_curseforge.sh` with unified API
- **Fuzzy Matching**: `fuzz_match.sh` for intelligent mod name resolution
- **Dependency Resolution**: `remote_resolver.sh` with project ID validation
- **empack.yml Processing**: `empack_reader.sh` parsing dependency specifications
- **Command Generation**: Automated packwiz command generation from dependency specs

## Architectural Decision: Session-Scoped Dependency Injection Pattern

### Pattern Selection Rationale

After comprehensive analysis of the DisplayProvider crisis and evaluation of candidate patterns, the **Session-Scoped Dependency Injection Pattern** emerges as the optimal solution. This pattern resolves the immediate ownership conflicts while establishing a foundation for all future feature development.

**Key Advantages**:
- **Lifetime Clarity**: Each `execute_command` creates a `CommandSession` that owns all ephemeral state
- **Ownership Resolution**: Session owns `MultiProgress`, provides `Box<dyn ProgressTracker + '_>` with explicit lifetimes
- **Test Isolation**: Each command execution is self-contained with mockable dependencies
- **Resource Management**: Automatic cleanup when session ends, preventing resource leaks
- **Type Safety**: Compile-time verification of all dependencies and lifetimes

### Core Architecture Pattern

**The CommandSession**:
```rust
pub struct CommandSession<'a> {
    multi_progress: MultiProgress,
    status_provider: Box<dyn StatusProvider + 'a>,
    progress_provider: Box<dyn ProgressProvider + 'a>,
    // Additional session-scoped capabilities
}

impl<'a> CommandSession<'a> {
    pub fn new() -> Self { /* ... */ }
    
    pub fn status_provider(&self) -> &dyn StatusProvider { /* ... */ }
    pub fn progress_provider(&self) -> &dyn ProgressProvider { /* ... */ }
}
```

**Session-Scoped Execution**:
```rust
pub fn execute_command(command: EmpackCommand) -> Result<(), EmpackError> {
    let session = CommandSession::new();
    
    match command {
        EmpackCommand::Add(args) => handle_add(&session, args),
        EmpackCommand::Build(args) => handle_build(&session, args),
        // All commands receive session reference
    }
}
```

**Business Logic Integration**:
```rust
pub fn handle_add(session: &CommandSession, args: AddArgs) -> Result<(), EmpackError> {
    // Pure business logic using session-provided capabilities
    let status = session.status_provider();
    let progress = session.progress_provider();
    
    // All user interaction through session interfaces
    status.info("Starting mod search...");
    let bar = progress.create_progress_bar(100)?;
    
    // Business logic remains pure and testable
}
```

## Implementation Roadmap

### Phase 1: DisplayProvider Crisis Resolution (1 week)
**Immediate Priority: Restore Compilation**

1. **Implement CommandSession Architecture**:
   ```rust
   pub struct CommandSession {
       multi_progress: MultiProgress,
       // Session owns all display state
   }
   
   impl CommandSession {
       pub fn create_progress_bar(&self, length: u64) -> Box<dyn ProgressTracker + '_> {
           // Returns progress bar borrowing from self.multi_progress
       }
   }
   ```

2. **Refactor DisplayProvider Traits**:
   - Change return types to `Box<dyn ProgressTracker + '_>` with explicit lifetimes
   - Session provides factory methods instead of static constructors
   - Maintain existing business logic interfaces

3. **Update execute_command Integration**:
   - Create `CommandSession` at the start of each command
   - Pass session reference to all handle_* functions
   - Verify all six compilation errors are resolved

4. **Implement Test Infrastructure**:
   - Create `MockCommandSession` for testing
   - Verify all existing tests pass with new architecture
   - Add session-specific test coverage

**Success Criteria**: All code compiles cleanly, existing functionality preserved, test suite passes, session pattern validated.

### Phase 2: Session-Scoped State Management (2-3 weeks)
**Extend pattern to all state management**

1. **Expand CommandSession Capabilities**:
   - Add `FileSystemProvider` for file operations
   - Add `NetworkProvider` for API interactions
   - Add `ConfigProvider` for configuration access
   - Add `CacheProvider` for cache management

2. **Implement Session-Scoped Providers**:
   ```rust
   impl CommandSession {
       pub fn filesystem(&self) -> &dyn FileSystemProvider { /* ... */ }
       pub fn network(&self) -> &dyn NetworkProvider { /* ... */ }
       pub fn config(&self) -> &dyn ConfigProvider { /* ... */ }
       pub fn cache(&self) -> &dyn CacheProvider { /* ... */ }
   }
   ```

3. **Refactor Existing Commands**:
   - Update `handle_build` to use session providers
   - Update `handle_init` to use session providers
   - Update `handle_clean` to use session providers
   - Maintain existing functionality while improving testability

4. **Type-Level Runtime Boundaries**:
   - Create `PreInitSession` and `PostInitSession` types
   - Encode pre-init/post-init constraints in the type system
   - Prevent invalid operations at compile time

### Phase 3: Feature Completion (2-3 weeks)
**Implement missing features using established session pattern**

1. **`empack add/remove` commands**:
   - Port V2 search logic to Rust using session providers
   - Implement dependency resolution engine within session context
   - Build complete mod lifecycle management
   - Full test coverage using mock sessions

2. **Complete `empack sync`**:
   - Implement dry-run mode using execution plans
   - Add empack.yml → pack.toml reconciliation logic
   - Use session providers for configuration diffing

3. **`empack version` command**:
   - Simple implementation matching V1 functionality
   - Use session for consistent output formatting

4. **Complete build targets**:
   - Finish server, client-full, server-full implementations
   - Use session providers for consistent testing

## Risk Assessment & Mitigation

### Technical Risks
- **Lifetime Complexity**: Session-scoped lifetimes might create complex borrowing chains
  - *Mitigation*: Keep session interfaces simple, use factory methods for complex objects
- **Performance Overhead**: Session creation/destruction on every command
  - *Mitigation*: Profile session overhead, optimize hot paths, session pooling if needed
- **Testing Integration**: Mock sessions might not accurately represent production behavior
  - *Mitigation*: Integration tests with real sessions, careful mock validation

### Architecture Risks
- **Scope Creep**: Session pattern might expand beyond display providers
  - *Mitigation*: Strict boundaries, implement display first, extend incrementally
- **Backwards Compatibility**: Changes might break existing command interfaces
  - *Mitigation*: Maintain existing public APIs, internal refactoring only
- **Future Icebergs**: Additional ownership conflicts in other modules
  - *Mitigation*: Apply session pattern consistently, address conflicts as they arise

## Success Metrics

### Immediate Success Criteria (Phase 1)
- **Compilation**: All six DisplayProvider errors resolved
- **Test Coverage**: Existing test suite passes with new architecture
- **API Compatibility**: No breaking changes to existing command interfaces
- **Performance**: No measurable regression in command execution time

### Medium-term Success Criteria (Phase 2)
- **Pattern Consistency**: All state management follows session-scoped pattern
- **Type Safety**: Pre-init/post-init constraints enforced at compile time
- **Test Coverage**: >90% coverage for business logic through mock sessions
- **Architecture Debt**: Global state dependencies eliminated

### Long-term Success Criteria (Phase 3)
- **Feature Completeness**: All v1/v2 features implemented using session pattern
- **Maintainability**: New features can be added without architectural changes
- **Performance**: Session overhead <1% of total command execution time
- **Reliability**: Comprehensive testing enables confident refactoring

## Conclusion

The Session-Scoped Dependency Injection Pattern provides the architectural foundation needed to resolve the immediate DisplayProvider crisis while establishing a robust pattern for all future development. This decision directly addresses the fundamental impedance mismatch between our design goals and Rust's ownership model.

By committing to this pattern, we establish:
- **Ownership Clarity**: Clear lifetime management for all shared resources
- **Compilation Success**: Resolution of all DisplayProvider lifetime errors
- **Test Isolation**: Complete testability through session-scoped mocking
- **Architectural Consistency**: Unified pattern for all state management
- **Future Resilience**: Framework for handling similar ownership conflicts

This architectural decision transforms the current crisis into a strategic advantage, establishing empack as a systematically designed, maintainable, and extensible modpack development tool built on sound Rust principles.

**The Path Forward**: 
1. Phase 1 resolves the immediate compilation crisis
2. Phase 2 extends the pattern to all state management
3. Phase 3 completes the feature set using established patterns

This is not just a fix—it is the foundation for empack's evolution into a production-ready, professionally architected tool.

---

**Decision Approved**: Atlas-Claude, Atlas-Gemini, Mannie  
**Implementation Start**: Immediate  
**Next Review**: After Phase 1 completion (compilation success)