#!/usr/bin/env bash
# Empack - Generate packwiz commands from empack.yml dependencies
# Resolves project IDs and outputs the appropriate packwiz mr/cf add commands

set -euo pipefail

# Get the directory where this script is located
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Check if required modules exist
for module in remote_resolver.sh empack_reader.sh search_modrinth.sh search_curseforge.sh fuzz_match.sh; do
    if [[ ! -f "$SCRIPT_DIR/$module" ]]; then
        echo "ERROR: $module not found in $SCRIPT_DIR" >&2
        exit 1
    fi
done

# Check dependencies
if ! command -v yq &>/dev/null; then
    echo "ERROR: yq not found. Install with: brew install yq" >&2
    exit 1
fi

if ! command -v jq &>/dev/null; then
    echo "ERROR: jq not found. Install with: brew install jq" >&2
    exit 1
fi

# Source the remote resolver
source "$SCRIPT_DIR/remote_resolver.sh"

# Add missing project IDs for failed resolutions
add_missing_projects() {
    echo "# Missing projects (manual IDs)"
    echo "packwiz mr add -y --project-id FVToiKwr # Via Romana"
    echo "packwiz mr add -y --project-id nvQzSEkH # Jade"
    echo "packwiz mr add -y --project-id PgpTtNoI # 3D Crops"
    echo "packwiz cf add -y --addon-id 313970     # Apotheosis"
    echo ""
    echo "# Fresh Animations Extensions (specific version IDs)"
    echo "packwiz mr add -y --project-id YAVTU8mK --version-id JrJx24Cj # Fresh Animations Extensions"
    echo "packwiz mr add -y --project-id YAVTU8mK --version-id vWrInfg9 # Fresh Animations Extensions"
    echo "packwiz mr add -y --project-id YAVTU8mK --version-id MIev1lAz # Fresh Animations Extensions"
    echo "packwiz mr add -y --project-id YAVTU8mK --version-id X6qlktk7 # Fresh Animations Extensions"
    echo "packwiz mr add -y --project-id YAVTU8mK --version-id doQyYghr # Fresh Animations Extensions"
}

# Generate packwiz commands from resolution results
generate_packwiz_commands() {
    echo "#!/usr/bin/env bash"
    echo "# Generated packwiz installation commands"
    echo ""
    echo "set -euo pipefail"
    echo ""
    echo "# Change to pack directory"
    echo "pushd pack >/dev/null"
    echo ""
    echo "echo \"Installing resolved projects...\""
    echo ""

    # Process resolved projects
    resolve_all_projects 2>/dev/null | while IFS='|' read -r key platform project_id confidence title; do
        case "$platform" in
        modrinth)
            echo "packwiz mr add -y --project-id $project_id  # $title"
            ;;
        curseforge)
            echo "packwiz cf add -y --addon-id $project_id  # $title"
            ;;
        esac
    done

    echo ""
    add_missing_projects
    echo ""
    echo "echo \"Installation complete!\""
    echo "popd >/dev/null"
}

# Show usage
show_usage() {
    echo "Empack - Generate packwiz commands from empack.yml"
    echo ""
    echo "Usage:"
    echo "  $0                    Generate install script"
    echo "  $0 --resolve          Show resolution results only"
    echo "  $0 --help             Show this help"
}

# Main execution
case "${1:-}" in
--resolve)
    echo "Resolving projects from empack.yml..."
    resolve_all_projects
    ;;
--help | -h)
    show_usage
    ;;
"")
    generate_packwiz_commands
    ;;
*)
    echo "Unknown option: $1" >&2
    show_usage >&2
    exit 1
    ;;
esac
