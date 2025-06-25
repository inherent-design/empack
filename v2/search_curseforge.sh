#!/usr/bin/env bash
# CurseForge Search Module - Clean API for project lookup

# Only set strict mode if not already sourced
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    set -euo pipefail
fi

# Source required modules
source ./empack_reader.sh

# Load environment variables
if [[ -f .env.local ]]; then
    source .env.local
fi

# API base URL
CF_API_BASE="https://api.curseforge.com/v1"

# Verbose mode flag (can be overridden by sourcing script)
CF_VERBOSE=${CF_VERBOSE:-false}

# Pure data function for structured output
# Returns: project_id|project_type|found_title|downloads
cf_get_project_data() {
    local title="$1"
    local project_type="${2:-mod}"
    local minecraft_version="${3:-}"
    local mod_loader="${4:-}"

    # Check if API key is available
    if [[ -z "${CURSEFORGE_API_CLIENT_KEY:-}" ]]; then
        echo ""
        return 1
    fi

    # Use pack.toml defaults if version/modloader not specified
    if [[ -z "$minecraft_version" || -z "$mod_loader" ]]; then
        local pack_config=$(read_pack_config)
        if [[ -n "$pack_config" ]]; then
            IFS='|' read -r pack_version pack_loader <<<"$pack_config"
            minecraft_version="${minecraft_version:-$pack_version}"
            mod_loader="${mod_loader:-$pack_loader}"
        fi
    fi

    # Normalize project types to Modrinth standard, then map to CurseForge classIds
    local normalized_type="$project_type"
    case "$project_type" in
    "texture-pack" | "texturepack")
        normalized_type="resourcepack"
        ;;
    "data-pack")
        normalized_type="datapack"
        ;;
    esac

    # Map normalized types to CurseForge class IDs
    local class_id=""
    case "$normalized_type" in
    "mod")
        class_id="6"
        ;;
    "resourcepack")
        class_id="12" # texture-packs in CurseForge
        ;;
    "datapack")
        class_id="17" # data-packs in CurseForge
        ;;
    *)
        class_id="6" # Default to mod
        normalized_type="mod"
        ;;
    esac

    # Build query parameters
    local query_params="gameId=432&classId=${class_id}&searchFilter=$(echo "$title" | jq -sRr @uri)&sortField=6&sortOrder=desc"

    # Add optional filters
    if [[ -n "$minecraft_version" ]]; then
        query_params="${query_params}&gameVersion=${minecraft_version}"
    fi

    if [[ -n "$mod_loader" ]]; then
        # ModLoaderType values: 1=Forge, 4=Fabric, 5=Quilt, 6=NeoForge
        local mod_loader_id=""
        case "$mod_loader" in
        "forge")
            mod_loader_id="1"
            ;;
        "fabric")
            mod_loader_id="4"
            ;;
        "quilt")
            mod_loader_id="5"
            ;;
        "neoforge")
            mod_loader_id="6"
            ;;
        esac
        if [[ -n "$mod_loader_id" ]]; then
            query_params="${query_params}&modLoaderType=${mod_loader_id}"
        fi
    fi

    # Build encoded URL for debug/actual use
    local search_url="${CF_API_BASE}/mods/search?${query_params}"

    # Fetch and parse JSON
    local response=""
    response=$(curl -s -H "x-api-key: ${CURSEFORGE_API_CLIENT_KEY}" "$search_url" 2>/dev/null || echo '{"data":[]}')

    # Extract structured data from first result
    local project_id found_title downloads
    project_id=$(echo "$response" | jq -r '.data[0].id // ""' 2>/dev/null)
    found_title=$(echo "$response" | jq -r '.data[0].name // ""' 2>/dev/null)
    downloads=$(echo "$response" | jq -r '.data[0].downloadCount // 0' 2>/dev/null)

    # Return structured data if we found a result
    if [[ -n "$project_id" && "$project_id" != "null" && "$project_id" != "" ]]; then
        echo "$project_id|$normalized_type|$found_title|$downloads"
    else
        echo ""
    fi
}

# Legacy function for backward compatibility
cf_get_project_id() {
    local data=$(cf_get_project_data "$@")
    if [[ -n "$data" ]]; then
        echo "$data" | cut -d'|' -f1 # Return just the project ID
    else
        echo ""
    fi
}

# Show usage information
show_usage() {
    echo "CurseForge Search Module - Clean API for project lookup"
    echo ""
    echo "Usage when sourced:"
    echo "  data=\$(cf_get_project_data \"title\" \"type\" \"version\" \"loader\")"
    echo "  id=\$(cf_get_project_id \"title\" \"type\" \"version\" \"loader\")"
    echo ""
    echo "Direct usage:"
    echo "  $0 [--verbose] [--help]  TODO: Search all projects from empack.yml"
    echo "  --verbose, -v            Enable verbose output"
    echo "  --help, -h               Show this help message"
}

# Only run if executed directly (not sourced)
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    # Parse command line arguments
    while [[ $# -gt 0 ]]; do
        case $1 in
        -v | --verbose)
            CF_VERBOSE=true
            shift
            ;;
        --help | -h)
            show_usage
            exit 0
            ;;
        *)
            break
            ;;
        esac
    done
fi
