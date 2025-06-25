#!/usr/bin/env bash
# Module: init
# Description: Modpack initialization with runtime boundary compliance
# Dependencies: core, logger, utils, boundaries, deps, dev-templates, api, validation

# Prevent multiple loading
if [ "${EMPACK_MODULE_INIT:-}" = "loaded" ]; then
    return 0
fi
readonly EMPACK_MODULE_INIT="loaded"

#=============================================================================
# INIT STATE VARIABLES
#=============================================================================

# Init state (EMPACK_INIT_* namespace)
declare -g EMPACK_INIT_MODE=""
declare -g EMPACK_INIT_CURRENT_STEP=""
declare -g EMPACK_INIT_STATUS=""
declare -g EMPACK_INIT_ERROR_MESSAGE=""
declare -g EMPACK_INIT_START_TIME=""
declare -g EMPACK_INIT_DURATION=""
declare -g EMPACK_INIT_RESOLVED_MODLOADER=""
declare -g EMPACK_INIT_RESOLVED_MINECRAFT_VERSION=""
declare -g EMPACK_INIT_RESOLVED_MODLOADER_VERSION=""
declare -g EMPACK_INIT_PERSONALIZATION_COMPLETE="false"
declare -g EMPACK_INIT_RUNTIME_BOUNDARY_VALIDATED="false"

# Clear init state
clear_init_state() {
    EMPACK_INIT_MODE=""
    EMPACK_INIT_CURRENT_STEP=""
    EMPACK_INIT_STATUS=""
    EMPACK_INIT_ERROR_MESSAGE=""
    EMPACK_INIT_START_TIME=""
    EMPACK_INIT_DURATION=""
    EMPACK_INIT_RESOLVED_MODLOADER=""
    EMPACK_INIT_RESOLVED_MINECRAFT_VERSION=""
    EMPACK_INIT_RESOLVED_MODLOADER_VERSION=""
    EMPACK_INIT_PERSONALIZATION_COMPLETE="false"
    EMPACK_INIT_RUNTIME_BOUNDARY_VALIDATED="false"
    log_debug "Init state cleared"
}

#=============================================================================
# INITIALIZATION SYSTEM
#=============================================================================
# 
# This module handles modpack initialization ensuring strict pre-init
# phase compliance and proper transition to post-init phase.
#

# Main init command implementation with three-mode architecture
init_command() {
    EMPACK_INIT_START_TIME=$(date +%s)
    EMPACK_INIT_CURRENT_STEP="initialization"
    log_info "Initializing empack modpack development environment..."
    
    # Ensure we're in pre-init phase with state tracking
    if ! require_pre_init "initialization"; then
        EMPACK_INIT_STATUS="error"
        EMPACK_INIT_ERROR_MESSAGE="Runtime boundary violation - must be in pre-init phase"
        log_error "$EMPACK_INIT_ERROR_MESSAGE"
        return 1
    fi
    EMPACK_INIT_RUNTIME_BOUNDARY_VALIDATED="true"
    
    # Check if safe for initialization
    if ! is_safe_for_initialization; then
        log_error "Directory not safe for initialization"
        log_error "Target directory must be empty or contain only basic development files"
        return 1
    fi
    
    # Quick dependency check
    if ! quick_dependency_check; then
        log_warning "Some dependencies are missing"
        log_warning "Run 'empack requirements' for setup guidance"
        log_warning "Continuing with available tools..."
    fi
    
    # Three-mode initialization logic with state tracking
    if [ "$EMPACK_CORE_NON_INTERACTIVE" = "true" ]; then
        # Non-interactive mode: check for zero-config vs explicit config
        if is_zero_config_mode; then
            # Golden Path: Zero-config bootstrap
            EMPACK_INIT_MODE="zero_config"
            EMPACK_INIT_CURRENT_STEP="zero_config_execution"
            init_zero_config
        else
            # Expert mode: Explicit configuration with validation
            EMPACK_INIT_MODE="explicit_config"
            EMPACK_INIT_CURRENT_STEP="explicit_config_execution"
            init_explicit_config
        fi
    else
        # Interactive mode: Guided experience with binary choices
        EMPACK_INIT_MODE="interactive"
        EMPACK_INIT_CURRENT_STEP="interactive_execution"
        init_interactive
    fi
}

