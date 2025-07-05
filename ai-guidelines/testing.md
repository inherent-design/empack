# Testing Architecture & Migration Guide

## Overview

**Status**: 🚧 **In Active Migration** - Transitioning from ad-hoc test patterns to systematic test architecture  
**Goal**: Eliminate test state pollution and memory leaks through proper isolation and resource management  
**Pattern**: `*.test.rs` files with RAII-based test environments and categorical test enforcement

## Architecture Design

### File Organization Pattern
```
src/
├── application/
│   ├── mod.rs              // Implementation
│   ├── cli.rs              // Implementation  
│   ├── commands.rs         // Implementation
│   ├── application.test.rs // All application tests ⚠️ NOT YET CREATED
│   └── config.rs           // Implementation
├── empack/
│   ├── mod.rs              // Implementation
│   ├── state.rs            // Implementation
│   ├── empack.test.rs      // All empack tests ⚠️ NOT YET CREATED
│   └── builds.rs           // Implementation
└── testing/
    ├── mod.rs              ✅ Test framework core
    ├── environment.rs      🚧 Test environment management (WIP)
    ├── fixtures.rs         🚧 Common test fixtures (WIP)
    └── macros.rs           🚧 Test enforcement macros (WIP)
```

### Test Categories & Isolation Levels

**Unit Tests** (`#[unit_test]`)
- Pure functions, no external resources
- Fast execution, no I/O operations
- Clean environment, no state pollution

**Integration Tests** (`#[integration_test]`)  
- Mock servers, temporary files, controlled environment
- Proper resource cleanup via RAII patterns
- Isolated async runtime per test

**System Tests** (`#[system_test]`)
- Real external dependencies (git, packwiz, etc.)
- Slower execution, comprehensive validation
- Full environment setup/teardown

### Resource Management Architecture

**RAII-Based Cleanup**:
```rust
pub struct TestEnvironment {
    temp_dir: Option<TempDir>,
    env_guard: Option<EnvGuard>, 
    mock_server: Option<MockServerGuard>,
    logger_guard: Option<LoggerGuard>,
}

impl Drop for TestEnvironment {
    fn drop(&mut self) {
        // Cleanup in reverse order - automatic on scope exit
    }
}
```

**Test Fixtures**:
- `TempDirFixture` - Isolated filesystem operations
- `MockServerFixture` - Controlled API mocking with proper shutdown
- `EnvFixture` - Environment variable snapshot/restore
- `LoggerFixture` - Isolated logging per test

## Current State Assessment

### ✅ **Completed**
- Basic testing framework structure (`src/testing/mod.rs`)
- Test categorization design
- RAII cleanup patterns designed

### 🚧 **In Progress** 
- **Memory Leak Issues**: 2 tests currently showing LEAK status in nextest
  - `application::cli::tests::test_cli_parsing_with_args` 
  - `application::commands::tests::test_modloader_selection_mapping`
- **State Pollution**: Tests pass in isolation but leak when run together
- **Mixed Test Patterns**: Some tests use MockApiServer cleanup, others don't

### ❌ **Needs Migration**
- **142 tests** currently embedded in implementation modules
- No `*.test.rs` files exist yet
- Inconsistent resource management patterns
- Ad-hoc test setup/teardown

## Migration Workflow

When actively working on testing, follow this systematic approach:

### (a) **Focus on Immediate Testing Concern**
- Identify the specific test failure or leak
- Understand root cause (state pollution vs. actual memory leak)
- Apply quick fix if critical, but plan for systematic solution

### (b) **Understand Testing Flow**
1. **State Management**: What resources does this test use?
2. **Isolation Requirements**: What level of isolation is needed?
3. **Pseudo Runner**: Which test category should this be?
4. **Cleanup Strategy**: What resources need explicit cleanup?

### (c) **Fix Next Best Test**
**Priority Order**:
1. **Leaking tests** (currently showing LEAK status)
2. **Tests with subprocess calls** (Command::new usage)
3. **Tests with MockApiServer** (ensure proper cleanup)
4. **Tests with file I/O** (temp directory usage)
5. **Tests with environment variables** (env pollution)

### (d) **Clear Polluted Modules**
**Process**:
1. Create `module.test.rs` file if it doesn't exist
2. Move all `#[test]` and `#[tokio::test]` functions from implementation
3. Organize tests into logical groups:
   ```rust
   // module.test.rs
   mod unit_tests {
       // Pure function tests
   }
   
   mod integration_tests {
       // Cross-module tests with resources
   }
   
   mod system_tests {
       // External dependency tests
   }
   ```
