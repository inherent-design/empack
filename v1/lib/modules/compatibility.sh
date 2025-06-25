#!/usr/bin/env bash
# Module: compatibility
# Description: 3D compatibility matrix analysis and auto-fill architecture
# Dependencies: core, logger, utils, deps, api, validation

# Prevent multiple loading
if [ "${EMPACK_MODULE_COMPATIBILITY:-}" = "loaded" ]; then
  return 0
fi
readonly EMPACK_MODULE_COMPATIBILITY="loaded"

#=============================================================================
# COMPATIBILITY STATE VARIABLES
#=============================================================================

# Compatibility state (EMPACK_COMPATIBILITY_* namespace)
declare -g EMPACK_COMPATIBILITY_LAST_VALIDATION_TYPE=""
declare -g EMPACK_COMPATIBILITY_MATRIX_STATUS=""
declare -g EMPACK_COMPATIBILITY_ERROR_SOURCE=""
declare -g EMPACK_COMPATIBILITY_ERROR_MESSAGE=""
declare -g EMPACK_COMPATIBILITY_VALIDATED_MODLOADER=""
declare -g EMPACK_COMPATIBILITY_VALIDATED_MINECRAFT_VERSION=""
declare -g EMPACK_COMPATIBILITY_VALIDATED_MODLOADER_VERSION=""
declare -g EMPACK_COMPATIBILITY_RECOMMENDED_MODLOADER=""
declare -g EMPACK_COMPATIBILITY_RECOMMENDED_MINECRAFT_VERSION=""
declare -g EMPACK_COMPATIBILITY_RECOMMENDED_MODLOADER_VERSION=""
declare -g EMPACK_COMPATIBILITY_AUTO_FILL_USED="false"
declare -g EMPACK_COMPATIBILITY_INPUT_STABILIZATION_COMPLETE="false"

# Clear compatibility state
clear_compatibility_state() {
    EMPACK_COMPATIBILITY_LAST_VALIDATION_TYPE=""
    EMPACK_COMPATIBILITY_MATRIX_STATUS=""
    EMPACK_COMPATIBILITY_ERROR_SOURCE=""
    EMPACK_COMPATIBILITY_ERROR_MESSAGE=""
    EMPACK_COMPATIBILITY_VALIDATED_MODLOADER=""
    EMPACK_COMPATIBILITY_VALIDATED_MINECRAFT_VERSION=""
    EMPACK_COMPATIBILITY_VALIDATED_MODLOADER_VERSION=""
    EMPACK_COMPATIBILITY_RECOMMENDED_MODLOADER=""
    EMPACK_COMPATIBILITY_RECOMMENDED_MINECRAFT_VERSION=""
    EMPACK_COMPATIBILITY_RECOMMENDED_MODLOADER_VERSION=""
    EMPACK_COMPATIBILITY_AUTO_FILL_USED="false"
    EMPACK_COMPATIBILITY_INPUT_STABILIZATION_COMPLETE="false"
    log_debug "Compatibility state cleared"
}

#=============================================================================
# 3D COMPATIBILITY MATRIX ANALYSIS
#=============================================================================

# Core Input Stabilization: Validate that the three core pieces work together
# This is the keystone function that enables the auto-fill architecture
validate_compatibility_matrix() {
  local modloader="$1"
  local minecraft_version="$2"
  local modloader_version="$3"
  local source="${4:-user-provided}"

  EMPACK_COMPATIBILITY_LAST_VALIDATION_TYPE="full_matrix"
  EMPACK_COMPATIBILITY_ERROR_SOURCE="$source"
  
  log_debug "3D compatibility analysis: $modloader $minecraft_version $modloader_version (source: $source)"

  # Step 1: Use validation.sh for basic format and existence checks
  if ! validate_core_arguments "$modloader" "$minecraft_version" "$modloader_version"; then
    EMPACK_COMPATIBILITY_MATRIX_STATUS="error_basic_validation"
    EMPACK_COMPATIBILITY_ERROR_MESSAGE="Basic validation failed - invalid format or non-existent versions"
    log_error "$EMPACK_COMPATIBILITY_ERROR_MESSAGE"
    return 1
  fi

  # Step 2: Advanced compatibility matrix analysis
  case "$modloader" in
    "none")
      # Vanilla is always compatible with any valid Minecraft version
      EMPACK_COMPATIBILITY_MATRIX_STATUS="valid_vanilla"
      log_debug "Vanilla modloader - compatibility confirmed"
      return 0
      ;;
    "neoforge")
      validate_neoforge_compatibility_matrix "$minecraft_version" "$modloader_version" "$source"
      ;;
    "fabric")
      validate_fabric_compatibility_matrix "$minecraft_version" "$modloader_version" "$source"
      ;;
    "quilt")
      validate_quilt_compatibility_matrix "$minecraft_version" "$modloader_version" "$source"
      ;;
    *)
      EMPACK_COMPATIBILITY_MATRIX_STATUS="error_unknown_modloader"
      EMPACK_COMPATIBILITY_ERROR_MESSAGE="Unknown modloader for compatibility analysis: $modloader"
      log_error "$EMPACK_COMPATIBILITY_ERROR_MESSAGE"
      return 1
      ;;
  esac
}

