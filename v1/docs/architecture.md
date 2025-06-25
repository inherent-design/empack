# empack Architecture: Runtime Boundary Architecture for Professional Modpack Development

## Executive Summary

empack is a sophisticated, standalone Minecraft modpack development tool that transforms modpack creation from manual, error-prone processes into automated, professional workflows. The system implements a critical **runtime boundary architecture** that cleanly separates pre-initialization (setup) and post-initialization (operational) phases, enabling intelligent ecosystem integration and predictable user experiences.

## System Overview

### Core Philosophy

**Mission**: Professional modpack development tooling with loader-first ecosystem selection, intelligent API-driven auto-fill, and predictable flag behavior.

**Key Innovation**: Runtime boundary architecture enabling clean separation of setup vs operational functionality, combined with unified state management for reliable cross-module communication.

### High-Level Architecture

```
empack System Architecture
├── Runtime Boundary Management
│   ├── Pre-Init Phase (Setup)
│   │   ├── Dependency validation
│   │   ├── API integration & version resolution
│   │   ├── Environment bootstrapping
│   │   └── Static template processing
│   └── Post-Init Phase (Operations)
│       ├── Build orchestration
│       ├── Dynamic template processing
│       ├── Distribution creation
│       └── Archive generation
├── Unified State Management
│   ├── Global state variables (EMPACK_MODULE_ENTITY_PROPERTY)
│   ├── Module interface contracts
│   ├── Cross-module communication
│   └── Error state propagation
└── Professional User Experience
    ├── Progressive disclosure
    ├── Intelligent auto-fill
    ├── Safe testing environments
    └── Clear feedback systems
```

## Core Architectural Concepts

### 1. Runtime Boundary Architecture

**Critical Concept**: empack operates with a fundamental **runtime boundary** separating pre-initialization and post-initialization phases based on the existence of a valid `pack/pack.toml` structure.

#### Pre-Init Phase (Before valid empack + pack/pack.toml structure)

**Available Operations**:
- Dependency checking and validation
- Flag parsing and configuration
- API integration for version resolution
- Modloader ecosystem selection
- Development environment setup

**Commands**: `requirements`, `init`, `version`, `help`

**Templates**: Static development files (.gitignore, .actrc, GitHub workflows)

**Responsibility**: empack owns completely

**Boundary Detection**:
```bash
is_pre_init() {
    [ ! -f "$EMPACK_TARGET_DIR/pack/pack.toml" ]
}
```

#### Post-Init Phase (After valid empack + pack/pack.toml established)

**Available Operations**:
- Build operations and orchestration
- pack.toml variable extraction
- Distribution creation and packaging
- Archive generation with proper structure

**Commands**: `mrpack`, `client`, `server`, `client-full`, `server-full`, `clean`, `all`

**Templates**: Dynamic build files requiring {{VARIABLES}} from pack.toml

**Responsibility**: Shared (empack builds + packwiz content management)

**Critical Boundary Rule**: Pre-init functions NEVER assume pack.toml exists, post-init functions ALWAYS require valid pack.toml.

### 2. Unified State Management Architecture

**Philosophy**: Eliminate stdout pollution and enable clean data flow through global state variables while maintaining user-facing logging independence.

#### State Variable Naming Convention

**Pattern**: `EMPACK_MODULE_ENTITY_PROPERTY`

**Examples**:
```bash
# API Module
EMPACK_API_CALL_STATUS=""
EMPACK_API_MINECRAFT_LATEST_VERSION=""
EMPACK_API_ERROR_MESSAGE=""

# Command Execution Module  
EMPACK_COMMAND_EXEC_CURRENT_COMMAND=""
EMPACK_COMMAND_EXEC_VALIDATION_STATUS=""
EMPACK_COMMAND_EXEC_LAST_HANDLER_RESULT=""

# Validation Module
EMPACK_VALIDATION_FORMAT_STATUS=""
EMPACK_VALIDATION_ERROR_COUNT=""
EMPACK_VALIDATION_LAST_ERROR_MESSAGE=""
```

#### State Management Benefits

- **Logging Independence**: Debug mode works without breaking functionality
- **Clean Data Flow**: State variables separate from diagnostic output
- **No Brittle Parsing**: Eliminates stdout parsing and IFS manipulation
- **User-Centric Logging**: Messages go to users, not hidden in stderr

### 3. Module Interface Contracts

**Required Functions** (Every Module Must Implement):
```bash
clear_${module}_state()      # Reset all EMPACK_${MODULE}_* variables
export_${module}_state()     # Export all EMPACK_${MODULE}_* variables  
get_${module}_status()       # Return: "ready|error|incomplete|unknown"
validate_${module}_state()   # Return: 0=valid, 1=invalid + error details
```

