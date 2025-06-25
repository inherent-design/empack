#!/usr/bin/env bash
# Module: validation
# Description: Pure format and existence validation (no compatibility logic)
# Dependencies: core, logger, utils, deps, api

# Prevent multiple loading
if [ "${EMPACK_MODULE_VALIDATION:-}" = "loaded" ]; then
  return 0
fi
readonly EMPACK_MODULE_VALIDATION="loaded"

#=============================================================================
# VALIDATION STATE VARIABLES
#=============================================================================

# Validation state (EMPACK_VALIDATION_* namespace)
declare -g EMPACK_VALIDATION_LAST_VALIDATION_TYPE=""
declare -g EMPACK_VALIDATION_MODLOADER_STATUS=""
declare -g EMPACK_VALIDATION_MINECRAFT_VERSION_STATUS=""
declare -g EMPACK_VALIDATION_MODLOADER_VERSION_STATUS=""
declare -g EMPACK_VALIDATION_PERSONALIZATION_STATUS=""
declare -g EMPACK_VALIDATION_ERROR_COUNT="0"
declare -g EMPACK_VALIDATION_LAST_ERROR_MESSAGE=""
declare -g EMPACK_VALIDATION_DEFAULT_NAME=""
declare -g EMPACK_VALIDATION_DEFAULT_AUTHOR=""
declare -g EMPACK_VALIDATION_DEFAULT_VERSION=""

# Clear validation state
clear_validation_state() {
    EMPACK_VALIDATION_LAST_VALIDATION_TYPE=""
    EMPACK_VALIDATION_MODLOADER_STATUS=""
    EMPACK_VALIDATION_MINECRAFT_VERSION_STATUS=""
    EMPACK_VALIDATION_MODLOADER_VERSION_STATUS=""
    EMPACK_VALIDATION_PERSONALIZATION_STATUS=""
    EMPACK_VALIDATION_ERROR_COUNT="0"
    EMPACK_VALIDATION_LAST_ERROR_MESSAGE=""
    EMPACK_VALIDATION_DEFAULT_NAME=""
    EMPACK_VALIDATION_DEFAULT_AUTHOR=""
    EMPACK_VALIDATION_DEFAULT_VERSION=""
    log_debug "Validation state cleared"
}

#=============================================================================
# FORMAT VALIDATION FUNCTIONS
#=============================================================================

# Validate modloader name format with state tracking
validate_modloader_format() {
  local modloader="$1"
  EMPACK_VALIDATION_LAST_VALIDATION_TYPE="modloader_format"
  
  if [ -z "$modloader" ]; then
    EMPACK_VALIDATION_MODLOADER_STATUS="error_empty"
    EMPACK_VALIDATION_LAST_ERROR_MESSAGE="Modloader is required"
    log_error "$EMPACK_VALIDATION_LAST_ERROR_MESSAGE"
    return 1
  fi

  case "$modloader" in
    "neoforge" | "fabric" | "quilt" | "none")
      EMPACK_VALIDATION_MODLOADER_STATUS="valid_format"
      log_debug "Valid modloader format: $modloader"
      return 0
      ;;
    *)
      EMPACK_VALIDATION_MODLOADER_STATUS="error_invalid"
      EMPACK_VALIDATION_LAST_ERROR_MESSAGE="Invalid modloader: $modloader"
      log_error "$EMPACK_VALIDATION_LAST_ERROR_MESSAGE"
      log_error "Valid options: neoforge, fabric, quilt, none"
      return 1
      ;;
  esac
}

# Validate semver format (loose validation)
validate_semver_format() {
  local version="$1"
  local context="${2:-version}"
  
  if [ -z "$version" ]; then
    log_error "$context is required"
    return 1
  fi

  # Loose semver validation - allows various formats used by modloaders
  if [[ $version =~ ^[0-9]+\.[0-9]+(\.[0-9]+)?([-.][a-zA-Z0-9]+)*$ ]]; then
    log_debug "Valid $context format: $version"
    return 0
  else
    log_warning "$context does not follow standard format: $version"
    log_warning "Recommended format: X.Y.Z (e.g., 1.21.1)"
    return 0  # Warning only, not error for flexibility
  fi
}