#=============================================================================
# THREE INITIALIZATION MODES
#=============================================================================

# Check if user provided explicit modloader configuration flags
is_zero_config_mode() {
    # Zero-config if no explicit modloader flags provided
    [ -z "${EMPACK_CORE_MODLOADER:-}" ] && 
    [ -z "${EMPACK_CORE_MINECRAFT_VERSION:-}" ] && 
    [ -z "${EMPACK_CORE_MODLOADER_VERSION:-}" ]
}

# Mode 1: Zero-config bootstrap (Golden Path - 80% use case)
init_zero_config() {
    EMPACK_INIT_CURRENT_STEP="stabilizing_core_input"
    log_debug "Entering zero-config initialization mode"
    
    # Use compatibility.sh for core input stabilization
    if ! stabilize_core_input; then
        EMPACK_INIT_STATUS="error"
        EMPACK_INIT_ERROR_MESSAGE="Failed to resolve intelligent defaults"
        log_error "$EMPACK_INIT_ERROR_MESSAGE"
        return 1
    fi
    
    # Apply the validated compatibility state to environment
    if ! apply_compatibility_state; then
        EMPACK_INIT_STATUS="error"
        EMPACK_INIT_ERROR_MESSAGE="Failed to apply compatibility state"
        log_error "$EMPACK_INIT_ERROR_MESSAGE"
        return 1
    fi
    
    # Store resolved values in init state
    EMPACK_INIT_RESOLVED_MODLOADER="$EMPACK_CORE_MODLOADER"
    EMPACK_INIT_RESOLVED_MINECRAFT_VERSION="$EMPACK_CORE_MINECRAFT_VERSION"
    EMPACK_INIT_RESOLVED_MODLOADER_VERSION="$EMPACK_CORE_MODLOADER_VERSION"
    
    # Get default personalization values
    EMPACK_INIT_CURRENT_STEP="personalization"
    local personalization
    personalization=$(get_default_personalization) || {
        EMPACK_INIT_STATUS="error"
        EMPACK_INIT_ERROR_MESSAGE="Failed to get default personalization"
        log_error "$EMPACK_INIT_ERROR_MESSAGE"
        return 1
    }
    
    # Parse personalization defaults
    local default_name default_version default_author
    eval "$personalization"  # Sets name=, version=, author= variables
    EMPACK_INIT_PERSONALIZATION_COMPLETE="true"
    
    # Execute initialization with resolved values
    EMPACK_INIT_CURRENT_STEP="execution"
    if ! execute_initialization \
        "$EMPACK_CORE_MODLOADER" \
        "$EMPACK_CORE_MINECRAFT_VERSION" \
        "$EMPACK_CORE_MODLOADER_VERSION" \
        "$name" \
        "$version" \
        "$author"; then
        return 1
    fi
    
    local end_time=$(date +%s)
    EMPACK_INIT_DURATION=$((end_time - EMPACK_INIT_START_TIME))
    EMPACK_INIT_STATUS="complete"
    
    log_success "Complete! Ready to build in ${EMPACK_INIT_DURATION} seconds"
    show_next_steps
    return 0
}

