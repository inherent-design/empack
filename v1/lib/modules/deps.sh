#!/usr/bin/env bash
# Module: deps
# Description: Dependency validation system with Flutter doctor-style setup guidance
# Dependencies: core, logger, utils

# Prevent multiple loading
if [ "${EMPACK_MODULE_DEPS:-}" = "loaded" ]; then
  return 0
fi
readonly EMPACK_MODULE_DEPS="loaded"

#=============================================================================
# DEPENDENCY CHECKING SYSTEM
#=============================================================================

# Dependency check result structure: "status|name|version|recommendations"
# status: "✅" for found, "❌" for missing
# recommendations: pipe-separated list of fix suggestions

# Check packwiz installation and provide guidance
check_packwiz() {
  local resolved_path
  resolved_path=$(find_dependency packwiz)
  if [ $? -eq 0 ]; then
    # Try to get version using go version command (packwiz has no --version flag)
    local version=$(
      go version -m $(which packwiz) | grep -E "^\s*mod\s+" | awk '{print substr($3, 2)}'
      2>/dev/null || echo "unknown"
    )
    log_debug "Found packwiz at: $resolved_path"

    # Set global state
    DEPS_PACKWIZ_STATUS="✅"
    DEPS_PACKWIZ_NAME="packwiz"
    DEPS_PACKWIZ_VERSION="$version"
    DEPS_PACKWIZ_RECOMMENDATIONS=""
  else
    log_debug "Packwiz not found"
    local recommendations=()
    if command_exists go; then
      recommendations+=("Install via Go: go install github.com/packwiz/packwiz@latest")
    fi
    recommendations+=("Download binary from: https://github.com/packwiz/packwiz/actions")
    recommendations+=("Extract binary to PATH or current directory")

    # Set global state
    DEPS_PACKWIZ_STATUS="❌"
    DEPS_PACKWIZ_NAME="packwiz"
    DEPS_PACKWIZ_VERSION="not found"
    DEPS_PACKWIZ_RECOMMENDATIONS="$(
      IFS='|'
      echo "${recommendations[*]}"
    )"
  fi
}

# Check tomlq/tq installation and provide guidance
check_tomlq() {
  local cmd_found=""

  # Check for tq first (preferred), then tomlq
  local resolved_path
  resolved_path=$(find_dependency tq)
  if [ $? -eq 0 ]; then
    cmd_found="tq"
    local version=$("$resolved_path" --version 2>/dev/null | awk '{print substr($2, 1)}' || echo "unknown")
    log_debug "Found tq at: $resolved_path"

    # Set global state
    DEPS_TOMLQ_STATUS="✅"
    DEPS_TOMLQ_NAME="tq"
    DEPS_TOMLQ_VERSION="$version"
    DEPS_TOMLQ_RECOMMENDATIONS=""
  else
    resolved_path=$(find_dependency tomlq)
    if [ $? -eq 0 ]; then
      cmd_found="tomlq"
      local version=$("$resolved_path" --version 2>/dev/null | head -n 1 || echo "unknown")
      log_debug "Found tomlq at: $resolved_path"

      # Set global state
      DEPS_TOMLQ_STATUS="✅"
      DEPS_TOMLQ_NAME="tomlq"
      DEPS_TOMLQ_VERSION="$version"
      DEPS_TOMLQ_RECOMMENDATIONS=""
    else
      log_debug "tomlq/tq not found"
      local recommendations=()
      # Provide installation guidance
      if command_exists cargo; then
        recommendations+=("Install via Cargo: cargo install tomlq")
      fi
      recommendations+=("Download binary from: https://github.com/cryptaliagy/tomlq/releases/latest")
      recommendations+=("Extract binary to PATH or current directory")

      # Set global state
      DEPS_TOMLQ_STATUS="❌"
      DEPS_TOMLQ_NAME="tomlq"
      DEPS_TOMLQ_VERSION="not found"
      DEPS_TOMLQ_RECOMMENDATIONS="$(
        IFS='|'
        echo "${recommendations[*]}"
      )"
    fi
  fi
}