**Required State Variables** (Every Module Must Provide):
```bash
EMPACK_${MODULE}_STATUS=""           # "ready|error|incomplete|unknown"
EMPACK_${MODULE}_ERROR_MESSAGE=""    # Last error message if status=error
EMPACK_${MODULE}_LAST_OPERATION=""   # Last operation performed
```

## System Components

### Core Infrastructure (`lib/core.sh`)

**Responsibilities**:
- Module loading with dependency order management
- Global command registry arrays
- Application configuration state
- Cross-module state utilities
- Flag parsing and configuration

**Key Features**:
- Bootstrap system with minimal dependencies
- Enhanced resolution: PATH → CWD → modpack directory
- Professional error handling with trap management
- Flag architecture supporting three-tier system

### Module Architecture (`lib/modules/`)

#### Dependency Order
```
core.sh → (no dependencies)
logger.sh → core.sh
utils.sh → core.sh, logger.sh
deps.sh → core.sh, logger.sh, utils.sh
boundaries.sh → core.sh, logger.sh, deps.sh
commands.sh → core.sh, logger.sh, boundaries.sh
api.sh → core.sh, logger.sh, utils.sh, deps.sh
validation.sh → core.sh, logger.sh, utils.sh, deps.sh, api.sh
compatibility.sh → core.sh, logger.sh, utils.sh, deps.sh, api.sh, validation.sh
init.sh → core.sh, logger.sh, utils.sh, boundaries.sh, deps.sh, api.sh, validation.sh, compatibility.sh
dev-templates.sh → core.sh, logger.sh, utils.sh, boundaries.sh
build-templates.sh → core.sh, logger.sh, utils.sh, boundaries.sh
builds.sh → core.sh, logger.sh, utils.sh, boundaries.sh, build-templates.sh
```

#### Module Specifications

**logger.sh** - Logging and Output Management
- Hierarchical logging system (debug, info, success, warning, error)
- Log level management based on flags (--debug, --verbose, --quiet)
- Clean separation of user messages vs diagnostic output
- Semantic indicators and progressive disclosure

**utils.sh** - File Operations and API Utilities
- Enhanced dependency resolution (PATH → CWD → modpack)
- Safe file operations with validation
- Download utilities with progress tracking
- Cross-platform compatibility helpers

**deps.sh** - Dependency Validation System
- Flutter doctor-style requirements checking
- Tool availability detection with version parsing
- Installation guidance with platform-specific instructions
- Dependency state management and caching

**boundaries.sh** - Runtime Phase Management
- Pre-init vs post-init detection
- Phase transition validation
- Command availability enforcement
- Template lifecycle management

**commands.sh** - Command Registry and Routing
- Sophisticated five-array command system
- Runtime boundary enforcement
- Two-pass execution pipeline (validation + execution)
- Command ordering and deduplication
- Meta-command expansion ("all" → "mrpack client server")

**api.sh** - API Integration for Version Resolution
- Multi-modloader API integration (NeoForge, Fabric, Quilt, Vanilla)
- Version compatibility checking
- Intelligent fallback handling
- Rate limiting and caching support

**validation.sh** - Format and Workflow Validation
- Configuration format validation
- Workflow state verification
- Input sanitization and type checking
- Integration with compatibility checking

**compatibility.sh** - Ecosystem Analysis and Auto-Fill
- Modloader ecosystem understanding
- Version compatibility matrix validation
- Intelligent default selection
- Core input stabilization

**init.sh** - Three-Mode Initialization System
- Zero-config golden path with API defaults
- Explicit non-interactive with flag enhancement
- Interactive with auto-fill pre-population
- packwiz integration and bootstrapping

**dev-templates.sh** - Pre-Init Static Templates
- Static template processing (.gitignore, .actrc, workflows)
- Template validation and verification
- Variable substitution for development environment
- Template category management

**build-templates.sh** - Post-Init Dynamic Templates
- Dynamic template processing requiring pack.toml variables
- Configuration file generation (instance.cfg, server.properties)
- Multi-target template support
- Integration with build system

**builds.sh** - Build System Implementation
- Multi-target build orchestration
- Archive generation with proper structure
- Distribution packaging and validation
- Integration with pack.toml metadata

## User Experience Architecture

### Loader-First Auto-Fill Philosophy

**Core Principle**: Predictable enhancement - flags act as auto-fill defaults while -y controls interactive behavior.

#### Three Initialization Modes

**1. Zero-Config Golden Path**:
```bash
empack init -y
# → API resolves: neoforge + latest stable + compatible minecraft
# → Smart defaults for personalization
# → Ready to build in ~3 seconds
```