# NeoForge compatibility matrix analysis with state tracking
validate_neoforge_compatibility_matrix() {
  local minecraft_version="$1"
  local neoforge_version="$2"
  local source="$3"

  EMPACK_COMPATIBILITY_LAST_VALIDATION_TYPE="neoforge_matrix"
  log_debug "Analyzing NeoForge compatibility: MC $minecraft_version, NeoForge $neoforge_version"

  # API-driven compatibility check (preferred)
  local compatible_mc_versions
  if compatible_mc_versions=$(get_minecraft_versions_for_neoforge "$neoforge_version"); then
    if echo "$compatible_mc_versions" | grep -q "^$minecraft_version$"; then
      EMPACK_COMPATIBILITY_MATRIX_STATUS="valid_neoforge_api"
      log_debug "API confirms: NeoForge $neoforge_version supports Minecraft $minecraft_version"
      return 0
    else
      EMPACK_COMPATIBILITY_MATRIX_STATUS="error_neoforge_incompatible"
      handle_compatibility_error "$source" "NeoForge $neoforge_version" "Minecraft $minecraft_version" \
        "NeoForge $neoforge_version does not support Minecraft $minecraft_version" \
        "$(echo "$compatible_mc_versions" | head -3 | tr '\n' ' ')"
      return 1
    fi
  fi

  EMPACK_COMPATIBILITY_MATRIX_STATUS="warning_api_unavailable"
  log_error "API unavailable, using heuristic compatibility analysis"
  return -1
}

# Fabric compatibility matrix analysis
validate_fabric_compatibility_matrix() {
  local minecraft_version="$1"
  local fabric_version="$2"
  local source="$3"

  log_debug "Analyzing Fabric compatibility: MC $minecraft_version, Fabric $fabric_version"

  # API-driven compatibility check (preferred)
  local compatible_mc_versions
  if compatible_mc_versions=$(get_minecraft_versions_for_fabric "$fabric_version"); then
    if echo "$compatible_mc_versions" | grep -q "^$minecraft_version$"; then
      log_debug "API confirms: Fabric $fabric_version supports Minecraft $minecraft_version"
      return 0
    else
      handle_compatibility_error "$source" "Fabric $fabric_version" "Minecraft $minecraft_version" \
        "Fabric $fabric_version does not support Minecraft $minecraft_version" \
        "$(echo "$compatible_mc_versions" | head -3 | tr '\n' ' ')"
      return 1
    fi
  fi

  log_error "API unavailable, assuming Fabric compatibility (Fabric generally supports most MC versions)"
  return -1
}

# Quilt compatibility matrix analysis
validate_quilt_compatibility_matrix() {
  local minecraft_version="$1"
  local quilt_version="$2"
  local source="$3"

  log_debug "Analyzing Quilt compatibility: MC $minecraft_version, Quilt $quilt_version"

  # API-driven compatibility check (preferred)
  local compatible_mc_versions
  if compatible_mc_versions=$(get_minecraft_versions_for_quilt "$quilt_version"); then
    if echo "$compatible_mc_versions" | grep -q "^$minecraft_version$"; then
      log_debug "API confirms: Quilt $quilt_version supports Minecraft $minecraft_version"
      return 0
    else
      handle_compatibility_error "$source" "Quilt $quilt_version" "Minecraft $minecraft_version" \
        "Quilt $quilt_version does not support Minecraft $minecraft_version" \
        "$(echo "$compatible_mc_versions" | head -3 | tr '\n' ' ')"
      return 1
    fi
  fi

  log_error "API unavailable, assuming Quilt compatibility (Quilt generally supports most MC versions)"
  return -1
}

