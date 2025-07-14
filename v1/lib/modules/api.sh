#!/usr/bin/env bash
# Module: api
# Description: Clean API integration for version resolution with deduplicated helpers
# Dependencies: core, logger, utils, deps

# Prevent multiple loading
if [ "${EMPACK_MODULE_API:-}" = "loaded" ]; then
    return 0
fi
readonly EMPACK_MODULE_API="loaded"

#=============================================================================
# API STATE-BASED ARCHITECTURE (Clean Data Flow)
#=============================================================================

#=============================================================================
# API STATE VARIABLES
#=============================================================================

# API state (EMPACK_API_* namespace)
declare -g EMPACK_API_CALL_STATUS=""
declare -g EMPACK_API_ERROR_MESSAGE=""
declare -g EMPACK_API_LAST_URL_FETCHED=""
declare -g EMPACK_API_MINECRAFT_LATEST_VERSION=""
declare -g EMPACK_API_MINECRAFT_ALL_VERSIONS=""
declare -g EMPACK_API_NEOFORGE_LATEST_VERSION=""
declare -g EMPACK_API_NEOFORGE_STABLE_VERSION=""
declare -g EMPACK_API_NEOFORGE_ALL_VERSIONS=""
declare -g EMPACK_API_FABRIC_LATEST_VERSION=""
declare -g EMPACK_API_FABRIC_STABLE_VERSION=""
declare -g EMPACK_API_FABRIC_ALL_VERSIONS=""
declare -g EMPACK_API_QUILT_LATEST_VERSION=""
declare -g EMPACK_API_QUILT_STABLE_VERSION=""
declare -g EMPACK_API_QUILT_ALL_VERSIONS=""
declare -g EMPACK_API_FORGE_LATEST_VERSION=""
declare -g EMPACK_API_FORGE_STABLE_VERSION=""
declare -g EMPACK_API_FORGE_ALL_VERSIONS=""
declare -g EMPACK_API_RESOLVED_MODLOADER=""
declare -g EMPACK_API_RESOLVED_MODLOADER_VERSION=""
declare -g EMPACK_API_RESOLVED_MINECRAFT_VERSION=""

# Clear API state
clear_api_state() {
    EMPACK_API_CALL_STATUS=""
    EMPACK_API_ERROR_MESSAGE=""
    EMPACK_API_LAST_URL_FETCHED=""
    EMPACK_API_MINECRAFT_LATEST_VERSION=""
    EMPACK_API_MINECRAFT_ALL_VERSIONS=""
    EMPACK_API_NEOFORGE_LATEST_VERSION=""
    EMPACK_API_NEOFORGE_STABLE_VERSION=""
    EMPACK_API_NEOFORGE_ALL_VERSIONS=""
    EMPACK_API_FABRIC_LATEST_VERSION=""
    EMPACK_API_FABRIC_STABLE_VERSION=""
    EMPACK_API_FABRIC_ALL_VERSIONS=""
    EMPACK_API_QUILT_LATEST_VERSION=""
    EMPACK_API_QUILT_STABLE_VERSION=""
    EMPACK_API_QUILT_ALL_VERSIONS=""
    EMPACK_API_FORGE_LATEST_VERSION=""
    EMPACK_API_FORGE_STABLE_VERSION=""
    EMPACK_API_FORGE_ALL_VERSIONS=""
    EMPACK_API_RESOLVED_MODLOADER=""
    EMPACK_API_RESOLVED_MODLOADER_VERSION=""
    EMPACK_API_RESOLVED_MINECRAFT_VERSION=""
    log_debug "API state cleared"
}

#=============================================================================
# GENERIC API UTILITIES (Deduplicated)
#=============================================================================

# Generic JSON API fetcher with error handling and state tracking
fetch_json_api() {
    local url="$1"
    local description="$2"
    local jq_query="$3"

    log_debug "Fetching $description from $url"
    EMPACK_API_LAST_URL_FETCHED="$url"

    # Check dependencies first using state management
    if ! jq_available; then
        EMPACK_API_CALL_STATUS="error"
        EMPACK_API_ERROR_MESSAGE="jq not available for JSON parsing"
        log_error "$EMPACK_API_ERROR_MESSAGE"
        return 1
    fi

    local response
    if ! response=$(curl -s "$url"); then
        EMPACK_API_CALL_STATUS="error"
        EMPACK_API_ERROR_MESSAGE="Failed to fetch $description from $url"
        log_error "$EMPACK_API_ERROR_MESSAGE"
        return 1
    fi

    local result
    if ! result=$(echo "$response" | jq -r "$jq_query"); then
        EMPACK_API_CALL_STATUS="error"
        EMPACK_API_ERROR_MESSAGE="Failed to parse $description with jq query: $jq_query"
        log_error "$EMPACK_API_ERROR_MESSAGE"
        return 1
    fi

    if [ -z "$result" ] || [ "$result" = "null" ]; then
        EMPACK_API_CALL_STATUS="error"
        EMPACK_API_ERROR_MESSAGE="No data found in $description"
        log_error "$EMPACK_API_ERROR_MESSAGE"
        return 1
    fi

    EMPACK_API_CALL_STATUS="success"
    echo "$result"
    return 0
}

