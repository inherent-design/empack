#!/usr/bin/env bash
# Module: boundaries
# Description: Runtime boundary validation and phase management using deps state
# Dependencies: core, logger, utils, deps

# Prevent multiple loading
if [ "${EMPACK_MODULE_BOUNDARIES:-}" = "loaded" ]; then
    return 0
fi
readonly EMPACK_MODULE_BOUNDARIES="loaded"

#=============================================================================
# BOUNDARY STATE VARIABLES
#=============================================================================

# Boundary state (EMPACK_BOUNDARY_* namespace)
declare -g EMPACK_BOUNDARY_CURRENT_PHASE=""
declare -g EMPACK_BOUNDARY_PACK_TOML_VALID="false"
declare -g EMPACK_BOUNDARY_TRANSITION_REASON=""
declare -g EMPACK_BOUNDARY_LAST_VALIDATION_RESULT=""
declare -g EMPACK_BOUNDARY_DETECTION_METHOD=""
declare -g EMPACK_BOUNDARY_PHASE_DETECTED="false"

# Clear boundary state
clear_boundary_state() {
    EMPACK_BOUNDARY_CURRENT_PHASE=""
    EMPACK_BOUNDARY_PACK_TOML_VALID="false"
    EMPACK_BOUNDARY_TRANSITION_REASON=""
    EMPACK_BOUNDARY_LAST_VALIDATION_RESULT=""
    EMPACK_BOUNDARY_DETECTION_METHOD=""
    EMPACK_BOUNDARY_PHASE_DETECTED="false"
    log_debug "Boundary state cleared"
}

#=============================================================================
# RUNTIME PHASE DETECTION
#=============================================================================

# Check if we're in pre-init phase (before valid empack + pack/pack.toml structure)
is_pre_init() {
    # Check if pack.toml exists and is valid
    local pack_toml="$EMPACK_CORE_TARGET_DIR/pack/pack.toml"
    
    if [ ! -f "$pack_toml" ]; then
        EMPACK_BOUNDARY_DETECTION_METHOD="NO_PACK_TOML"
        EMPACK_BOUNDARY_PACK_TOML_VALID="false"
        return 0  # No pack.toml = pre-init
    fi
    
    # Use deps module state to check for TOML parsers
    if ! tomlq_available; then
        log_debug "No TOML parser available, assuming valid pack.toml"
        EMPACK_BOUNDARY_DETECTION_METHOD="NO_PARSER_ASSUME_VALID"
        EMPACK_BOUNDARY_PACK_TOML_VALID="assumed"
        return 1  # Assume post-init if we can't validate
    fi
    
    # Use enhanced dependency resolution from utils to find TOML parser
    local toml_cmd
    if toml_cmd=$(find_dependency tq); then
        # Validate pack.toml structure
        if ! "$toml_cmd" -f "$pack_toml" -r 'name' >/dev/null 2>&1; then
            log_debug "pack.toml exists but invalid structure (tq validation)"
            EMPACK_BOUNDARY_DETECTION_METHOD="TQ_VALIDATION_FAILED"
            EMPACK_BOUNDARY_PACK_TOML_VALID="false"
            return 0  # Invalid = pre-init
        fi
        EMPACK_BOUNDARY_DETECTION_METHOD="TQ_VALIDATION_SUCCESS"
        EMPACK_BOUNDARY_PACK_TOML_VALID="true"
    elif toml_cmd=$(find_dependency tomlq); then
        # Validate pack.toml structure  
        if ! "$toml_cmd" -r '.name' "$pack_toml" >/dev/null 2>&1; then
            log_debug "pack.toml exists but invalid structure (tomlq validation)"
            EMPACK_BOUNDARY_DETECTION_METHOD="TOMLQ_VALIDATION_FAILED"
            EMPACK_BOUNDARY_PACK_TOML_VALID="false"
            return 0  # Invalid = pre-init
        fi
        EMPACK_BOUNDARY_DETECTION_METHOD="TOMLQ_VALIDATION_SUCCESS"
        EMPACK_BOUNDARY_PACK_TOML_VALID="true"
    fi
    
    return 1  # Valid pack.toml = post-init
}

# Check if we're in post-init phase (after valid empack + pack/pack.toml structure)
is_post_init() {
    ! is_pre_init
}

