#!/usr/bin/env bash
# Module: init
# Description: Bootstrap and initialization functionality  
# Dependencies: core, logger, utils, templates, deps

# Prevent multiple loading
if [ "${EMPACK_MODULE_INIT:-}" = "loaded" ]; then
    return 0
fi
readonly EMPACK_MODULE_INIT="loaded"

#=============================================================================
# INITIALIZATION FUNCTIONS
#=============================================================================

# Create complete directory structure for modpack development
create_directory_structure() {
    log_step "Creating directory structure..."
    
    # Build directories with .gitkeep files
    local build_dirs=("dist/client" "dist/client-full" "dist/server" "dist/server-full")
    for dir in "${build_dirs[@]:-}"; do
        ensure_directory "$EMPACK_TARGET_DIR/$dir"
        touch "$EMPACK_TARGET_DIR/$dir/.gitkeep"
    done
    
    # Template directories
    ensure_directory "$EMPACK_TARGET_DIR/templates/client"
    ensure_directory "$EMPACK_TARGET_DIR/templates/server"
    
    # GitHub directories
    ensure_directory "$EMPACK_TARGET_DIR/.github/workflows"
    ensure_directory "$EMPACK_TARGET_DIR/.github/actions"
    
    # Installer directory
    ensure_directory "$EMPACK_TARGET_DIR/installer"
    
    log_success "Directory structure created"
}

# Download required dependencies and tools
download_dependencies() {
    log_step "Downloading dependencies..."
    
    local bootstrap_jar="$EMPACK_TARGET_DIR/installer/packwiz-installer-bootstrap.jar"
    
    # Download packwiz-installer-bootstrap.jar if not present
    if [ ! -f "$bootstrap_jar" ]; then
        download_file "$PACKWIZ_BOOTSTRAP_URL" "$bootstrap_jar"
    else
        log_debug "packwiz-installer-bootstrap.jar already exists"
    fi
    
    log_success "Dependencies downloaded"
}

# Install all required templates
install_all_init_templates() {
    log_step "Installing templates..."
    
    # Install configuration templates
    install_config_templates
    
    # Install client templates  
    install_client_templates
    
    # Install server templates
    install_server_templates
    
    # Install GitHub workflow templates
    install_github_templates
    
    log_success "Templates installed"
}

# Check if directory appears to be an existing modpack
is_existing_modpack() {
    [ -f "$EMPACK_TARGET_DIR/pack/pack.toml" ] || [ -f "$EMPACK_TARGET_DIR/pack.toml" ]
}

# Check if directory is safe to initialize
is_safe_to_initialize() {
    # Ensure target directory exists
    ensure_directory "$EMPACK_TARGET_DIR"
    
    # Check if target directory is empty
    local file_count=$(find "$EMPACK_TARGET_DIR" -maxdepth 1 -type f | wc -l)
    local dir_count=$(find "$EMPACK_TARGET_DIR" -maxdepth 1 -type d ! -name "$(basename "$EMPACK_TARGET_DIR")" | wc -l)
    
    # Allow initialization if:
    # 1. Directory is empty
    # 2. Only has basic files (.git, .gitignore, README.md, etc.)
    # 3. Already appears to be a modpack
    
    if [ "$file_count" -eq 0 ] && [ "$dir_count" -eq 0 ]; then
        return 0  # Empty directory
    fi
    
    if is_existing_modpack; then
        return 0  # Existing modpack
    fi
    
    # Check for common safe files
    local safe_files=(".gitignore" "README.md" "LICENSE" ".actrc")
    local has_unsafe_files=false
    
    while IFS= read -r -d '' file; do
        local basename_file=$(basename "$file")
        local is_safe=false
        
        for safe_file in "${safe_files[@]:-}"; do
            if [ "$basename_file" = "$safe_file" ]; then
                is_safe=true
                break
            fi
        done
        
        if [ "$is_safe" = false ] && [ "$basename_file" != ".git" ]; then
            has_unsafe_files=true
            break
        fi
    done < <(find "$EMPACK_TARGET_DIR" -maxdepth 1 -type f -print0)
    
    [ "$has_unsafe_files" = false ]
}