# Handle compatibility errors with detailed messaging
handle_compatibility_error() {
  local source="$1"
  local modloader_desc="$2"
  local minecraft_desc="$3"
  local reason="$4"
  local suggestions="$5"

  if [ "$source" = "system-generated" ]; then
    log_error "INTERNAL ERROR: System generated invalid combination: $modloader_desc + $minecraft_desc"
    log_error "This should never happen - please report this bug"
    log_error "Include this information: empack version $(cat "$EMPACK_ROOT/VERSION" 2>/dev/null || echo "unknown")"
  else
    log_error "Compatibility matrix validation failed: $modloader_desc + $minecraft_desc"
    log_error "Reason: $reason"
    if [ -n "$suggestions" ]; then
      log_info "Compatible versions: $suggestions"
    fi
    log_info "Use 'empack versions' to see available options"
    log_warning "NOTE: 'empack versions' not yet implemented"
  fi
}

#=============================================================================
# AUTO-FILL ARCHITECTURE: INTELLIGENT DEFAULTS
#=============================================================================

# Get recommended defaults for zero-config golden path
# This is the key function that enables "empack init -y" to work instantly
get_recommended_defaults() {
  local preferred_modloader="${1:-neoforge}" # Default to NeoForge if not specified

  log_info "🔍 Resolving intelligent defaults for modloader: $preferred_modloader"

  case "$preferred_modloader" in
    "neoforge")
      get_neoforge_recommended_defaults
      ;;
    "fabric")
      get_fabric_recommended_defaults
      ;;
    "quilt")
      get_quilt_recommended_defaults
      ;;
    "none")
      get_vanilla_recommended_defaults
      ;;
    *)
      log_error "Unknown modloader for defaults: $preferred_modloader"
      return 1
      ;;
  esac
}

# Get NeoForge recommended defaults (latest stable approach) with state management
get_neoforge_recommended_defaults() {
  log_debug "Getting NeoForge recommended defaults"

  # Get latest stable NeoForge version (state-based)
  if ! get_stable_neoforge_version; then
    log_error "Failed to get latest stable NeoForge version"
    return 1
  fi
  local neoforge_version="$EMPACK_API_NEOFORGE_STABLE_VERSION"

  # Get the latest Minecraft version supported by this NeoForge (heuristic for now)
  local minecraft_version
  case "${neoforge_version%%.*}" in
    "21")
      minecraft_version="1.21.1"
      ;;
    "20")
      minecraft_version="1.20.1"
      ;;
    *)
      if ! get_latest_minecraft_version; then
        log_error "Failed to get latest Minecraft version"
        return 1
      fi
      minecraft_version="$EMPACK_API_MINECRAFT_LATEST_VERSION"
      ;;
  esac

  # Store in compatibility state variables
  EMPACK_COMPATIBILITY_RECOMMENDED_MODLOADER="neoforge"
  EMPACK_COMPATIBILITY_RECOMMENDED_MODLOADER_VERSION="$neoforge_version"
  EMPACK_COMPATIBILITY_RECOMMENDED_MINECRAFT_VERSION="$minecraft_version"

  log_debug "Recommended defaults: NeoForge $neoforge_version + Minecraft $minecraft_version"
  return 0
}

# Get Fabric recommended defaults
get_fabric_recommended_defaults() {
  log_debug "Getting Fabric recommended defaults"

  # Get latest stable Fabric version (state-based)
  if ! get_stable_fabric_version; then
    log_error "Failed to get latest stable Fabric version"
    return 1
  fi
  local fabric_version="$EMPACK_API_FABRIC_STABLE_VERSION"

  # Get latest Minecraft version compatible with Fabric
  if ! get_latest_minecraft_version; then
    log_error "Failed to get latest Minecraft version for Fabric"
    return 1
  fi
  local minecraft_version="$EMPACK_API_MINECRAFT_LATEST_VERSION"

  # Store in compatibility state variables
  EMPACK_COMPATIBILITY_RECOMMENDED_MODLOADER="fabric"
  EMPACK_COMPATIBILITY_RECOMMENDED_MODLOADER_VERSION="$fabric_version"
  EMPACK_COMPATIBILITY_RECOMMENDED_MINECRAFT_VERSION="$minecraft_version"

  log_debug "Recommended defaults: Fabric $fabric_version + Minecraft $minecraft_version"
  return 0
}

