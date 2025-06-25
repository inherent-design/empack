#!/usr/bin/env bash
# Module: dev-templates
# Description: Pre-init static template management (no pack.toml dependency)
# Dependencies: core, logger, utils, boundaries

# Prevent multiple loading
if [ "${EMPACK_MODULE_DEV_TEMPLATES:-}" = "loaded" ]; then
    return 0
fi
readonly EMPACK_MODULE_DEV_TEMPLATES="loaded"

#=============================================================================
# DEV-TEMPLATES STATE VARIABLES
#=============================================================================

# Dev-templates state (EMPACK_DEV_TEMPLATES_* namespace)
declare -g EMPACK_DEV_TEMPLATES_LAST_OPERATION=""
declare -g EMPACK_DEV_TEMPLATES_PROCESSING_STATUS=""
declare -g EMPACK_DEV_TEMPLATES_ERROR_MESSAGE=""
declare -g EMPACK_DEV_TEMPLATES_FILES_PROCESSED="0"
declare -g EMPACK_DEV_TEMPLATES_LAST_TEMPLATE_PATH=""
declare -g EMPACK_DEV_TEMPLATES_RUNTIME_BOUNDARY_VALIDATED="false"

# Clear dev-templates state
clear_dev_templates_state() {
    EMPACK_DEV_TEMPLATES_LAST_OPERATION=""
    EMPACK_DEV_TEMPLATES_PROCESSING_STATUS=""
    EMPACK_DEV_TEMPLATES_ERROR_MESSAGE=""
    EMPACK_DEV_TEMPLATES_FILES_PROCESSED="0"
    EMPACK_DEV_TEMPLATES_LAST_TEMPLATE_PATH=""
    EMPACK_DEV_TEMPLATES_RUNTIME_BOUNDARY_VALIDATED="false"
    log_debug "Dev-templates state cleared"
}

#=============================================================================
# PRE-INIT TEMPLATE SYSTEM
#=============================================================================
# 
# This module handles static development templates that do NOT require
# pack.toml variables. These are used during initialization and setup.
# 
# Runtime Boundary: PRE-INIT ONLY
# - Templates processed here must not depend on pack.toml content
# - Simple text substitution or direct copy operations only
#

