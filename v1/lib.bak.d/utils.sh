#!/usr/bin/env bash
# Module: utils
# Description: File operations, downloads, and general utilities
# Dependencies: core, logger

# Prevent multiple loading
if [ "${EMPACK_MODULE_UTILS:-}" = "loaded" ]; then
    return 0
fi
readonly EMPACK_MODULE_UTILS="loaded"

#=============================================================================
# FILE AND DIRECTORY UTILITIES
#=============================================================================

# Safe directory creation with logging
ensure_directory() {
    local dir="$1"
    if [ ! -d "$dir" ]; then
        mkdir -p "$dir"
        log_debug "Created directory: $dir"
    fi
}

# Check if a command exists in PATH
command_exists() {
    command -v "$1" >/dev/null 2>&1
}

# Enhanced dependency resolution: PATH -> CWD -> modpack directory
find_dependency() {
    local cmd="$1"
    local path=""
    
    # 1. Check PATH first (standard system installation)
    if command_exists "$cmd"; then
        path=$(command -v "$cmd")
        echo "$path"
        return 0
    fi
    
    # 2. Check current working directory
    if [ -x "./$cmd" ]; then
        path="$(pwd)/$cmd"
        echo "$path"
        return 0
    fi
    
    # 3. Check modpack directory (for --modpack-directory isolation)
    if [ -n "${EMPACK_TARGET_DIR:-}" ] && [ "$EMPACK_TARGET_DIR" != "." ]; then
        if [ -x "$EMPACK_TARGET_DIR/$cmd" ]; then
            path="$EMPACK_TARGET_DIR/$cmd"
            echo "$path"
            return 0
        fi
    fi
    
    # Not found in any location
    return 1
}

# Get resolved dependency path (for display/debugging)
resolve_dependency_path() {
    local cmd="$1"
    local path
    
    path=$(find_dependency "$cmd")
    if [ $? -eq 0 ]; then
        echo "$path"
        return 0
    else
        echo "not found"
        return 1
    fi
}

# Get command version using various common patterns
get_command_version() {
    local cmd="$1"
    local version_arg="${2:---version}"
    
    if command_exists "$cmd"; then
        "$cmd" "$version_arg" 2>/dev/null | head -n 1 || echo "unknown"
    else
        echo "not found"
    fi
}

# Robust file downloading with retries and progress indication
download_file() {
    local url="$1"
    local output="$2"
    local retries="${3:-3}"
    local delay="${4:-2}"
    
    log_download "Downloading $(basename "$output")..."
    
    for i in $(seq 1 "$retries"); do
        if curl -L -o "$output" "$url" 2>/dev/null; then
            log_success "Downloaded $(basename "$output")"
            return 0
        else
            if [ "$i" -lt "$retries" ]; then
                log_warning "Download attempt $i failed, retrying in ${delay}s..."
                sleep "$delay"
            fi
        fi
    done
    
    log_error "Failed to download $url after $retries attempts"
    return 1
}

# Check if file was recently modified (for caching purposes)
file_recently_modified() {
    local file="$1"
    local max_age_minutes="${2:-60}"
    
    if [ ! -f "$file" ]; then
        return 1
    fi
    
    # Get file modification time and current time
    local file_time=$(stat -f "%m" "$file" 2>/dev/null || stat -c "%Y" "$file" 2>/dev/null)
    local current_time=$(date +%s)
    local age_seconds=$((current_time - file_time))
    local max_age_seconds=$((max_age_minutes * 60))
    
    [ "$age_seconds" -lt "$max_age_seconds" ]
}

# Safely create temporary directory
create_temp_directory() {
    local prefix="${1:-empack}"
    local temp_dir
    
    temp_dir=$(mktemp -d -t "${prefix}.XXXXXX") || {
        log_error "Failed to create temporary directory"
        return 1
    }
    
    log_debug "Created temporary directory: $temp_dir"
    echo "$temp_dir"
}

# Cleanup temporary directory
cleanup_temp_directory() {
    local temp_dir="$1"
    
    if [ -n "$temp_dir" ] && [ -d "$temp_dir" ]; then
        rm -rf "$temp_dir"
        log_debug "Cleaned up temporary directory: $temp_dir"
    fi
}

