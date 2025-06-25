#!/usr/bin/env bash
# Module: core
# Description: Bootstrap, global state management, and module loading
# Dependencies: none (foundation module)

# Prevent multiple loading
if [ "${EMPACK_MODULE_CORE:-}" = "loaded" ]; then
  return 0
fi
readonly EMPACK_MODULE_CORE="loaded"

#=============================================================================
# CORE CONSTANTS AND CONFIGURATION
#=============================================================================

# Version and identity
readonly EMPACK_VERSION="0.5.0-alpha"
readonly EMPACK_NAME="empack"

# Default configuration
readonly EMPACK_DEFAULT_TARGET_DIR="."

# Module loading order for dependency resolution
readonly EMPACK_MODULE_ORDER=(
  "logger"          # Configurable, dynamic logging
  "utils"           # Common: environment, file, network, etc.
  "deps"            # packwiz, mrpack-install, java, jq, etc.
  "boundaries"      # Pre-Post init boundary transition
  "commands"        # Command registry, parsing and execution
  "api"             # Remote API calls for live version information
  "compatibility"   # Inter-version compatibility checks (Minecraft vs. modloader)
  "validation"      # Argument and option validation
  "dev-templates"   # empack bootstrap templates
  "init"            # empack modpack bootstrapper
  "build-templates" # Build templates
  "builds"          # Build operations
)

#=============================================================================
# GLOBAL STATE MANAGEMENT - CLEAN BREAK ARCHITECTURE
#=============================================================================

# Core state variables (EMPACK_CORE_* namespace)
declare -g EMPACK_CORE_TARGET_DIR="${EMPACK_DEFAULT_TARGET_DIR}"
declare -g EMPACK_CORE_VERBOSE="false"
declare -g EMPACK_CORE_DEBUG="false"
declare -g EMPACK_CORE_QUIET="false"
declare -g EMPACK_CORE_DRY_RUN="false"
declare -g EMPACK_CORE_NON_INTERACTIVE="false"
declare -g EMPACK_CORE_RUNTIME_PHASE=""
declare -g EMPACK_CORE_INITIALIZATION_COMPLETE="false"

# Command registry storage (global arrays for commands.sh)
declare -A EMPACK_COMMANDS                 # command names
declare -A EMPACK_COMMAND_DESCRIPTIONS     # help text
declare -A EMPACK_COMMAND_HANDLERS         # function names
declare -A EMPACK_COMMAND_ORDER            # execution priority
declare -A EMPACK_COMMAND_REQUIRES_MODPACK # runtime boundary enforcement

#=============================================================================
# GLOBAL STATE UTILITIES
#=============================================================================

# Clear all state variables for a specific module
clear_module_state() {
  local module="$1"
  if [ -z "$module" ]; then
    echo "❌ clear_module_state: module name required" >&2
    return 1
  fi

  # Convert module name to uppercase for variable prefix
  local prefix=$(echo "$module" | tr '[:lower:]' '[:upper:]')

  # Clear all variables matching the pattern EMPACK_MODULE_*
  for var in $(compgen -v "EMPACK_${prefix}_"); do
    declare -g "$var"=""
  done

  return 0
}

# Export all state variables for a specific module
export_module_state() {
  local module="$1"
  if [ -z "$module" ]; then
    echo "❌ export_module_state: module name required" >&2
    return 1
  fi

  # Convert module name to uppercase for variable prefix
  local prefix=$(echo "$module" | tr '[:lower:]' '[:upper:]')

  # Export all variables matching the pattern EMPACK_MODULE_*
  for var in $(compgen -v "EMPACK_${prefix}_"); do
    export "$var"
  done

  return 0
}

# Get state variable value
get_state() {
  local var_name="$1"
  if [ -z "$var_name" ]; then
    echo "❌ get_state: variable name required" >&2
    return 1
  fi

  # Use indirect reference to get variable value
  echo "${!var_name:-}"
}

# Set state variable value
set_state() {
  local var_name="$1"
  local value="$2"
  if [ -z "$var_name" ]; then
    echo "❌ set_state: variable name required" >&2
    return 1
  fi

  # Use declare to set the variable globally
  declare -g "$var_name"="$value"
  return 0
}