#=============================================================================
# EXISTENCE VALIDATION FUNCTIONS
#=============================================================================

# Validate minecraft version exists via API with state tracking
validate_minecraft_version_exists() {
  local minecraft_version="$1"
  EMPACK_VALIDATION_LAST_VALIDATION_TYPE="minecraft_version_existence"
  
  if [ -z "$minecraft_version" ]; then
    EMPACK_VALIDATION_MINECRAFT_VERSION_STATUS="error_empty"
    EMPACK_VALIDATION_LAST_ERROR_MESSAGE="Minecraft version is required"
    log_error "$EMPACK_VALIDATION_LAST_ERROR_MESSAGE"
    return 1
  fi

  log_debug "Checking if Minecraft version exists: $minecraft_version"

  # Use API state to check if we have version data
  if ! get_all_minecraft_versions; then
    EMPACK_VALIDATION_MINECRAFT_VERSION_STATUS="warning_no_api"
    log_warning "Could not fetch Minecraft versions for validation"
    return 0 # Allow if we can't validate
  fi

  if echo "$EMPACK_API_MINECRAFT_ALL_VERSIONS" | grep -q "^$minecraft_version$"; then
    EMPACK_VALIDATION_MINECRAFT_VERSION_STATUS="valid_exists"
    log_debug "Minecraft version exists: $minecraft_version"
    return 0
  else
    EMPACK_VALIDATION_MINECRAFT_VERSION_STATUS="error_not_found"
    EMPACK_VALIDATION_LAST_ERROR_MESSAGE="Minecraft version '$minecraft_version' does not exist"
    log_error "$EMPACK_VALIDATION_LAST_ERROR_MESSAGE"
    log_info "Use 'empack versions minecraft' to see available versions"
    return 1
  fi
}

# Validate modloader version exists via API with state tracking
validate_modloader_version_exists() {
  local modloader="$1"
  local modloader_version="$2"
  EMPACK_VALIDATION_LAST_VALIDATION_TYPE="modloader_version_existence"

  if [ -z "$modloader" ] || [ -z "$modloader_version" ]; then
    EMPACK_VALIDATION_MODLOADER_VERSION_STATUS="error_empty"
    EMPACK_VALIDATION_LAST_ERROR_MESSAGE="Both modloader and version are required"
    log_error "$EMPACK_VALIDATION_LAST_ERROR_MESSAGE"
    return 1
  fi

  # No version validation needed for 'none' modloader
  if [ "$modloader" = "none" ]; then
    EMPACK_VALIDATION_MODLOADER_VERSION_STATUS="valid_none"
    log_debug "No modloader, no version validation needed"
    return 0
  fi

  log_debug "Checking if $modloader version exists: $modloader_version"

  local state_var=""
  case "$modloader" in
    "neoforge")
      if ! get_all_neoforge_versions; then
        EMPACK_VALIDATION_MODLOADER_VERSION_STATUS="warning_no_api"
        log_warning "Could not fetch NeoForge versions for validation"
        return 0
      fi
      state_var="EMPACK_API_NEOFORGE_ALL_VERSIONS"
      ;;
    "fabric")
      if ! get_all_fabric_versions; then
        EMPACK_VALIDATION_MODLOADER_VERSION_STATUS="warning_no_api"
        log_warning "Could not fetch Fabric versions for validation"
        return 0
      fi
      state_var="EMPACK_API_FABRIC_ALL_VERSIONS"
      ;;
    "quilt")
      if ! get_all_quilt_versions; then
        EMPACK_VALIDATION_MODLOADER_VERSION_STATUS="warning_no_api"
        log_warning "Could not fetch Quilt versions for validation"
        return 0
      fi
      state_var="EMPACK_API_QUILT_ALL_VERSIONS"
      ;;
    *)
      EMPACK_VALIDATION_MODLOADER_VERSION_STATUS="error_unknown_modloader"
      EMPACK_VALIDATION_LAST_ERROR_MESSAGE="Unknown modloader for version validation: $modloader"
      log_error "$EMPACK_VALIDATION_LAST_ERROR_MESSAGE"
      return 1
      ;;
  esac

  # Use indirect reference to access the state variable
  local all_versions="${!state_var}"
  if echo "$all_versions" | grep -q "^$modloader_version$"; then
    EMPACK_VALIDATION_MODLOADER_VERSION_STATUS="valid_exists"
    log_debug "$modloader version exists: $modloader_version"
    return 0
  else
    EMPACK_VALIDATION_MODLOADER_VERSION_STATUS="error_not_found"
    EMPACK_VALIDATION_LAST_ERROR_MESSAGE="$modloader version '$modloader_version' does not exist"
    log_error "$EMPACK_VALIDATION_LAST_ERROR_MESSAGE"
    log_info "Use 'empack versions $modloader' to see available versions"
    return 1
  fi
}