# Interactive modloader selection with smart defaults
prompt_modloader_selection() {
    # Check for non-interactive mode or environment override
    if [ -n "${EMPACK_MODLOADER:-}" ]; then
        echo "$EMPACK_MODLOADER"
        return 0
    fi
    
    # Send prompts to stderr to avoid capturing them
    echo "🚀 Choose your modloader:" >&2
    echo "  1. NeoForge (recommended for most modpacks)" >&2
    echo "  2. Fabric (performance and client mods)" >&2
    echo "  3. Quilt (experimental features)" >&2
    echo "  4. Vanilla (pure Minecraft)" >&2
    echo >&2
    
    while true; do
        read -p "Selection [1]: " -r selection
        
        # Default to NeoForge if no input
        [ -z "$selection" ] && selection="1"
        
        case "$selection" in
            1|neoforge|NeoForge)
                echo "neoforge"
                return 0
                ;;
            2|fabric|Fabric)
                echo "fabric"
                return 0
                ;;
            3|quilt|Quilt)
                echo "quilt"
                return 0
                ;;
            4|vanilla|Vanilla)
                echo "vanilla"
                return 0
                ;;
            *)
                echo "❌ Invalid selection. Please choose 1-4."
                ;;
        esac
    done
}

# Get recommended Minecraft versions for a given modloader
get_recommended_minecraft_versions() {
    local modloader="$1"
    
    # Fallback versions (latest recommended > latest stable > latest)
    local fallback_versions=("1.21.1" "1.21" "1.20.6" "1.20.4" "1.20.1")
    
    case "$modloader" in
        vanilla)
            # For vanilla, get latest stable release
            if command_exists curl; then
                local latest_release=$(curl -s "https://launchermeta.mojang.com/mc/game/version_manifest.json" 2>/dev/null | \
                                     grep -o '"release":"[^"]*"' | head -1 | cut -d'"' -f4)
                [ -n "$latest_release" ] && echo "$latest_release" && return 0
            fi
            echo "1.21.4"  # Current stable fallback
            ;;
        neoforge|fabric|quilt)
            # For modloaders, prefer ecosystem-proven versions
            # TODO: Implement API-based ranking logic
            # Priority: modloader ecosystem maturity over latest
            echo "1.21.1"  # Known stable with good modloader support
            ;;
        *)
            echo "${fallback_versions[0]}"
            ;;
    esac
}

# Enhanced version selection with stable/latest/custom options
prompt_version_selection() {
    local modloader="$1"
    local stable_version="$2"
    local latest_version="$3"
    
    # If stable and latest are the same, just return stable
    if [ "$stable_version" = "$latest_version" ]; then
        echo "$stable_version"
        return 0
    fi
    
    # Send prompts to stderr to avoid capturing them
    echo >&2
    echo "🔧 $modloader version options:" >&2
    echo "  1. $stable_version (stable) ← recommended" >&2
    
    # Only show latest option if it's newer than stable
    if version_greater_than "$latest_version" "$stable_version"; then
        echo "  2. $latest_version (latest)" >&2
        echo "  3. Custom version" >&2
    else
        echo "  2. Custom version" >&2
    fi
    echo >&2
    
    while true; do
        read -p "Selection [1]: " -r selection
        
        # Default to stable if no input
        [ -z "$selection" ] && selection="1"
        
        case "$selection" in
            1|stable)
                echo "$stable_version"
                return 0
                ;;
            2)
                if version_greater_than "$latest_version" "$stable_version"; then
                    echo "$latest_version"
                    return 0
                else
                    # Option 2 is custom when latest == stable
                    read -p "Enter custom version: " -r custom_version
                    if [ -n "$custom_version" ]; then
                        echo "$custom_version"
                        return 0
                    else
                        echo "❌ Custom version cannot be empty"
                    fi
                fi
                ;;
            3|custom)
                if version_greater_than "$latest_version" "$stable_version"; then
                    read -p "Enter custom version: " -r custom_version
                    if [ -n "$custom_version" ]; then
                        echo "$custom_version"
                        return 0
                    else
                        echo "❌ Custom version cannot be empty"
                    fi
                else
                    echo "❌ Invalid selection. Please choose 1-2."
                fi
                ;;
            *)
                if version_greater_than "$latest_version" "$stable_version"; then
                    echo "❌ Invalid selection. Please choose 1-3."
                else
                    echo "❌ Invalid selection. Please choose 1-2."
                fi
                ;;
        esac
    done
}