# Check mrpack-install installation and provide guidance
check_mrpack_install() {
  local resolved_path
  resolved_path=$(find_dependency mrpack-install)
  if [ $? -eq 0 ]; then
    local version=$("$resolved_path" -V 2>/dev/null | awk '{print substr($2, 2)}' || echo "unknown")
    log_debug "Found mrpack-install at: $resolved_path"

    # Set global state
    DEPS_MRPACK_STATUS="✅"
    DEPS_MRPACK_NAME="mrpack-install"
    DEPS_MRPACK_VERSION="$version"
    DEPS_MRPACK_RECOMMENDATIONS=""
  else
    log_debug "mrpack-install not found"
    local recommendations=()
    recommendations+=("Download binary from: https://github.com/nothub/mrpack-install/releases/latest")
    recommendations+=("Extract binary to PATH or current directory")

    # Set global state
    DEPS_MRPACK_STATUS="❌"
    DEPS_MRPACK_NAME="mrpack-install"
    DEPS_MRPACK_VERSION="not found"
    DEPS_MRPACK_RECOMMENDATIONS="$(
      IFS='|'
      echo "${recommendations[*]}"
    )"
  fi
}

# Check Java installation
check_java() {
  local resolved_path
  resolved_path=$(find_dependency java)
  if [ $? -eq 0 ]; then
    local version=$("$resolved_path" --version 2>/dev/null | head -n 1 | awk '{print $2}' 2>/dev/null || echo "unknown")
    log_debug "Found java at: $resolved_path"

    # Set global state
    DEPS_JAVA_STATUS="✅"
    DEPS_JAVA_NAME="java"
    DEPS_JAVA_VERSION="$version"
    DEPS_JAVA_RECOMMENDATIONS=""
  else
    log_debug "java not found"
    local recommendations=()
    recommendations+=("Install Java 21+ from: https://adoptium.net/")
    recommendations+=("Or use your system package manager")

    # Set global state
    DEPS_JAVA_STATUS="❌"
    DEPS_JAVA_NAME="java"
    DEPS_JAVA_VERSION="not found"
    DEPS_JAVA_RECOMMENDATIONS="$(
      IFS='|'
      echo "${recommendations[*]}"
    )"
  fi
}

# Check Git installation (optional but recommended)
check_git() {
  local resolved_path
  resolved_path=$(find_dependency git)
  if [ $? -eq 0 ]; then
    local version=$("$resolved_path" --version 2>/dev/null | awk '{print $3}' || echo "unknown")
    log_debug "Found git at: $resolved_path"

    # Set global state
    DEPS_GIT_STATUS="✅"
    DEPS_GIT_NAME="git"
    DEPS_GIT_VERSION="$version"
    DEPS_GIT_RECOMMENDATIONS=""
  else
    log_debug "git not found"
    local recommendations=()
    recommendations+=("Install Git from: https://git-scm.com/")
    recommendations+=("Or use your system package manager")

    # Set global state
    DEPS_GIT_STATUS="❌"
    DEPS_GIT_NAME="git"
    DEPS_GIT_VERSION="not found"
    DEPS_GIT_RECOMMENDATIONS="$(
      IFS='|'
      echo "${recommendations[*]}"
    )"
  fi
}

# Check jq installation for robust JSON parsing (critical for API integration)
check_jq() {
  local resolved_path
  resolved_path=$(find_dependency jq)
  if [ $? -eq 0 ]; then
    local version=$("$resolved_path" --version 2>/dev/null | awk '{print substr($1, 4)}' || echo "unknown")
    log_debug "Found jq at: $resolved_path"

    # Set global state
    DEPS_JQ_STATUS="✅"
    DEPS_JQ_NAME="jq"
    DEPS_JQ_VERSION="$version"
    DEPS_JQ_RECOMMENDATIONS=""
  else
    log_debug "jq not found"
    local recommendations=()
    recommendations+=("Download from: https://github.com/jqlang/jq/releases/latest")
    recommendations+=("Rename binary to 'jq' and make executable")
    recommendations+=("Add to PATH or place in modpack directory")
    recommendations+=("Platform binaries: jq-linux64, jq-macos-amd64, jq-windows-amd64.exe")

    # Set global state
    DEPS_JQ_STATUS="❌"
    DEPS_JQ_NAME="jq"
    DEPS_JQ_VERSION="not found"
    DEPS_JQ_RECOMMENDATIONS="$(
      IFS='|'
      echo "${recommendations[*]}"
    )"
  fi
}