#=============================================================================
# PERSONALIZATION VALIDATION (Optional Fields)
#=============================================================================

# Validate modpack name format
validate_name_format() {
  local name="$1"
  
  if [ -z "$name" ]; then
    return 0  # Name is optional
  fi

  # Basic name validation - reasonable length
  if [ ${#name} -gt 100 ]; then
    log_error "Modpack name too long (max 100 characters): $name"
    return 1
  fi

  # Check for problematic characters
  if [[ $name =~ [\<\>\:\"\/\\\|\?\*] ]]; then
    log_error "Modpack name contains invalid characters: $name"
    log_error 'Avoid: < > : " / \ | ? *'
    return 1
  fi

  log_debug "Valid name format: $name"
  return 0
}

# Validate author format
validate_author_format() {
  local author="$1"
  
  if [ -z "$author" ]; then
    return 0  # Author is optional
  fi

  # Basic author validation - reasonable length
  if [ ${#author} -gt 50 ]; then
    log_error "Author name too long (max 50 characters): $author"
    return 1
  fi

  log_debug "Valid author format: $author"
  return 0
}

# Validate pack version format
validate_pack_version_format() {
  local version="$1"
  
  if [ -z "$version" ]; then
    return 0  # Version is optional
  fi

  # Use semver validation for pack version
  validate_semver_format "$version" "pack version"
}

# Validate all personalization fields
validate_personalization() {
  local name="$1"
  local author="$2"
  local version="$3"

  local errors=0

  # Validate each field independently
  if ! validate_name_format "$name"; then
    errors=$((errors + 1))
  fi

  if ! validate_author_format "$author"; then
    errors=$((errors + 1))
  fi

  if ! validate_pack_version_format "$version"; then
    errors=$((errors + 1))
  fi

  if [ $errors -gt 0 ]; then
    log_error "Personalization validation failed ($errors errors)"
    return 1
  fi

  log_debug "Personalization validation passed"
  return 0
}

#=============================================================================
# SMART DEFAULT GENERATION (Format Only, No API Calls)
#=============================================================================

# Get default personalization values with smart fallbacks using state
get_default_personalization() {
  local target_dir="${EMPACK_CORE_TARGET_DIR:-$(pwd)}"

  # Default name: basename of directory, with quote detection
  local default_name
  default_name=$(basename "$target_dir")

  # Check if name needs quoting (spaces or special chars)
  if [[ $default_name =~ [[:space:]] ]] || [[ $default_name =~ [^a-zA-Z0-9._-] ]]; then
    log_debug "Default name needs quoting: $default_name"
    default_name="\"$default_name\""
  fi

  # Default version: 0.0.0
  local default_version="0.0.0"

  # Default author: CLI > git config > "Unknown"
  local default_author="Unknown"

  # Try git config for author
  if command -v git >/dev/null 2>&1; then
    local git_name
    if git_name=$(git config --global user.name 2>/dev/null) && [ -n "$git_name" ]; then
      default_author="$git_name"
      log_debug "Using git config for default author: $default_author"
    fi
  fi

  # Store in state variables for reuse
  EMPACK_VALIDATION_DEFAULT_NAME="$default_name"
  EMPACK_VALIDATION_DEFAULT_VERSION="$default_version"
  EMPACK_VALIDATION_DEFAULT_AUTHOR="$default_author"

  # Output as structured data for parsing
  echo "name=$default_name"
  echo "version=$default_version"
  echo "author=$default_author"
}

#=============================================================================
# COMPREHENSIVE VALIDATION FUNCTIONS
#=============================================================================

# Validate core arguments (format + existence, no compatibility) with state tracking
validate_core_arguments() {
  local modloader="$1"
  local minecraft_version="$2"
  local modloader_version="$3"
  
  EMPACK_VALIDATION_LAST_VALIDATION_TYPE="core_arguments"
  EMPACK_VALIDATION_ERROR_COUNT="0"

  log_debug "Validating core arguments individually"

  # Format validation
  if ! validate_modloader_format "$modloader"; then
    EMPACK_VALIDATION_ERROR_COUNT=$((EMPACK_VALIDATION_ERROR_COUNT + 1))
  fi

  if ! validate_semver_format "$minecraft_version" "minecraft version"; then
    EMPACK_VALIDATION_ERROR_COUNT=$((EMPACK_VALIDATION_ERROR_COUNT + 1))
  fi

  # For 'none' modloader, modloader_version should be empty or match minecraft_version
  if [ "$modloader" = "none" ]; then
    if [ -n "$modloader_version" ] && [ "$modloader_version" != "$minecraft_version" ]; then
      log_warning "Modloader version ignored for 'none' modloader"
    fi
  else
    if ! validate_semver_format "$modloader_version" "$modloader version"; then
      EMPACK_VALIDATION_ERROR_COUNT=$((EMPACK_VALIDATION_ERROR_COUNT + 1))
    fi
  fi

  # Existence validation
  if ! validate_minecraft_version_exists "$minecraft_version"; then
    EMPACK_VALIDATION_ERROR_COUNT=$((EMPACK_VALIDATION_ERROR_COUNT + 1))
  fi

  if ! validate_modloader_version_exists "$modloader" "$modloader_version"; then
    EMPACK_VALIDATION_ERROR_COUNT=$((EMPACK_VALIDATION_ERROR_COUNT + 1))
  fi

  if [ "$EMPACK_VALIDATION_ERROR_COUNT" -gt 0 ]; then
    log_error "Core arguments validation failed ($EMPACK_VALIDATION_ERROR_COUNT errors)"
    return 1
  fi

  log_debug "Core arguments validation passed"
  return 0
}

# Validate complete arguments for non-interactive mode with state tracking
validate_complete_arguments() {
  local modloader="$1"
  local minecraft_version="$2"
  local modloader_version="$3"
  local name="$4"
  local author="$5"
  local version="$6"

  EMPACK_VALIDATION_LAST_VALIDATION_TYPE="complete_arguments"
  log_debug "Validating complete arguments for non-interactive mode"

  # Validate core arguments (format + existence)
  if ! validate_core_arguments "$modloader" "$minecraft_version" "$modloader_version"; then
    return 1
  fi

  # Validate personalization arguments
  if ! validate_personalization "$name" "$author" "$version"; then
    EMPACK_VALIDATION_PERSONALIZATION_STATUS="error"
    return 1
  fi

  EMPACK_VALIDATION_PERSONALIZATION_STATUS="valid"
  log_debug "Complete arguments validation passed"
  return 0
}

#=============================================================================
# MODULE INTERFACE CONTRACT
#=============================================================================

# Standard module interface - export validation state variables
export_validation_state() {
    echo "EMPACK_VALIDATION_LAST_VALIDATION_TYPE='$EMPACK_VALIDATION_LAST_VALIDATION_TYPE'"
    echo "EMPACK_VALIDATION_MODLOADER_STATUS='$EMPACK_VALIDATION_MODLOADER_STATUS'"
    echo "EMPACK_VALIDATION_MINECRAFT_VERSION_STATUS='$EMPACK_VALIDATION_MINECRAFT_VERSION_STATUS'"
    echo "EMPACK_VALIDATION_MODLOADER_VERSION_STATUS='$EMPACK_VALIDATION_MODLOADER_VERSION_STATUS'"
    echo "EMPACK_VALIDATION_PERSONALIZATION_STATUS='$EMPACK_VALIDATION_PERSONALIZATION_STATUS'"
    echo "EMPACK_VALIDATION_ERROR_COUNT='$EMPACK_VALIDATION_ERROR_COUNT'"
    echo "EMPACK_VALIDATION_LAST_ERROR_MESSAGE='$EMPACK_VALIDATION_LAST_ERROR_MESSAGE'"
    echo "EMPACK_VALIDATION_DEFAULT_NAME='$EMPACK_VALIDATION_DEFAULT_NAME'"
    echo "EMPACK_VALIDATION_DEFAULT_AUTHOR='$EMPACK_VALIDATION_DEFAULT_AUTHOR'"
    echo "EMPACK_VALIDATION_DEFAULT_VERSION='$EMPACK_VALIDATION_DEFAULT_VERSION'"
}

# Get current module status
get_validation_status() {
    local status="operational"
    local details=""
    
    if [ "$EMPACK_VALIDATION_ERROR_COUNT" != "0" ] && [ "$EMPACK_VALIDATION_ERROR_COUNT" != "" ]; then
        status="error"
        details="$EMPACK_VALIDATION_ERROR_COUNT validation errors: $EMPACK_VALIDATION_LAST_ERROR_MESSAGE"
    elif [ -n "$EMPACK_VALIDATION_LAST_VALIDATION_TYPE" ]; then
        status="active"
        details="Last validation: $EMPACK_VALIDATION_LAST_VALIDATION_TYPE"
    fi
    
    echo "status=$status"
    echo "last_validation_type=$EMPACK_VALIDATION_LAST_VALIDATION_TYPE"
    echo "error_count=$EMPACK_VALIDATION_ERROR_COUNT"
    echo "modloader_status=$EMPACK_VALIDATION_MODLOADER_STATUS"
    echo "minecraft_version_status=$EMPACK_VALIDATION_MINECRAFT_VERSION_STATUS"
    echo "modloader_version_status=$EMPACK_VALIDATION_MODLOADER_VERSION_STATUS"
    echo "personalization_status=$EMPACK_VALIDATION_PERSONALIZATION_STATUS"
    echo "details=$details"
}

# Validate validation module state and configuration
validate_validation_state() {
    local validation_passed=true
    local errors=()
    
    # Check if API module functions are available (dependency)
    if ! declare -F get_all_minecraft_versions >/dev/null 2>&1; then
        errors+=("Function get_all_minecraft_versions not available from api module")
        validation_passed=false
    fi
    
    if ! declare -F get_all_neoforge_versions >/dev/null 2>&1; then
        errors+=("Function get_all_neoforge_versions not available from api module")
        validation_passed=false
    fi
    
    if ! declare -F get_all_fabric_versions >/dev/null 2>&1; then
        errors+=("Function get_all_fabric_versions not available from api module")
        validation_passed=false
    fi
    
    if ! declare -F get_all_quilt_versions >/dev/null 2>&1; then
        errors+=("Function get_all_quilt_versions not available from api module")
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
    
    # Check if we can access git command for defaults
    if ! command -v git >/dev/null 2>&1; then
        log_debug "git command not available - will use fallback for default author"
    fi
    
    # Verify core module state variables are accessible
    if [ -z "${EMPACK_CORE_TARGET_DIR:-}" ]; then
        log_debug "EMPACK_CORE_TARGET_DIR not set - will use current directory for defaults"
    fi
    
    echo "validation_passed=$validation_passed"
    if [ ${#errors[@]} -gt 0 ]; then
        echo "errors=${errors[*]}"
    fi
    
    return $([ "$validation_passed" = true ] && echo 0 || echo 1)
}

# Export validation functions
export -f validate_modloader_format validate_semver_format validate_minecraft_version_exists
export -f validate_modloader_version_exists validate_name_format validate_author_format validate_pack_version_format
export -f validate_personalization get_default_personalization validate_core_arguments validate_complete_arguments
export -f clear_validation_state
# Module interface contract
export -f export_validation_state get_validation_status validate_validation_state

log_debug "Validation module loaded - pure format and existence validation"