# Mode 2: Interactive guided experience (15% use case)
init_interactive() {
    log_debug "Entering interactive initialization mode"
    
    log_info "🚀 Welcome to empack - Professional Modpack Development"
    log_info ""
    
    # Step 1: Modloader selection
    local modloader
    if ! modloader=$(prompt_modloader_selection); then
        log_error "Modloader selection cancelled"
        return 1
    fi
    
    # Step 2: Version selection (binary choice)
    local modloader_version
    if ! modloader_version=$(prompt_version_selection "$modloader"); then
        log_error "Version selection cancelled"
        return 1
    fi
    
    # Step 3: Determine minecraft version
    local minecraft_version
    minecraft_version=$(get_recommended_minecraft_for_modloader "$modloader") || {
        log_error "Failed to determine Minecraft version"
        return 1
    }
    
    # Step 4: Personalization (optional)
    local name version author
    if ! get_personalization_interactive name version author; then
        log_warning "Using defaults for personalization"
        local defaults
        defaults=$(get_default_personalization)
        eval "$defaults"
    fi
    
    # Execute initialization
    if ! execute_initialization "$modloader" "$minecraft_version" "$modloader_version" "$name" "$version" "$author"; then
        return 1
    fi
    
    show_next_steps
    return 0
}

# Mode 3: Expert explicit configuration (5% use case)
init_explicit_config() {
    log_debug "Entering explicit configuration mode"
    
    # Validate that all required arguments are provided
    if ! validate_cli_init_args \
        "$EMPACK_CORE_MODLOADER" \
        "$EMPACK_CORE_MINECRAFT_VERSION" \
        "$EMPACK_CORE_MODLOADER_VERSION" \
        "${EMPACK_CORE_NAME:-}" \
        "${EMPACK_CORE_VERSION:-}" \
        "${EMPACK_CORE_AUTHOR:-}"; then
        return 1
    fi
    
    # Get defaults for missing personalization
    local name="${EMPACK_CORE_NAME:-}"
    local version="${EMPACK_CORE_VERSION:-}"
    local author="${EMPACK_CORE_AUTHOR:-}"
    
    if [ -z "$name" ] || [ -z "$version" ] || [ -z "$author" ]; then
        local defaults
        defaults=$(get_default_personalization)
        eval "$defaults"
        
        # Use provided values or fall back to defaults
        name="${EMPACK_CORE_NAME:-$name}"
        version="${EMPACK_CORE_VERSION:-$version}"
        author="${EMPACK_CORE_AUTHOR:-$author}"
    fi
    
    # Execute initialization with explicit configuration
    if ! execute_initialization \
        "$EMPACK_CORE_MODLOADER" \
        "$EMPACK_CORE_MINECRAFT_VERSION" \
        "$EMPACK_CORE_MODLOADER_VERSION" \
        "$name" \
        "$version" \
        "$author"; then
        return 1
    fi
    
    show_next_steps
    return 0
}

#=============================================================================
# INTERACTIVE PROMPTS (For Mode 2)
#=============================================================================

# Prompt for modloader selection
prompt_modloader_selection() {
    echo "Choose your modloader:"
    echo "  1. NeoForge (recommended for most modpacks)"
    echo "  2. Fabric (performance and client mods)"
    echo "  3. Quilt (experimental features)"
    echo "  4. Vanilla (pure Minecraft)"
    echo ""
    
    local selection
    read -p "Selection [1]: " selection
    selection=${selection:-1}
    
    case "$selection" in
        1) echo "neoforge" ;;
        2) echo "fabric" ;;
        3) echo "quilt" ;;
        4) echo "none" ;;
        *) 
            log_error "Invalid selection: $selection"
            return 1
            ;;
    esac
}

# Prompt for version selection (binary choice: stable vs latest)
prompt_version_selection() {
    local modloader="$1"
    
    log_success "Selected $modloader"
    log_info "🔍 Finding version options for $modloader..."
    
    local versions
    versions=$(get_binary_choice_versions "$modloader") || {
        log_error "Failed to get version options"
        return 1
    }
    
    local -a version_array
    read -ra version_array <<< "$versions"
    
    if [ ${#version_array[@]} -eq 1 ]; then
        # Only one version available
        echo "Using ${version_array[0]} (only version available)"
        echo "${version_array[0]}"
    else
        # Binary choice: stable vs latest
        echo "$modloader version options:"
        echo "  1. ${version_array[0]} (stable) ← recommended"
        echo "  2. ${version_array[1]} (latest)"
        echo ""
        
        local selection
        read -p "Selection [1]: " selection
        selection=${selection:-1}
        
        case "$selection" in
            1) echo "${version_array[0]}" ;;
            2) echo "${version_array[1]}" ;;
            *)
                log_error "Invalid selection: $selection"
                return 1
                ;;
        esac
    fi
}

