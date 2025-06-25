#!/usr/bin/env bash
# Module: core
# Description: Core bootstrap, module loading, and application constants
# Dependencies: none

# Prevent multiple loading
if [ "${EMPACK_MODULE_CORE:-}" = "loaded" ]; then
    return 0
fi
readonly EMPACK_MODULE_CORE="loaded"

#=============================================================================
# CORE CONSTANTS AND CONFIGURATION
#=============================================================================

readonly EMPACK_VERSION="2.0.0"
readonly PACKWIZ_BOOTSTRAP_URL="https://github.com/packwiz/packwiz-installer-bootstrap/releases/latest/download/packwiz-installer-bootstrap.jar"

# Module load tracking
declare -A EMPACK_LOADED_MODULES

# Global associative arrays - declared here to ensure proper scope
declare -A EMPACK_COMMANDS
declare -A EMPACK_COMMAND_DESCRIPTIONS
declare -A EMPACK_COMMAND_HANDLERS
declare -A EMPACK_COMMAND_ORDER
declare -A EMPACK_COMMAND_REQUIRES_MODPACK

declare -A BUILD_TARGETS
declare -A BUILD_DEPENDENCIES

declare -A EMPACK_TEMPLATES
declare -A EMPACK_TEMPLATE_SOURCES
declare -A EMPACK_TEMPLATE_TARGETS
declare -A EMPACK_TEMPLATE_PROCESS_VARS

#=============================================================================
# GLOBAL FLAGS AND OPTIONS
#=============================================================================

# Program-level flags that affect behavior across all commands
EMPACK_VERBOSE=false
EMPACK_DEBUG=false
EMPACK_QUIET=false
EMPACK_DRY_RUN=false
EMPACK_SHOW_HELP=false
EMPACK_SHOW_VERSION=false
EMPACK_TARGET_DIR="."

# Export flag variables so other modules can access them
export EMPACK_VERBOSE EMPACK_DEBUG EMPACK_QUIET EMPACK_DRY_RUN EMPACK_TARGET_DIR

#=============================================================================
# MODULE LOADING SYSTEM
#=============================================================================

# Load a module by name
empack_load_module() {
    local module="$1"
    local module_path="$EMPACK_ROOT/lib/$module.sh"
    
    # Check if already loaded
    if [ "${EMPACK_LOADED_MODULES[$module]:-}" = "loaded" ]; then
        return 0
    fi
    
    # Verify module exists
    if [ ! -f "$module_path" ]; then
        echo "❌ Fatal: Cannot find module '$module' at $module_path" >&2
        exit 1
    fi
    
    # Load the module
    source "$module_path"
    EMPACK_LOADED_MODULES["$module"]="loaded"
}

# Initialize the application by loading all required modules
empack_init() {
    # Load modules in dependency order
    empack_load_module "logger"
    empack_load_module "utils"
    empack_load_module "commands"
    empack_load_module "deps"
    empack_load_module "templates"
    empack_load_module "builds"
    empack_load_module "init"
    
    # Initialize registries after all modules are loaded
    register_all_commands
    register_all_build_targets  
    register_all_templates
}

# Note: Global flag parsing is now handled directly in empack_main() to avoid scoping issues

# Main application entry point
empack_main() {
    # Parse global flags directly (no subprocess)
    local -a remaining_args=()
    
    while [ $# -gt 0 ]; do
        case "$1" in
            -v|--verbose)
                EMPACK_VERBOSE=true
                ;;
            -d|--debug)
                EMPACK_DEBUG=true
                EMPACK_VERBOSE=true  # Debug implies verbose
                ;;
            -q|--quiet)
                EMPACK_QUIET=true
                ;;
            --dry-run)
                EMPACK_DRY_RUN=true
                ;;
            -m|--modpack-directory)
                shift
                if [ $# -eq 0 ]; then
                    echo "❌ --modpack-directory requires a directory path" >&2
                    exit 1
                fi
                EMPACK_TARGET_DIR="${1%/}"
                ;;
            -h|--help)
                print_help
                exit 0
                ;;
            -V|--version)
                echo "empack version $EMPACK_VERSION"
                exit 0
                ;;
            -*)
                echo "❌ Unknown flag: $1" >&2
                echo "❌ Use 'empack --help' for usage information" >&2
                exit 1
                ;;
            *)
                # Not a flag, add to remaining arguments
                remaining_args+=("$1")
                ;;
        esac
        shift
    done
    
    # Set log level based on flags
    if [ "$EMPACK_DEBUG" = true ]; then
        export EMPACK_LOG_LEVEL=0  # DEBUG
    elif [ "$EMPACK_VERBOSE" = true ]; then
        export EMPACK_LOG_LEVEL=1  # INFO
    elif [ "$EMPACK_QUIET" = true ]; then
        export EMPACK_LOG_LEVEL=3  # WARNING
    fi
    
    # Handle no arguments or explicit help command
    if [ ${#remaining_args[@]} -eq 0 ] || [ "${remaining_args[0]:-}" = "help" ]; then
        print_help
        exit 0
    fi
    
    # Handle explicit version command
    if [ "${remaining_args[0]:-}" = "version" ]; then
        echo "empack version $EMPACK_VERSION"
        exit 0
    fi
    
    # Parse and route commands with remaining arguments
    parse_and_execute_commands "${remaining_args[@]}"
}

# Get version information
empack_version() {
    echo "$EMPACK_VERSION"
}

# Check if we're in a modpack directory (for build commands)
empack_validate_modpack_directory() {
    local pack_file="$EMPACK_TARGET_DIR/pack/pack.toml"
    if [ ! -f "$pack_file" ]; then
        log_error "Not in a modpack directory ($pack_file not found)"
        if [ "$EMPACK_TARGET_DIR" != "." ]; then
            log_error "Run 'empack --modpack-directory \"$EMPACK_TARGET_DIR\" init' to set up a modpack development environment"
        else
            log_error "Run 'empack init' to set up a modpack development environment"
        fi
        exit 1
    fi
}

# Export functions for use by other modules
export -f empack_load_module empack_version empack_validate_modpack_directory