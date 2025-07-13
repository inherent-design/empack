# Testing Architecture & Migration Guide

## Core Testing Philosophy

**MANDATORY PRINCIPLE**: Clean separation between tests and implementations.

- **Implementation**: `mod.rs` (business logic only)
- **Unit Tests**: `mod.test.rs` (adjacent test files)
- **Integration Tests**: `tests/` directories or `tests.rs` files
- **NO EMBEDDED TESTS**: Never use `#[cfg(test)]` in implementation modules

## Architecture Design

### File Organization Pattern (REQUIRED)
```
src/
├── application/
│   ├── mod.rs              // Implementation ONLY
│   ├── mod.test.rs         // Unit tests for mod.rs
│   ├── cli.rs              // Implementation ONLY
│   ├── cli.test.rs         // Unit tests for cli.rs ✅ COMPLETE
│   ├── commands.rs         // Implementation ONLY
│   ├── commands.test.rs    // Unit tests for commands.rs ⚠️ TO CREATE
│   └── tests/              // Integration tests directory
│       └── config_integration.rs ✅ COMPLETE
├── empack/
│   ├── mod.rs              // Implementation ONLY
│   ├── mod.test.rs         // Unit tests for mod.rs ⚠️ TO CREATE
│   ├── state.rs            // Implementation ONLY
│   ├── state.test.rs       // Unit tests for state.rs ⚠️ TO CREATE
│   ├── parsing.rs          // Implementation ONLY
│   ├── parsing.test.rs     // Unit tests for parsing.rs ✅ COMPLETE
│   └── builds.rs           // Implementation ONLY
│       builds.test.rs      // Unit tests for builds.rs ⚠️ TO CREATE
├── primitives/
│   ├── mod.rs              // Implementation ONLY
│   ├── mod.test.rs         // Unit tests for mod.rs ⚠️ TO CREATE
│   ├── empack.rs           // Implementation ONLY
│   ├── empack.test.rs      // Unit tests for empack.rs ✅ COMPLETE
└── testing/
    ├── mod.rs              ✅ Test framework core
    ├── filesystem.rs       ✅ Filesystem test utilities
    ├── environment.rs      🚧 Test environment management (FUTURE)
    ├── fixtures.rs         🚧 Common test fixtures (FUTURE)
    └── macros.rs           🚧 Test enforcement macros (FUTURE)
```

### Test Categories & Isolation Levels

**Unit Tests** (`*.test.rs` files)
- Test single modules in isolation
- Use test utilities from `testing/` module
- Fast execution, no external dependencies
- Clean separation from implementation

**Integration Tests** (`tests/` directories)  
- Test interaction between modules
- Mock external dependencies when needed
- Proper resource cleanup via RAII patterns
- Isolated environments per test

**System Tests** (separate `empack-tests/` crate - FUTURE)
- Real external dependencies (git, packwiz, etc.)
- End-to-end workflow validation
- Full environment setup/teardown

### Resource Management Architecture

**Test Infrastructure**:
```rust
// In testing/mod.rs
pub use filesystem::TempDirFixture;
// Future: pub use environment::TestEnvironment;
// Future: pub use fixtures::{MockServerFixture, EnvFixture};
```

**Test Pattern**:
```rust
// In module.test.rs
use crate::testing::TempDirFixture;
use super::*; // Import module being tested

#[test]
fn test_functionality() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = TempDirFixture::new()?;
    // Test with automatic cleanup
    Ok(())
}
```

## Current Migration Status

### ✅ **Clean Separation Achieved**
- `application/cli.test.rs` - Unit tests separated from implementation
- `empack/parsing.test.rs` - Unit tests separated from implementation  
- `primitives/empack.test.rs` - Unit tests separated from implementation
- `testing/filesystem.rs` - Test infrastructure operational

### 🚧 **DUPLICATED TESTS (IMMEDIATE CLEANUP REQUIRED)**
These modules have tests in BOTH implementation and separate files - **DUPLICATES MUST BE REMOVED**:
- `application/cli.rs` - Remove embedded tests, keep `cli.test.rs`
- `empack/parsing.rs` - Remove embedded tests, keep `parsing.test.rs`
- `primitives/empack.rs` - Remove embedded tests, keep `empack.test.rs`

### ❌ **EMBEDDED TESTS (MIGRATION REQUIRED)**
These modules have tests embedded in implementation - **MUST BE MOVED TO SEPARATE FILES**:
- `application/env.rs` → `application/env.test.rs`
- `application/loader.rs` → `application/loader.test.rs`
- `application/mod.rs` → `application/mod.test.rs`
- `empack/builds.rs` → `empack/builds.test.rs`
- `empack/config.rs` → `empack/config.test.rs`
- `empack/mod.rs` → `empack/mod.test.rs`
- `empack/resolved_project.rs` → `empack/resolved_project.test.rs`
- `empack/search.rs` → `empack/search.test.rs`
- `empack/search_intent.rs` → `empack/search_intent.test.rs`
- `empack/state.rs` → `empack/state.test.rs`
- `empack/templates.rs` → `empack/templates.test.rs`
- `empack/versions.rs` → `empack/versions.test.rs`
- `logger/mod.rs` → `logger/mod.test.rs`
- `networking/mod.rs` → `networking/mod.test.rs`
- `platform/mod.rs` → `platform/mod.test.rs`
- `primitives/mod.rs` → `primitives/mod.test.rs`
- `terminal/capabilities.rs` → `terminal/capabilities.test.rs`