# Get personalization interactively (TODO: implement)
get_personalization_interactive() {
    # For now, use defaults - full implementation in future phase
    return 1
}

#=============================================================================
# CORE INITIALIZATION EXECUTION
#=============================================================================

# Execute the actual initialization process
execute_initialization() {
    local modloader="$1"
    local minecraft_version="$2"
    local modloader_version="$3"
    local name="$4"
    local version="$5"
    local author="$6"
    
    log_info "📦 Creating development environment..."
    
    # Create pack directory
    local pack_dir="$EMPACK_CORE_TARGET_DIR/pack"
    if ! mkdir -p "$pack_dir"; then
        EMPACK_INIT_STATUS="error"
        EMPACK_INIT_ERROR_MESSAGE="Failed to create pack directory: $pack_dir"
        log_error "$EMPACK_INIT_ERROR_MESSAGE"
        return 1
    fi
    
    log_debug "Created pack directory: $pack_dir"
    
    # Process development templates (pre-init phase)
    EMPACK_INIT_CURRENT_STEP="processing_dev_templates"
    log_info "📋 Processing development templates..."
    if ! process_dev_templates; then
        EMPACK_INIT_STATUS="error"
        EMPACK_INIT_ERROR_MESSAGE="Failed to process development templates"
        log_error "$EMPACK_INIT_ERROR_MESSAGE"
        return 1
    fi
    
    # Initialize packwiz modpack
    EMPACK_INIT_CURRENT_STEP="packwiz_initialization"
    log_info "📦 Initializing packwiz modpack..."
    
    # Build packwiz init command with proper arguments
    local packwiz_args=()
    packwiz_args+=("--modloader" "$modloader")
    packwiz_args+=("--mc-version" "$minecraft_version")
    
    # Add modloader version for non-none modloaders
    if [ "$modloader" != "none" ]; then
        case "$modloader" in
            "neoforge")
                packwiz_args+=("--neoforge-version" "$modloader_version")
                ;;
            "fabric")
                packwiz_args+=("--fabric-version" "$modloader_version")
                ;;
            "quilt")
                packwiz_args+=("--quilt-version" "$modloader_version")
                ;;
        esac
    fi
    
    # Add personalization arguments
    packwiz_args+=("--name" "$name")
    packwiz_args+=("--author" "$author")
    packwiz_args+=("--version" "$version")
    
    # Execute packwiz init in pack directory
    log_debug "Running packwiz init with args: ${packwiz_args[*]}"
    if ! (cd "$pack_dir" && packwiz init "${packwiz_args[@]}"); then
        EMPACK_INIT_STATUS="error"
        EMPACK_INIT_ERROR_MESSAGE="packwiz init failed"
        log_error "$EMPACK_INIT_ERROR_MESSAGE"
        log_error "Check that packwiz is installed and the parameters are valid"
        return 1
    fi
    
    # Validate that pack.toml was created successfully
    EMPACK_INIT_CURRENT_STEP="validation"
    if [ ! -f "$pack_dir/pack.toml" ]; then
        EMPACK_INIT_STATUS="error"
        EMPACK_INIT_ERROR_MESSAGE="packwiz init succeeded but pack.toml not found"
        log_error "$EMPACK_INIT_ERROR_MESSAGE"
        return 1
    fi
    
    log_debug "packwiz initialization completed successfully"
    
    # Transition to post-init phase
    EMPACK_INIT_CURRENT_STEP="runtime_boundary_transition"
    log_debug "Transitioning to post-init phase..."
    if ! transition_to_post_init; then
        EMPACK_INIT_STATUS="error"
        EMPACK_INIT_ERROR_MESSAGE="Failed to transition to post-init phase"
        log_error "$EMPACK_INIT_ERROR_MESSAGE"
        return 1
    fi
    
    EMPACK_INIT_STATUS="complete"
    log_success "Initialization complete!"
    
    return 0
}