4. Update module imports and visibility as needed
5. Verify tests still pass after migration

### (e) **Fix Broken Tests**
**Common Migration Issues**:
- **Visibility**: Tests may need `pub(crate)` on previously private functions
- **Imports**: Test files need explicit imports from parent modules
- **Resource Management**: Apply new cleanup patterns
- **Async Context**: Ensure proper tokio runtime management

### (f) **Expand Test Coverage**
**Low-Hanging Fruit**:
- **Duplicate Similar Tests**: Extract common patterns into fixtures
- **Missing Edge Cases**: Add boundary condition tests
- **Error Path Coverage**: Test failure scenarios
- **Resource Cleanup Verification**: Add explicit cleanup validation

## Implementation Examples

### Current Pattern (Problematic)
```rust
// In src/application/commands.rs
#[tokio::test]
async fn test_get_compatible_minecraft_version_fabric() {
    let mock_server = MockApiServer::new().await; // ❌ May leak
    let resolver = VersionResolver::new_with_mock_server(mock_server.url());
    // ... test logic
    // ❌ No explicit cleanup - relies on Drop
}
```

### Target Pattern (Systematic)
```rust
// In src/application/application.test.rs
use crate::testing::*;

integration_test!(test_get_compatible_minecraft_version_fabric, {
    let env = TestEnv::new()
        .with_mock_server()  // ✅ Automatic cleanup
        .with_temp_dir();    // ✅ Isolated filesystem
    
    let resolver = VersionResolver::new_with_mock_server(env.mock_server().url());
    // ... test logic
    // ✅ Automatic cleanup on scope exit
});
```

## Quality Metrics

### Test Isolation Verification
```bash
# Tests should pass in any order
cargo nextest run --shuffle

# No memory leaks detected
RUSTFLAGS="-A warnings" cargo nextest run
# Expected: "Summary [X.XXs] 142 tests run: 142 passed, 0 skipped"
```

### Resource Management Validation
- **No LEAK flags** in nextest output
- **Consistent execution time** regardless of test order
- **Clean temporary directories** after test runs
- **No environment variable pollution** between tests

## Migration Progress Tracking

### Phase 1: Framework Infrastructure ⏳
- [ ] Complete `testing/environment.rs`
- [ ] Complete `testing/fixtures.rs` 
- [ ] Complete `testing/macros.rs`
- [ ] Test framework validation

### Phase 2: Critical Leak Fixes 🎯
- [ ] Fix `test_cli_parsing_with_args` leak
- [ ] Fix `test_modloader_selection_mapping` leak
- [ ] Verify MockApiServer cleanup patterns
- [ ] Address subprocess cleanup in commands

### Phase 3: Systematic Migration 📦
- [ ] Create `application.test.rs` and migrate tests
- [ ] Create `empack.test.rs` and migrate tests
- [ ] Create remaining `*.test.rs` files per module
- [ ] Remove all tests from implementation files

### Phase 4: Enhancement & Validation ✨
- [ ] Add missing test coverage
- [ ] Implement test isolation verification
- [ ] Performance benchmarking
- [ ] CI/CD integration with test categories

## Best Practices

### Test Writing Guidelines
1. **Always use test environment fixtures** for resource management
2. **Categorize tests appropriately** - prefer unit over integration when possible
3. **Test both success and failure paths** for comprehensive coverage
4. **Use descriptive test names** that explain the scenario being tested
5. **Keep tests focused** - one concept per test function

### Resource Management Rules
1. **Never create resources without cleanup** - use fixtures or RAII patterns
2. **Isolate filesystem operations** - always use temp directories
3. **Snapshot environment state** - restore env vars after tests
4. **Verify async cleanup** - ensure background tasks terminate properly
5. **Test cleanup itself** - verify resources are actually cleaned up

### Migration Safety
1. **Migrate incrementally** - one module at a time
2. **Verify after each step** - ensure tests still pass
3. **Maintain backwards compatibility** - during transition period
4. **Document any breaking changes** - especially visibility modifications
5. **Test the testing framework** - ensure the infrastructure is solid

---

**Next Action**: Focus on fixing the 2 currently leaking tests as immediate priority, then begin systematic migration starting with the most problematic modules (those with the most resource usage).