**2. Explicit Non-Interactive**:
```bash
empack init -y --modloader fabric --mc-version 1.21.1 --name "Performance Pack"
# → Uses provided flags + smart defaults for missing values
# → Core input stabilization validates compatibility
# → No prompts, immediate initialization
```

**3. Interactive with Auto-Fill**:
```bash
empack init --modloader fabric --name "Performance Pack"
# → Shows all prompts for educational value
# → Flags pre-populate defaults
# → User can change any defaults during prompts
```

### Progressive Disclosure

- **Simple Defaults**: Zero-config path works immediately
- **Advanced Options**: Available when needed via flags
- **Educational Value**: Interactive mode teaches ecosystem choices
- **Safe Testing**: --modpack-directory isolation for development

## Integration Boundaries

### empack Responsibilities
- Professional initialization and environment setup
- API integration and version resolution  
- Build system and distribution creation
- Development tooling and automation

### packwiz Responsibilities
- Modpack content and mod management
- pack.toml maintenance and mod metadata
- Mod installation and updates

### Clean Integration Pattern
```bash
# 1. empack handles professional initialization
empack init -y --modloader fabric

# 2. User uses packwiz for modpack content
cd pack
packwiz mr install sodium
packwiz mr install lithium
cd ..

# 3. empack handles professional build system
empack mrpack client server
```

## Quality Standards

### Technical Excellence
- **Runtime Boundary**: Clean separation prevents template contamination
- **API Integration**: Robust version resolution with graceful fallbacks  
- **Professional UX**: Sub-10 second init with intelligent defaults
- **Build Reliability**: Multi-target distributions work across environments

### User Experience  
- **Progressive Disclosure**: Simple defaults, advanced options available
- **Safe Testing**: --modpack-directory isolation for development
- **Clear Feedback**: Emoji-based progress with actionable error messages  
- **Intelligent Automation**: Non-interactive mode preserves smart decision-making

### Architecture Quality
- **Modular Design**: Clean separation of concerns with minimal coupling
- **Extensibility**: Easy addition of modloaders, commands, templates
- **Maintainability**: Professional code organization enabling collaboration
- **Testing Coverage**: Comprehensive validation across scenarios

## Implementation Status

### Completed (Phase 1-3)
✅ **Bootstrap System**: Module loading with dependency order management
✅ **Runtime Boundary**: Pre-init vs post-init phase detection and enforcement
✅ **Logging System**: Professional hierarchical output with semantic indicators
✅ **Command Registry**: Sophisticated five-array system with runtime boundary enforcement
✅ **State Management**: Unified EMPACK_MODULE_ENTITY_PROPERTY pattern
✅ **Module Interfaces**: Standard 4-function interface contracts

### In Progress (Phase 4)
🔧 **API Integration**: Version resolution functions need completion
🔧 **Initialization**: execute_initialization() requires implementation
🔧 **Template Processing**: Actual template logic needs development
🔧 **Build System**: build-templates.sh and builds.sh modules need creation

### Future (Phase 5)
🔮 **Advanced UX**: Fuzzy finder integration, preset system
🔮 **Ecosystem Expansion**: Additional modloader support
🔮 **Distribution**: empack.sh installation endpoint
🔮 **Documentation**: Complete user guides and API documentation

## Extension Points

### Modloader Support
- Plugin architecture for new modloaders
- API integration templates
- Compatibility matrix extensions
- Build target customization

### Template System
- Custom template categories
- Variable substitution engine
- Template validation framework
- Community template sharing

### Build Targets
- Platform-specific builds
- Custom archive formats
- Distribution channel integration
- Deployment automation

### User Experience
- Fuzzy finder integration
- Preset system for common configurations
- Interactive tutorial system
- Community configuration sharing

## Performance Considerations

### Initialization Speed
- API caching for version resolution
- Parallel dependency checking
- Lazy loading of non-critical modules
- Efficient template processing

### Build Performance
- Incremental builds where possible
- Parallel archive generation
- Smart change detection
- Resource optimization

### Memory Management
- Module state cleanup
- Temporary file management
- Large file handling
- Resource monitoring

## Security Considerations

### Input Validation
- Flag parsing security
- Template variable sanitization
- File path validation
- API response verification

### File Operations
- Safe temporary file handling
- Permission management
- Path traversal prevention
- Archive integrity validation

### Network Security
- API endpoint validation
- TLS certificate verification
- Rate limiting implementation
- Secure download handling

---

**Architecture Status**: Foundation complete, functional implementation in progress. The runtime boundary architecture provides a solid foundation for professional modpack development tooling with clear separation of concerns and extensible design patterns.

**Next Phase**: Complete functional implementation of API integration, initialization execution, template processing, and build system to achieve working end-to-end workflows.