# Show next steps to user
show_next_steps() {
    log_success "Use 'cd pack && packwiz mr install <mod>' to add mods"
    log_success "Use 'empack mrpack/client/server' to build distributions"
}

#=============================================================================
# HELPER FUNCTIONS (Missing implementations)
#=============================================================================

# Check if directory is safe for initialization
is_safe_for_initialization() {
    local target_dir="$EMPACK_CORE_TARGET_DIR"
    
    log_debug "Checking if directory is safe for initialization: $target_dir"
    
    # Check if directory is empty or contains only basic development files
    if [ ! -d "$target_dir" ]; then
        log_debug "Target directory does not exist, safe to initialize"
        return 0
    fi
    
    # Count non-hidden files and directories
    local file_count
    file_count=$(find "$target_dir" -maxdepth 1 -not -path "$target_dir" -not -name ".*" | wc -l)
    
    if [ "$file_count" -eq 0 ]; then
        log_debug "Directory is empty, safe to initialize"
        return 0
    fi
    
    # Allow certain safe files/directories
    local safe_patterns=(
        "README.md"
        "LICENSE"
        ".git"
        ".gitignore"
        ".actrc"
        "docs"
        "scripts"
    )
    
    # Check if all files match safe patterns
    local unsafe_files=0
    while IFS= read -r -d '' file; do
        local basename_file
        basename_file=$(basename "$file")
        local is_safe=false
        
        for pattern in "${safe_patterns[@]}"; do
            if [[ $basename_file == $pattern ]]; then
                is_safe=true
                break
            fi
        done
        
        if [ "$is_safe" = false ]; then
            log_debug "Found potentially unsafe file: $basename_file"
            unsafe_files=$((unsafe_files + 1))
        fi
    done < <(find "$target_dir" -maxdepth 1 -not -path "$target_dir" -not -name ".*" -print0)
    
    if [ $unsafe_files -eq 0 ]; then
        log_debug "All files are safe, directory is safe for initialization"
        return 0
    else
        log_debug "Found $unsafe_files potentially unsafe files"
        return 1
    fi
}

# Quick dependency check for essential tools
quick_dependency_check() {
    log_debug "Performing quick dependency check for initialization"
    
    local missing_deps=()
    
    # Check for packwiz (essential for initialization)
    if ! command -v packwiz >/dev/null 2>&1; then
        missing_deps+=("packwiz")
    fi
    
    # Check for basic tools
    if ! command -v git >/dev/null 2>&1; then
        missing_deps+=("git")
    fi
    
    if [ ${#missing_deps[@]} -eq 0 ]; then
        log_debug "Quick dependency check passed"
        return 0
    else
        log_debug "Missing dependencies: ${missing_deps[*]}"
        return 1
    fi
}

# Validate CLI init arguments for explicit config mode
validate_cli_init_args() {
    local modloader="$1"
    local minecraft_version="$2"
    local modloader_version="$3"
    local name="$4"
    local version="$5"
    local author="$6"
    
    log_debug "Validating CLI init arguments for explicit config mode"
    
    # Use validation.sh for core argument validation
    if ! validate_core_arguments "$modloader" "$minecraft_version" "$modloader_version"; then
        log_error "Core argument validation failed"
        return 1
    fi
    
    # Use validation.sh for personalization validation
    if ! validate_personalization "$name" "$author" "$version"; then
        log_error "Personalization validation failed"
        return 1
    fi
    
    log_debug "CLI init arguments validation passed"
    return 0
}

# Get binary choice versions for interactive mode (placeholder)
get_binary_choice_versions() {
    local modloader="$1"
    
    log_debug "Getting binary choice versions for $modloader"
    
    case "$modloader" in
        "neoforge")
            # Return stable and latest versions
            if get_stable_neoforge_version && get_latest_neoforge_version; then
                echo "$EMPACK_API_NEOFORGE_STABLE_VERSION $EMPACK_API_NEOFORGE_LATEST_VERSION"
            else
                log_error "Failed to get NeoForge versions"
                return 1
            fi
            ;;
        "fabric")
            if get_stable_fabric_version && get_latest_fabric_version; then
                echo "$EMPACK_API_FABRIC_STABLE_VERSION $EMPACK_API_FABRIC_LATEST_VERSION"
            else
                log_error "Failed to get Fabric versions"
                return 1
            fi
            ;;
        "quilt")
            if get_stable_quilt_version && get_latest_quilt_version; then
                echo "$EMPACK_API_QUILT_STABLE_VERSION $EMPACK_API_QUILT_LATEST_VERSION"
            else
                log_error "Failed to get Quilt versions"
                return 1
            fi
            ;;
        "none")
            # For vanilla, just return the latest minecraft version
            if get_latest_minecraft_version; then
                echo "$EMPACK_API_MINECRAFT_LATEST_VERSION"
            else
                log_error "Failed to get Minecraft version"
                return 1
            fi
            ;;
        *)
            log_error "Unknown modloader for version selection: $modloader"
            return 1
            ;;
    esac
}