# Detect and cache runtime phase
detect_runtime_phase() {
    if [ "$EMPACK_BOUNDARY_PHASE_DETECTED" = "true" ]; then
        return 0  # Already detected
    fi
    
    if is_pre_init; then
        EMPACK_BOUNDARY_CURRENT_PHASE="pre-init"
        log_debug "Runtime phase: PRE-INIT (no valid pack.toml structure)"
    else
        EMPACK_BOUNDARY_CURRENT_PHASE="post-init"
        log_debug "Runtime phase: POST-INIT (valid pack.toml structure exists)"
    fi
    
    EMPACK_BOUNDARY_PHASE_DETECTED="true"
    return 0
}

#=============================================================================
# RUNTIME BOUNDARY ENFORCEMENT
#=============================================================================

# Require pre-init phase (fail if in post-init)
require_pre_init() {
    local operation="${1:-operation}"
    
    detect_runtime_phase
    
    if [ "$EMPACK_BOUNDARY_CURRENT_PHASE" != "pre-init" ]; then
        log_error "$operation requires pre-initialization phase"
        log_error "This operation is only available before modpack structure is established"
        EMPACK_BOUNDARY_LAST_VALIDATION_RESULT="FAILED_POST_INIT_BLOCKED"
        return 1
    fi
    
    EMPACK_BOUNDARY_LAST_VALIDATION_RESULT="PASSED_PRE_INIT"
    return 0
}

# Require post-init phase (fail if in pre-init)
require_post_init() {
    local operation="${1:-operation}"
    
    detect_runtime_phase
    
    if [ "$EMPACK_BOUNDARY_CURRENT_PHASE" = "pre-init" ]; then
        log_error "$operation requires valid modpack structure"
        log_error "Run 'empack init' first to initialize the modpack environment"
        EMPACK_BOUNDARY_LAST_VALIDATION_RESULT="FAILED_PRE_INIT_BLOCKED"
        return 1
    fi
    
    EMPACK_BOUNDARY_LAST_VALIDATION_RESULT="PASSED_POST_INIT"
    return 0
}

# Transition from pre-init to post-init (called after successful initialization)
transition_to_post_init() {
    local reason="${1:-initialization_complete}"
    
    log_debug "Transitioning from pre-init to post-init phase"
    EMPACK_BOUNDARY_CURRENT_PHASE="post-init"
    EMPACK_BOUNDARY_PHASE_DETECTED="true"
    EMPACK_BOUNDARY_TRANSITION_REASON="$reason"
    
    # Verify transition is valid
    if is_pre_init; then
        log_warning "Phase transition attempted but pack.toml validation failed"
        EMPACK_BOUNDARY_CURRENT_PHASE="pre-init"
        EMPACK_BOUNDARY_TRANSITION_REASON="transition_failed_validation"
        return 1
    fi
    
    log_success "Runtime boundary transition complete: PRE-INIT → POST-INIT"
    return 0
}

#=============================================================================
# BOUNDARY VALIDATION HELPERS
#=============================================================================

