#!/usr/bin/env bash
# Module: deps
# Description: Dependency validation system with setup guidance
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
    local status="❌"
    local version="not found"
    local recommendations=()
    
    if command_exists packwiz; then
        # Use go version command to get packwiz version
        version=$(go version -m "$(which packwiz)" 2>/dev/null | head -n 3 | tail -n 1 | awk '{print $3}' 2>/dev/null || echo "unknown")
        status="✅"
    else
        # Provide installation guidance
        if command_exists go; then
            recommendations+=("Install via Go: go install github.com/packwiz/packwiz@latest")
        fi
        recommendations+=("Download binary from: https://github.com/packwiz/packwiz/actions")
        recommendations+=("Extract binary to PATH or current directory")
    fi
    
    echo "$status|packwiz|$version|$(IFS='|'; echo "${recommendations[*]}")"
}

# Check tomlq/tq installation and provide guidance
check_tomlq() {
    local status="❌"
    local version="not found"
    local recommendations=()
    local cmd_found=""
    
    # Check for tq first, then tomlq
    if command_exists tq; then
        cmd_found="tq"
        version=$(get_command_version tq --version)
        status="✅"
    elif command_exists tomlq; then
        cmd_found="tomlq"
        version=$(get_command_version tomlq --version)
        status="✅"
    else
        # Provide installation guidance
        if command_exists cargo; then
            recommendations+=("Install via Cargo: cargo install tomlq")
        fi
        recommendations+=("Download binary from: https://github.com/cryptaliagy/tomlq/releases/latest")
        recommendations+=("Extract binary to PATH or current directory")
    fi
    
    echo "$status|tomlq ($cmd_found)|$version|$(IFS='|'; echo "${recommendations[*]}")"
}

# Check mrpack-install installation and provide guidance
check_mrpack_install() {
    local status="❌"
    local version="not found"
    local recommendations=()
    
    if command_exists mrpack-install; then
        version=$(get_command_version mrpack-install -V)
        status="✅"
    else
        recommendations+=("Download binary from: https://github.com/nothub/mrpack-install/releases/latest")
        recommendations+=("Extract binary to PATH or current directory")
    fi
    
    echo "$status|mrpack-install|$version|$(IFS='|'; echo "${recommendations[*]}")"
}

# Check Java installation
check_java() {
    local status="❌"
    local version="not found"
    local recommendations=()
    
    if command_exists java; then
        version=$(java --version 2>/dev/null | head -n 1 | awk '{print $2}' 2>/dev/null || echo "unknown")
        status="✅"
    else
        recommendations+=("Install Java 21+ from: https://adoptium.net/")
        recommendations+=("Or use your system package manager")
    fi
    
    echo "$status|java|$version|$(IFS='|'; echo "${recommendations[*]}")"
}

# Check Git installation (optional but recommended)
check_git() {
    local status="❌"
    local version="not found"
    local recommendations=()
    
    if command_exists git; then
        version=$(get_command_version git --version | awk '{print $3}')
        status="✅"
    else
        recommendations+=("Install Git from: https://git-scm.com/")
        recommendations+=("Or use your system package manager")
    fi
    
    echo "$status|git|$version|$(IFS='|'; echo "${recommendations[*]}")"
}

# Check jq installation for robust JSON parsing
check_jq() {
    local status="❌"
    local version="not found"
    local recommendations=()
    local resolved_path=""
    
    # Use enhanced dependency resolution
    resolved_path=$(find_dependency jq)
    if [ $? -eq 0 ]; then
        version=$("$resolved_path" --version 2>/dev/null | head -n 1 || echo "unknown")
        status="✅"
        log_debug "Found jq at: $resolved_path"
    else
        recommendations+=("Download from: https://github.com/jqlang/jq/releases/latest")
        recommendations+=("Rename binary to 'jq' and make executable")
        recommendations+=("Add to PATH or place in modpack directory")
        recommendations+=("Platform binaries: jq-linux64, jq-macos-amd64, jq-windows-amd64.exe")
    fi
    
    echo "$status|jq|$version|$(IFS='|'; echo "${recommendations[*]}")"
}