## Migration Workflow (MANDATORY STEPS)

### Phase 1: Clean Up Duplicates (IMMEDIATE)
For each duplicated module:
1. **Verify** the separate `.test.rs` file has all tests
2. **Remove** all `#[cfg(test)]` sections from implementation file
3. **Test** that `cargo test` passes after removal
4. **Commit** each cleanup individually

### Phase 2: Migrate Embedded Tests
For each module with embedded tests:
1. **Create** `module.test.rs` file
2. **Move** all test code from implementation to test file
3. **Add** `use super::*;` to import module being tested
4. **Update** any visibility (`pub(crate)`) needed for testing
5. **Remove** all `#[cfg(test)]` sections from implementation
6. **Verify** tests pass with `cargo test`
7. **Commit** each migration individually

### Phase 3: Validation
- **Run** `cargo test` - all tests must pass
- **Verify** no `#[cfg(test)]` remains in implementation files
- **Confirm** clean separation is maintained

## Implementation Examples

### ❌ **FORBIDDEN PATTERN**
```rust
// In src/application/commands.rs - NEVER DO THIS
pub fn some_function() {
    // implementation
}

#[cfg(test)]  // ❌ FORBIDDEN - NO TESTS IN IMPLEMENTATION
mod tests {
    #[test]
    fn test_some_function() {
        // test code
    }
}
```

### ✅ **REQUIRED PATTERN**
```rust
// In src/application/commands.rs - Implementation only
pub fn some_function() {
    // implementation only
}

// In src/application/commands.test.rs - Tests only
use super::*;

#[test]
fn test_some_function() {
    // test code
}
```

### ✅ **INTEGRATION TEST PATTERN**
```rust
// In src/application/tests/config_integration.rs
use crate::testing::TempDirFixture;
use crate::application::*;

#[test]
fn test_config_integration() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = TempDirFixture::new()?;
    // Integration test with automatic cleanup
    Ok(())
}
```

## Quality Metrics

### Migration Success Criteria
```bash
# All tests pass
cargo test

# No embedded tests remain
rg "#\[cfg\(test\)\]" crates/empack-lib/src/ 
# Expected: No matches found

# Clean separation verified
find crates/empack-lib/src/ -name "*.test.rs" | wc -l
# Expected: One .test.rs file per implementation module
```

### Test Execution Validation
```bash
# Tests pass in any order (isolation verification)
cargo nextest run --shuffle

# No memory leaks
RUSTFLAGS="-A warnings" cargo nextest run
# Expected: All tests passing, no LEAK flags
```

## Migration Progress Tracking

### ✅ **Phase 1: Clean Up Duplicates**
- [ ] Remove embedded tests from `application/cli.rs` (keep `cli.test.rs`)
- [ ] Remove embedded tests from `empack/parsing.rs` (keep `parsing.test.rs`)
- [ ] Remove embedded tests from `primitives/empack.rs` (keep `empack.test.rs`)

### ⏳ **Phase 2: Migrate Remaining Embedded Tests**
- [ ] `application/env.rs` → `application/env.test.rs`
- [ ] `application/loader.rs` → `application/loader.test.rs`
- [ ] `application/mod.rs` → `application/mod.test.rs`
- [ ] `empack/builds.rs` → `empack/builds.test.rs`
- [ ] `empack/config.rs` → `empack/config.test.rs`
- [ ] `empack/state.rs` → `empack/state.test.rs`
- [ ] `empack/search.rs` → `empack/search.test.rs`
- [ ] `logger/mod.rs` → `logger/mod.test.rs`
- [ ] `networking/mod.rs` → `networking/mod.test.rs`
- [ ] `platform/mod.rs` → `platform/mod.test.rs`
- [ ] `primitives/mod.rs` → `primitives/mod.test.rs`
- [ ] `terminal/capabilities.rs` → `terminal/capabilities.test.rs`

### 🎯 **Phase 3: Validation & Enhancement**
- [ ] Verify no `#[cfg(test)]` in implementation files
- [ ] All tests pass with clean separation
- [ ] Test isolation validation
- [ ] Enhanced test infrastructure (future)

## Best Practices

### Absolute Rules
1. **NEVER** embed tests in implementation modules
2. **ALWAYS** use separate `.test.rs` files for unit tests
3. **MAINTAIN** clean import separation (`use super::*;`)
4. **USE** test infrastructure from `testing/` module
5. **VERIFY** tests pass after every migration step

### Test Writing Guidelines
1. **One concept per test function** - focused, clear tests
2. **Use descriptive test names** explaining the scenario
3. **Test both success and failure paths** for comprehensive coverage
4. **Use test fixtures** for resource management and cleanup
5. **Keep tests isolated** - no dependencies between tests

### Migration Safety
1. **Migrate incrementally** - one module at a time
2. **Verify after each step** - `cargo test` must pass
3. **Commit frequently** - individual module migrations
4. **Document visibility changes** - any `pub(crate)` additions needed
5. **Maintain test coverage** - no tests lost during migration

---

**NEXT ACTION**: Begin Phase 1 by cleaning up the 3 duplicated modules, then proceed systematically through Phase 2 migration.