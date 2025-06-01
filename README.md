# Empack - Standalone Minecraft Modpack Development Tool

Version 2.0.0 - Modular Architecture

## Overview

Empack is a comprehensive build system and development environment for Minecraft modpacks. It provides dependency checking, environment bootstrapping, and multi-target building with template processing.

## Features

- **Dependency Management**: Flutter doctor-style validation with setup guidance
- **Environment Bootstrapping**: Complete development environment initialization
- **Multi-Target Building**: Client, server, and Modrinth distributions
- **Template Processing**: Automated configuration and deployment templates
- **CWD Operation**: Works from any directory (not tied to script location)

## Quick Start

### 1. Check Dependencies
```bash
./empack requirements
```

### 2. Initialize New Modpack
```bash
mkdir my-modpack && cd my-modpack
/path/to/empack init
```

### 3. Build Modpack
```bash
empack mrpack          # Build .mrpack file
empack client server   # Build distributions
empack all             # Build everything
```

## Commands

### Setup Commands
- `requirements` - Check tool dependencies and show setup guidance
- `init` - Initialize modpack development environment
- `help` - Show help message
- `version` - Show version information

### Build Commands
- `clean` - Clean build directories
- `mrpack` - Build Modrinth-compatible .mrpack file
- `client` - Build bootstrapped client installer
- `server` - Build bootstrapped server installer
- `client-full` - Build non-redistributable client (includes all mods)
- `server-full` - Build non-redistributable server (includes all mods)
- `all` - Build mrpack, client, and server

## Architecture

### Module Structure

```
empack-src/
├── empack              # Main entry point
├── lib/                # Core library modules
│   ├── core.sh         # Bootstrap and module loading
│   ├── logger.sh       # Logging and output management
│   ├── utils.sh        # File operations and utilities
│   ├── commands.sh     # Command registry and routing
│   ├── deps.sh         # Dependency validation system
│   ├── templates.sh    # Template management
│   ├── builds.sh       # Build system implementation
│   └── init.sh         # Bootstrap/initialization
├── templates/          # Template files
│   ├── client/         # Client-specific templates
│   ├── server/         # Server-specific templates
│   └── github/         # GitHub workflow templates
└── README.md          # This file
```

### Module Dependencies

```
core.sh → (no dependencies)
logger.sh → core.sh
utils.sh → core.sh, logger.sh
commands.sh → core.sh, logger.sh
deps.sh → core.sh, logger.sh, utils.sh
templates.sh → core.sh, logger.sh, utils.sh
builds.sh → core.sh, logger.sh, utils.sh, templates.sh
init.sh → core.sh, logger.sh, utils.sh, templates.sh, deps.sh
```

## Dependencies

### Required Tools
- **packwiz** - Modpack management and export
- **tomlq/tq** - TOML file processing
- **mrpack-install** - Server installation tool
- **java** - Java runtime for Minecraft

### Installation Guidance
Run `empack requirements` for detailed installation instructions.

### Packwiz Installation
```bash
# Via Go (if installed)
go install github.com/packwiz/packwiz@latest

# Or download from GitHub Actions
# See: https://github.com/packwiz/packwiz/actions
```

### tomlq Installation
```bash
# Via Cargo (if installed)
cargo install tomlq

# Or download from releases
# See: https://github.com/cryptaliagy/tomlq/releases/latest
```

### mrpack-install Installation
```bash
# Download from releases
# See: https://github.com/nothub/mrpack-install/releases/latest
```

## Template System

Empack uses a modular template system for generating configuration files:

### Template Variables
- `{{NAME}}` - Pack name from pack.toml
- `{{VERSION}}` - Pack version from pack.toml
- `{{AUTHOR}}` - Pack author from pack.toml
- `{{MC_VERSION}}` - Minecraft version from pack.toml
- `{{FABRIC_VERSION}}` - Fabric version from pack.toml

### Template Categories
- **Configuration**: .gitignore, .actrc
- **Client**: Instance configuration, MMC pack metadata
- **Server**: Installation scripts, server properties
- **GitHub**: Workflow files for CI/CD

## Build Targets

### Standard Builds
- **mrpack**: Modrinth-compatible distribution (redistributable)
- **client**: Prism/MultiMC client with packwiz bootstrap (redistributable)
- **server**: Minecraft server with packwiz bootstrap (redistributable)

### Full Builds (Non-Redistributable)
- **client-full**: Client with all mods pre-downloaded
- **server-full**: Server with all mods pre-downloaded

## Development

### Adding New Commands
1. Register command in `lib/commands.sh`:
   ```bash
   register_command "mycommand" "Description" "handler_function" order
   ```

2. Implement handler function in appropriate module
3. Export handler function

### Adding New Templates
1. Create template file in `templates/` directory
2. Register in `lib/templates.sh`:
   ```bash
   register_template "name" "source_path" "target_path" process_vars
   ```

### Adding New Build Targets
1. Register target in `lib/builds.sh`:
   ```bash
   register_build_target "name" "handler_function" "dependencies"
   ```

2. Implement handler function

## Examples

### Initialize and Build New Modpack
```bash
mkdir my-awesome-pack
cd my-awesome-pack
empack init
# Follow interactive prompts
empack requirements  # Verify setup
empack all          # Build all targets
```

### Add Mods and Rebuild
```bash
packwiz mr install sodium
packwiz mr install iris
empack mrpack       # Rebuild for distribution
```

### Clean and Rebuild
```bash
empack clean        # Clean build artifacts
empack client server # Rebuild distributions
```

## License

This tool is designed for use with the layer_1 modpack development workflow.

## Contributing

When modifying empack:
1. Follow the modular architecture
2. Maintain clean module dependencies
3. Update documentation
4. Test all commands and build targets
5. Ensure templates remain functional