# Generic XML API fetcher with error handling and state tracking
fetch_xml_api() {
    local url="$1"
    local description="$2"
    local xpath_query="$3"

    log_debug "Fetching $description from $url"
    EMPACK_API_LAST_URL_FETCHED="$url"

    # Check dependencies first using state management
    if ! xq_available; then
        EMPACK_API_CALL_STATUS="error"
        EMPACK_API_ERROR_MESSAGE="xq not available for XML parsing"
        log_error "$EMPACK_API_ERROR_MESSAGE"
        return 1
    fi

    local response
    if ! response=$(curl -s "$url"); then
        EMPACK_API_CALL_STATUS="error"
        EMPACK_API_ERROR_MESSAGE="Failed to fetch $description from $url"
        log_error "$EMPACK_API_ERROR_MESSAGE"
        return 1
    fi

    local result
    if ! result=$(echo "$response" | xq -x "$xpath_query"); then
        EMPACK_API_CALL_STATUS="error"
        EMPACK_API_ERROR_MESSAGE="Failed to parse $description with xpath query: $xpath_query"
        log_error "$EMPACK_API_ERROR_MESSAGE"
        return 1
    fi

    if [ -z "$result" ]; then
        EMPACK_API_CALL_STATUS="error"
        EMPACK_API_ERROR_MESSAGE="No data found in $description"
        log_error "$EMPACK_API_ERROR_MESSAGE"
        return 1
    fi

    EMPACK_API_CALL_STATUS="success"
    echo "$result"
    return 0
}

#=============================================================================
# MINECRAFT VERSION API (State-Based)
#=============================================================================

# Get latest stable Minecraft release version (sets EMPACK_API_MINECRAFT_LATEST_VERSION)
get_latest_minecraft_version() {
    log_debug "Fetching latest Minecraft version"

    local result
    result=$(fetch_json_api \
        "https://launchermeta.mojang.com/mc/game/version_manifest.json" \
        "Minecraft version manifest" \
        ".latest.release")

    if [ $? -eq 0 ] && [ -n "$result" ]; then
        EMPACK_API_MINECRAFT_LATEST_VERSION="$result"
        EMPACK_API_CALL_STATUS="success"
        log_debug "Latest Minecraft version: $EMPACK_API_MINECRAFT_LATEST_VERSION"
        return 0
    else
        EMPACK_API_CALL_STATUS="error"
        EMPACK_API_ERROR_MESSAGE="Failed to fetch latest Minecraft version"
        log_error "$EMPACK_API_ERROR_MESSAGE"
        return 1
    fi
}

# Get all stable Minecraft release versions (sets EMPACK_API_MINECRAFT_ALL_VERSIONS)
get_all_minecraft_versions() {
    log_debug "Fetching all Minecraft versions"

    local result
    result=$(fetch_json_api \
        "https://launchermeta.mojang.com/mc/game/version_manifest.json" \
        "Minecraft version list" \
        '.versions[] | select(.type == "release") | .id')

    if [ $? -eq 0 ] && [ -n "$result" ]; then
        EMPACK_API_MINECRAFT_ALL_VERSIONS="$result"
        EMPACK_API_CALL_STATUS="success"
        log_debug "Fetched $(echo "$EMPACK_API_MINECRAFT_ALL_VERSIONS" | wc -l) Minecraft versions"
        return 0
    else
        EMPACK_API_CALL_STATUS="error"
        EMPACK_API_ERROR_MESSAGE="Failed to fetch Minecraft versions"
        log_error "$EMPACK_API_ERROR_MESSAGE"
        return 1
    fi
}

#=============================================================================
# NEOFORGE VERSION API (State-Based)
#=============================================================================

