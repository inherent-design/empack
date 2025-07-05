# empack Testing Migration Bootstrap

## Mission Overview

**Objective**: Migrate 142 tests from in-module architecture (`src/`) to systematic `*.test.rs` architecture (`tests/`) to eliminate memory leak brittleness and improve test isolation.

**Current Status**: 
- 142 tests total, 1 memory leak detected (`test_search_platform_enum`)
- Tests embedded in modules causing shared state and race conditions
- Need systematic migration to prevent manual error-prone work

## Context Files Required

### Atlas Framework Context
Read these foundational files first to understand the AI coordination framework:

1. **Atlas Prime Directive**: `../llm/atlas/src-markdown/CLAUDE.md` - Core Atlas identity and orchestration framework
2. **AC-MQ Protocol**: `../llm/atlas/src-markdown/ATLAS-CLAUDINI-MQ.md` - Atlas-Gemini coordination protocol (reference for patterns)

### empack Project Context  
Then read these files to understand the specific project:

3. **Project Architecture**: `./ai-guidelines/init.md` - Core empack identity and compositional orchestrator
4. **Development Workflows**: `./ai-guidelines/reinit.md` - Operational procedures and health monitoring
5. **Dimensional Navigation**: `./ai-guidelines/userland.md` - Multi-layered development framework
6. **Testing Strategy**: `./ai-guidelines/testing.md` - Target migration architecture and patterns
7. **Current Status**: `./ai-guidelines/plan.md` - Production excellence and current achievement state

### Implementation Context
Finally, examine the current codebase:

8. **Current Codebase**: `./crates/empack-lib/src/` - All source files with embedded tests
9. **Migration Context**: `./testing-migration-context.md` - Detailed analysis from Atlas-Claude coordination

### Additional Atlas Resources (Optional Reference)
If you need deeper context on the Atlas framework and coordination patterns:

- **Atlas Magic Resources**: `../llm/magic/*.md` - Additional Atlas coordination and methodology files
- **Atlas Source Documentation**: `../llm/atlas/src-markdown/*.md` - Complete Atlas framework documentation
- **Production Context**: `../llm/atlas/CURRENT_CONTEXT.md` - Current Atlas operational context (if available)

## Problem Analysis Completed

### Root Cause of Brittleness
- **In-module integration tests** create shared state between test runs
- Tests have access to private module state, encouraging tight coupling
- Parallel test execution with implicit dependencies causes race conditions
- `test_search_platform_enum` leak indicates order-of-execution dependency

### Migration Strategy (Hybrid Approach)
1. **Script-Assisted Migration** (90% automation)
2. **Manual Review & Fix** (compiler-guided precision)
3. **Systematic Validation** (module-by-module testing)

## Coordination Protocol

### Atlas-Gemini Role (Commander/Large-Scale Analysis)
- **Strategy & Planning**: High-level migration approach
- **Script Development**: Automated migration tooling
- **Pattern Analysis**: Broad troubleshooting and solution templates
- **Cross-Module Analysis**: Large-scale codebase understanding

### Claude Role (Executor/Surgical Implementation)  
- **Targeted Migration**: Specific compiler error fixes
- **Code Generation**: Boilerplate test file creation
- **Surgical Refactoring**: Precise code modifications
- **Validation**: Individual test verification

## Immediate Actions Required

### Phase 1: Diagnostic Isolation
1. **Isolate Leaky Test**: Move `test_search_platform_enum` to `tests/search.rs`
2. **Diagnostic Testing**: Run isolated test multiple times to confirm leak source
3. **Validate Strategy**: Confirm root cause before full migration

### Phase 2: Migration Script Development
1. **Script Requirements**:
   - Parse all `.rs` files in `crates/empack-lib/src/`
   - Extract `#[cfg(test)] mod tests { ... }` blocks
   - Create corresponding `tests/*.rs` files
   - Generate appropriate import statements
   - Preserve test function signatures and content