# Find command with fallback options
find_command() {
    local primary="$1"
    shift
    local fallbacks=("$@")
    
    if command_exists "$primary"; then
        echo "$primary"
        return 0
    fi
    
    for fallback in "${fallbacks[@]:-}"; do
        if command_exists "$fallback"; then
            echo "$fallback"
            return 0
        fi
    done
    
    return 1
}

# Extract archive files
extract_archive() {
    local archive="$1"
    local dest_dir="$2"
    
    ensure_directory "$dest_dir"
    
    case "$archive" in
        *.zip)
            if command_exists unzip; then
                unzip -q "$archive" -d "$dest_dir"
            else
                log_error "unzip command not found, cannot extract $archive"
                return 1
            fi
            ;;
        *.tar.gz|*.tgz)
            tar -xzf "$archive" -C "$dest_dir"
            ;;
        *.tar)
            tar -xf "$archive" -C "$dest_dir"
            ;;
        *)
            log_error "Unsupported archive format: $archive"
            return 1
            ;;
    esac
}

# Validate that we're in a git repository (optional check)
is_git_repository() {
    git rev-parse --git-dir >/dev/null 2>&1
}

#=============================================================================
# API INTEGRATION UTILITIES
#=============================================================================

# Fetch latest stable Minecraft version from Mojang API
get_minecraft_latest_stable() {
    if ! command_exists curl; then
        echo "1.21.4"  # Fallback
        return 1
    fi
    
    local manifest_json
    manifest_json=$(curl -s "https://launchermeta.mojang.com/mc/game/version_manifest.json" 2>/dev/null)
    
    if [ -z "$manifest_json" ]; then
        echo "1.21.4"  # Fallback
        return 1
    fi
    
    # Use jq for robust JSON parsing (hard dependency)
    local latest_release
    local jq_path
    jq_path=$(find_dependency jq)
    
    if [ $? -ne 0 ] || [ -z "$jq_path" ]; then
        echo "1.21.4"  # Fallback
        log_error "jq not found - required for API parsing"
        return 1
    fi
    
    latest_release=$(echo "$manifest_json" | "$jq_path" -r '.latest.release' 2>/dev/null)
    
    if [ -n "$latest_release" ]; then
        echo "$latest_release"
        return 0
    else
        echo "1.21.4"  # Fallback
        return 1
    fi
}

# Fetch NeoForge versions (stable and latest) 
get_neoforge_versions() {
    if ! command_exists curl; then
        echo "21.1.174"  # Stable fallback
        echo "21.1.174"  # Latest fallback (same as stable)
        return 1
    fi
    
    local maven_xml
    maven_xml=$(curl -s "https://maven.neoforged.net/releases/net/neoforged/neoforge/maven-metadata.xml" 2>/dev/null)
    
    if [ -z "$maven_xml" ]; then
        echo "21.1.174"  # Stable fallback
        echo "21.1.174"  # Latest fallback
        return 1
    fi
    
    # Use xq for robust XML parsing (hard dependency)
    local stable_version latest_version
    local xq_path
    xq_path=$(find_dependency xq)
    
    if [ $? -ne 0 ] || [ -z "$xq_path" ]; then
        echo "21.1.174"  # Fallback
        echo "21.1.174"  # Fallback
        log_error "xq not found - required for XML parsing"
        return 1
    fi
    
    # Extract all versions using xq, then filter for stable/latest
    local all_versions
    all_versions=$(echo "$maven_xml" | "$xq_path" -r '.metadata.versioning.versions.version[]' 2>/dev/null | sort -V -r)
    
    # Latest stable (first non-beta version)
    stable_version=$(echo "$all_versions" | grep -v -i "beta\|alpha\|rc" | head -1)
    
    # Latest overall (first version)
    latest_version=$(echo "$all_versions" | head -1)
    
    echo "${stable_version:-21.1.174}"
    echo "${latest_version:-21.1.174}"
}

