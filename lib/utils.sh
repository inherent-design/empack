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
    
    local latest_release
    latest_release=$(curl -s "https://launchermeta.mojang.com/mc/game/version_manifest.json" 2>/dev/null | \
                    grep -o '"release":"[^"]*"' | head -1 | cut -d'"' -f4)
    
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
    
    # Extract all versions and find stable (non-beta) and latest
    local all_versions
    all_versions=$(echo "$maven_xml" | grep -o '<version>[^<]*</version>' | sed 's/<[^>]*>//g' | sort -V -r)
    
    # Latest stable (first non-beta version)
    local stable_version
    stable_version=$(echo "$all_versions" | grep -v -i "beta\|alpha\|rc" | head -1)
    
    # Latest overall (first version)
    local latest_version
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
    
    # Extract stable version (stable=true)
    local stable_version
    stable_version=$(echo "$fabric_json" | grep -B 2 -A 2 '"stable":true' | grep '"version"' | head -1 | sed 's/.*"version":"\([^"]*\)".*/\1/')
    
    # Extract latest version (first entry)
    local latest_version
    latest_version=$(echo "$fabric_json" | grep '"version"' | head -1 | sed 's/.*"version":"\([^"]*\)".*/\1/')
    
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
    
    # Extract all versions and find stable (non-beta) and latest
    local all_versions
    all_versions=$(echo "$quilt_json" | grep '"version"' | sed 's/.*"version":"\([^"]*\)".*/\1/')
    
    # Latest stable (first non-beta version)
    local stable_version
    stable_version=$(echo "$all_versions" | grep -v -i "beta\|alpha\|rc" | head -1)
    
    # Latest overall (first version)
    local latest_version
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

# Export utility functions
export -f ensure_directory command_exists get_command_version download_file
export -f file_recently_modified create_temp_directory cleanup_temp_directory
export -f find_command extract_archive is_git_repository
export -f get_minecraft_latest_stable get_neoforge_versions get_fabric_versions get_quilt_versions version_greater_than