# Validate that pack.toml exists and has required structure
validate_pack_toml() {
    local pack_toml="$EMPACK_CORE_TARGET_DIR/pack/pack.toml"
    
    if [ ! -f "$pack_toml" ]; then
        log_error "pack.toml not found at $pack_toml"
        EMPACK_BOUNDARY_LAST_VALIDATION_RESULT="FAILED_NO_FILE"
        return 1
    fi
    
    # Use deps module to check for TOML parser availability
    if ! tomlq_available; then
        log_warning "No TOML parser available, skipping validation"
        EMPACK_BOUNDARY_LAST_VALIDATION_RESULT="SKIPPED_NO_PARSER"
        return 0
    fi
    
    # Find TOML parser using utils enhanced dependency resolution
    local toml_cmd
    if toml_cmd=$(find_dependency tq); then
        log_debug "Using tq for pack.toml validation"
    elif toml_cmd=$(find_dependency tomlq); then
        log_debug "Using tomlq for pack.toml validation"
    else
        log_warning "TOML parser not found despite availability check"
        EMPACK_BOUNDARY_LAST_VALIDATION_RESULT="SKIPPED_PARSER_NOT_FOUND"
        return 0
    fi
    
    # Validate required fields
    local required_fields=("name" "author" "version")
    local missing_fields=()
    
    for field in "${required_fields[@]}"; do
        if ! "$toml_cmd" -f "$pack_toml" -r "$field" >/dev/null 2>&1; then
            missing_fields+=("$field")
        fi
    done
    
    if [ ${#missing_fields[@]} -gt 0 ]; then
        log_error "pack.toml missing required fields: ${missing_fields[*]}"
        EMPACK_BOUNDARY_LAST_VALIDATION_RESULT="FAILED_MISSING_FIELDS"
        return 1
    fi
    
    log_debug "pack.toml validation successful"
    EMPACK_BOUNDARY_LAST_VALIDATION_RESULT="PASSED_VALIDATION"
    return 0
}

# Check if directory structure is safe for initialization
is_safe_for_initialization() {
    # Ensure target directory exists
    if [ ! -d "$EMPACK_CORE_TARGET_DIR" ]; then
        log_debug "Target directory does not exist, safe for initialization"
        return 0
    fi
    
    # Check if already looks like a modpack
    if [ -f "$EMPACK_CORE_TARGET_DIR/pack/pack.toml" ]; then
        log_debug "Existing modpack detected, safe for re-initialization"
        return 0
    fi
    
    # Check for common safe files
    local safe_files=(".gitignore" "README.md" "LICENSE" ".actrc" ".git")
    local file_count=$(find "$EMPACK_CORE_TARGET_DIR" -maxdepth 1 -type f | wc -l)
    local dir_count=$(find "$EMPACK_CORE_TARGET_DIR" -maxdepth 1 -type d ! -name "$(basename "$EMPACK_CORE_TARGET_DIR")" | wc -l)
    
    # Empty directory is safe
    if [ "$file_count" -eq 0 ] && [ "$dir_count" -eq 0 ]; then
        log_debug "Empty directory, safe for initialization"
        return 0
    fi
    
    # Check if all files are in safe list
    local has_unsafe_files=false
    while IFS= read -r -d '' file; do
        local basename_file=$(basename "$file")
        local is_safe=false
        
        for safe_file in "${safe_files[@]}"; do
            if [ "$basename_file" = "$safe_file" ]; then
                is_safe=true
                break
            fi
        done
        
        if [ "$is_safe" = false ]; then
            has_unsafe_files=true
            break
        fi
    done < <(find "$EMPACK_CORE_TARGET_DIR" -maxdepth 1 -type f -print0)
    
    if [ "$has_unsafe_files" = true ]; then
        log_debug "Directory contains potentially conflicting files"
        return 1
    fi
    
    log_debug "Directory appears safe for initialization"
    return 0
}

#=============================================================================
# PHASE-AWARE COMMAND HELPERS
#=============================================================================

# Execute command with phase requirement checking
execute_with_phase_check() {
    local phase_requirement="$1"
    local command_name="$2"
    shift 2
    
    case "$phase_requirement" in
        "pre-init")
            if ! require_pre_init "$command_name"; then
                return 1
            fi
            ;;
        "post-init")
            if ! require_post_init "$command_name"; then
                return 1
            fi
            ;;
        "any")
            # No phase requirement
            ;;
        *)
            log_error "Invalid phase requirement: $phase_requirement"
            return 1
            ;;
    esac
    
    # Execute the command
    "$@"
}

# Get current phase for display purposes
get_current_phase() {
    detect_runtime_phase
    echo "$EMPACK_BOUNDARY_CURRENT_PHASE"
}

# Check if we're ready for a specific phase transition
ready_for_transition() {
    local target_phase="$1"
    
    case "$target_phase" in
        "post-init")
            # Ready if we can validate pack.toml and have required deps
            if validate_pack_toml && tomlq_available; then
                return 0
            else
                return 1
            fi
            ;;
        "pre-init")
            # Can always transition back to pre-init
            return 0
            ;;
        *)
            log_error "Invalid target phase: $target_phase"
            return 1
            ;;
    esac
}

# Export boundary functions
export -f is_pre_init is_post_init detect_runtime_phase
export -f require_pre_init require_post_init transition_to_post_init
export -f validate_pack_toml is_safe_for_initialization
export -f execute_with_phase_check get_current_phase ready_for_transition
export -f clear_boundary_state