# Fetch Fabric versions (stable and latest)
get_fabric_versions() {
    if ! command_exists curl; then
        echo "0.16.14"  # Stable fallback
        echo "0.16.14"  # Latest fallback
        return 1
    fi
    
    local fabric_json
    fabric_json=$(curl -s "https://meta.fabricmc.net/v2/versions/loader" 2>/dev/null)
    
    if [ -z "$fabric_json" ]; then
        echo "0.16.14"  # Stable fallback
        echo "0.16.14"  # Latest fallback
        return 1
    fi
    
    # Use jq for robust JSON parsing (hard dependency)
    local stable_version latest_version
    local jq_path
    jq_path=$(find_dependency jq)
    
    if [ $? -ne 0 ] || [ -z "$jq_path" ]; then
        echo "0.16.14"  # Fallback
        echo "0.16.14"  # Fallback
        log_error "jq not found - required for API parsing"
        return 1
    fi
    
    stable_version=$(echo "$fabric_json" | "$jq_path" -r '.[] | select(.stable == true) | .version' 2>/dev/null | head -1)
    latest_version=$(echo "$fabric_json" | "$jq_path" -r '.[0].version' 2>/dev/null)
    
    echo "${stable_version:-0.16.14}"
    echo "${latest_version:-0.16.14}"
}

# Fetch Quilt versions (stable and latest)
get_quilt_versions() {
    if ! command_exists curl; then
        echo "0.27.0"   # Stable fallback
        echo "0.27.0"   # Latest fallback
        return 1
    fi
    
    local quilt_json
    quilt_json=$(curl -s "https://meta.quiltmc.org/v3/versions/loader" 2>/dev/null)
    
    if [ -z "$quilt_json" ]; then
        echo "0.27.0"   # Stable fallback
        echo "0.27.0"   # Latest fallback
        return 1
    fi
    
    # Use jq for robust JSON parsing (hard dependency)
    local stable_version latest_version
    local jq_path
    jq_path=$(find_dependency jq)
    
    if [ $? -ne 0 ] || [ -z "$jq_path" ]; then
        echo "0.27.0"   # Fallback
        echo "0.27.0"   # Fallback
        log_error "jq not found - required for API parsing"
        return 1
    fi
    
    # Get all versions, then filter for stable (non-beta) and latest
    local all_versions
    all_versions=$(echo "$quilt_json" | "$jq_path" -r '.[].version' 2>/dev/null)
    
    # Latest stable (first non-beta version)
    stable_version=$(echo "$all_versions" | grep -v -i "beta\|alpha\|rc" | head -1)
    
    # Latest overall (first version)
    latest_version=$(echo "$all_versions" | head -1)
    
    echo "${stable_version:-0.27.0}"
    echo "${latest_version:-0.27.0}"
}

# Compare versions (returns 0 if v1 > v2, 1 if v1 <= v2)
version_greater_than() {
    local v1="$1"
    local v2="$2"
    
    # Use sort -V to compare versions
    local greater
    greater=$(printf '%s\n%s\n' "$v1" "$v2" | sort -V -r | head -1)
    
    [ "$greater" = "$v1" ] && [ "$v1" != "$v2" ]
}

# Execute a command or function in the pack directory with proper error handling
run_in_pack() {
    local pack_dir="$EMPACK_TARGET_DIR/pack"
    
    # Ensure pack directory exists
    if [ ! -d "$pack_dir" ]; then
        log_error "Pack directory not found: $pack_dir"
        return 1
    fi
    
    # Change to pack directory
    pushd "$pack_dir" > /dev/null || {
        log_error "Failed to enter pack directory: $pack_dir"
        return 1
    }
    
    # Execute the command/function with all arguments
    "$@"
    local exit_code=$?
    
    # Always return to original directory
    popd > /dev/null
    
    return $exit_code
}

# Export utility functions
export -f ensure_directory command_exists find_dependency resolve_dependency_path get_command_version download_file
export -f file_recently_modified create_temp_directory cleanup_temp_directory
export -f find_command extract_archive is_git_repository run_in_pack
export -f get_minecraft_latest_stable get_neoforge_versions get_fabric_versions get_quilt_versions version_greater_than