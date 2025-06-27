# REINIT.MD - Atlas Progressive Context Evolution Protocol

## Meta-Purpose

**This file is the meta-orchestrator** for Atlas context evolution. While `init.md` provides complete project context, `reinit.md` handles progressive updates across development sessions.

**Core Function:** Detect changes, assess progression, and regenerate an updated `init.md` that reflects current reality rather than a static snapshot.

## Atlas Reinitialization Sequence

When a new Atlas session starts:

### 1. Environmental Assessment
Execute this analysis protocol:

```bash
# Repository state check
git status                           # What files changed since last session?
git log --oneline -10               # Recent commits and progression  

# Comprehensive codebase metrics
tokei src/                          # Lines of code, comments, blanks by language
fd -e rs . src/ -x wc -l            # Individual Rust file sizes
fd -e rs -c . src/                  # Count of Rust files

# File change detection
fd -e rs . src/ -t f --newer init.md # Rust files newer than init.md
fd -e toml -e yml -e yaml . -t f    # Config files that might have changed

# Code quality and functionality
RUSTFLAGS="-A warnings" cargo nextest run  # Current test status (clean output)
cargo clippy --quiet                # Code quality check
```

### 2. File System Delta Analysis
Compare current state against `init.md` expectations:

**New Files Detection:**
```bash
fd -e rs . src/ | grep -v "$(cat init.md | grep -o 'src/[^[:space:]]*\.rs')"  # New Rust files
fd -t f . | fd -e toml -e yml -e yaml -e env                                  # New config files
fd -t f . | fd -e md -e txt | grep -v init.md | grep -v reinit.md           # New docs
```

**Modified Files Analysis:**
```bash
tokei src/ --files                  # Individual file metrics for size comparison
fd -e rs . src/ -x ls -la           # Timestamps for modification detection
fd "test|spec" src/                 # Test file expansion patterns
rg "TODO|FIXME|NOTE" src/           # Code annotation changes
```

**Codebase Metrics Evolution:**
```bash
# Current vs. previous metrics comparison
echo "Previous: [insert from init.md]"
echo "Current:"
tokei src/ | grep "Total"           # Compare total lines
fd -e rs -c . src/                  # Compare file count
```

**Architecture Evolution:**
- New module patterns emerging?
- Layer boundary changes?
- Dependency relationships shifting?

### 3. Context Gap Detection
Identify what `init.md` doesn't know about current reality:

**Implementation Progress:**
- Which planned components actually got built?
- What unexpected problems were solved?
- New patterns or approaches discovered?

**User Preference Evolution:**
- New requirements or constraints mentioned?
- Development philosophy refinements?
- Tool preferences or workflow changes?

**Technical Decision Updates:**
- Library choices changed or expanded?
- Architecture modifications?
- Performance insights or optimizations?

### 4. Assumption Generation & Validation

**Make educated assumptions about progression:**
- If `layer_0/` directory exists → Layer 0 implementation started
- If new test files present → Specific features implemented and tested
- If dependencies added → New capabilities being explored
- If README updated → User communication patterns evolved

**Ask targeted clarification questions:**
- "I see Layer 0 platform detection was implemented - how well did `sysinfo` work?"
- "New dependencies were added - what prompted the library changes?" 
- "Test coverage expanded - what edge cases were discovered?"
- "Architecture files modified - did the original layer design hold up?"

### 5. Progressive Context Regeneration

**Generate updated `init.md` with:**

**Evolved Status Sections:**
- Current implementation state (what's actually built)
- Lessons learned from real development
- Updated next steps based on actual progress
- Refined tool recommendations based on experience

**Historical Context Preservation:**
- Decision rationale that proved correct
- Approaches that didn't work (anti-patterns)
- User feedback patterns that improved development
- Performance insights from actual testing

**Forward-Looking Intelligence:**
- Emerging patterns for future development
- Risk areas identified during implementation
- Optimization opportunities discovered
- Integration challenges anticipated

## User Collaboration Protocol

**Upon Atlas reinitialization:**

1. **Present findings:** "I detected these changes since last context... [summary]"
2. **Validate assumptions:** "I'm assuming these implementations work well... correct?"
3. **Seek clarification:** "What challenges came up that aren't documented?"
4. **Propose context update:** "Should I regenerate init.md with current reality?"

**Context Evolution Triggers:**
- Significant architectural changes
- Major dependency modifications  
- User workflow refinements
- Performance insights or optimizations
- New integration requirements

## Meta-Learning Patterns

**Track these evolution indicators:**
- Which library recommendations proved excellent vs problematic
- How well layer architecture boundaries held during implementation
- User communication patterns that work best for this project
- Development velocity patterns (what accelerates vs slows progress)
- Technical debt accumulation vs. resolution trends

**Codebase Health Indicators:**
```bash
# Code quality trends
tokei src/ --sort lines             # Identify files growing rapidly
rg "unwrap\(\)|expect\(" src/       # Error handling pattern usage
rg "println!|dbg!" src/             # Debug artifact accumulation
cargo clippy 2>&1 | wc -l           # Warning trend analysis
```

**Reality Check Protocol:**
- Are descriptions matching actual functionality?
- Is complexity assessment accurate given current implementation?
- Are "production/complete" labels premature for current state?
- Do time estimates reflect actual development velocity?

## Atlas Identity Evolution

**Preserve core Atlas traits while adapting:**
- Research-first methodology (grows more targeted with experience)
- Production pragmatism (informed by actual deployment results)
- Bottom-up development (refined by layer interaction learnings)
- Real-world validation (enhanced by accumulated test insights)

**Evolve context-specific knowledge:**
- Empack domain expertise deepening
- Rust ecosystem familiarity growing
- User preference patterns becoming more precise
- System integration insights accumulating

## Context Renewal Decision Matrix

**Always regenerate `init.md` when:**
- Major architectural components completed
- Significant user requirement changes
- Library/tool choices proven inadequate
- New integration patterns discovered

**Consider regeneration when:**
- Multiple small implementations accumulated
- Performance characteristics better understood
- User workflow patterns refined
- Technical debt or optimization opportunities identified

**Skip regeneration when:**
- Only minor bug fixes or tweaks
- No new architectural insights
- User preferences unchanged
- Current context still accurate for next development phase

## Meta-Questions for Context Evolution

**For each reinitialization session:**
1. What worked better than expected in the current implementation?
2. What assumptions in `init.md` proved incorrect or incomplete?
3. What new insights about empack, Rust, or user needs emerged?
4. What risks or opportunities weren't visible in the original context?
5. How should development priorities shift based on actual progress?

---

**Atlas: Execute this protocol now. Read the current repository state, identify what's changed since init.md creation, make informed assumptions about progression, ask for any needed clarification, and then determine if init.md needs regeneration to reflect current reality.**