# Get latest NeoForge version (sets EMPACK_API_NEOFORGE_LATEST_VERSION)
get_latest_neoforge_version() {
    log_debug "Fetching latest NeoForge version"

    local result
    result=$(fetch_xml_api \
        "https://maven.neoforged.net/releases/net/neoforged/neoforge/maven-metadata.xml" \
        "NeoForge Maven metadata" \
        "//latest")

    if [ $? -eq 0 ] && [ -n "$result" ]; then
        EMPACK_API_NEOFORGE_LATEST_VERSION="$result"
        EMPACK_API_CALL_STATUS="success"
        log_debug "Latest NeoForge version: $EMPACK_API_NEOFORGE_LATEST_VERSION"
        return 0
    else
        EMPACK_API_CALL_STATUS="error"
        EMPACK_API_ERROR_MESSAGE="Failed to fetch latest NeoForge version"
        log_error "$EMPACK_API_ERROR_MESSAGE"
        return 1
    fi
}

# Get stable NeoForge version (sets EMPACK_API_NEOFORGE_STABLE_VERSION)
get_stable_neoforge_version() {
    log_debug "Fetching stable NeoForge version"

    # First get all versions
    if ! get_all_neoforge_versions; then
        return 1
    fi

    local stable_version
    stable_version=$(echo "$EMPACK_API_NEOFORGE_ALL_VERSIONS" | grep -v -E '(beta|alpha|rc)' | tail -1)

    if [ -z "$stable_version" ]; then
        log_warning "No stable NeoForge version found, using latest"
        if get_latest_neoforge_version; then
            EMPACK_API_NEOFORGE_STABLE_VERSION="$EMPACK_API_NEOFORGE_LATEST_VERSION"
            return 0
        else
            return 1
        fi
    fi

    EMPACK_API_NEOFORGE_STABLE_VERSION="$stable_version"
    EMPACK_API_CALL_STATUS="success"
    log_debug "Stable NeoForge version: $EMPACK_API_NEOFORGE_STABLE_VERSION"
    return 0
}

# Get all NeoForge versions (sets EMPACK_API_NEOFORGE_ALL_VERSIONS)
get_all_neoforge_versions() {
    log_debug "Fetching all NeoForge versions"

    local result
    result=$(fetch_xml_api \
        "https://maven.neoforged.net/releases/net/neoforged/neoforge/maven-metadata.xml" \
        "NeoForge version list" \
        "//version")

    if [ $? -eq 0 ] && [ -n "$result" ]; then
        EMPACK_API_NEOFORGE_ALL_VERSIONS="$result"
        EMPACK_API_CALL_STATUS="success"
        log_debug "Fetched $(echo "$EMPACK_API_NEOFORGE_ALL_VERSIONS" | wc -l) NeoForge versions"
        return 0
    else
        EMPACK_API_CALL_STATUS="error"
        EMPACK_API_ERROR_MESSAGE="Failed to fetch NeoForge versions"
        log_error "$EMPACK_API_ERROR_MESSAGE"
        return 1
    fi
}

#=============================================================================
# FABRIC VERSION API (State-Based)
#=============================================================================

# Get latest Fabric Loader version (sets EMPACK_API_FABRIC_LATEST_VERSION)
get_latest_fabric_version() {
    log_debug "Fetching latest Fabric version"

    local result
    result=$(fetch_json_api \
        "https://meta.fabricmc.net/v2/versions/loader" \
        "Fabric Loader versions" \
        ".[0].version")

    if [ $? -eq 0 ] && [ -n "$result" ]; then
        EMPACK_API_FABRIC_LATEST_VERSION="$result"
        EMPACK_API_CALL_STATUS="success"
        log_debug "Latest Fabric version: $EMPACK_API_FABRIC_LATEST_VERSION"
        return 0
    else
        EMPACK_API_CALL_STATUS="error"
        EMPACK_API_ERROR_MESSAGE="Failed to fetch latest Fabric version"
        log_error "$EMPACK_API_ERROR_MESSAGE"
        return 1
    fi
}

# Get stable Fabric Loader version (sets EMPACK_API_FABRIC_STABLE_VERSION)
get_stable_fabric_version() {
    log_debug "Fetching stable Fabric version"

    local stable_version
    stable_version=$(fetch_json_api \
        "https://meta.fabricmc.net/v2/versions/loader" \
        "stable Fabric Loader version" \
        '.[] | select(.stable == true) | .version' | head -1)

    if [ -n "$stable_version" ]; then
        EMPACK_API_FABRIC_STABLE_VERSION="$stable_version"
        EMPACK_API_CALL_STATUS="success"
        log_debug "Stable Fabric version: $EMPACK_API_FABRIC_STABLE_VERSION"
        return 0
    else
        log_warning "No stable Fabric version found, using latest"
        if get_latest_fabric_version; then
            EMPACK_API_FABRIC_STABLE_VERSION="$EMPACK_API_FABRIC_LATEST_VERSION"
            return 0
        else
            return 1
        fi
    fi
}

