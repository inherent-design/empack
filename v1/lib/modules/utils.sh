#!/usr/bin/env bash
# Module: utils
# Description: File operations, downloads, and general utilities with state management
# Dependencies: core, logger

# Prevent multiple loading
if [ "${EMPACK_MODULE_UTILS:-}" = "loaded" ]; then
    return 0
fi
readonly EMPACK_MODULE_UTILS="loaded"

#=============================================================================
# UTILS STATE VARIABLES
#=============================================================================

# Utility state (EMPACK_UTILS_* namespace)
declare -g EMPACK_UTILS_LAST_DOWNLOAD_PATH=""
declare -g EMPACK_UTILS_DEPENDENCY_RESOLUTION_METHOD=""
declare -g EMPACK_UTILS_TEMP_DIRECTORIES=""
declare -g EMPACK_UTILS_ARCHIVE_EXTRACT_COUNT="0"
declare -g EMPACK_UTILS_LAST_COMMAND_FOUND=""

# Clear utils state
clear_utils_state() {
    EMPACK_UTILS_LAST_DOWNLOAD_PATH=""
    EMPACK_UTILS_DEPENDENCY_RESOLUTION_METHOD=""
    EMPACK_UTILS_TEMP_DIRECTORIES=""
    EMPACK_UTILS_ARCHIVE_EXTRACT_COUNT="0"
    EMPACK_UTILS_LAST_COMMAND_FOUND=""
    log_debug "Utils state cleared"
}

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
        EMPACK_UTILS_DEPENDENCY_RESOLUTION_METHOD="PATH"
        EMPACK_UTILS_LAST_COMMAND_FOUND="$cmd"
        echo "$path"
        return 0
    fi
    
    # 2. Check current working directory
    if [ -x "./$cmd" ]; then
        path="$(pwd)/$cmd"
        EMPACK_UTILS_DEPENDENCY_RESOLUTION_METHOD="CWD"
        EMPACK_UTILS_LAST_COMMAND_FOUND="$cmd"
        echo "$path"
        return 0
    fi
    
    # 3. Check modpack directory (for --modpack-directory isolation)
    if [ -n "${EMPACK_CORE_TARGET_DIR:-}" ] && [ "$EMPACK_CORE_TARGET_DIR" != "." ]; then
        if [ -x "$EMPACK_CORE_TARGET_DIR/$cmd" ]; then
            path="$EMPACK_CORE_TARGET_DIR/$cmd"
            EMPACK_UTILS_DEPENDENCY_RESOLUTION_METHOD="MODPACK_DIR"
            EMPACK_UTILS_LAST_COMMAND_FOUND="$cmd"
            echo "$path"
            return 0
        fi
    fi
    
    # Not found in any location
    EMPACK_UTILS_DEPENDENCY_RESOLUTION_METHOD="NOT_FOUND"
    EMPACK_UTILS_LAST_COMMAND_FOUND=""
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
            EMPACK_UTILS_LAST_DOWNLOAD_PATH="$output"
            log_success "Downloaded $(basename "$output")"
            return 0
        else
            if [ "$i" -lt "$retries" ]; then
                log_warning "Download attempt $i failed, retrying in ${delay}s..."
                sleep "$delay"
            fi
        fi
    done
    
    EMPACK_UTILS_LAST_DOWNLOAD_PATH=""
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
    
    # Track temporary directories in state
    if [ -n "$EMPACK_UTILS_TEMP_DIRECTORIES" ]; then
        EMPACK_UTILS_TEMP_DIRECTORIES="$EMPACK_UTILS_TEMP_DIRECTORIES:$temp_dir"
    else
        EMPACK_UTILS_TEMP_DIRECTORIES="$temp_dir"
    fi
    
    log_debug "Created temporary directory: $temp_dir"
    echo "$temp_dir"
}