# Pre-init template processing with state tracking
process_dev_templates() {
    EMPACK_DEV_TEMPLATES_LAST_OPERATION="processing"
    log_debug "Processing development templates (pre-init phase)"
    
    # Ensure we're in pre-init phase with state tracking
    if ! require_pre_init "dev-templates processing"; then
        EMPACK_DEV_TEMPLATES_PROCESSING_STATUS="error"
        EMPACK_DEV_TEMPLATES_ERROR_MESSAGE="Runtime boundary violation - must be in pre-init phase"
        log_error "$EMPACK_DEV_TEMPLATES_ERROR_MESSAGE"
        return 1
    fi
    EMPACK_DEV_TEMPLATES_RUNTIME_BOUNDARY_VALIDATED="true"
    
    # Process static development templates
    local templates_dir="$EMPACK_ROOT/templates"
    local target_dir="$EMPACK_CORE_TARGET_DIR"
    local files_processed=0
    
    # Process .gitignore template
    if [ -f "$templates_dir/gitignore.template" ]; then
        log_debug "Processing .gitignore template"
        EMPACK_DEV_TEMPLATES_LAST_TEMPLATE_PATH="$templates_dir/gitignore.template"
        if cp "$templates_dir/gitignore.template" "$target_dir/.gitignore"; then
            files_processed=$((files_processed + 1))
            log_debug "Created .gitignore"
        else
            EMPACK_DEV_TEMPLATES_PROCESSING_STATUS="error"
            EMPACK_DEV_TEMPLATES_ERROR_MESSAGE="Failed to create .gitignore"
            log_error "$EMPACK_DEV_TEMPLATES_ERROR_MESSAGE"
            return 1
        fi
    fi
    
    # Process .actrc template for ACT (GitHub Actions local testing)
    if [ -f "$templates_dir/actrc.template" ]; then
        log_debug "Processing .actrc template"
        EMPACK_DEV_TEMPLATES_LAST_TEMPLATE_PATH="$templates_dir/actrc.template"
        if cp "$templates_dir/actrc.template" "$target_dir/.actrc"; then
            files_processed=$((files_processed + 1))
            log_debug "Created .actrc"
        else
            EMPACK_DEV_TEMPLATES_PROCESSING_STATUS="error"
            EMPACK_DEV_TEMPLATES_ERROR_MESSAGE="Failed to create .actrc"
            log_error "$EMPACK_DEV_TEMPLATES_ERROR_MESSAGE"
            return 1
        fi
    fi
    
    # Process GitHub workflow templates
    if [ -d "$templates_dir/github" ]; then
        log_debug "Processing GitHub workflow templates"
        
        # Create .github/workflows directory
        if ! mkdir -p "$target_dir/.github/workflows"; then
            EMPACK_DEV_TEMPLATES_PROCESSING_STATUS="error"
            EMPACK_DEV_TEMPLATES_ERROR_MESSAGE="Failed to create .github/workflows directory"
            log_error "$EMPACK_DEV_TEMPLATES_ERROR_MESSAGE"
            return 1
        fi
        
        # Copy workflow templates
        for template_file in "$templates_dir/github"/*.yml.template; do
            if [ -f "$template_file" ]; then
                local base_name
                base_name=$(basename "$template_file" .template)
                EMPACK_DEV_TEMPLATES_LAST_TEMPLATE_PATH="$template_file"
                
                if cp "$template_file" "$target_dir/.github/workflows/$base_name"; then
                    files_processed=$((files_processed + 1))
                    log_debug "Created .github/workflows/$base_name"
                else
                    EMPACK_DEV_TEMPLATES_PROCESSING_STATUS="error"
                    EMPACK_DEV_TEMPLATES_ERROR_MESSAGE="Failed to create .github/workflows/$base_name"
                    log_error "$EMPACK_DEV_TEMPLATES_ERROR_MESSAGE"
                    return 1
                fi
            fi
        done
    fi
    
    EMPACK_DEV_TEMPLATES_FILES_PROCESSED="$files_processed"
    
    EMPACK_DEV_TEMPLATES_PROCESSING_STATUS="complete"
    log_debug "Development template processing complete (${EMPACK_DEV_TEMPLATES_FILES_PROCESSED} files processed)"
    return 0
}

#=============================================================================
# MODULE INTERFACE CONTRACT
#=============================================================================

# Standard module interface - export dev-templates state variables
export_dev_templates_state() {
    echo "EMPACK_DEV_TEMPLATES_LAST_OPERATION='$EMPACK_DEV_TEMPLATES_LAST_OPERATION'"
    echo "EMPACK_DEV_TEMPLATES_PROCESSING_STATUS='$EMPACK_DEV_TEMPLATES_PROCESSING_STATUS'"
    echo "EMPACK_DEV_TEMPLATES_ERROR_MESSAGE='$EMPACK_DEV_TEMPLATES_ERROR_MESSAGE'"
    echo "EMPACK_DEV_TEMPLATES_FILES_PROCESSED='$EMPACK_DEV_TEMPLATES_FILES_PROCESSED'"
    echo "EMPACK_DEV_TEMPLATES_LAST_TEMPLATE_PATH='$EMPACK_DEV_TEMPLATES_LAST_TEMPLATE_PATH'"
    echo "EMPACK_DEV_TEMPLATES_RUNTIME_BOUNDARY_VALIDATED='$EMPACK_DEV_TEMPLATES_RUNTIME_BOUNDARY_VALIDATED'"
}

# Get current module status
get_dev_templates_status() {
    local status="operational"
    local details=""
    
    if [ "$EMPACK_DEV_TEMPLATES_PROCESSING_STATUS" = "error" ]; then
        status="error"
        details="$EMPACK_DEV_TEMPLATES_ERROR_MESSAGE"
    elif [ "$EMPACK_DEV_TEMPLATES_PROCESSING_STATUS" = "complete" ]; then
        status="complete"
        details="Processed $EMPACK_DEV_TEMPLATES_FILES_PROCESSED template files"
    elif [ -n "$EMPACK_DEV_TEMPLATES_LAST_OPERATION" ]; then
        status="active"
        details="Operation: $EMPACK_DEV_TEMPLATES_LAST_OPERATION"
    fi
    
    echo "status=$status"
    echo "processing_status=$EMPACK_DEV_TEMPLATES_PROCESSING_STATUS"
    echo "files_processed=$EMPACK_DEV_TEMPLATES_FILES_PROCESSED"
    echo "last_template_path=$EMPACK_DEV_TEMPLATES_LAST_TEMPLATE_PATH"
    echo "runtime_boundary_validated=$EMPACK_DEV_TEMPLATES_RUNTIME_BOUNDARY_VALIDATED"
    echo "details=$details"
}

# Validate dev-templates module state and configuration
validate_dev_templates_state() {
    local validation_passed=true
    local errors=()
    
    # Check if boundary functions are available
    if ! declare -F require_pre_init >/dev/null 2>&1; then
        errors+=("Function require_pre_init not available from boundaries module")
        validation_passed=false
    fi
    
    # Check logger functions are available
    if ! declare -F log_debug >/dev/null 2>&1; then
        errors+=("Function log_debug not available from logger module")
        validation_passed=false
    fi
    
    if ! declare -F log_error >/dev/null 2>&1; then
        errors+=("Function log_error not available from logger module")
        validation_passed=false
    fi
    
    # Check if we can access template directory (when EMPACK_ROOT is available)
    if [ -n "${EMPACK_ROOT:-}" ] && [ -d "$EMPACK_ROOT/templates" ]; then
        log_debug "Template directory available at $EMPACK_ROOT/templates"
    else
        log_debug "Template directory not yet available - normal during early initialization"
    fi
    
    echo "validation_passed=$validation_passed"
    if [ ${#errors[@]} -gt 0 ]; then
        echo "errors=${errors[*]}"
    fi
    
    return $([ "$validation_passed" = true ] && echo 0 || echo 1)
}

# Export template functions
export -f process_dev_templates clear_dev_templates_state
# Module interface contract
export -f export_dev_templates_state get_dev_templates_status validate_dev_templates_state