# Get recommended minecraft version for a modloader (for interactive mode)
get_recommended_minecraft_for_modloader() {
    local modloader="$1"
    
    log_debug "Getting recommended Minecraft version for $modloader"
    
    case "$modloader" in
        "neoforge")
            # For NeoForge, get the latest stable and derive compatible MC version
            if get_stable_neoforge_version; then
                get_minecraft_version_for_neoforge_version "$EMPACK_API_NEOFORGE_STABLE_VERSION"
            else
                log_error "Failed to get stable NeoForge version"
                return 1
            fi
            ;;
        "fabric"|"quilt"|"none")
            # For Fabric, Quilt, and Vanilla, use latest Minecraft
            if get_latest_minecraft_version; then
                echo "$EMPACK_API_MINECRAFT_LATEST_VERSION"
            else
                log_error "Failed to get latest Minecraft version"
                return 1
            fi
            ;;
        *)
            log_error "Unknown modloader for Minecraft version recommendation: $modloader"
            return 1
            ;;
    esac
}

#=============================================================================
# MODULE INTERFACE CONTRACT
#=============================================================================

# Standard module interface - export init state variables
export_init_state() {
    echo "EMPACK_INIT_MODE='$EMPACK_INIT_MODE'"
    echo "EMPACK_INIT_CURRENT_STEP='$EMPACK_INIT_CURRENT_STEP'"
    echo "EMPACK_INIT_STATUS='$EMPACK_INIT_STATUS'"
    echo "EMPACK_INIT_ERROR_MESSAGE='$EMPACK_INIT_ERROR_MESSAGE'"
    echo "EMPACK_INIT_START_TIME='$EMPACK_INIT_START_TIME'"
    echo "EMPACK_INIT_DURATION='$EMPACK_INIT_DURATION'"
    echo "EMPACK_INIT_RESOLVED_MODLOADER='$EMPACK_INIT_RESOLVED_MODLOADER'"
    echo "EMPACK_INIT_RESOLVED_MINECRAFT_VERSION='$EMPACK_INIT_RESOLVED_MINECRAFT_VERSION'"
    echo "EMPACK_INIT_RESOLVED_MODLOADER_VERSION='$EMPACK_INIT_RESOLVED_MODLOADER_VERSION'"
    echo "EMPACK_INIT_PERSONALIZATION_COMPLETE='$EMPACK_INIT_PERSONALIZATION_COMPLETE'"
    echo "EMPACK_INIT_RUNTIME_BOUNDARY_VALIDATED='$EMPACK_INIT_RUNTIME_BOUNDARY_VALIDATED'"
}

