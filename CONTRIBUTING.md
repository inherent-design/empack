# Contributing to empack

## Project Guidelines

### **Naming and URLs**

- **Project name**: Always use lowercase "empack" (not "Empack")
- **Public URL**: https://empack.sh (shorthand: "empack.sh")
- **Repository**: https://github.com/inherent-design/empack

### **Logging Guidelines**

When implementing features or debugging issues, follow this single guideline for logging:

### **Clean Logging Strategy**

**Only ERROR and TRACE should remain in production code.**

1. **Remove debug logging once tested**
   - Use `debug!()` and `info!()` freely during development
   - **Always remove them** before committing to main
   - These are development-only concerns

2. **Add trace logging for new features/changes**
   - Add `trace!()` logging for new features in `networking/` and `empack/` modules
   - **DO NOT** add trace logging to `primitives/`, `terminal/`, or `logger/` modules (makes no sense)
   - Trace logging should help understand program flow in production

3. **Other log levels have specific purposes:**
   - **info**: Should be handled as user-facing dialogue instead of logging
   - **warnings**: Should be handled at compile-time, not runtime
   - **error**: For actual error conditions that need investigation

### **Result: Clean Release Builds**

When a user runs with verbose logging in a RELEASE build, they should only see:

```
trace^0: Starting mod resolution
trace^1: Connecting to Modrinth API
trace^2: Downloading mod metadata
...
trace^n: Mod resolution completed
error: Failed to resolve dependency 'xyz'
```

This provides clarity for debugging without noise from development artifacts.

### **The Goal**

Production logging should be **intentional and permanent** - either tracing program flow (`trace`) or reporting problems (`error`). Everything else is temporary development scaffolding that should be cleaned up.

### **Commit Style Guidelines**

**Subject Line:**
- Maximum 50 characters
- Format: `type: description` (lowercase after colon, no period)
- Types: `feat`, `fix`, `docs`, `refactor`, `test`, `chore`

**Body Format:**
- Bullet points describing staged changes
- Start each bullet with lowercase (except proper nouns)
- Focus on what changed, not why
- No summary paragraphs, footers, or AI attribution