# Check xq installation for robust XML parsing
check_xq() {
    local status="❌"
    local version="not found"
    local recommendations=()
    local resolved_path=""
    
    # Use enhanced dependency resolution
    resolved_path=$(find_dependency xq)
    if [ $? -eq 0 ]; then
        version=$("$resolved_path" --version 2>/dev/null | head -n 1 || echo "unknown")
        status="✅"
        log_debug "Found xq at: $resolved_path"
    else
        # Provide installation guidance similar to packwiz (Go-based tool)
        if command_exists go; then
            recommendations+=("Install via Go: go install github.com/sibprogrammer/xq@latest")
        fi
        recommendations+=("Download from: https://github.com/sibprogrammer/xq/releases/latest")
        recommendations+=("Extract binary to PATH or place in modpack directory")
        recommendations+=("Platform binaries: xq_darwin_amd64, xq_linux_amd64, xq_windows_amd64.exe")
    fi
    
    echo "$status|xq|$version|$(IFS='|'; echo "${recommendations[*]}")"
}

# Helper function to check if jq is available for API functions
jq_available() {
    find_dependency jq >/dev/null 2>&1
}

# Helper function to check if xq is available for XML parsing
xq_available() {
    find_dependency xq >/dev/null 2>&1
}

# Parse a dependency check result
parse_dep_check() {
    local result="$1"
    local -n status_ref=$2
    local -n name_ref=$3
    local -n version_ref=$4
    local -n recs_ref=$5
    
    IFS='|' read -r status_ref name_ref version_ref recs_ref <<< "$result"
}

# Display dependency check results
display_dep_results() {
    local -a check_results=("$@")
    local ready_count=0
    local total_count=${#check_results[@]}
    
    echo
    log_info "Dependency status:"
    
    # Display status for each dependency
    for result in "${check_results[@]:-}"; do
        local status name version recs
        parse_dep_check "$result" status name version recs
        
        echo "$status $name ($version)"
        [ "$status" = "✅" ] && ((ready_count++))
    done
    
    echo
    echo "📋 Requirements Summary:"
    
    for result in "${check_results[@]:-}"; do
        local status name version recs
        parse_dep_check "$result" status name version recs
        
        local status_text
        if [ "$status" = "✅" ]; then
            status_text="Ready"
        else
            status_text="Missing"
        fi
        
        echo "- $name: $status $(echo $status_text)"
    done
    
    echo
    
    # Show fix recommendations for missing tools
    for result in "${check_results[@]:-}"; do
        local status name version recs
        parse_dep_check "$result" status name version recs
        
        if [[ "$status" = "❌" && -n "$recs" ]]; then
            echo "🔧 Fix $name:"
            IFS='|' read -ra rec_array <<< "$recs"
            for rec in "${rec_array[@]:-}"; do
                [ -n "$rec" ] && echo "   $rec"
            done
            echo
        fi
    done
    
    # Final status
    if [ "$ready_count" -eq "$total_count" ]; then
        log_success "All requirements met! ($ready_count/$total_count)"
        return 0
    else
        log_warning "Requirements incomplete: $ready_count/$total_count tools ready"
        return 1
    fi
}

# Main requirements checking command
requirements_command() {
    log_info "Checking empack requirements..."
    
    # Run all dependency checks
    local packwiz_check=$(check_packwiz)
    local tomlq_check=$(check_tomlq)
    local mrpack_check=$(check_mrpack_install)
    local java_check=$(check_java)
    local jq_check=$(check_jq)
    local xq_check=$(check_xq)
    local git_check=$(check_git)
    
    # Display results
    display_dep_results "$packwiz_check" "$tomlq_check" "$mrpack_check" "$java_check" "$jq_check" "$xq_check" "$git_check"
}

# Validate dependencies required for build operations
validate_build_dependencies() {
    log_debug "Validating build dependencies..."
    
    local missing_deps=()
    
    # Check critical build dependencies
    if ! command_exists packwiz; then
        missing_deps+=("packwiz")
    fi
    
    # Check for tomlq or tq
    if ! command_exists tq && ! command_exists tomlq; then
        missing_deps+=("tomlq")
    fi
    
    if ! command_exists mrpack-install; then
        missing_deps+=("mrpack-install")
    fi
    
    if ! command_exists java; then
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

# Quick dependency check (for init command)
quick_dependency_check() {
    local missing=()
    
    command_exists packwiz || missing+=("packwiz")
    (command_exists tq || command_exists tomlq) || missing+=("tomlq")
    command_exists mrpack-install || missing+=("mrpack-install")
    command_exists java || missing+=("java")
    
    if [ ${#missing[@]} -gt 0 ]; then
        return 1
    fi
    return 0
}

# Export dependency functions
export -f check_packwiz check_tomlq check_mrpack_install check_java check_jq check_xq check_git jq_available xq_available
export -f requirements_command validate_build_dependencies quick_dependency_check