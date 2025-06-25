#!/usr/bin/env bash
# Modrinth Search Module - Clean API for project lookup

# Only set strict mode if not already sourced
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    set -euo pipefail
fi

# Source required modules
source ./empack_reader.sh

# API base URL
MR_API_BASE="https://api.modrinth.com/v2/search"

# Verbose mode flag (can be overridden by sourcing script)
MR_VERBOSE=${MR_VERBOSE:-false}

# Pure data function for structured output
# Returns: project_id|project_type|found_title|downloads
mr_get_project_data() {
    local title="$1"
    local project_type="${2:-mod}"
    local minecraft_version="${3:-}"
    local mod_loader="${4:-}"

    # Use pack.toml defaults if version/modloader not specified
    if [[ -z "$minecraft_version" || -z "$mod_loader" ]]; then
        local pack_config=$(read_pack_config)
        if [[ -n "$pack_config" ]]; then
            IFS='|' read -r pack_version pack_loader <<<"$pack_config"
            minecraft_version="${minecraft_version:-$pack_version}"
            mod_loader="${mod_loader:-$pack_loader}"
        fi
    fi

    # Normalize project types to Modrinth standard
    local normalized_type="$project_type"
    case "$project_type" in
    "texture-pack" | "texturepack")
        normalized_type="resourcepack"
        ;;
    "data-pack")
        normalized_type="datapack"
        ;;
    esac

    # Build search query
    local facet_parts=()

    # Add project type facet
    facet_parts+=("[\"project_type:${normalized_type}\"]")

    # Add optional filters
    if [[ -n "$minecraft_version" ]]; then
        facet_parts+=("[\"versions:${minecraft_version}\"]")
    fi

    if [[ -n "$mod_loader" ]]; then
        facet_parts+=("[\"categories:${mod_loader}\"]")
    fi

    # Build facets string
    local facets_raw=""
    if [[ ${#facet_parts[@]} -gt 0 ]]; then
        local IFS=','
        facets_raw="[${facet_parts[*]}]"
    fi

    # Build encoded URL
    local search_url="${MR_API_BASE}?query=$(echo "$title" | jq -sRr @uri)&facets=$(echo "$facets_raw" | jq -sRr @uri)"

    # Make API request
    local response=$(curl -s "$search_url" 2>/dev/null || echo '{"hits":[]}')

    # If we have a mod_loader filter and got results, try to find a better match
    local project_id found_title downloads
    if [[ -n "$mod_loader" ]] && [[ $(echo "$response" | jq '.hits | length') -gt 0 ]]; then
        # Try to find a result that actually supports the modloader
        local best_match=$(echo "$response" | jq -r --arg loader "$mod_loader" '
            .hits[] |
            select(.categories[] | test($loader; "i")) |
            select(.title | test("'$title'"; "i")) |
            {project_id, title, downloads} |
            @base64' | head -1)

        if [[ -n "$best_match" ]]; then
            project_id=$(echo "$best_match" | base64 -d | jq -r '.project_id')
            found_title=$(echo "$best_match" | base64 -d | jq -r '.title')
            downloads=$(echo "$best_match" | base64 -d | jq -r '.downloads')
        else
            # Fall back to first result
            project_id=$(echo "$response" | jq -r '.hits[0].project_id // ""' 2>/dev/null)
            found_title=$(echo "$response" | jq -r '.hits[0].title // ""' 2>/dev/null)
            downloads=$(echo "$response" | jq -r '.hits[0].downloads // 0' 2>/dev/null)
        fi
    else
        # Extract structured data from first result
        project_id=$(echo "$response" | jq -r '.hits[0].project_id // ""' 2>/dev/null)
        found_title=$(echo "$response" | jq -r '.hits[0].title // ""' 2>/dev/null)
        downloads=$(echo "$response" | jq -r '.hits[0].downloads // 0' 2>/dev/null)
    fi

    # Return structured data if we found a result
    if [[ -n "$project_id" && "$project_id" != "null" && "$project_id" != "" ]]; then
        echo "$project_id|$normalized_type|$found_title|$downloads"
    else
        echo ""
    fi
}

# Legacy function for backward compatibility
mr_get_project_id() {
    local data=$(mr_get_project_data "$@")
    if [[ -n "$data" ]]; then
        echo "$data" | cut -d'|' -f1 # Return just the project ID
    else
        echo ""
    fi
}

# Show usage information
show_usage() {
    echo "Modrinth Search Module - Clean API for project lookup"
    echo ""
    echo "Usage when sourced:"
    echo "  data=\$(mr_get_project_data \"title\" \"type\" \"version\" \"loader\")"
    echo "  id=\$(mr_get_project_id \"title\" \"type\" \"version\" \"loader\")"
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
            MR_VERBOSE=true
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