# Check xq installation for robust XML parsing (critical for Maven APIs)
check_xq() {
  local resolved_path
  resolved_path=$(find_dependency xq)
  if [ $? -eq 0 ]; then
    local version=$("$resolved_path" --version 2>/dev/null | awk '{print substr($3, 1)}' || echo "unknown")
    log_debug "Found xq at: $resolved_path"

    # Set global state
    DEPS_XQ_STATUS="✅"
    DEPS_XQ_NAME="xq"
    DEPS_XQ_VERSION="$version"
    DEPS_XQ_RECOMMENDATIONS=""
  else
    log_debug "xq not found"
    local recommendations=()
    # Provide installation guidance similar to packwiz (Go-based tool)
    if command_exists go; then
      recommendations+=("Install via Go: go install github.com/sibprogrammer/xq@latest")
    fi
    recommendations+=("Download from: https://github.com/sibprogrammer/xq/releases/latest")
    recommendations+=("Extract binary to PATH or place in modpack directory")
    recommendations+=("Platform binaries: xq_darwin_amd64, xq_linux_amd64, xq_windows_amd64.exe")

    # Set global state
    DEPS_XQ_STATUS="❌"
    DEPS_XQ_NAME="xq"
    DEPS_XQ_VERSION="not found"
    DEPS_XQ_RECOMMENDATIONS="$(
      IFS='|'
      echo "${recommendations[*]}"
    )"
  fi
}

# Check curl installation (usually present but validate)
check_curl() {
  local resolved_path
  resolved_path=$(find_dependency curl)
  if [ $? -eq 0 ]; then
    local version=$("$resolved_path" --version 2>/dev/null | head -n 1 | awk '{print $2}' || echo "unknown")
    log_debug "Found curl at: $resolved_path"

    # Set global state
    DEPS_CURL_STATUS="✅"
    DEPS_CURL_NAME="curl"
    DEPS_CURL_VERSION="$version"
    DEPS_CURL_RECOMMENDATIONS=""
  else
    log_debug "curl not found"
    local recommendations=()
    recommendations+=("Install curl via system package manager")
    recommendations+=("Usually pre-installed on most systems")

    # Set global state
    DEPS_CURL_STATUS="❌"
    DEPS_CURL_NAME="curl"
    DEPS_CURL_VERSION="not found"
    DEPS_CURL_RECOMMENDATIONS="$(
      IFS='|'
      echo "${recommendations[*]}"
    )"
  fi
}

#=============================================================================
# DEPENDENCY AVAILABILITY HELPERS
#=============================================================================

# Helper function to check if jq is available for API functions
jq_available() {
  find_dependency jq >/dev/null 2>&1
}

# Helper function to check if xq is available for XML parsing
xq_available() {
  find_dependency xq >/dev/null 2>&1
}

# Helper function to check if tomlq/tq is available
tomlq_available() {
  find_dependency tq >/dev/null 2>&1 || find_dependency tomlq >/dev/null 2>&1
}

#=============================================================================
# DEPENDENCY RESULT PROCESSING
#=============================================================================

# Parse a dependency check result
parse_dep_check() {
  local result="$1"
  local -n status_ref=$2
  local -n name_ref=$3
  local -n version_ref=$4
  local -n recs_ref=$5

  IFS='|' read -r status_ref name_ref version_ref recs_ref <<<"$result"
}

# Display dependency check results using global state (no stdout parsing!)
display_dep_results_from_state() {
  local ready_count=0
  local total_count=8

  echo
  log_info "empack dependency status:"

  # Array of all dependency state variables
  local -a deps=(
    "PACKWIZ" "TOMLQ" "MRPACK" "JAVA" "JQ" "XQ" "CURL" "GIT"
  )

  # Display status for each dependency
  for dep in "${deps[@]}"; do
    local status_var="DEPS_${dep}_STATUS"
    local name_var="DEPS_${dep}_NAME"
    local version_var="DEPS_${dep}_VERSION"

    local status="${!status_var}"
    local name="${!name_var}"
    local version="${!version_var}"

    echo "$status $name ($version)"
    if [ "$status" = "✅" ]; then
      ready_count=$((ready_count + 1))
    fi
  done

  echo
  echo "📋 Requirements Summary:"

  # Summary for each dependency
  for dep in "${deps[@]}"; do
    local status_var="DEPS_${dep}_STATUS"
    local name_var="DEPS_${dep}_NAME"

    local status="${!status_var}"
    local name="${!name_var}"

    local status_text
    if [ "$status" = "✅" ]; then
      status_text="✅ Ready"
    else
      status_text="❌ Missing"
    fi

    echo "- $name: $status_text"
  done

  echo

  # Show fix recommendations for missing tools
  for dep in "${deps[@]}"; do
    local status_var="DEPS_${dep}_STATUS"
    local name_var="DEPS_${dep}_NAME"
    local recs_var="DEPS_${dep}_RECOMMENDATIONS"

    local status="${!status_var}"
    local name="${!name_var}"
    local recs="${!recs_var}"

    if [[ $status == "❌" && -n $recs ]]; then
      echo "🔧 Fix $name:"
      IFS='|' read -ra rec_array <<<"$recs"
      for rec in "${rec_array[@]:-}"; do
        [ -n "$rec" ] && echo "   $rec"
      done
      echo
    fi
  done

  # Final status with actionable next steps
  if [ "$ready_count" -eq "$total_count" ]; then
    log_success "All requirements met! ($ready_count/$total_count)"
    echo
    echo "🚀 Next steps:"
    echo "   empack init                    # Initialize modpack development environment"
    echo "   empack --modpack-directory /tmp/test init  # Test in isolated directory"
    return 0
  else
    log_warning "Requirements incomplete: $ready_count/$total_count tools ready"
    echo
    echo "⚠️  Some tools are missing. Install them using the guidance above, then:"
    echo "   empack requirements            # Re-check dependencies"
    echo "   empack init                    # Initialize when ready"
    return 1
  fi
}