# Get all Fabric Loader versions (sets EMPACK_API_FABRIC_ALL_VERSIONS)
get_all_fabric_versions() {
    log_debug "Fetching all Fabric versions"

    local result
    result=$(fetch_json_api \
        "https://meta.fabricmc.net/v2/versions/loader" \
        "Fabric Loader version list" \
        ".[].version")

    if [ $? -eq 0 ] && [ -n "$result" ]; then
        EMPACK_API_FABRIC_ALL_VERSIONS="$result"
        EMPACK_API_CALL_STATUS="success"
        log_debug "Fetched $(echo "$EMPACK_API_FABRIC_ALL_VERSIONS" | wc -l) Fabric versions"
        return 0
    else
        EMPACK_API_CALL_STATUS="error"
        EMPACK_API_ERROR_MESSAGE="Failed to fetch Fabric versions"
        log_error "$EMPACK_API_ERROR_MESSAGE"
        return 1
    fi
}

#=============================================================================
# QUILT VERSION API (State-Based)
#=============================================================================

# Get latest Quilt Loader version (sets EMPACK_API_QUILT_LATEST_VERSION)
get_latest_quilt_version() {
    log_debug "Fetching latest Quilt version"

    local result
    result=$(fetch_json_api \
        "https://meta.quiltmc.org/v3/versions/loader" \
        "Quilt Loader versions" \
        ".[0].version")

    if [ $? -eq 0 ] && [ -n "$result" ]; then
        EMPACK_API_QUILT_LATEST_VERSION="$result"
        EMPACK_API_CALL_STATUS="success"
        log_debug "Latest Quilt version: $EMPACK_API_QUILT_LATEST_VERSION"
        return 0
    else
        EMPACK_API_CALL_STATUS="error"
        EMPACK_API_ERROR_MESSAGE="Failed to fetch latest Quilt version"
        log_error "$EMPACK_API_ERROR_MESSAGE"
        return 1
    fi
}

# Get stable Quilt Loader version (sets EMPACK_API_QUILT_STABLE_VERSION)
get_stable_quilt_version() {
    log_debug "Fetching stable Quilt version"

    # First get all versions
    if ! get_all_quilt_versions; then
        return 1
    fi

    local stable_version
    stable_version=$(echo "$EMPACK_API_QUILT_ALL_VERSIONS" | grep -v -E '(beta|alpha|rc)' | head -1)

    if [ -z "$stable_version" ]; then
        log_warning "No stable Quilt version found, using latest"
        if get_latest_quilt_version; then
            EMPACK_API_QUILT_STABLE_VERSION="$EMPACK_API_QUILT_LATEST_VERSION"
            return 0
        else
            return 1
        fi
    fi

    EMPACK_API_QUILT_STABLE_VERSION="$stable_version"
    EMPACK_API_CALL_STATUS="success"
    log_debug "Stable Quilt version: $EMPACK_API_QUILT_STABLE_VERSION"
    return 0
}

# Get all Quilt Loader versions (sets EMPACK_API_QUILT_ALL_VERSIONS)
get_all_quilt_versions() {
    log_debug "Fetching all Quilt versions"

    local result
    result=$(fetch_json_api \
        "https://meta.quiltmc.org/v3/versions/loader" \
        "Quilt Loader version list" \
        ".[].version")

    if [ $? -eq 0 ] && [ -n "$result" ]; then
        EMPACK_API_QUILT_ALL_VERSIONS="$result"
        EMPACK_API_CALL_STATUS="success"
        log_debug "Fetched $(echo "$EMPACK_API_QUILT_ALL_VERSIONS" | wc -l) Quilt versions"
        return 0
    else
        EMPACK_API_CALL_STATUS="error"
        EMPACK_API_ERROR_MESSAGE="Failed to fetch Quilt versions"
        log_error "$EMPACK_API_ERROR_MESSAGE"
        return 1
    fi
}

#=============================================================================
# FORGE VERSION API (State-Based)
#=============================================================================

# Get latest Forge version (sets EMPACK_API_FORGE_LATEST_VERSION)
get_latest_forge_version() {
    log_debug "Fetching latest Forge version"

    local result
    result=$(fetch_xml_api \
        "https://maven.minecraftforge.net/net/minecraftforge/forge/maven-metadata.xml" \
        "Forge Maven metadata" \
        "//latest")

    if [ $? -eq 0 ] && [ -n "$result" ]; then
        EMPACK_API_FORGE_LATEST_VERSION="$result"
        EMPACK_API_CALL_STATUS="success"
        log_debug "Latest Forge version: $EMPACK_API_FORGE_LATEST_VERSION"
        return 0
    else
        EMPACK_API_CALL_STATUS="error"
        EMPACK_API_ERROR_MESSAGE="Failed to fetch latest Forge version"
        log_error "$EMPACK_API_ERROR_MESSAGE"
        return 1
    fi
}

