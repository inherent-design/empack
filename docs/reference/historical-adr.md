# Architectural decision record: session-scoped dependency injection pattern

> Historical context: this ADR reflects the January 2025 decision point for the session-scoped DI shift. It is not the current source of truth for feature completeness, verification, or release readiness. For current status, see [`../README.md`](../README.md) and [`testing.md`](testing.md).

**Date**: 2025-01-14
**Status**: APPROVED
**Participants**: Atlas-Claude, Atlas-Gemini, Mannie

## Summary

Following architectural analysis of the DisplayProvider compilation crisis and the broader empack ecosystem, we identified a fundamental impedance mismatch between trait-based architecture and Rust's ownership model. This document formalizes the commitment to the session-scoped dependency injection pattern as the foundation for resolving the DisplayProvider issues and completing the remaining feature implementation.

## State assessment (January 2025)

### Infrastructure maturity

- CLI parsing (clap), configuration management (empack.yml + pack.toml), logging infrastructure, networking layer, and terminal capabilities detection were in place
- Well-structured modular architecture with clean separation of concerns
- Test migration to `.test.rs` files completed, establishing a testing foundation

### DisplayProvider crisis

The DisplayProvider trait implementation encountered six compilation errors due to lifetime mismatches between `Box<dyn ProgressTracker>` return types and the `indicatif` library's borrowing requirements.

Root cause: fundamental impedance mismatch between owned trait objects (`Box<dyn Trait>`) and `indicatif::MultiProgress`, which requires borrowed references with explicit lifetimes. This collision between v1's ambient state model and Rust's explicit ownership requirements was a design issue, not a bug.

### Feature implementation state

Fully implemented at the time: `init`, `build` (mrpack and client targets), `clean`, `requirements`.

Blocked by architecture: all user-facing commands requiring progress indication or interactive prompts, and testing infrastructure for commands using DisplayProvider.

Not yet implemented: `version`, mod search and resolution, dependency resolution, empack.yml dependency processing.

## Architectural analysis

### Impedance mismatch

The v1 Bash implementation operated on an ambient state model where global environment variables (`EMPACK_*`) created a shared information space accessible from any execution context. Rust enforces explicit ownership where every piece of data must have a clear owner, borrowers must be explicitly scoped, and the compiler validates all references. The DisplayProvider crisis was the first irreconcilable collision between these models.

### Immutable principles

Two principles constitute the core design contract of empack:

1. **Runtime boundary enforcement**: strict separation between pre-init and post-init states prevents destructive operations and ensures operations only occur in valid contexts. This must survive translation to Rust through the type system.

2. **Progressive disclosure of complexity**: the "loader-first auto-fill" philosophy where systems are maximally intelligent by default while allowing expert access. This informs API design throughout the Rust implementation.

## Decision: session-scoped dependency injection

After evaluating candidate patterns, the session-scoped dependency injection pattern was selected.

Key properties:

- **Lifetime clarity**: each `execute_command` creates a `CommandSession` that owns all ephemeral state
- **Ownership resolution**: session owns `MultiProgress`, provides `Box<dyn ProgressTracker + '_>` with explicit lifetimes
- **Test isolation**: each command execution is self-contained with mockable dependencies
- **Resource management**: automatic cleanup when session ends

### Core pattern

```rust
pub struct CommandSession<'a> {
    multi_progress: MultiProgress,
    status_provider: Box<dyn StatusProvider + 'a>,
    progress_provider: Box<dyn ProgressProvider + 'a>,
}

pub fn execute_command(command: EmpackCommand) -> Result<(), EmpackError> {
    let session = CommandSession::new();
    match command {
        EmpackCommand::Add(args) => handle_add(&session, args),
        EmpackCommand::Build(args) => handle_build(&session, args),
        // All commands receive session reference
    }
}
```

## Implementation roadmap (January 2025)

### Phase 1: DisplayProvider crisis resolution

Implement `CommandSession` architecture, refactor DisplayProvider traits to use explicit lifetimes, update `execute_command` integration, and implement test infrastructure with `MockCommandSession`.

### Phase 2: session-scoped state management

Expand `CommandSession` with `FileSystemProvider`, `NetworkProvider`, `ConfigProvider`, and `CacheProvider`. Create `PreInitSession` and `PostInitSession` types to encode runtime boundaries in the type system.

### Phase 3: feature completion

Port v2 search logic, implement dependency resolution, complete `empack sync` with dry-run mode, add `empack version`, and finish remaining build targets.

## Risk assessment

- **Lifetime complexity**: session-scoped lifetimes might create complex borrowing chains. Mitigation: keep session interfaces simple, use factory methods for complex objects.
- **Testing integration**: mock sessions might not accurately represent production behavior. Mitigation: integration tests with real sessions, careful mock validation.
- **Scope creep**: session pattern might expand beyond display providers. Mitigation: strict boundaries, implement display first, extend incrementally.

## Success metrics

- Phase 1: all six DisplayProvider errors resolved, existing tests pass, no breaking API changes
- Phase 2: all state management follows session-scoped pattern, pre-init/post-init constraints enforced at compile time
- Phase 3: all v1/v2 features implemented, new features addable without architectural changes

---

**Decision Approved**: Atlas-Claude, Atlas-Gemini, Mannie
**Implementation Start**: Immediate (January 2025)
**Next Review**: After Phase 1 completion
