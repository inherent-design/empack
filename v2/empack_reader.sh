#!/usr/bin/env bash
# Empack YAML Reader Module - Source-able library for reading empack.yml configuration
# Provides unified access to project definitions and pack configuration

# Only set strict mode if not already sourced
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    set -euo pipefail
fi

# Function to read pack.toml and extract minecraft version and modloader
read_pack_config() {
    local pack_file="pack/pack.toml"

    if [[ ! -f "$pack_file" ]]; then
        echo "" # Return empty if no pack.toml
        return 1
    fi

    # Extract minecraft version
    local minecraft_version=$(grep '^minecraft = ' "$pack_file" | sed 's/minecraft = "\(.*\)"/\1/')

    # Extract modloader (neoforge or forge)
    local modloader=""
    if grep -q '^neoforge = ' "$pack_file"; then
        modloader="neoforge"
    elif grep -q '^forge = ' "$pack_file"; then
        modloader="forge"
    fi

    echo "${minecraft_version}|${modloader}"
}

# Function to find empack.yml file (handles relative paths)
find_empack_file() {
    local empack_file="empack.yml"
    if [[ ! -f "$empack_file" && -f "../empack.yml" ]]; then
        empack_file="../empack.yml"
    fi

    if [[ ! -f "$empack_file" ]]; then
        echo ""
        return 1
    fi

    echo "$empack_file"
}

# Function to get all project definitions from empack.yml
# Returns: key|"title|type|version|loader" for each project
get_all_projects() {
    local empack_file
    empack_file=$(find_empack_file)

    if [[ -z "$empack_file" ]]; then
        echo "❌ ERROR: empack.yml not found!" >&2
        return 1
    fi

    # Check if yq is available
    if ! command -v yq &>/dev/null; then
        echo "❌ ERROR: yq command not found!" >&2
        echo "Please install yq: brew install yq" >&2
        return 1
    fi

    # Get all dependency keys and their search queries
    yq eval '.empack.dependencies[] | keys | .[]' "$empack_file" | while read -r key; do
        # Get the search query for this key
        search_query=$(yq eval ".empack.dependencies[] | select(has(\"$key\")) | .$key" "$empack_file")
        echo "$key|$search_query"
    done
}

# Function to parse project specification into components
# Supports flexible syntax:
#   "Title|type" - both version and loader from pack.toml
#   "Title|type|" - same as above (trailing | ignored)
#   "Title|type||" - same as above (empty fields ignored)
#   "Title|type|1.20.1" - explicit version, loader from pack.toml
#   "Title|type||forge" - version from pack.toml, explicit loader
#   "Title|type|1.20.1|forge" - both explicit
# Output: title|type|version|loader (fills in pack.toml defaults for empty fields)
parse_project_spec() {
    local spec="$1"

    # Parse the specification, handling variable number of fields
    # Split on | and handle empty fields properly
    local fields
    IFS='|' read -ra fields <<<"$spec"

    # Extract fields with defaults
    local title="${fields[0]:-}"
    local project_type="${fields[1]:-}"
    local minecraft_version="${fields[2]:-}"
    local mod_loader="${fields[3]:-}"

    # Validate required fields
    if [[ -z "$title" || -z "$project_type" ]]; then
        echo "❌ ERROR: Invalid project spec '$spec' - title and type are required" >&2
        return 1
    fi

    # Use pack.toml defaults for empty version/modloader
    if [[ -z "$minecraft_version" || -z "$mod_loader" ]]; then
        local pack_config
        pack_config=$(read_pack_config)
        if [[ -n "$pack_config" ]]; then
            IFS='|' read -r pack_version pack_loader <<<"$pack_config"
            minecraft_version="${minecraft_version:-$pack_version}"
            mod_loader="${mod_loader:-$pack_loader}"
        fi
    fi

    echo "$title|$project_type|$minecraft_version|$mod_loader"
}

# Function to validate empack.yml exists and is readable
validate_empack() {
    local empack_file
    empack_file=$(find_empack_file)

    if [[ -z "$empack_file" ]]; then
        echo "❌ ERROR: empack.yml not found!" >&2
        echo "Please create empack.yml with dependencies configuration." >&2
        return 1
    fi

    if ! command -v yq &>/dev/null; then
        echo "❌ ERROR: yq command not found!" >&2
        echo "Please install yq: brew install yq" >&2
        return 1
    fi

    return 0
}

# Self-test function
run_self_test() {
    echo "=========================================="
    echo "Empack YAML Reader Module Self-Test"
    echo "=========================================="
    echo ""

    if ! validate_empack; then
        return 1
    fi

    echo "✅ empack.yml found and yq available"
    echo ""

    # Show pack configuration
    pack_config=$(read_pack_config)
    if [[ -n "$pack_config" ]]; then
        IFS='|' read -r version loader <<<"$pack_config"
        echo "📦 Pack Configuration:"
        echo "   Minecraft: $version"
        echo "   Loader: $loader"
        echo ""
    fi

    # Test parsing syntax variations
    echo "🧪 Testing Parsing Syntax:"
    test_specs=(
        "Citadel|mod"
        "AppleSkin|mod|1.20.1|forge"
        "Open Loader|mod||forge"
        "EMI|mod|1.20.1"
        "Test Mod|mod|"
        "Another Test|mod||"
    )

    for spec in "${test_specs[@]}"; do
        parsed=$(parse_project_spec "$spec")
        echo "   '$spec' → $parsed"
    done
    echo ""

    # Show first few real projects
    echo "🔍 Sample Real Projects:"
    local count=0
    # Use process substitution to avoid SIGPIPE when breaking early
    while IFS='|' read -r key spec && [[ $count -lt 5 ]]; do
        parsed=$(parse_project_spec "$spec")
        echo "   $key: $parsed"
        count=$((count + 1))
    done < <(get_all_projects 2>/dev/null)

    echo ""
    echo "✅ Self-test complete"
    return 0
}

# Default action: show all projects in parsed format (useful for other scripts)
run_default() {
    if ! validate_empack >/dev/null 2>&1; then
        echo "❌ empack.yml not found or yq not available" >&2
        return 1
    fi

    # Output all projects in standardized format: key|title|type|version|loader
    get_all_projects | while IFS='|' read -r key spec; do
        parsed=$(parse_project_spec "$spec")
        echo "$key|$parsed"
    done
}

# Only run if executed directly (not sourced)
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    case "${1:-}" in
    --self-test)
        run_self_test
        ;;
    --help | -h)
        echo "Usage: $0 [--self-test] [--help]"
        echo ""
        echo "Default: Output all projects in parsed format (key|title|type|version|loader)"
        echo "  --self-test    Run comprehensive tests of the module functionality"
        echo "  --help, -h     Show this help message"
        exit 0
        ;;
    *)
        run_default
        ;;
    esac
fi
