#!/usr/bin/env bash
# LAYER_2 Modpack Installation Script
# Minecraft 1.20.1 NeoForge - Automated Modrinth Installation

set -eo pipefail

# Performance tracking
SCRIPT_START=$(date +%s%N)
INSTALL_COUNT=0
ERROR_COUNT=0

# Dry-run mode flag
DRY_RUN=false

# Source the search modules and empack reader
source ./search_modrinth.sh
source ./search_curseforge.sh
source ./empack_reader.sh

# User-definable project mapping: Search Query -> Project ID
# Leave value empty ("") to auto-search, or provide known project ID
# System will auto-populate missing keys after searching
declare -A USER_PROJECT_MAP=(
    # TESTS
    # ["Citadel"]="jJfV67b1"       # Known good ID
    # ["Moonlight Lib"]="twkfQtEc" # Known good ID
    # ["Botarium"]=""              # Auto-search

    # Add more entries here or let system auto-discover
)

# Layer 1: Resolved Project IDs (will be auto-populated)
declare -A PROJECT_IDS=()

# Platform tracking: modrinth or curseforge
declare -A PROJECT_PLATFORMS=()

# Project search and resolution
resolve_project_ids() {
    log_info "Resolving project IDs..."

    local search_count=0
    local cache_count=0

    # Get all projects using empack_reader functions directly
    if ! validate_empack >/dev/null 2>&1; then
        log_error "❌ Failed to validate empack.yml configuration"
        return 1
    fi

    # Use empack_reader functions to get parsed project data
    while IFS='|' read -r key spec; do
        # Parse each project spec using empack_reader's parser
        local parsed_spec=$(parse_project_spec "$spec")
        IFS='|' read -r title project_type minecraft_version mod_loader <<<"$parsed_spec"

        # Check if user provided a project ID
        if [[ -n "${USER_PROJECT_MAP[$title]:-}" ]]; then
            PROJECT_IDS["$title"]="${USER_PROJECT_MAP[$title]}"
            PROJECT_PLATFORMS["$title"]="modrinth"  # Default cached entries to modrinth
            log_info "  Using cached ID for '$title': ${USER_PROJECT_MAP[$title]} (assuming Modrinth)"
            cache_count=$((cache_count + 1))
        else
            # Auto-search for project ID - try Modrinth first, then CurseForge
            log_info "  Searching for '$title' on Modrinth..."
            local project_id=$(mr_get_project_id "$title" "$project_type" "$minecraft_version" "$mod_loader")
            local platform=""
            
            if [[ -n "$project_id" ]]; then
                platform="modrinth"
                log_info "  Found on Modrinth: '$title' = $project_id"
            else
                log_info "  Not found on Modrinth, trying CurseForge..."
                # Try CurseForge as fallback using the renamed function
                local cf_project_id=$(cf_get_project_id "$title" "$project_type" "$minecraft_version" "$mod_loader")
                
                if [[ -n "$cf_project_id" ]]; then
                    project_id="$cf_project_id"
                    platform="curseforge"
                    log_info "  Found on CurseForge: '$title' = $project_id"
                else
                    log_error "  ❌ Not found on either Modrinth or CurseForge: '$title'"
                fi
            fi
            
            if [[ -n "$project_id" && -n "$platform" ]]; then
                PROJECT_IDS["$title"]="$project_id"
                PROJECT_PLATFORMS["$title"]="$platform"
                USER_PROJECT_MAP["$title"]="$project_id" # Cache for future runs
                search_count=$((search_count + 1))
            fi
        fi
    done < <(get_all_projects)

    log_info "📊 Resolution complete: $cache_count cached, $search_count searched"
}

# Layer 2: Project ID -> Version/File ID Strategy
# Format: PROJECT_ID="version_id" or PROJECT_ID="version1 version2 version3"
declare -A VERSION_OVERRIDES=(
    # Fresh Animations Extensions needs specific version
    ["YsxfClyG"]="gwr0Ugwy"
)

# Enable strict mode after array declarations
set -u

# Logging functions
log_info() {
    echo "[INFO] $*"
}

log_success() {
    echo "[SUCCESS] $*"
}

log_error() {
    echo "[ERROR] $*" >&2
    ERROR_COUNT=$((ERROR_COUNT + 1))
}

log_warning() {
    echo "[WARNING] $*" >&2
}

# Execute command with dry-run support
execute_packwiz() {
    local cmd="$*"

    if [[ $DRY_RUN == true ]]; then
        echo "[DRY-RUN] $cmd"
        return 0
    else
        eval "$cmd"
    fi
}