# Get stable Forge version (sets EMPACK_API_FORGE_STABLE_VERSION)
get_stable_forge_version() {
    log_debug "Fetching stable Forge version"

    # First get all versions
    if ! get_all_forge_versions; then
        return 1
    fi

    # Filter for stable versions (exclude beta, alpha, rc)
    local stable_version
    stable_version=$(echo "$EMPACK_API_FORGE_ALL_VERSIONS" | grep -v -E '(beta|alpha|rc)' | head -1)

    if [ -z "$stable_version" ]; then
        log_warning "No stable Forge version found, using latest"
        if get_latest_forge_version; then
            EMPACK_API_FORGE_STABLE_VERSION="$EMPACK_API_FORGE_LATEST_VERSION"
            return 0
        else
            return 1
        fi
    fi

    EMPACK_API_FORGE_STABLE_VERSION="$stable_version"
    EMPACK_API_CALL_STATUS="success"
    log_debug "Stable Forge version: $EMPACK_API_FORGE_STABLE_VERSION"
    return 0
}

# Get all Forge versions (sets EMPACK_API_FORGE_ALL_VERSIONS)
get_all_forge_versions() {
    log_debug "Fetching all Forge versions"

    local result
    result=$(fetch_xml_api \
        "https://maven.minecraftforge.net/net/minecraftforge/forge/maven-metadata.xml" \
        "Forge version list" \
        "//version")

    if [ $? -eq 0 ] && [ -n "$result" ]; then
        EMPACK_API_FORGE_ALL_VERSIONS="$result"
        EMPACK_API_CALL_STATUS="success"
        log_debug "Fetched $(echo "$EMPACK_API_FORGE_ALL_VERSIONS" | wc -l) Forge versions"
        return 0
    else
        EMPACK_API_CALL_STATUS="error"
        EMPACK_API_ERROR_MESSAGE="Failed to fetch Forge versions"
        log_error "$EMPACK_API_ERROR_MESSAGE"
        return 1
    fi
}

# Get latest stable Forge version for specific Minecraft version
get_stable_forge_version_for_minecraft() {
    local minecraft_version="$1"
    
    if [ -z "$minecraft_version" ]; then
        log_error "Minecraft version required"
        return 1
    fi

    log_debug "Fetching stable Forge version for Minecraft $minecraft_version"

    # Get all versions first
    if ! get_all_forge_versions; then
        return 1
    fi

    # Filter versions for this Minecraft version and find stable
    local filtered_versions
    filtered_versions=$(echo "$EMPACK_API_FORGE_ALL_VERSIONS" | grep "^$minecraft_version-")
    
    if [ -z "$filtered_versions" ]; then
        log_error "No Forge versions found for Minecraft $minecraft_version"
        return 1
    fi

    # Try to find stable version first (exclude beta, alpha, rc)
    local stable_version
    stable_version=$(echo "$filtered_versions" | grep -v -E '(beta|alpha|rc)' | head -1)

    if [ -z "$stable_version" ]; then
        log_warning "No stable Forge version found for Minecraft $minecraft_version, using latest available"
        stable_version=$(echo "$filtered_versions" | head -1)
    fi

    if [ -z "$stable_version" ]; then
        log_error "No valid Forge version found for Minecraft $minecraft_version"
        return 1
    fi

    echo "$stable_version"
    return 0
}

# Get Minecraft version from Forge version string
get_minecraft_version_from_forge_version() {
    local forge_version="$1"
    
    if [ -z "$forge_version" ]; then
        log_error "Forge version required"
        return 1
    fi

    # Extract MC version from format: "1.21-51.0.33" -> "1.21"
    echo "$forge_version" | cut -d'-' -f1
}

#=============================================================================
# COMPATIBILITY MATRIX API FUNCTIONS
#=============================================================================

# Get latest stable Minecraft version (alias for compatibility.sh)
get_latest_stable_minecraft() {
    get_latest_minecraft_version
}

# Get latest stable Fabric version (alias for compatibility.sh)
get_latest_stable_fabric() {
    get_stable_fabric_version
}

# Get latest stable Quilt version (alias for compatibility.sh)
get_latest_stable_quilt() {
    get_stable_quilt_version
}

# Get latest stable Forge version (alias for compatibility.sh)
get_latest_stable_forge() {
    get_stable_forge_version
}