#=============================================================================
# MODULE LOADING SYSTEM
#=============================================================================

# Load a single module with dependency checking
load_module() {
  local module_name="$1"
  local module_path="$EMPACK_ROOT/lib/modules/${module_name}.sh"

  if [ ! -f "$module_path" ]; then
    echo "❌ Fatal: Cannot find module '$module_name' at $module_path" >&2
    return 1
  fi

  # Source the module
  if ! source "$module_path"; then
    echo "❌ Fatal: Failed to load module '$module_name'" >&2
    return 1
  fi

  return 0
}

# Load all modules in dependency order
load_all_modules() {
  local failed_modules=()

  for module in "${EMPACK_MODULE_ORDER[@]}"; do
    if ! load_module "$module"; then
      failed_modules+=("$module")
    fi

    # Set log level after logger module loads
    if [ "$module" = "logger" ]; then
      if [ "$EMPACK_CORE_DEBUG" = "true" ] || [ "$EMPACK_CORE_VERBOSE" = "true" ]; then
        set_log_level debug
      elif [ "$EMPACK_CORE_QUIET" = "true" ]; then
        set_log_level warning
      else
        set_log_level info
      fi
    fi
  done

  if [ ${#failed_modules[@]} -gt 0 ]; then
    echo "❌ Fatal: Failed to load modules: ${failed_modules[*]}" >&2
    echo "❌ Check that all required modules exist in $EMPACK_ROOT/lib/modules/" >&2
    return 1
  fi

  return 0
}

#=============================================================================
# FLAG PARSING SYSTEM - CLEAN BREAK
#=============================================================================

# Program-level flags (empack owns completely)
declare -A PROGRAM_FLAGS=(
  ["--verbose"]="EMPACK_CORE_VERBOSE=true"
  ["-v"]="EMPACK_CORE_VERBOSE=true"
  ["--debug"]="EMPACK_CORE_DEBUG=true"
  ["-d"]="EMPACK_CORE_DEBUG=true"
  ["--quiet"]="EMPACK_CORE_QUIET=true"
  ["-q"]="EMPACK_CORE_QUIET=true"
  ["--dry-run"]="EMPACK_CORE_DRY_RUN=true"
  ["--non-interactive"]="EMPACK_CORE_NON_INTERACTIVE=true"
  ["-y"]="EMPACK_CORE_NON_INTERACTIVE=true"
  ["--modpack-directory"]="SET_TARGET_DIR"
  ["-m"]="SET_TARGET_DIR"
  ["--help"]="SHOW_HELP"
  ["-h"]="SHOW_HELP"
  ["--version"]="SHOW_VERSION"
  ["-V"]="SHOW_VERSION"
)

# Hybrid pass-through flags (empack validates + passes to packwiz)
declare -A HYBRID_FLAGS=(
  ["--modloader"]="VALIDATE_MODLOADER"
  ["--minecraft-version"]="VALIDATE_MC_VERSION"
  ["--mc-version"]="VALIDATE_MC_VERSION"
  ["--neoforge-version"]="VALIDATE_NEOFORGE_VERSION"
  ["--fabric-version"]="VALIDATE_FABRIC_VERSION"
  ["--quilt-version"]="VALIDATE_QUILT_VERSION"
  ["--name"]="VALIDATE_NAME"
  ["--author"]="VALIDATE_AUTHOR"
  ["--version"]="VALIDATE_VERSION"
)

# Storage for pass-through flags
declare -a PASS_THROUGH_FLAGS=()

# Validate hybrid flag values
validate_hybrid_flag() {
  local validation_type="$1"
  local value="$2"

  case "$validation_type" in
  "VALIDATE_MODLOADER")
    case "$value" in
    "neoforge" | "fabric" | "quilt" | "none") return 0 ;;
    *) return 1 ;;
    esac
    ;;
  "VALIDATE_MC_VERSION")
    if [[ $value =~ ^[0-9]+\.[0-9]+(\.[0-9]+)?$ ]]; then
      return 0
    else
      return 1
    fi
    ;;
  "VALIDATE_NEOFORGE_VERSION" | "VALIDATE_FABRIC_VERSION" | "VALIDATE_QUILT_VERSION")
    if [[ $value =~ ^[0-9]+ ]]; then
      return 0
    else
      return 1
    fi
    ;;
  "VALIDATE_NAME" | "VALIDATE_AUTHOR")
    if [ -n "$value" ] && [ ${#value} -le 100 ]; then
      return 0
    else
      return 1
    fi
    ;;
  "VALIDATE_VERSION")
    if [[ $value =~ ^[0-9]+\.[0-9]+\.[0-9]+([-.][a-zA-Z0-9]+)*$ ]]; then
      return 0
    else
      return 1
    fi
    ;;
  *)
    return 0 # Allow unknown types for future extension
    ;;
  esac
}