# Cleanup temporary directory
cleanup_temp_directory() {
    local temp_dir="$1"
    
    if [ -n "$temp_dir" ] && [ -d "$temp_dir" ]; then
        rm -rf "$temp_dir"
        
        # Remove from state tracking
        EMPACK_UTILS_TEMP_DIRECTORIES=$(echo "$EMPACK_UTILS_TEMP_DIRECTORIES" | sed "s|:*$temp_dir:*||g" | sed 's/^://;s/:$//')
        
        log_debug "Cleaned up temporary directory: $temp_dir"
    fi
}

# Cleanup all tracked temporary directories
cleanup_all_temp_directories() {
    if [ -n "$EMPACK_UTILS_TEMP_DIRECTORIES" ]; then
        IFS=':' read -ra temp_dirs <<< "$EMPACK_UTILS_TEMP_DIRECTORIES"
        for temp_dir in "${temp_dirs[@]}"; do
            if [ -d "$temp_dir" ]; then
                rm -rf "$temp_dir"
                log_debug "Cleaned up temporary directory: $temp_dir"
            fi
        done
        EMPACK_UTILS_TEMP_DIRECTORIES=""
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
                local extract_result=$?
            else
                log_error "unzip command not found, cannot extract $archive"
                return 1
            fi
            ;;
        *.tar.gz|*.tgz)
            tar -xzf "$archive" -C "$dest_dir"
            local extract_result=$?
            ;;
        *.tar)
            tar -xf "$archive" -C "$dest_dir"
            local extract_result=$?
            ;;
        *)
            log_error "Unsupported archive format: $archive"
            return 1
            ;;
    esac
    
    if [ "${extract_result:-0}" -eq 0 ]; then
        EMPACK_UTILS_ARCHIVE_EXTRACT_COUNT=$((EMPACK_UTILS_ARCHIVE_EXTRACT_COUNT + 1))
        log_debug "Successfully extracted archive: $archive"
        return 0
    else
        log_error "Failed to extract archive: $archive"
        return 1
    fi
}

# Validate that we're in a git repository (optional check)
is_git_repository() {
    git rev-parse --git-dir >/dev/null 2>&1
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
    local pack_dir="$EMPACK_CORE_TARGET_DIR/pack"
    
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

# Verify file integrity using basic checks
verify_file_integrity() {
    local file="$1"
    local expected_size="${2:-}" # Optional
    
    if [ ! -f "$file" ]; then
        log_error "File does not exist: $file"
        return 1
    fi
    
    if [ ! -r "$file" ]; then
        log_error "File is not readable: $file"
        return 1
    fi
    
    if [ -n "$expected_size" ]; then
        local actual_size=$(stat -f "%z" "$file" 2>/dev/null || stat -c "%s" "$file" 2>/dev/null)
        if [ "$actual_size" != "$expected_size" ]; then
            log_error "File size mismatch: expected $expected_size, got $actual_size"
            return 1
        fi
    fi
    
    log_debug "File integrity verified: $file"
    return 0
}

# Safe file copy with verification
safe_copy_file() {
    local src="$1"
    local dest="$2"
    local verify="${3:-true}"
    
    if [ ! -f "$src" ]; then
        log_error "Source file does not exist: $src"
        return 1
    fi
    
    # Ensure destination directory exists
    local dest_dir=$(dirname "$dest")
    ensure_directory "$dest_dir"
    
    # Copy file
    if cp "$src" "$dest"; then
        log_debug "Copied file: $src → $dest"
        
        # Verify if requested
        if [ "$verify" = "true" ]; then
            local src_size=$(stat -f "%z" "$src" 2>/dev/null || stat -c "%s" "$src" 2>/dev/null)
            if ! verify_file_integrity "$dest" "$src_size"; then
                log_error "File copy verification failed"
                rm -f "$dest"
                return 1
            fi
        fi
        
        return 0
    else
        log_error "Failed to copy file: $src → $dest"
        return 1
    fi
}

# Export utility functions
export -f ensure_directory command_exists find_dependency resolve_dependency_path get_command_version download_file
export -f file_recently_modified create_temp_directory cleanup_temp_directory cleanup_all_temp_directories
export -f find_command extract_archive is_git_repository run_in_pack version_greater_than
export -f verify_file_integrity safe_copy_file clear_utils_state