# Get recommended modloader version with API integration and version selection
get_recommended_modloader_version() {
    local modloader="$1"
    local mc_version="$2"
    local stable_version=""
    local latest_version=""
    
    case "$modloader" in
        neoforge)
            # Fetch stable and latest versions from API
            local versions
            versions=$(get_neoforge_versions)
            stable_version=$(echo "$versions" | head -1)
            latest_version=$(echo "$versions" | tail -1)
            ;;
        fabric)
            # Fetch stable and latest versions from API
            local versions
            versions=$(get_fabric_versions)
            stable_version=$(echo "$versions" | head -1)
            latest_version=$(echo "$versions" | tail -1)
            ;;
        quilt)
            # Fetch stable and latest versions from API
            local versions
            versions=$(get_quilt_versions)
            stable_version=$(echo "$versions" | head -1)
            latest_version=$(echo "$versions" | tail -1)
            ;;
        *)
            echo ""
            return 1
            ;;
    esac
    
    # Check for non-interactive mode or environment override
    if [ -n "${EMPACK_MODLOADER_VERSION:-}" ]; then
        echo "$EMPACK_MODLOADER_VERSION"
        return 0
    fi
    
    # Interactive version selection
    prompt_version_selection "$modloader" "$stable_version" "$latest_version"
}

# Generate sophisticated packwiz init flags with ecosystem-aware version resolution
generate_packwiz_init_flags() {
    local modloader="$1"
    local flags=()
    
    # Default modpack name (based on directory name)
    local pack_name=$(basename "$EMPACK_TARGET_DIR")
    [ "$pack_name" = "." ] && pack_name="MyModpack"
    flags+=(--name "$pack_name")
    
    # Default author
    flags+=(--author "${USER:-ModpackAuthor}")
    
    # Stage 2: Get recommended Minecraft version for selected modloader
    local mc_version
    mc_version=$(get_recommended_minecraft_versions "$modloader")
    flags+=(--mc-version "$mc_version")
    
    # Stage 3: Add modloader-specific flags with ecosystem-aware versions
    case "$modloader" in
        neoforge)
            local neoforge_version
            neoforge_version=$(get_recommended_modloader_version "neoforge" "$mc_version")
            flags+=(--modloader neoforge)
            flags+=(--neoforge-version "$neoforge_version")
            ;;
        fabric)
            local fabric_version
            fabric_version=$(get_recommended_modloader_version "fabric" "$mc_version")
            flags+=(--modloader fabric)
            flags+=(--fabric-version "$fabric_version")
            ;;
        quilt)
            local quilt_version
            quilt_version=$(get_recommended_modloader_version "quilt" "$mc_version")
            flags+=(--modloader quilt)
            flags+=(--quilt-version "$quilt_version")
            ;;
        vanilla)
            # Vanilla requires explicit --modloader none
            flags+=(--modloader none)
            ;;
        *)
            log_warning "Unknown modloader '$modloader', defaulting to NeoForge"
            local neoforge_version
            neoforge_version=$(get_recommended_modloader_version "neoforge" "$mc_version")
            flags+=(--modloader neoforge)
            flags+=(--neoforge-version "$neoforge_version")
            ;;
    esac
    
    # Default modpack version
    flags+=(--version "1.0.0")
    
    # Enable reinit for safe re-runs and non-interactive mode
    flags+=(--reinit)
    flags+=(-y)  # Non-interactive mode to prevent stdin prompts
    
    printf '%s\n' "${flags[@]}"
}

# Run packwiz init with smart defaults
run_packwiz_init() {
    local modloader="$1"
    log_step "Running packwiz init for modpack setup..."
    
    if ! command_exists packwiz; then
        log_error "packwiz command not found"
        log_error "Please install packwiz before running init"
        return 1
    fi
    
    # Generate flags for non-interactive init
    local -a init_flags
    mapfile -t init_flags < <(generate_packwiz_init_flags "$modloader")
    
    log_info "Initializing modpack with smart defaults..."
    log_debug "packwiz init flags: ${init_flags[*]}"
    
    # Create pack directory and run packwiz init inside it
    ensure_directory "$EMPACK_TARGET_DIR/pack"
    
    if run_in_pack packwiz init "${init_flags[@]}"; then
        log_success "Packwiz initialization complete"
        return 0
    else
        log_error "Packwiz initialization failed"
        return 1
    fi
}

# Validate initialization by attempting a test build
validate_initialization() {
    log_step "Validating setup with test build..."
    
    if [ ! -f "$EMPACK_TARGET_DIR/pack/pack.toml" ]; then
        log_warning "No pack.toml found - skipping validation"
        return 0
    fi
    
    # Attempt to build mrpack as validation from pack directory
    if run_in_pack build_mrpack_impl >/dev/null 2>&1; then
        log_success "✅ Initialization validated successfully!"
        return 0
    else
        log_warning "Test build failed - please check pack.toml configuration"
        return 1
    fi
}