# Get Quilt recommended defaults
get_quilt_recommended_defaults() {
  log_debug "Getting Quilt recommended defaults"

  # Get latest stable Quilt version (state-based)
  if ! get_stable_quilt_version; then
    log_error "Failed to get latest stable Quilt version"
    return 1
  fi
  local quilt_version="$EMPACK_API_QUILT_STABLE_VERSION"

  # Get latest Minecraft version compatible with Quilt
  if ! get_latest_minecraft_version; then
    log_error "Failed to get latest Minecraft version for Quilt"
    return 1
  fi
  local minecraft_version="$EMPACK_API_MINECRAFT_LATEST_VERSION"

  # Store in compatibility state variables
  EMPACK_COMPATIBILITY_RECOMMENDED_MODLOADER="quilt"
  EMPACK_COMPATIBILITY_RECOMMENDED_MODLOADER_VERSION="$quilt_version"
  EMPACK_COMPATIBILITY_RECOMMENDED_MINECRAFT_VERSION="$minecraft_version"

  log_debug "Recommended defaults: Quilt $quilt_version + Minecraft $minecraft_version"
  return 0
}

# Get vanilla (no modloader) recommended defaults
get_vanilla_recommended_defaults() {
  log_debug "Getting vanilla recommended defaults"

  # Get latest stable Minecraft version (state-based)
  if ! get_latest_minecraft_version; then
    log_error "Failed to get latest stable Minecraft version"
    return 1
  fi
  local minecraft_version="$EMPACK_API_MINECRAFT_LATEST_VERSION"

  # Store in compatibility state variables
  EMPACK_COMPATIBILITY_RECOMMENDED_MODLOADER="none"
  EMPACK_COMPATIBILITY_RECOMMENDED_MODLOADER_VERSION=""
  EMPACK_COMPATIBILITY_RECOMMENDED_MINECRAFT_VERSION="$minecraft_version"

  log_debug "Recommended defaults: Vanilla Minecraft $minecraft_version"
  return 0
}

#=============================================================================
# COMPATIBILITY STATE MANAGEMENT
#=============================================================================

# Store validated compatibility matrix in state variables
set_compatibility_state() {
  local modloader="$1"
  local minecraft_version="$2"
  local modloader_version="$3"

  EMPACK_COMPATIBILITY_VALIDATED_MODLOADER="$modloader"
  EMPACK_COMPATIBILITY_VALIDATED_MINECRAFT_VERSION="$minecraft_version"
  EMPACK_COMPATIBILITY_VALIDATED_MODLOADER_VERSION="$modloader_version"
  EMPACK_COMPATIBILITY_INPUT_STABILIZATION_COMPLETE="true"

  log_debug "Compatibility state set: $modloader $minecraft_version $modloader_version"
}

# Check if we have a validated compatibility matrix
is_compatibility_validated() {
  [ "$EMPACK_COMPATIBILITY_INPUT_STABILIZATION_COMPLETE" = "true" ]
}

# Get the current validated compatibility matrix
get_compatibility_state() {
  if ! is_compatibility_validated; then
    log_error "No validated compatibility matrix available"
    return 1
  fi

  echo "MODLOADER=$EMPACK_COMPATIBILITY_VALIDATED_MODLOADER"
  echo "MINECRAFT_VERSION=$EMPACK_COMPATIBILITY_VALIDATED_MINECRAFT_VERSION"
  echo "MODLOADER_VERSION=$EMPACK_COMPATIBILITY_VALIDATED_MODLOADER_VERSION"
}

#=============================================================================
# AUTO-FILL WORKFLOW INTEGRATION
#=============================================================================

