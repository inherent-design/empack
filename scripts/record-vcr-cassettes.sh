#!/usr/bin/env bash
#
# VCR Cassette Recording Script
# empack - Minecraft Modpack Lifecycle Management
# Date: 2025-12-23
#
# Records real API responses to JSON fixtures for VCR testing.
# Phase 1: 12 critical endpoints (Modrinth, CurseForge, Loaders, Minecraft)
#
# Usage:
#   ./scripts/record-vcr-cassettes.sh              # Record all cassettes
#   ./scripts/record-vcr-cassettes.sh --dry-run    # Preview without recording
#   ./scripts/record-vcr-cassettes.sh --only modrinth/search_sodium  # Record single cassette

set -euo pipefail

# Get script directory and project root
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# Source helper functions
# shellcheck source=scripts/lib/vcr_helpers.sh
source "$SCRIPT_DIR/lib/vcr_helpers.sh"

# Configuration
CASSETTES_DIR="$PROJECT_ROOT/crates/empack-tests/fixtures/cassettes"
ENV_FILE="$PROJECT_ROOT/.env.local"
DRY_RUN=false
ONLY_CASSETTE=""

# Parse arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --dry-run)
            DRY_RUN=true
            shift
            ;;
        --only)
            ONLY_CASSETTE="$2"
            shift 2
            ;;
        -h|--help)
            cat <<EOF
VCR Cassette Recording Script

Usage:
  $0 [OPTIONS]

Options:
  --dry-run           Preview cassettes without recording
  --only <name>       Record only specific cassette (e.g., modrinth/search_sodium)
  -h, --help          Show this help message

Examples:
  $0                                    # Record all 12 cassettes
  $0 --dry-run                          # Preview all cassettes
  $0 --only modrinth/search_sodium      # Record single Modrinth cassette
  $0 --only curseforge/search_jei       # Record single CurseForge cassette

Prerequisites:
  - curl, jq installed
  - .env.local file with CURSEFORGE_API_CLIENT_KEY (script expects this variable name)

Cassettes Recorded (Phase 1 - 12 total):
  Modrinth (4):
    - search_sodium
    - project_AANobbMI
    - dependencies_AANobbMI
    - versions_AANobbMI

  CurseForge (3):
    - search_jei
    - mod_238222
    - files_238222

  Loaders (4):
    - fabric_versions_1.21.1
    - forge_promotions
    - neoforge_versions
    - quilt_versions

  Minecraft (1):
    - version_manifest

EOF
            exit 0
            ;;
        *)
            log_error "Unknown option: $1"
            exit 1
            ;;
    esac
done

# Validate prerequisites
log_info "Validating prerequisites..."
validate_prerequisites || exit 1
validate_env_file "$ENV_FILE" || exit 1
log_success "Prerequisites validated"

# Create cassette directory structure
if [[ "$DRY_RUN" == false ]]; then
    log_info "Creating cassette directory structure..."
    mkdir -p "$CASSETTES_DIR"/{modrinth,curseforge,loaders,minecraft}
    log_success "Directory structure created"
fi