# Get current module status
get_init_status() {
    local status="operational"
    local details=""
    
    if [ "$EMPACK_INIT_STATUS" = "error" ]; then
        status="error"
        details="$EMPACK_INIT_ERROR_MESSAGE"
    elif [ "$EMPACK_INIT_STATUS" = "complete" ]; then
        status="complete"
        details="Initialized in ${EMPACK_INIT_DURATION}s using $EMPACK_INIT_MODE mode"
    elif [ -n "$EMPACK_INIT_CURRENT_STEP" ]; then
        status="active"
        details="Step: $EMPACK_INIT_CURRENT_STEP (mode: $EMPACK_INIT_MODE)"
    fi
    
    echo "status=$status"
    echo "mode=$EMPACK_INIT_MODE"
    echo "current_step=$EMPACK_INIT_CURRENT_STEP"
    echo "init_status=$EMPACK_INIT_STATUS"
    echo "resolved_modloader=$EMPACK_INIT_RESOLVED_MODLOADER"
    echo "resolved_minecraft_version=$EMPACK_INIT_RESOLVED_MINECRAFT_VERSION"
    echo "resolved_modloader_version=$EMPACK_INIT_RESOLVED_MODLOADER_VERSION"
    echo "personalization_complete=$EMPACK_INIT_PERSONALIZATION_COMPLETE"
    echo "runtime_boundary_validated=$EMPACK_INIT_RUNTIME_BOUNDARY_VALIDATED"
    echo "details=$details"
}

# Validate init module state and configuration
validate_init_state() {
    local validation_passed=true
    local errors=()
    
    # Check if required dependency modules are available
    if ! declare -F require_pre_init >/dev/null 2>&1; then
        errors+=("Function require_pre_init not available from boundaries module")
        validation_passed=false
    fi
    
    if ! declare -F stabilize_core_input >/dev/null 2>&1; then
        errors+=("Function stabilize_core_input not available from compatibility module")
        validation_passed=false
    fi
    
    if ! declare -F apply_compatibility_state >/dev/null 2>&1; then
        errors+=("Function apply_compatibility_state not available from compatibility module")
        validation_passed=false
    fi
    
    if ! declare -F get_default_personalization >/dev/null 2>&1; then
        errors+=("Function get_default_personalization not available from validation module")
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
    
    if ! declare -F log_info >/dev/null 2>&1; then
        errors+=("Function log_info not available from logger module")
        validation_passed=false
    fi
    
    if ! declare -F log_success >/dev/null 2>&1; then
        errors+=("Function log_success not available from logger module")
        validation_passed=false
    fi
    
    # Check for safety and dependency check functions
    if ! declare -F is_safe_for_initialization >/dev/null 2>&1; then
        errors+=("Function is_safe_for_initialization not available - should be defined in boundaries or deps module")
        validation_passed=false
    fi
    
    if ! declare -F quick_dependency_check >/dev/null 2>&1; then
        errors+=("Function quick_dependency_check not available - should be defined in deps module")
        validation_passed=false
    fi
    
    echo "validation_passed=$validation_passed"
    if [ ${#errors[@]} -gt 0 ]; then
        echo "errors=${errors[*]}"
    fi
    
    return $([ "$validation_passed" = true ] && echo 0 || echo 1)
}

# Export init functions
export -f init_command
export -f is_zero_config_mode
export -f init_zero_config
export -f init_interactive
export -f init_explicit_config
export -f prompt_modloader_selection
export -f prompt_version_selection
export -f get_personalization_interactive
export -f execute_initialization
export -f show_next_steps
export -f clear_init_state
# Helper functions
export -f is_safe_for_initialization
export -f quick_dependency_check
export -f validate_cli_init_args
export -f get_binary_choice_versions
export -f get_recommended_minecraft_for_modloader
# Module interface contract
export -f export_init_state get_init_status validate_init_state