# Core input stabilization workflow
# This function takes user inputs (which may be partial) and returns a stable,
# validated configuration suitable for pack creation
stabilize_core_input() {
  local provided_modloader="${EMPACK_CORE_MODLOADER:-}"
  local provided_minecraft="${EMPACK_CORE_MINECRAFT_VERSION:-}"
  local provided_modloader_version="${EMPACK_CORE_MODLOADER_VERSION:-}"

  log_info "🔍 Stabilizing core input configuration..."
  log_debug "Provided: modloader='$provided_modloader' minecraft='$provided_minecraft' modloader_version='$provided_modloader_version'"

  # Clear any previous compatibility state
  clear_compatibility_state

  # If we have all three pieces, validate them as a complete matrix
  if [ -n "$provided_modloader" ] && [ -n "$provided_minecraft" ] && [ -n "$provided_modloader_version" ]; then
    log_debug "Complete configuration provided, validating compatibility matrix"
    if validate_compatibility_matrix "$provided_modloader" "$provided_minecraft" "$provided_modloader_version" "user-provided"; then
      set_compatibility_state "$provided_modloader" "$provided_minecraft" "$provided_modloader_version"
      log_success "✅ Configuration validated: $provided_modloader $provided_modloader_version + Minecraft $provided_minecraft"
      return 0
    else
      log_error "Provided configuration is not compatible"
      return 1
    fi
  fi

  # Auto-fill missing pieces using intelligent defaults
  log_info "📦 Auto-filling missing configuration pieces..."

  # If no modloader provided, use default recommendations
  if [ -z "$provided_modloader" ]; then
    log_debug "No modloader specified, using recommended defaults"
    if get_recommended_defaults; then
      # Use the recommended values
      set_compatibility_state "$EMPACK_COMPATIBILITY_RECOMMENDED_MODLOADER" "$EMPACK_COMPATIBILITY_RECOMMENDED_MINECRAFT_VERSION" "$EMPACK_COMPATIBILITY_RECOMMENDED_MODLOADER_VERSION"
      log_success "✅ Auto-filled complete configuration using intelligent defaults"
      return 0
    else
      log_error "Failed to get recommended defaults"
      return 1
    fi
  fi

  # If modloader provided but missing version info, auto-fill compatible versions
  log_debug "Modloader specified ($provided_modloader), auto-filling compatible versions"
  if get_recommended_defaults "$provided_modloader"; then
    # Start with recommended defaults for this modloader
    local final_modloader="$EMPACK_COMPATIBILITY_RECOMMENDED_MODLOADER"
    local final_minecraft="$EMPACK_COMPATIBILITY_RECOMMENDED_MINECRAFT_VERSION"
    local final_modloader_version="$EMPACK_COMPATIBILITY_RECOMMENDED_MODLOADER_VERSION"

    # Override with user-provided values where specified
    if [ -n "$provided_minecraft" ]; then
      final_minecraft="$provided_minecraft"
      log_debug "Using user-provided Minecraft version: $provided_minecraft"

      # If user provided a specific Minecraft version, we need to find a compatible modloader version
      if [ "$provided_modloader" != "none" ]; then
        local compatible_modloader_versions
        case "$provided_modloader" in
          "neoforge")
            compatible_modloader_versions=$(get_neoforge_versions_for_minecraft "$provided_minecraft")
            ;;
          "fabric")
            compatible_modloader_versions=$(get_fabric_versions_for_minecraft "$provided_minecraft")
            ;;
          "quilt")
            compatible_modloader_versions=$(get_quilt_versions_for_minecraft "$provided_minecraft")
            ;;
        esac

        if [ -n "$compatible_modloader_versions" ]; then
          # Use the latest compatible version
          final_modloader_version=$(echo "$compatible_modloader_versions" | head -1)
          log_debug "Auto-selected compatible $provided_modloader version: $final_modloader_version"
        fi
      fi
    fi

    if [ -n "$provided_modloader_version" ]; then
      final_modloader_version="$provided_modloader_version"
      log_debug "Using user-provided modloader version: $provided_modloader_version"
    fi

    # Final compatibility validation
    if validate_compatibility_matrix "$final_modloader" "$final_minecraft" "$final_modloader_version" "auto-filled"; then
      set_compatibility_state "$final_modloader" "$final_minecraft" "$final_modloader_version"
      log_success "✅ Auto-filled and validated configuration: $final_modloader $final_modloader_version + Minecraft $final_minecraft"
      return 0
    else
      log_error "Auto-filled configuration failed compatibility validation"
      return 1
    fi
  else
    log_error "Failed to auto-fill configuration for modloader: $provided_modloader"
    return 1
  fi
}