# Get Minecraft versions supported by specific NeoForge version
get_minecraft_versions_for_neoforge() {
    local neoforge_version="$1"
    if [ -z "$neoforge_version" ]; then
        log_error "NeoForge version required"
        return 1
    fi

    # For now, use the same logic as get_minecraft_version_for_neoforge_version
    # but return it as a list (single item for now)
    get_minecraft_version_for_neoforge_version "$neoforge_version"
}

# Get NeoForge versions that support specific Minecraft version
get_neoforge_versions_for_minecraft() {
    local minecraft_version="$1"
    if [ -z "$minecraft_version" ]; then
        log_error "Minecraft version required"
        return 1
    fi

    log_debug "Getting NeoForge versions for Minecraft $minecraft_version"

    # Get all NeoForge versions and filter by heuristic compatibility
    local all_versions
    if ! all_versions=$(get_all_neoforge_versions); then
        log_error "Failed to fetch NeoForge versions"
        return 1
    fi

    # Filter based on MC version compatibility heuristics
    case "$minecraft_version" in
    "1.21" | "1.21."*)
        echo "$all_versions" | grep "^21\."
        ;;
    "1.20" | "1.20."*)
        echo "$all_versions" | grep "^20\."
        ;;
    *)
        log_warning "Unknown compatibility for Minecraft $minecraft_version"
        echo "$all_versions" | head -5 # Return some recent versions
        ;;
    esac
}

# Get Minecraft versions supported by specific Fabric version
get_minecraft_versions_for_fabric() {
    local fabric_version="$1"
    if [ -z "$fabric_version" ]; then
        log_error "Fabric version required"
        return 1
    fi

    # Fabric generally supports most recent Minecraft versions
    # For simplicity, return the latest few Minecraft versions
    get_all_minecraft_versions | head -5
}

# Get Fabric versions that support specific Minecraft version
get_fabric_versions_for_minecraft() {
    local minecraft_version="$1"
    if [ -z "$minecraft_version" ]; then
        log_error "Minecraft version required"
        return 1
    fi

    # Fabric generally supports most Minecraft versions with latest loader
    # Return recent Fabric versions
    get_all_fabric_versions | head -5
}

# Get Minecraft versions supported by specific Quilt version
get_minecraft_versions_for_quilt() {
    local quilt_version="$1"
    if [ -z "$quilt_version" ]; then
        log_error "Quilt version required"
        return 1
    fi

    # Quilt generally supports most recent Minecraft versions
    # For simplicity, return the latest few Minecraft versions
    get_all_minecraft_versions | head -5
}

# Get Quilt versions that support specific Minecraft version
get_quilt_versions_for_minecraft() {
    local minecraft_version="$1"
    if [ -z "$minecraft_version" ]; then
        log_error "Minecraft version required"
        return 1
    fi

    # Quilt generally supports most Minecraft versions with latest loader
    # Return recent Quilt versions
    get_all_quilt_versions | head -5
}

# Get latest Minecraft version compatible with Fabric
get_latest_minecraft_for_fabric() {
    local fabric_version="${1:-}"

    # Fabric generally supports latest Minecraft quickly
    get_latest_minecraft_version
}

# Get latest Minecraft version compatible with Quilt
get_latest_minecraft_for_quilt() {
    local quilt_version="${1:-}"

    # Quilt generally supports latest Minecraft quickly
    get_latest_minecraft_version
}

# Get Minecraft versions supported by specific Forge version
get_minecraft_versions_for_forge() {
    local forge_version="$1"
    if [ -z "$forge_version" ]; then
        log_error "Forge version required"
        return 1
    fi

    # Extract MC version from Forge version format: "1.21-51.0.33" -> "1.21"
    get_minecraft_version_from_forge_version "$forge_version"
}

# Get Forge versions that support specific Minecraft version
get_forge_versions_for_minecraft() {
    local minecraft_version="$1"
    if [ -z "$minecraft_version" ]; then
        log_error "Minecraft version required"
        return 1
    fi

    log_debug "Getting Forge versions for Minecraft $minecraft_version"

    # Get all Forge versions and filter by Minecraft version
    if ! get_all_forge_versions; then
        log_error "Failed to fetch Forge versions"
        return 1
    fi

    # Filter versions for this Minecraft version
    echo "$EMPACK_API_FORGE_ALL_VERSIONS" | grep "^$minecraft_version-"
}