# Define all cassettes to record
declare -A CASSETTES=(
    # Modrinth (4 cassettes)
    ["modrinth/search_sodium"]="https://api.modrinth.com/v2/search|{\"User-Agent\":\"empack-tests/0.1.0\"}|{\"query\":\"sodium\",\"limit\":\"10\"}"
    ["modrinth/project_AANobbMI"]="https://api.modrinth.com/v2/project/AANobbMI|{\"User-Agent\":\"empack-tests/0.1.0\"}|{}"
    ["modrinth/dependencies_AANobbMI"]="https://api.modrinth.com/v2/project/AANobbMI/dependencies|{\"User-Agent\":\"empack-tests/0.1.0\"}|{}"
    ["modrinth/versions_AANobbMI"]="https://api.modrinth.com/v2/project/AANobbMI/version|{\"User-Agent\":\"empack-tests/0.1.0\"}|{\"game_versions\":[\"1.21.1\"],\"loaders\":[\"fabric\"]}"

    # CurseForge (3 cassettes)
    ["curseforge/search_jei"]="https://api.curseforge.com/v1/mods/search|{\"x-api-key\":\"$CURSEFORGE_API_CLIENT_KEY\"}|{\"gameId\":\"432\",\"classId\":\"6\",\"searchFilter\":\"jei\",\"pageSize\":\"10\"}"
    ["curseforge/mod_238222"]="https://api.curseforge.com/v1/mods/238222|{\"x-api-key\":\"$CURSEFORGE_API_CLIENT_KEY\"}|{}"
    ["curseforge/files_238222"]="https://api.curseforge.com/v1/mods/238222/files|{\"x-api-key\":\"$CURSEFORGE_API_CLIENT_KEY\"}|{\"pageSize\":\"10\"}"

    # Loaders (4 cassettes)
    ["loaders/fabric_versions_1.21.1"]="https://meta.fabricmc.net/v2/versions/loader/1.21.1|{}|{}"
    ["loaders/forge_promotions"]="https://files.minecraftforge.net/net/minecraftforge/forge/promotions_slim.json|{}|{}"
    ["loaders/neoforge_versions"]="https://maven.neoforged.net/api/maven/versions/releases/net/neoforged/neoforge|{}|{}"
    ["loaders/quilt_versions"]="https://meta.quiltmc.org/v3/versions/loader|{}|{}"

    # Minecraft (1 cassette)
    ["minecraft/version_manifest"]="https://launchermeta.mojang.com/mc/game/version_manifest.json|{}|{}"
)

# Filter cassettes if --only specified
if [[ -n "$ONLY_CASSETTE" ]]; then
    if [[ ! -v CASSETTES["$ONLY_CASSETTE"] ]]; then
        log_error "Cassette not found: $ONLY_CASSETTE"
        log_error "Available cassettes: ${!CASSETTES[*]}"
        exit 1
    fi

    # Create temporary array with only selected cassette
    declare -A FILTERED_CASSETTES
    FILTERED_CASSETTES["$ONLY_CASSETTE"]="${CASSETTES[$ONLY_CASSETTE]}"
    unset CASSETTES
    declare -A CASSETTES
    for key in "${!FILTERED_CASSETTES[@]}"; do
        CASSETTES["$key"]="${FILTERED_CASSETTES[$key]}"
    done
fi

# Record cassettes
log_info "Recording ${#CASSETTES[@]} cassette(s)..."
echo ""

recorded_count=0
failed_count=0
skipped_count=0

for cassette_name in "${!CASSETTES[@]}"; do
    # Parse cassette spec (URL|headers|query)
    IFS='|' read -r url headers_json query_json <<< "${CASSETTES[$cassette_name]}"

    # Build output path
    output_path="$CASSETTES_DIR/${cassette_name}.json"

    # Dry run mode
    if [[ "$DRY_RUN" == true ]]; then
        log_info "DRY RUN: Would record $cassette_name"
        log_info "  URL: $url"
        log_info "  Query: $query_json"
        log_info "  Output: $output_path"
        echo ""
        skipped_count=$((skipped_count + 1))
        continue
    fi

    # Record cassette
    if record_endpoint "$cassette_name" "$url" "$headers_json" "$query_json" "$output_path"; then
        recorded_count=$((recorded_count + 1))

        # Sanitize API keys from cassette
        sanitize_cassette "$output_path"

        # Verify cassette is valid
        verify_cassette "$output_path"
    else
        failed_count=$((failed_count + 1))
    fi

    echo ""
done

# Summary
echo ""
log_info "════════════════════════════════════════"
log_info "Recording Summary"
log_info "════════════════════════════════════════"

if [[ "$DRY_RUN" == true ]]; then
    log_info "Dry run completed"
    log_info "  Cassettes previewed: $skipped_count"
else
    log_success "Recorded: $recorded_count cassettes"
    if [[ $failed_count -gt 0 ]]; then
        log_error "Failed: $failed_count cassettes"
    fi

    # Print cassette directory summary
    echo ""
    print_cassette_summary "$CASSETTES_DIR"
fi

echo ""
log_info "Next steps:"
log_info "  1. Verify cassettes: ls -lh $CASSETTES_DIR/*/*.json"
log_info "  2. Inspect cassette: jq . $CASSETTES_DIR/modrinth/search_sodium.json"
log_info "  3. Commit fixtures: git add $CASSETTES_DIR"

exit 0
