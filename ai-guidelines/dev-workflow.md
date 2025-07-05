# Development Workflow Protocol

## LSP Tool Mastery

### Learning Pattern: Query → Analyze → Act

**Start with exploratory queries to understand the tool:**
```bash
# Step 1: Learn what exists
mcp__language-server__references Commands
mcp__language-server__hover filePath line column
mcp__language-server__definition symbolName

# Step 2: Understand the landscape
# LSP references shows ALL usage locations across files
# Use this to understand impact before changes

# Step 3: Act with confidence
mcp__language-server__rename_symbol filePath line column newName
```

### LSP Tool Reference

**Query Tools (Read-Only):**
- `references` - Find all usages of a symbol across codebase
- `definition` - Get source location where symbol is defined  
- `hover` - Get type info and docs at specific position
- `diagnostics` - Get compilation errors/warnings for file

**Action Tools (Modify Code):**
- `rename_symbol` - Rename across all references (atomic operation)
- `edit_file` - Apply line-based edits with precise ranges

### Effective LSP Usage Patterns

**Before Renaming Fields/Variables:**
```bash
# 1. Explore impact scope
mcp__language-server__references fieldName

# 2. Verify the exact location  
mcp__language-server__hover filePath lineNum columnNum

# 3. Execute rename (updates ALL references)
mcp__language-server__rename_symbol filePath lineNum columnNum newFieldName
```

**Understanding Complex Types:**
```bash
# Find all uses to understand the type's role
mcp__language-server__references TypeName

# Get the source definition for context
mcp__language-server__definition TypeName

# Check hover info for quick type verification
mcp__language-server__hover filePath line column
```

**LSP Success Tips:**
- **Patience with syntax**: LSP tools are precise - learn the exact parameter formats
- **Use references liberally**: Understanding usage patterns prevents breaking changes
- **Hover for quick info**: Faster than reading files for type verification
- **Rename over manual edits**: Atomic operations prevent missed references
- **Column numbers matter**: LSP is position-sensitive for accuracy

## Before Creating ANY New Type

```bash
# 1. Search for existing implementations
mcp__language-server__references TypeName
mcp__language-server__definition StructName

# 2. Check primitives for shared types
Grep "struct|enum" crates/empack-lib/src/primitives/

# 3. If unsure, ask: "Does X already exist?"
```

## Core Types Reference (Anti-Duplication)

**Already Exist - DO NOT Recreate:**
- `ModLoader` → `crates/empack-lib/src/empack/parsing.rs` (NeoForge, Fabric, Quilt, Forge, Vanilla)
- `PackMetadata` → `crates/empack-lib/src/empack/config.rs` (pack.toml parsing)
- `PackVersions` → `crates/empack-lib/src/empack/config.rs` (flexible loader_versions HashMap)
- `BuildTarget` → `crates/empack-lib/src/primitives/` (shared enums)
- `StateTransition` → `crates/empack-lib/src/primitives/` (shared enums)

## Session Start Protocol

1. **Read context**: `init.md` for current state
2. **Audit existing**: Search before creating
3. **Import existing**: Reuse, don't duplicate
4. **Test early**: Verify integration works

## Development Rules

- **LSP First**: Search before implementing
- **Primitives Check**: Shared types live there  
- **Import > Create**: Reuse existing functionality
- **Test Integration**: Compile + test after changes

## Quick Commands

```bash
# Find type references
mcp__language-server__references ModLoader

# Check what exists
Grep "pub struct" crates/empack-lib/src/empack/

# Test compilation
cargo check -p empack-lib
```

**Remember**: Time spent searching prevents hours of refactoring duplicates.