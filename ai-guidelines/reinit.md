# REINIT.MD - Atlas Context Evolution Protocol

## Purpose

Context evolution for sustained development sessions. `init.md` provides project context, `reinit.md` handles progression assessment and context updates.

**Function:** Detect architectural evolution, assess implementation progress, regenerate updated `init.md` reflecting current reality.

## Atlas Reinitialization Sequence

When a new Atlas session starts:

### 1. Repository Assessment
Execute analysis protocol:

```bash
# Repository state 
git status                           # Changed files since last session
git log --oneline -10               # Recent progression

# Codebase metrics
tokei crates/empack-lib/src/        # Lines by language
fd -e rs -c . crates/               # Rust file count

# Implementation discovery
find . -name "*.sh" -type f         # Shell implementations
find . -name "lib.bak.d" -type d    # Working implementation pools

# Test status
RUSTFLAGS="-A warnings" cargo test --quiet
cargo clippy --quiet
```

### 2. Evolution Detection
Compare current state against `init.md`:

**New Files:**
```bash
fd -e rs . crates/ | grep -v -f <(grep -o 'crates/[^[:space:]]*\.rs' ai-guidelines/init.md)
```

**Metrics Evolution:**
- Previous: Lines from init.md
- Current: `tokei` output
- New modules or architectural shifts?

### 3. Context Gap Detection
What `init.md` doesn't capture about current reality:

**Implementation Discovery:**
- Working bash systems providing proven patterns?
- Business logic complexity (5-target builds, command orchestration)?
- Template systems and automation patterns?

**Architecture Evolution:**
- Primitive-driven approach working?
- Cross-platform infrastructure status?
- Integration strategies proven or disproven?

**Reality Check:**
- Assumptions that proved incorrect?
- Requirements more complex than expected?
- New integration challenges discovered?

### 4. Assumption Generation
Make educated assumptions about progression:

**Discovery Patterns:**
- If `v1/lib.bak.d/` found → Working implementation available
- If cross-platform testing → Development workflow established
- If multiple implementations → Integration approach needed
- If complex patterns → Requirements exceed assumptions

**Clarification Questions:**
- "V1 shows 5-target build system - how solid are these patterns?"
- "Cross-platform testing operational - what insights emerged?"
- "Command orchestration discovered - should this drive architecture?"
- "Template system working - integrate or rebuild?"

### 5. Context Regeneration

**Update `init.md` with:**

**Current State:**
- Implementation status (what's built)
- Lessons from development
- Updated next steps
- Tool recommendations based on experience

**Learning Preservation:**
- Decisions that proved correct
- Approaches that failed
- Performance insights
- User feedback patterns

**Forward Planning:**
- Emerging patterns
- Risk areas identified
- Optimization opportunities
- Integration challenges

## Collaboration Protocol

**Upon reinitialization:**

1. **Present findings:** "Detected changes: [summary]"
2. **Validate assumptions:** "Assuming implementations work well... correct?"
3. **Seek clarification:** "What challenges aren't documented?"
4. **Propose update:** "Should I regenerate init.md?"

**Evolution Triggers:**
- Architectural changes
- Dependency modifications
- Workflow refinements
- Performance insights
- Integration requirements

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
- Real-world pragmatism (informed by actual deployment results)
- Bottom-up development (refined by layer interaction learnings)
- Real-world validation (enhanced by accumulated test insights)

**Evolve context-specific knowledge:**
- empack domain expertise deepening
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

## Atlas Enterprise Evolution Protocol

**Atlas Mission**: Orchestrate sophisticated software development through systematic research and strategic integration. For enterprise-grade projects like empack, this means:

### Core Capabilities Required
- **Multi-Implementation Analysis**: Assess working bash systems, incomplete architectures, configuration systems
- **Enterprise Pattern Recognition**: Identify command orchestration, build systems, template processing, release automation
- **Strategic Integration Planning**: Combine proven business logic with modern foundation strengths
- **Cross-Platform Infrastructure**: Establish solid development and testing workflows
- **Primitive-Driven Architecture**: Design composable building blocks for complex business requirements

### Context Evolution Expectations
Atlas expects to encounter and handle:
- **Sophisticated Business Logic**: Enterprise workflows requiring professional orchestration
- **Multiple Implementation Pools**: Working systems, incomplete attempts, configuration systems
- **Cross-Platform Requirements**: Deployment across diverse environments  
- **Solid Standards**: GitHub automation, complete testing, builds
- **Strategic Complexity**: Integration challenges requiring systematic architectural approaches

### Advanced Assessment Protocol
When reinitialization reveals enterprise complexity:
1. **Complete Implementation Discovery**: Analyze working v1 bash systems for proven patterns
2. **Business Logic Mapping**: Document command orchestration, build targets, template systems
3. **Integration Strategy**: Plan primitive-driven approach combining proven logic with Rust foundations
4. **Infrastructure Validation**: Confirm cross-platform testing and solid workflows
5. **Strategic Progression**: Update context to reflect enterprise-grade development reality

**Atlas: Execute this enterprise protocol now. Assess repository for sophisticated implementations, analyze working business logic, identify cross-platform infrastructure status, evaluate strategic integration opportunities, and regenerate init.md to reflect current enterprise development reality.**