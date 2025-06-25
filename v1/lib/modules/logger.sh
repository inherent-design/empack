#!/usr/bin/env bash
# Module: logger
# Description: Dual-format logging with color support and terminal detection
# Dependencies: core

# Prevent multiple loading
if [ "${EMPACK_MODULE_LOGGER:-}" = "loaded" ]; then
    return 0
fi
readonly EMPACK_MODULE_LOGGER="loaded"

#=============================================================================
# LOGGING CONFIGURATION
#=============================================================================

# Log levels
readonly LOG_LEVEL_DEBUG=0
readonly LOG_LEVEL_INFO=1
readonly LOG_LEVEL_SUCCESS=2
readonly LOG_LEVEL_WARNING=3
readonly LOG_LEVEL_ERROR=4

# Current log level (default: INFO)
export EMPACK_LOG_LEVEL=${EMPACK_LOG_LEVEL:-$LOG_LEVEL_INFO}

# Color detection and configuration
should_use_colors() {
    # Check if output is a TTY, TERM is set properly, and NO_COLOR is not set
    [ -t 1 ] && [ "${TERM:-}" != "dumb" ] && [ -z "${NO_COLOR:-}" ]
}

# Initialize color support
if should_use_colors; then
    export EMPACK_USE_COLORS=true
    readonly COLOR_RESET='\033[0m'
    readonly COLOR_DEBUG='\033[2;37m'    # dim white
    readonly COLOR_INFO='\033[1;34m'     # bright blue
    readonly COLOR_SUCCESS='\033[1;32m'  # bright green
    readonly COLOR_WARNING='\033[1;33m'  # bright yellow
    readonly COLOR_ERROR='\033[1;31m'    # bright red
else
    export EMPACK_USE_COLORS=false
    readonly COLOR_RESET=''
    readonly COLOR_DEBUG=''
    readonly COLOR_INFO=''
    readonly COLOR_SUCCESS=''
    readonly COLOR_WARNING=''
    readonly COLOR_ERROR=''
fi

#=============================================================================
# FORMATTING FUNCTIONS
#=============================================================================

# Generate ISO 8601 timestamp
get_timestamp() {
    date '+%Y-%m-%dT%H:%M:%S'
}

# Get component name from calling context
get_component() {
    # Try to extract module name from BASH_SOURCE (caller of log_* function)
    local source_file="${BASH_SOURCE[4]:-}"
    if [[ $source_file =~ /([^/]+)\.sh$ ]]; then
        echo "${BASH_REMATCH[1]}"
    else
        echo "empack"
    fi
}

# Simple format: [LEVEL] message (with selective coloring)
format_simple() {
    local level_name="$1"
    local color="$2"
    shift 2
    
    local message="$*"
    
    if [ "$EMPACK_USE_COLORS" = "true" ]; then
        echo "[$level_name] ${color}${message}${COLOR_RESET}"
    else
        echo "[$level_name] $message"
    fi
}

# Structured format: timestamp [LEVEL] component=name message="text" key=value (with selective coloring)
format_structured() {
    local level_name="$1"
    local color="$2"
    shift 2
    
    local timestamp
    timestamp=$(get_timestamp)
    
    local message="$1"
    shift
    
    # Parse additional structured data
    local structured_data=""
    local component=""
    
    # Look for component= parameter or auto-detect
    while [ $# -gt 0 ]; do
        case "$1" in
            component=*)
                component="${1#component=}"
                ;;
            *=*)
                structured_data="$structured_data $1"
                ;;
            *)
                # If it's not key=value, treat as additional message text
                message="$message $1"
                ;;
        esac
        shift
    done
    
    # Auto-detect component if not provided
    if [ -z "$component" ]; then
        component=$(get_component)
    fi
    
    # Build structured log line with selective coloring
    if [ "$EMPACK_USE_COLORS" = "true" ]; then
        echo "${timestamp} [${level_name}] component=${component} message=\"${color}${message}${COLOR_RESET}\"${structured_data}"
    else
        echo "${timestamp} [${level_name}] component=${component} message=\"${message}\"${structured_data}"
    fi
}

#=============================================================================
# CORE LOGGING FUNCTIONS
#=============================================================================

# Check if we should log at a given level
should_log() {
    local level="$1"
    [ "$level" -ge "$EMPACK_LOG_LEVEL" ]
}

# Core logging function with dual-format support
log_message() {
    local level="$1"
    local level_name="$2"
    local color="$3"
    shift 3
    
    if ! should_log "$level"; then
        return 0
    fi
    
    # Choose format based on verbose mode (formats handle their own coloring)
    local formatted_message
    if [ "${EMPACK_CORE_VERBOSE:-false}" = "true" ]; then
        formatted_message=$(format_structured "$level_name" "$color" "$@")
    else
        formatted_message=$(format_simple "$level_name" "$color" "$@")
    fi
    
    # Route to appropriate output
    if [ "$level" -ge "$LOG_LEVEL_ERROR" ]; then
        echo -e "$formatted_message" >&2
    else
        echo -e "$formatted_message"
    fi
}

#=============================================================================
# PUBLIC LOGGING INTERFACE
#=============================================================================

# Core logging functions
log_debug() {
    log_message "$LOG_LEVEL_DEBUG" "DEBUG" "$COLOR_DEBUG" "$@"
}

log_info() {
    log_message "$LOG_LEVEL_INFO" "INFO" "$COLOR_INFO" "$@"
}

log_success() {
    log_message "$LOG_LEVEL_SUCCESS" "SUCCESS" "$COLOR_SUCCESS" "$@"
}

log_warning() {
    log_message "$LOG_LEVEL_WARNING" "WARNING" "$COLOR_WARNING" "$@"
}

log_error() {
    log_message "$LOG_LEVEL_ERROR" "ERROR" "$COLOR_ERROR" "$@"
}

# Utility logging functions (semantic aliases)
log_progress() {
    log_info "$@"
}

log_download() {
    log_info "$@"
}

log_build() {
    log_info "$@"
}

log_step() {
    log_info "$@"
}

# Set log level
set_log_level() {
    case "$1" in
        debug) export EMPACK_LOG_LEVEL=$LOG_LEVEL_DEBUG ;;
        info) export EMPACK_LOG_LEVEL=$LOG_LEVEL_INFO ;;
        warning) export EMPACK_LOG_LEVEL=$LOG_LEVEL_WARNING ;;
        error) export EMPACK_LOG_LEVEL=$LOG_LEVEL_ERROR ;;
        *) 
            log_error "Invalid log level: $1"
            return 1
            ;;
    esac
}

#=============================================================================
# EXPORTS
#=============================================================================

# Export logging functions
export -f log_debug log_info log_success log_warning log_error
export -f log_progress log_download log_build log_step set_log_level
export -f should_use_colors get_timestamp get_component