# Get latest Minecraft version compatible with Forge
get_latest_minecraft_for_forge() {
    local forge_version="${1:-}"

    # If no specific forge version provided, get latest and extract MC version
    if [ -z "$forge_version" ]; then
        if get_latest_forge_version; then
            get_minecraft_version_from_forge_version "$EMPACK_API_FORGE_LATEST_VERSION"
        else
            return 1
        fi
    else
        get_minecraft_version_from_forge_version "$forge_version"
    fi
}

#=============================================================================
# GOLDEN PATH DEFAULT CHAIN RESOLUTION
#=============================================================================

# Get latest stable NeoForge version (alias for golden path clarity)
get_latest_stable_neoforge_version() {
    get_stable_neoforge_version
}

# Get Minecraft version compatible with specific NeoForge version
get_minecraft_version_for_neoforge_version() {
    local neoforge_version="$1"

    if [ -z "$neoforge_version" ]; then
        log_error "NeoForge version required"
        return 1
    fi

    log_debug "Determining Minecraft version for NeoForge $neoforge_version"

    # Extract major version from NeoForge version (e.g., 21.1.174 -> 21)
    local major_version
    major_version=$(echo "$neoforge_version" | cut -d'.' -f1)

    log_debug "NeoForge major version extracted: $major_version from $neoforge_version"

    case "$major_version" in
    "21")
        echo "1.21.1" # NeoForge 21.x.x supports MC 1.21.1
        ;;
    "20")
        echo "1.20.1" # NeoForge 20.x.x supports MC 1.20.1
        ;;
    *)
        log_warning "Unknown NeoForge major version: $major_version"
        log_warning "Falling back to latest Minecraft version"
        get_latest_minecraft_version
        ;;
    esac
}

# Complete default chain resolution for golden path
resolve_default_chain() {
    log_info "Resolving intelligent defaults..."

    # Step 1: Default modloader = neoforge
    local modloader="neoforge"
    log_debug "Default modloader: $modloader"

    # Step 2: Get latest stable neoforge version
    if ! get_stable_neoforge_version; then
        log_error "Failed to resolve NeoForge version"
        return 1
    fi
    local modloader_version="$EMPACK_API_NEOFORGE_STABLE_VERSION"
    log_debug "Resolved NeoForge version: $modloader_version"

    # Step 3: Get compatible minecraft version
    local minecraft_version
    if ! minecraft_version=$(get_minecraft_version_for_neoforge_version "$modloader_version"); then
        log_error "Failed to resolve Minecraft version"
        return 1
    fi
    log_debug "Resolved Minecraft version: $minecraft_version"

    # Store in API state variables
    EMPACK_API_RESOLVED_MODLOADER="$modloader"
    EMPACK_API_RESOLVED_MODLOADER_VERSION="$modloader_version"
    EMPACK_API_RESOLVED_MINECRAFT_VERSION="$minecraft_version"

    log_success "Using $modloader $modloader_version + Minecraft $minecraft_version (latest stable)"
    return 0
}

#=============================================================================
# MODULE INTERFACE CONTRACT
#=============================================================================

# Standard module interface - export API state variables
export_api_state() {
    echo "EMPACK_API_CALL_STATUS='$EMPACK_API_CALL_STATUS'"
    echo "EMPACK_API_ERROR_MESSAGE='$EMPACK_API_ERROR_MESSAGE'"
    echo "EMPACK_API_LAST_URL_FETCHED='$EMPACK_API_LAST_URL_FETCHED'"
    echo "EMPACK_API_MINECRAFT_LATEST_VERSION='$EMPACK_API_MINECRAFT_LATEST_VERSION'"
    echo "EMPACK_API_MINECRAFT_ALL_VERSIONS='$EMPACK_API_MINECRAFT_ALL_VERSIONS'"
    echo "EMPACK_API_NEOFORGE_LATEST_VERSION='$EMPACK_API_NEOFORGE_LATEST_VERSION'"
    echo "EMPACK_API_NEOFORGE_STABLE_VERSION='$EMPACK_API_NEOFORGE_STABLE_VERSION'"
    echo "EMPACK_API_NEOFORGE_ALL_VERSIONS='$EMPACK_API_NEOFORGE_ALL_VERSIONS'"
    echo "EMPACK_API_FABRIC_LATEST_VERSION='$EMPACK_API_FABRIC_LATEST_VERSION'"
    echo "EMPACK_API_FABRIC_STABLE_VERSION='$EMPACK_API_FABRIC_STABLE_VERSION'"
    echo "EMPACK_API_FABRIC_ALL_VERSIONS='$EMPACK_API_FABRIC_ALL_VERSIONS'"
    echo "EMPACK_API_QUILT_LATEST_VERSION='$EMPACK_API_QUILT_LATEST_VERSION'"
    echo "EMPACK_API_QUILT_STABLE_VERSION='$EMPACK_API_QUILT_STABLE_VERSION'"
    echo "EMPACK_API_QUILT_ALL_VERSIONS='$EMPACK_API_QUILT_ALL_VERSIONS'"
    echo "EMPACK_API_FORGE_LATEST_VERSION='$EMPACK_API_FORGE_LATEST_VERSION'"
    echo "EMPACK_API_FORGE_STABLE_VERSION='$EMPACK_API_FORGE_STABLE_VERSION'"
    echo "EMPACK_API_FORGE_ALL_VERSIONS='$EMPACK_API_FORGE_ALL_VERSIONS'"
    echo "EMPACK_API_RESOLVED_MODLOADER='$EMPACK_API_RESOLVED_MODLOADER'"
    echo "EMPACK_API_RESOLVED_MODLOADER_VERSION='$EMPACK_API_RESOLVED_MODLOADER_VERSION'"
    echo "EMPACK_API_RESOLVED_MINECRAFT_VERSION='$EMPACK_API_RESOLVED_MINECRAFT_VERSION'"
}