# Apply validated compatibility state to environment
apply_compatibility_state() {
  if ! is_compatibility_validated; then
    log_error "No validated compatibility state to apply"
    return 1
  fi

  export EMPACK_CORE_MODLOADER="$EMPACK_COMPATIBILITY_VALIDATED_MODLOADER"
  export EMPACK_CORE_MINECRAFT_VERSION="$EMPACK_COMPATIBILITY_VALIDATED_MINECRAFT_VERSION"
  export EMPACK_CORE_MODLOADER_VERSION="$EMPACK_COMPATIBILITY_VALIDATED_MODLOADER_VERSION"

  log_debug "Applied compatibility state to environment"
  return 0
}

#=============================================================================
# THREE-MODE INITIALIZATION SUPPORT
#=============================================================================

# Zero-config golden path: Use intelligent defaults
init_zero_config() {
  log_info "🚀 empack: Zero-config professional modpack development"

  if stabilize_core_input; then
    apply_compatibility_state
    log_success "✅ Ready to build in ~3 seconds"
    return 0
  else
    log_error "Zero-config initialization failed"
    return 1
  fi
}

# Explicit non-interactive: Use provided flags + intelligent defaults for missing pieces
init_explicit_non_interactive() {
  log_info "📦 Using provided flags + intelligent defaults"

  if stabilize_core_input; then
    apply_compatibility_state
    log_success "✅ Flags enhanced the golden path"
    return 0
  else
    log_error "Explicit non-interactive initialization failed"
    return 1
  fi
}

# Interactive with auto-fill: Flags pre-populate prompts
init_interactive_with_autofill() {
  log_info "🚀 Interactive modpack initialization with auto-fill"

  # This would be implemented in init.sh with prompt logic
  # For now, fall back to stabilization
  if stabilize_core_input; then
    apply_compatibility_state
    log_success "✅ Configuration stabilized for interactive prompts"
    return 0
  else
    log_error "Interactive initialization setup failed"
    return 1
  fi
}

#=============================================================================
# MODULE INTERFACE CONTRACT
#=============================================================================

# Standard module interface - export compatibility state variables
export_compatibility_state() {
    echo "EMPACK_COMPATIBILITY_LAST_VALIDATION_TYPE='$EMPACK_COMPATIBILITY_LAST_VALIDATION_TYPE'"
    echo "EMPACK_COMPATIBILITY_MATRIX_STATUS='$EMPACK_COMPATIBILITY_MATRIX_STATUS'"
    echo "EMPACK_COMPATIBILITY_ERROR_SOURCE='$EMPACK_COMPATIBILITY_ERROR_SOURCE'"
    echo "EMPACK_COMPATIBILITY_ERROR_MESSAGE='$EMPACK_COMPATIBILITY_ERROR_MESSAGE'"
    echo "EMPACK_COMPATIBILITY_VALIDATED_MODLOADER='$EMPACK_COMPATIBILITY_VALIDATED_MODLOADER'"
    echo "EMPACK_COMPATIBILITY_VALIDATED_MINECRAFT_VERSION='$EMPACK_COMPATIBILITY_VALIDATED_MINECRAFT_VERSION'"
    echo "EMPACK_COMPATIBILITY_VALIDATED_MODLOADER_VERSION='$EMPACK_COMPATIBILITY_VALIDATED_MODLOADER_VERSION'"
    echo "EMPACK_COMPATIBILITY_RECOMMENDED_MODLOADER='$EMPACK_COMPATIBILITY_RECOMMENDED_MODLOADER'"
    echo "EMPACK_COMPATIBILITY_RECOMMENDED_MINECRAFT_VERSION='$EMPACK_COMPATIBILITY_RECOMMENDED_MINECRAFT_VERSION'"
    echo "EMPACK_COMPATIBILITY_RECOMMENDED_MODLOADER_VERSION='$EMPACK_COMPATIBILITY_RECOMMENDED_MODLOADER_VERSION'"
    echo "EMPACK_COMPATIBILITY_AUTO_FILL_USED='$EMPACK_COMPATIBILITY_AUTO_FILL_USED'"
    echo "EMPACK_COMPATIBILITY_INPUT_STABILIZATION_COMPLETE='$EMPACK_COMPATIBILITY_INPUT_STABILIZATION_COMPLETE'"
}