2. **Script Output Structure**:
   ```
   src/application/commands.rs → tests/application_commands.rs
   src/empack/state.rs → tests/empack_state.rs
   src/primitives/mod.rs → tests/primitives.rs
   ```

### Phase 3: Systematic Migration
1. **Framework Setup**: Create `tests/common/mod.rs` for shared utilities
2. **Module-by-Module**: Migrate and validate one module at a time
3. **Compiler-Guided Fixes**: Address visibility and import issues
4. **Test Validation**: Ensure all 142 tests pass after migration

## Technical Context

### Current Test Distribution (from analysis)
- **application.rs**: 2 tests (environment variables, complex setup)
- **networking.rs**: 2 tests (MockApiServer usage)
- **logger.rs**: 2 tests (simple isolation)
- **Single-test modules**: Multiple modules with 1 test each

### Resource Management Patterns Found
- **MockApiServer**: Used correctly in networking.rs (auto-cleanup via Drop)
- **TempDir**: Used correctly in application.rs (auto-cleanup)
- **Environment Variables**: Problem source - modified without isolation
- **No subprocess calls**: No Command::new usage found

### Migration Priority
1. **High Priority**: `application.rs` (complex env var handling)
2. **Medium Priority**: `networking.rs`, `logger.rs` (MockApiServer patterns)
3. **Low Priority**: Single-test modules (simple, isolated)

## Files to Examine

### Core Implementation
- `crates/empack-lib/src/` - All source files with embedded tests
- `crates/empack-lib/Cargo.toml` - Current test configuration

### Architecture Documentation  
- `ai-guidelines/testing.md` - Target test architecture
- `ai-guidelines/userland.md` - Dimensional navigation framework

### Current Test Status
- Recent nextest run shows: `142 tests run: 142 passed (1 leaky)`
- Leak location: `test_search_platform_enum`

## Expected Deliverables

### From Atlas-Gemini
1. **Migration Script**: Automated tool for test extraction and file creation
2. **Pattern Analysis**: Common migration issues and solution templates
3. **Validation Framework**: Systematic testing approach for migration verification

### Coordination Handoffs
- Specific compiler errors for Claude to fix
- Targeted refactoring tasks with clear scope
- Module-specific migration instructions

## Success Criteria

1. **Zero Memory Leaks**: All tests pass with no LEAK flags
2. **142 Tests Migrated**: All tests successfully moved to `tests/` directory
3. **Systematic Architecture**: Clean `*.test.rs` structure with proper isolation
4. **Stable Test Suite**: No brittle interdependencies or race conditions

## Getting Started

### Step 1: Context Hydration
Read the context files in the order specified above. The Atlas framework files will give you the coordination methodology, while the empack files provide project-specific context.

### Step 2: Baseline Validation
Run this command to verify current test status:
```bash
RUSTFLAGS="-A warnings" cargo nextest run
```

Expected output should show the current leak for baseline comparison.

### Step 3: Begin Migration Work
Proceed with Phase 1: Diagnostic Isolation of the leaky test.

---

**Atlas-Gemini Launch Instructions**: 

You are Atlas-Gemini, operating under the Atlas Prime Directive from `../llm/atlas/src-markdown/CLAUDE.md`. You embody systematic investigation ("Nono, first, we research!"), compositional orchestration, and the Commander role in this testing migration project.

Please:
1. **Context Hydration**: Read all specified context files to understand the Atlas framework and empack project
2. **Confirm Understanding**: Acknowledge the mission, coordination protocol, and your role as Commander
3. **Begin Analysis**: Start with analyzing the current test distribution and proposing the migration script approach
4. **Coordinate**: Work with Claude (Executor) for surgical implementation tasks

Remember: You handle strategy, automation, and broad analysis. Claude handles specific compiler fixes and targeted refactoring. Together we're building meaningful technology through systematic investigation.