# Get current module status
get_api_status() {
    local status="operational"
    local details=""

    if [ "$EMPACK_API_CALL_STATUS" = "error" ]; then
        status="error"
        details="$EMPACK_API_ERROR_MESSAGE"
    elif [ -n "$EMPACK_API_LAST_URL_FETCHED" ]; then
        status="active"
        details="Last API call: $EMPACK_API_LAST_URL_FETCHED"
    fi

    echo "status=$status"
    echo "call_status=$EMPACK_API_CALL_STATUS"
    echo "last_url=$EMPACK_API_LAST_URL_FETCHED"
    echo "resolved_modloader=$EMPACK_API_RESOLVED_MODLOADER"
    echo "resolved_modloader_version=$EMPACK_API_RESOLVED_MODLOADER_VERSION"
    echo "resolved_minecraft_version=$EMPACK_API_RESOLVED_MINECRAFT_VERSION"
    echo "details=$details"
}

# Validate API module state and configuration
validate_api_state() {
    local validation_passed=true
    local errors=()

    # Check if required utilities are available
    if ! command -v curl >/dev/null 2>&1; then
        errors+=("curl command not available - required for API calls")
        validation_passed=false
    fi

    if ! jq_available; then
        errors+=("jq not available - required for JSON parsing")
        validation_passed=false
    fi

    if ! xq_available; then
        errors+=("xq not available - required for XML parsing")
        validation_passed=false
    fi

    # Check dependency functions are available
    if ! declare -F log_debug >/dev/null 2>&1; then
        errors+=("Function log_debug not available from logger module")
        validation_passed=false
    fi

    if ! declare -F log_error >/dev/null 2>&1; then
        errors+=("Function log_error not available from logger module")
        validation_passed=false
    fi

    if ! declare -F log_success >/dev/null 2>&1; then
        errors+=("Function log_success not available from logger module")
        validation_passed=false
    fi

    echo "validation_passed=$validation_passed"
    if [ ${#errors[@]} -gt 0 ]; then
        echo "errors=${errors[*]}"
    fi

    return $([ "$validation_passed" = true ] && echo 0 || echo 1)
}

# Export API functions (clean utility interface for validation.sh and compatibility.sh)
export -f fetch_json_api fetch_xml_api clear_api_state
export -f get_latest_minecraft_version get_all_minecraft_versions
export -f get_latest_neoforge_version get_stable_neoforge_version get_all_neoforge_versions
export -f get_latest_fabric_version get_stable_fabric_version get_all_fabric_versions
export -f get_latest_quilt_version get_stable_quilt_version get_all_quilt_versions
export -f get_latest_forge_version get_stable_forge_version get_all_forge_versions
export -f get_latest_stable_neoforge_version get_minecraft_version_for_neoforge_version resolve_default_chain
export -f get_stable_forge_version_for_minecraft get_minecraft_version_from_forge_version
# Compatibility matrix functions
export -f get_latest_stable_minecraft get_latest_stable_fabric get_latest_stable_quilt get_latest_stable_forge
export -f get_minecraft_versions_for_neoforge get_neoforge_versions_for_minecraft
export -f get_minecraft_versions_for_fabric get_fabric_versions_for_minecraft
export -f get_minecraft_versions_for_quilt get_quilt_versions_for_minecraft
export -f get_minecraft_versions_for_forge get_forge_versions_for_minecraft
export -f get_latest_minecraft_for_fabric get_latest_minecraft_for_quilt get_latest_minecraft_for_forge
# Module interface contract
export -f export_api_state get_api_status validate_api_state