# Display post-initialization guidance
show_post_init_guidance() {
    echo
    log_success "🎉 Modpack development environment is ready!"
    echo
    echo "Next steps:"
    echo "  📦 Add mods:           packwiz mr install <mod-name>"
    echo "  🔨 Build modpack:      empack mrpack"
    echo "  📋 Build distributions: empack client server"
    echo "  🔍 Check dependencies:  empack requirements"
    echo
    echo "For more information:"
    echo "  📖 Help:              empack help"
    echo "  🌐 Packwiz docs:      https://packwiz.infra.link/"
    echo
}

# Main initialization command
init_command() {
    log_info "Initializing modpack development environment..."
    echo
    
    # Check if we're in a safe location to initialize
    if ! is_safe_to_initialize; then
        log_error "Directory contains files that might conflict with initialization"
        echo
        echo "Consider running init in an empty directory or existing modpack directory."
        echo "Files found:"
        find . -maxdepth 1 -type f -exec basename {} \; | sort
        echo
        read -p "Continue anyway? (y/N): " -r confirm
        if [[ ! $confirm =~ ^[Yy]$ ]]; then
            log_info "Initialization cancelled"
            return 1
        fi
        echo
    fi
    
    # Check if this looks like an existing modpack
    if is_existing_modpack; then
        log_info "Detected existing modpack - updating environment"
    else
        log_info "Setting up new modpack development environment"
    fi
    
    # Check dependencies
    log_step "Checking dependencies..."
    if ! quick_dependency_check; then
        echo
        log_error "Missing required dependencies. Please install them first:"
        echo
        requirements_command
        return 1
    fi
    log_success "Dependencies satisfied"
    
    # Create directory structure
    create_directory_structure
    
    # Install templates  
    install_all_init_templates
    
    # Download dependencies
    download_dependencies
    
    echo
    
    # Run packwiz init if needed
    if [ ! -f "$EMPACK_TARGET_DIR/pack/pack.toml" ]; then
        # Get modloader selection
        log_info "Configuring modpack settings..."
        local selected_modloader
        selected_modloader=$(prompt_modloader_selection)
        log_success "Selected modloader: $selected_modloader"
        echo
        
        if ! run_packwiz_init "$selected_modloader"; then
            log_error "Failed to initialize modpack configuration"
            return 1
        fi
    else
        log_info "Found existing pack.toml - skipping packwiz init"
    fi
    
    # Validate setup
    validate_initialization
    
    # Show guidance
    show_post_init_guidance
    
    return 0
}

# Initialize empty modpack environment (for automation)
init_empty() {
    local name="$1"
    local author="$2"
    local mc_version="${3:-1.21.1}"
    local fabric_version="${4:-0.16.14}"
    
    if [ -z "$name" ] || [ -z "$author" ]; then
        log_error "Usage: init_empty <name> <author> [mc_version] [fabric_version]"
        return 1
    fi
    
    log_info "Creating empty modpack: $name by $author"
    
    # Create directory structure and templates
    create_directory_structure
    install_all_init_templates  
    download_dependencies
    
    # Create basic pack.toml
    ensure_directory "pack"
    cat > "pack/pack.toml" << EOF
name = "$name"
author = "$author"
version = "1.0.0"
pack-format = "packwiz:1.1.0"

[index]
file = "index.toml"
hash-format = "sha256"
hash = ""

[versions]
minecraft = "$mc_version"
fabric = "$fabric_version"
EOF
    
    # Create empty index.toml
    cat > "pack/index.toml" << EOF
hash-format = "sha256"

[[files]]
file = "mods/fabric-api.pw.toml"
hash = ""
metafile = true

[[files]]
file = "config/README.md"
hash = ""
EOF
    
    # Create basic structure
    ensure_directory "pack/mods"
    ensure_directory "pack/config"
    
    echo "This directory contains configuration files for your modpack." > "pack/config/README.md"
    
    log_success "Empty modpack environment created"
}

# Export initialization functions
export -f create_directory_structure download_dependencies install_all_init_templates
export -f is_existing_modpack is_safe_to_initialize run_packwiz_init validate_initialization
export -f show_post_init_guidance init_command init_empty prompt_version_selection