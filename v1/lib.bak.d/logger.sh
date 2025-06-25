#!/usr/bin/env bash
# Module: logger
# Description: Logging and output management with consistent formatting
# Dependencies: core

# Prevent multiple loading
if [ "${EMPACK_MODULE_LOGGER:-}" = "loaded" ]; then
    return 0
fi
readonly EMPACK_MODULE_LOGGER="loaded"

#=============================================================================
# LOGGING FUNCTIONS
#=============================================================================

# Log levels and formatting
readonly LOG_LEVEL_DEBUG=0
readonly LOG_LEVEL_INFO=1
readonly LOG_LEVEL_SUCCESS=2
readonly LOG_LEVEL_WARNING=3
readonly LOG_LEVEL_ERROR=4

# Current log level (default: INFO)
EMPACK_LOG_LEVEL=${EMPACK_LOG_LEVEL:-$LOG_LEVEL_INFO}

# Check if we should log at a given level
should_log() {
    local level="$1"
    [ "$level" -ge "$EMPACK_LOG_LEVEL" ]
}

# Core logging function
log_message() {
    local level="$1"
    local prefix="$2"
    shift 2
    
    if should_log "$level"; then
        if [ "$level" -ge "$LOG_LEVEL_ERROR" ]; then
            echo "$prefix $*" >&2
        else
            echo "$prefix $*"
        fi
    fi
}

# Public logging functions with emoji indicators
log_debug() {
    log_message "$LOG_LEVEL_DEBUG" "🔍" "$@"
}

log_info() {
    log_message "$LOG_LEVEL_INFO" "💡" "$@"
}

log_success() {
    log_message "$LOG_LEVEL_SUCCESS" "✅" "$@"
}

log_warning() {
    log_message "$LOG_LEVEL_WARNING" "⚠️ " "$@"
}

log_error() {
    log_message "$LOG_LEVEL_ERROR" "❌" "$@"
}

# Utility function for progress indication
log_progress() {
    log_message "$LOG_LEVEL_INFO" "🔄" "$@"
}

# Utility function for download indication
log_download() {
    log_message "$LOG_LEVEL_INFO" "📥" "$@"
}

# Utility function for build indication
log_build() {
    log_message "$LOG_LEVEL_INFO" "🔨" "$@"
}

# Utility function for step completion
log_step() {
    log_message "$LOG_LEVEL_INFO" "📋" "$@"
}

# Set log level
set_log_level() {
    case "$1" in
        debug) EMPACK_LOG_LEVEL=$LOG_LEVEL_DEBUG ;;
        info) EMPACK_LOG_LEVEL=$LOG_LEVEL_INFO ;;
        warning) EMPACK_LOG_LEVEL=$LOG_LEVEL_WARNING ;;
        error) EMPACK_LOG_LEVEL=$LOG_LEVEL_ERROR ;;
        *) 
            log_error "Invalid log level: $1"
            return 1
            ;;
    esac
}

# Export logging functions
export -f log_debug log_info log_success log_warning log_error
export -f log_progress log_download log_build log_step set_log_level