# Advanced install function with version strategy support
install_project() {
    local title="$1" # Now using search title instead of slug
    local project_id="${PROJECT_IDS[$title]:-}"
    local platform="${PROJECT_PLATFORMS[$title]:-modrinth}"

    if [[ -z "$project_id" ]]; then
        log_error "❌ No project ID found for: $title"
        return 1
    fi

    # Check if version override exists
    if [[ -n "${VERSION_OVERRIDES[$project_id]:-}" ]]; then
        local version_strategy="${VERSION_OVERRIDES[$project_id]}"
        local version_array=($version_strategy)

        if [[ ${#version_array[@]} -eq 1 ]]; then
            # Strategy (b): Single version ID
            log_info "Installing: $title (ID: $project_id, Version: ${version_array[0]}, Platform: $platform)"
            local version_cmd=""
            if [[ "$platform" == "curseforge" ]]; then
                version_cmd="packwiz curseforge add --addon-id \"$project_id\" --file-id \"${version_array[0]}\" -y"
            else
                version_cmd="packwiz modrinth add --project-id \"$project_id\" --version-id \"${version_array[0]}\" -y"
            fi
            
            if execute_packwiz "$version_cmd"; then
                log_success "Installed: $title (pinned version)"
                INSTALL_COUNT=$((INSTALL_COUNT + 1))
                return 0
            else
                log_error "❌ Failed to install: $title (pinned version: ${version_array[0]})"
                return 1
            fi
        else
            # Strategy (c): Multiple version IDs
            log_info "Installing: $title (ID: $project_id, Multiple versions: ${#version_array[@]})"
            local success_count=0
            for version_id in "${version_array[@]}"; do
                local version_cmd=""
                if [[ "$platform" == "curseforge" ]]; then
                    version_cmd="packwiz curseforge add --addon-id \"$project_id\" --file-id \"$version_id\" -y"
                else
                    version_cmd="packwiz modrinth add --project-id \"$project_id\" --version-id \"$version_id\" -y"
                fi
                
                if execute_packwiz "$version_cmd"; then
                    log_success "  Installed version: $version_id"
                    success_count=$((success_count + 1))
                else
                    log_error "  ❌ Failed version: $version_id"
                fi
            done

            if [[ $success_count -gt 0 ]]; then
                log_success "Installed: $title ($success_count/${#version_array[@]} versions)"
                INSTALL_COUNT=$((INSTALL_COUNT + success_count))
                return 0
            else
                log_error "❌ Failed to install any version of: $title"
                return 1
            fi
        fi
    else
        # Strategy (a): Auto-install latest compatible version
        log_info "Installing: $title (ID: $project_id, auto-version, Platform: $platform)"
        local auto_cmd=""
        if [[ "$platform" == "curseforge" ]]; then
            auto_cmd="packwiz curseforge add --addon-id \"$project_id\" -y"
        else
            auto_cmd="packwiz modrinth add --project-id \"$project_id\" -y"
        fi
        
        if execute_packwiz "$auto_cmd"; then
            log_success "Installed: $title (auto-version)"
            INSTALL_COUNT=$((INSTALL_COUNT + 1))
            return 0
        else
            log_error "❌ Failed to install: $title (auto-version)"
            return 1
        fi
    fi
}

# Verify packwiz is in correct directory
verify_environment() {
    if [[ ! -f "pack/pack.toml" ]]; then
        log_error "pack.toml not found. Please run from modpack root directory."
        exit 1
    fi

    local pack_version=$(grep "minecraft.*1\.20\.1" pack/pack.toml)
    if [[ -z "$pack_version" ]]; then
        log_warning "pack.toml may not be configured for Minecraft 1.20.1"
    fi

    log_info "Environment verified - ready to install mods"

    pushd pack >/dev/null
}

# Parse command line arguments
parse_args() {
    while [[ $# -gt 0 ]]; do
        case $1 in
        --dry-run | -n)
            DRY_RUN=true
            shift
            ;;
        --help | -h)
            echo "Usage: $0 [--dry-run|-n] [--help|-h]"
            echo "  --dry-run, -n    Show commands that would be executed without running them"
            echo "  --help, -h       Show this help message"
            exit 0
            ;;
        *)
            echo "Unknown option: $1"
            echo "Use --help for usage information"
            exit 1
            ;;
        esac
    done
}

# Main installation sequence
main() {
    parse_args "$@"

    echo "=========================================="
    echo "LAYER_2 Modpack Automated Installation"
    echo "Minecraft 1.20.1 NeoForge"
    if [[ $DRY_RUN == true ]]; then
        echo "(DRY-RUN MODE - Commands will be displayed, not executed)"
    fi
    echo "=========================================="

    verify_environment

    # Resolve all project IDs before installation
    resolve_project_ids

    echo ""
    echo "🚀 INSTALLING ALL PROJECTS FROM EMPACK.YML"
    echo ""
    
    # Install all projects using empack_reader functions directly  
    while IFS='|' read -r key spec; do
        # Parse each project spec using empack_reader's parser
        local parsed_spec=$(parse_project_spec "$spec")
        IFS='|' read -r title project_type minecraft_version mod_loader <<<"$parsed_spec"
        
        install_project "$title"
    done < <(get_all_projects)

    # Final statistics
    local total_duration=$(($(date +%s%N) - SCRIPT_START))
    local total_ms=$((total_duration / 1000000))

    echo ""
    echo "=========================================="
    echo "INSTALLATION COMPLETE"
    echo "=========================================="
    echo "✅ Successfully installed: $INSTALL_COUNT mods"
    echo "❌ Failed installations: $ERROR_COUNT"
    echo "⏱️ Total time: ${total_ms}ms"
    echo ""

    popd >/dev/null

    if [[ $ERROR_COUNT -eq 0 ]]; then
        echo "🎉 All projects installed successfully!"
        echo "Next steps:"
        echo "  1. Run 'packwiz refresh' to update index"
        echo "  2. Test in launcher"
    else
        echo "⚠️  Some projects failed to install. Check errors above."
        echo "You may need to install failed projects manually."
    fi
}

# Execute main function
main "$@"