# Export hybrid flag values to state variables
export_hybrid_flag() {
  local flag="$1"
  local value="$2"

  case "$flag" in
  "--modloader")
    set_state "EMPACK_CORE_MODLOADER" "$value"
    ;;
  "--minecraft-version" | "--mc-version")
    set_state "EMPACK_CORE_MINECRAFT_VERSION" "$value"
    ;;
  "--neoforge-version")
    set_state "EMPACK_CORE_MODLOADER_VERSION" "$value"
    set_state "EMPACK_CORE_MODLOADER" "neoforge"
    ;;
  "--fabric-version")
    set_state "EMPACK_CORE_MODLOADER_VERSION" "$value"
    set_state "EMPACK_CORE_MODLOADER" "fabric"
    ;;
  "--quilt-version")
    set_state "EMPACK_CORE_MODLOADER_VERSION" "$value"
    set_state "EMPACK_CORE_MODLOADER" "quilt"
    ;;
  "--name")
    set_state "EMPACK_CORE_NAME" "$value"
    ;;
  "--author")
    set_state "EMPACK_CORE_AUTHOR" "$value"
    ;;
  "--version")
    set_state "EMPACK_CORE_VERSION" "$value"
    ;;
  esac
}

# Parse command line flags
parse_flags() {
  local -a remaining_args=()
  local skip_next=false

  while [ $# -gt 0 ]; do
    if [ "$skip_next" = true ]; then
      skip_next=false
      shift
      continue
    fi

    local arg="$1"
    local handled=false

    # Handle program-level flags
    if [[ -v PROGRAM_FLAGS["$arg"] ]]; then
      local action="${PROGRAM_FLAGS[$arg]}"
      case "$action" in
      "SHOW_HELP")
        show_help
        exit 0
        ;;
      "SHOW_VERSION")
        echo "$EMPACK_NAME $EMPACK_VERSION"
        exit 0
        ;;
      "SET_TARGET_DIR")
        if [ -z "${2:-}" ]; then
          echo "❌ Error: $arg requires a directory path" >&2
          exit 1
        fi
        EMPACK_CORE_TARGET_DIR="${2%/}" # Strip trailing slash
        skip_next=true
        ;;
      *)
        # Set state variables
        eval "declare -g $action"
        ;;
      esac
      handled=true
    fi

    # Handle hybrid pass-through flags
    if [[ -v HYBRID_FLAGS["$arg"] ]] && [ "$handled" = false ]; then
      local validation="${HYBRID_FLAGS[$arg]}"
      if [ -z "${2:-}" ]; then
        echo "❌ Error: $arg requires a value" >&2
        exit 1
      fi

      # Validate and store the flag
      if validate_hybrid_flag "$validation" "$2"; then
        export_hybrid_flag "$arg" "$2"
        PASS_THROUGH_FLAGS+=("$arg" "$2")
      else
        echo "❌ Error: Invalid value for $arg: $2" >&2
        exit 1
      fi

      skip_next=true
      handled=true
    fi

    # Store unhandled flags for pass-through or as commands
    if [ "$handled" = false ]; then
      remaining_args+=("$arg")
    fi

    shift
  done

  # Return remaining arguments
  printf '%s\n' "${remaining_args[@]}"
}