#=============================================================================
# MAIN REQUIREMENTS COMMAND
#=============================================================================

# Main requirements checking command (Flutter doctor-style)
requirements_command() {
  log_info "Checking empack requirements..."

  # Run all dependency checks - they set global state
  check_packwiz
  check_tomlq
  check_mrpack_install
  check_java
  check_jq
  check_xq
  check_curl
  check_git

  # Display results using state
  display_dep_results_from_state
}

#=============================================================================
# VALIDATION FUNCTIONS FOR BUILD OPERATIONS
#=============================================================================

# Validate dependencies required for build operations (post-init commands)
validate_build_dependencies() {
  log_debug "Validating build dependencies..."

  local missing_deps=()

  # Check critical build dependencies using enhanced resolution
  if ! find_dependency packwiz >/dev/null; then
    missing_deps+=("packwiz")
  fi

  # Check for tomlq or tq
  if ! find_dependency tq >/dev/null && ! find_dependency tomlq >/dev/null; then
    missing_deps+=("tomlq")
  fi

  if ! find_dependency mrpack-install >/dev/null; then
    missing_deps+=("mrpack-install")
  fi

  if ! find_dependency java >/dev/null; then
    missing_deps+=("java")
  fi

  # Report missing dependencies
  if [ ${#missing_deps[@]} -gt 0 ]; then
    log_error "Missing required dependencies for build operations:"
    for dep in "${missing_deps[@]:-}"; do
      log_error "- $dep"
    done
    echo
    log_error "Run 'empack requirements' for setup guidance"
    return 1
  fi

  log_debug "All build dependencies satisfied"
  return 0
}

# Quick dependency check for API integration (for init command)
validate_api_dependencies() {
  log_debug "Validating API integration dependencies..."

  local missing_api_deps=()

  if ! jq_available; then
    missing_api_deps+=("jq")
  fi

  if ! xq_available; then
    missing_api_deps+=("xq")
  fi

  if ! find_dependency curl >/dev/null; then
    missing_api_deps+=("curl")
  fi

  if [ ${#missing_api_deps[@]} -gt 0 ]; then
    log_warning "Missing API integration dependencies: ${missing_api_deps[*]}"
    log_warning "Some features may not work properly without these tools"
    return 1
  fi

  log_debug "All API dependencies satisfied"
  return 0
}

# Quick dependency check for basic functionality
quick_dependency_check() {
  local missing=()

  find_dependency packwiz >/dev/null || missing+=("packwiz")
  (find_dependency tq >/dev/null || find_dependency tomlq >/dev/null) || missing+=("tomlq")
  find_dependency mrpack-install >/dev/null || missing+=("mrpack-install")
  find_dependency java >/dev/null || missing+=("java")

  if [ ${#missing[@]} -gt 0 ]; then
    return 1
  fi
  return 0
}

# Export dependency functions
export -f check_packwiz check_tomlq check_mrpack_install check_java check_jq check_xq check_curl check_git
export -f jq_available xq_available tomlq_available
export -f requirements_command validate_build_dependencies validate_api_dependencies quick_dependency_check