# Get current module status
get_compatibility_status() {
    local status="operational"
    local details=""
    
    if [ "$EMPACK_COMPATIBILITY_MATRIX_STATUS" = "error_basic_validation" ]; then
        status="error"
        details="Basic validation failed: $EMPACK_COMPATIBILITY_ERROR_MESSAGE"
    elif [ "$EMPACK_COMPATIBILITY_MATRIX_STATUS" = "error_neoforge_incompatible" ]; then
        status="error"
        details="NeoForge compatibility error: $EMPACK_COMPATIBILITY_ERROR_MESSAGE"
    elif [ "$EMPACK_COMPATIBILITY_INPUT_STABILIZATION_COMPLETE" = "true" ]; then
        status="validated"
        details="Stabilized: $EMPACK_COMPATIBILITY_VALIDATED_MODLOADER $EMPACK_COMPATIBILITY_VALIDATED_MODLOADER_VERSION + MC $EMPACK_COMPATIBILITY_VALIDATED_MINECRAFT_VERSION"
    elif [ -n "$EMPACK_COMPATIBILITY_LAST_VALIDATION_TYPE" ]; then
        status="active"
        details="Validating: $EMPACK_COMPATIBILITY_LAST_VALIDATION_TYPE"
    fi
    
    echo "status=$status"
    echo "matrix_status=$EMPACK_COMPATIBILITY_MATRIX_STATUS"
    echo "validated_modloader=$EMPACK_COMPATIBILITY_VALIDATED_MODLOADER"
    echo "validated_minecraft_version=$EMPACK_COMPATIBILITY_VALIDATED_MINECRAFT_VERSION"
    echo "validated_modloader_version=$EMPACK_COMPATIBILITY_VALIDATED_MODLOADER_VERSION"
    echo "stabilization_complete=$EMPACK_COMPATIBILITY_INPUT_STABILIZATION_COMPLETE"
    echo "details=$details"
}

# Validate compatibility module state and configuration
validate_compatibility_state() {
    local validation_passed=true
    local errors=()
    
    # Check if required dependency modules are available
    if ! declare -F validate_core_arguments >/dev/null 2>&1; then
        errors+=("Function validate_core_arguments not available from validation module")
        validation_passed=false
    fi
    
    if ! declare -F get_minecraft_versions_for_neoforge >/dev/null 2>&1; then
        errors+=("Function get_minecraft_versions_for_neoforge not available from api module")
        validation_passed=false
    fi
    
    if ! declare -F get_stable_neoforge_version >/dev/null 2>&1; then
        errors+=("Function get_stable_neoforge_version not available from api module")
        validation_passed=false
    fi
    
    if ! declare -F get_stable_fabric_version >/dev/null 2>&1; then
        errors+=("Function get_stable_fabric_version not available from api module")
        validation_passed=false
    fi
    
    if ! declare -F get_stable_quilt_version >/dev/null 2>&1; then
        errors+=("Function get_stable_quilt_version not available from api module")
        validation_passed=false
    fi
    
    if ! declare -F get_latest_minecraft_version >/dev/null 2>&1; then
        errors+=("Function get_latest_minecraft_version not available from api module")
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
    
    echo "validation_passed=$validation_passed"
    if [ ${#errors[@]} -gt 0 ]; then
        echo "errors=${errors[*]}"
    fi
    
    return $([ "$validation_passed" = true ] && echo 0 || echo 1)
}

# Export compatibility functions
export -f validate_compatibility_matrix validate_neoforge_compatibility_matrix validate_fabric_compatibility_matrix
export -f validate_quilt_compatibility_matrix handle_compatibility_error get_recommended_defaults
export -f get_neoforge_recommended_defaults get_fabric_recommended_defaults get_quilt_recommended_defaults
export -f get_vanilla_recommended_defaults stabilize_core_input clear_compatibility_state
export -f init_zero_config init_explicit_non_interactive init_interactive_with_autofill
export -f set_compatibility_state is_compatibility_validated get_compatibility_state apply_compatibility_state
# Module interface contract
export -f export_compatibility_state get_compatibility_status validate_compatibility_state

log_debug "Compatibility module loaded - 3D compatibility matrix and auto-fill architecture ready"