#=============================================================================
# HELP AND VERSION DISPLAY
#=============================================================================

show_help() {
  cat <<'EOF'
empack - Professional Minecraft Modpack Development Tool

USAGE:
    empack [OPTIONS] [COMMAND...]

OPTIONS:
    -v, --verbose              Enable verbose output
    -d, --debug                Enable debug output (implies --verbose)
    -q, --quiet                Suppress non-essential output
    --dry-run                  Preview operations without making changes
    -y, --non-interactive      Run in non-interactive mode
    -m, --modpack-directory    Set target directory for modpack operations
    -h, --help                 Show this help message
    -V, --version              Show version information

PRE-INIT COMMANDS (available before modpack setup):
    requirements               Check system dependencies
    init                       Initialize modpack development environment
    version                    Show version information
    help                       Show help information

POST-INIT COMMANDS (require valid modpack structure):
    mrpack                     Build .mrpack distribution
    client                     Build client installer
    server                     Build server installer
    client-full                Build client installer with direct mod downloads
    server-full                Build server installer with direct mod downloads
    clean                      Clean build artifacts
    all                        Build all distributions (mrpack client server)

INITIALIZATION FLAGS:
    --modloader TYPE           Set modloader (neoforge|fabric|quilt|none)
    --minecraft-version VER    Set Minecraft version
    --mc-version VER           Set Minecraft version (alias)
    --neoforge-version VER     Set NeoForge version
    --fabric-version VER       Set Fabric version
    --quilt-version VER        Set Quilt version
    --name NAME                Set modpack name
    --author AUTHOR            Set modpack author
    --version VER              Set modpack version

EXAMPLES:
    empack requirements                    # Check dependencies
    empack init                           # Interactive initialization
    empack --non-interactive --modloader fabric init
    empack --modpack-directory /tmp/test init
    empack mrpack client server          # Build distributions
    empack --verbose all                  # Build all with verbose output

For more information: https://github.com/empack/empack
EOF
}

#=============================================================================
# APPLICATION INITIALIZATION
#=============================================================================

# Initialize empack application
empack_init() {
  # Validate EMPACK_ROOT is set
  if [ -z "${EMPACK_ROOT:-}" ]; then
    echo "❌ Fatal: EMPACK_ROOT not set" >&2
    return 1
  fi

  # Ensure target directory is absolute path
  if [[ $EMPACK_CORE_TARGET_DIR != /* ]]; then
    EMPACK_CORE_TARGET_DIR="$(cd "$EMPACK_CORE_TARGET_DIR" 2>/dev/null && pwd)" || {
      echo "❌ Fatal: Cannot resolve target directory: $EMPACK_CORE_TARGET_DIR" >&2
      return 1
    }
  fi

  # Load all modules
  if ! load_all_modules; then
    return 1
  fi

  EMPACK_CORE_INITIALIZATION_COMPLETE="true"
  return 0
}

#=============================================================================
# MAIN APPLICATION ENTRY POINT
#=============================================================================

# Main application function
empack_main() {
  # Parse flags and get remaining arguments
  local temp_file=$(mktemp)
  parse_flags "$@" >"$temp_file"
  local -a args
  mapfile -t args <"$temp_file"
  rm "$temp_file"

  # Initialize application
  if ! empack_init; then
    echo "❌ Fatal: Failed to initialize empack" >&2
    exit 1
  fi

  log_debug "empack $EMPACK_VERSION initialized"
  log_debug "Target directory: $EMPACK_CORE_TARGET_DIR"
  log_debug "Remaining arguments: ${args[*]}"

  # Detect runtime phase
  detect_runtime_phase

  # If no commands provided, show help
  if [ ${#args[@]} -eq 0 ]; then
    show_help
    exit 0
  fi

  # Execute commands via command registry
  execute_commands "${args[@]}"
}

# Export core functions for module access
export -f load_module show_help empack_init empack_main
export -f clear_module_state export_module_state get_state set_state
export -f validate_hybrid_flag export_hybrid_flag
