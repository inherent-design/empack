#!/usr/bin/env bash
# Module: templates
# Description: Template management, installation, and variable substitution
# Dependencies: core, logger, utils

# Prevent multiple loading
if [ "${EMPACK_MODULE_TEMPLATES:-}" = "loaded" ]; then
    return 0
fi
readonly EMPACK_MODULE_TEMPLATES="loaded"

#=============================================================================
# TEMPLATE REGISTRY AND MANAGEMENT
#=============================================================================

# Template registry storage (arrays declared in core.sh)

# Pack information cache
PACK_INFO_LOADED=false
PACK_AUTHOR=""
PACK_NAME=""
PACK_VERSION=""
PACK_MC_VERSION=""
PACK_FABRIC_VERSION=""

# Register a template in the system
register_template() {
    local name="$1"
    local source_path="$2"
    local target_path="$3"
    local process_vars="${4:-true}"
    
    EMPACK_TEMPLATES["$name"]="$name"
    EMPACK_TEMPLATE_SOURCES["$name"]="$source_path"
    EMPACK_TEMPLATE_TARGETS["$name"]="$target_path"
    EMPACK_TEMPLATE_PROCESS_VARS["$name"]="$process_vars"
    
    log_debug "Registered template: $name ($source_path -> $target_path)"
}

# Register all available templates
register_all_templates() {
    log_debug "Registering all templates..."
    
    # Configuration templates
    register_template "gitignore" "templates/gitignore.template" ".gitignore" false
    register_template "actrc" "templates/actrc.template" ".actrc" false
    
    # Client templates
    register_template "client_instance" "templates/client/instance.cfg.template" "templates/client/instance.cfg.template" true
    register_template "client_mmc" "templates/client/mmc-pack.json.template" "templates/client/mmc-pack.json" true
    
    # Server templates
    register_template "server_install" "templates/server/install_pack.sh.template" "templates/server/install_pack.sh.template" true
    register_template "server_properties" "templates/server/server.properties.template" "templates/server/server.properties.template" true
    
    # GitHub workflow templates
    register_template "github_validate" "templates/github/validate.yml.template" ".github/workflows/validate.yml" false
    register_template "github_release" "templates/github/release.yml.template" ".github/workflows/release.yml" false
    
    log_debug "Template registration complete"
}

# Load pack information from pack.toml
load_pack_info() {
    if [ "$PACK_INFO_LOADED" = true ]; then
        return 0
    fi
    
    local pack_file="pack/pack.toml"
    
    if [ ! -f "$pack_file" ]; then
        log_error "Pack file not found: $pack_file"
        return 1
    fi
    
    # Find the appropriate toml command
    local toml_cmd
    if toml_cmd=$(find_command tq tomlq); then
        log_debug "Using TOML command: $toml_cmd"
    else
        log_error "No TOML query tool found (tq or tomlq required)"
        return 1
    fi
    
    # Extract pack information
    PACK_AUTHOR=$($toml_cmd -f "$pack_file" 'author' | sed 's/"//g' 2>/dev/null || echo "Unknown")
    PACK_NAME=$($toml_cmd -f "$pack_file" 'name' | sed 's/"//g' 2>/dev/null || echo "Unknown")
    PACK_VERSION=$($toml_cmd -f "$pack_file" 'version' | sed 's/"//g' 2>/dev/null || echo "Unknown")
    PACK_MC_VERSION=$($toml_cmd -f "$pack_file" 'versions.minecraft' | sed 's/"//g' 2>/dev/null || echo "Unknown")
    PACK_FABRIC_VERSION=$($toml_cmd -f "$pack_file" 'versions.fabric' | sed 's/"//g' 2>/dev/null || echo "Unknown")
    
    PACK_INFO_LOADED=true
    
    log_debug "Loaded pack info: $PACK_NAME v$PACK_VERSION (MC $PACK_MC_VERSION, Fabric $PACK_FABRIC_VERSION)"
    return 0
}

# Get pack information (loads if needed)
get_pack_info() {
    if ! load_pack_info; then
        return 1
    fi
    
    case "$1" in
        author) echo "$PACK_AUTHOR" ;;
        name) echo "$PACK_NAME" ;;
        version) echo "$PACK_VERSION" ;;
        mc_version) echo "$PACK_MC_VERSION" ;;
        fabric_version) echo "$PACK_FABRIC_VERSION" ;;
        *) log_error "Invalid pack info key: $1"; return 1 ;;
    esac
}

# Process template variables in content
process_template_variables() {
    local content="$1"
    
    # Load pack info if we need to process variables
    if ! load_pack_info; then
        log_warning "Could not load pack info for template processing"
        echo "$content"
        return 1
    fi
    
    # Perform variable substitution
    content="${content//\{\{AUTHOR\}\}/$PACK_AUTHOR}"
    content="${content//\{\{NAME\}\}/$PACK_NAME}"
    content="${content//\{\{VERSION\}\}/$PACK_VERSION}"
    content="${content//\{\{MC_VERSION\}\}/$PACK_MC_VERSION}"
    content="${content//\{\{FABRIC_VERSION\}\}/$PACK_FABRIC_VERSION}"
    
    echo "$content"
}

# Install a single template
install_template() {
    local template_name="$1"
    
    if [ -z "${EMPACK_TEMPLATES[$template_name]:-}" ]; then
        log_error "Unknown template: $template_name"
        return 1
    fi
    
    local source_path="${EMPACK_TEMPLATE_SOURCES[$template_name]}"
    local target_path="${EMPACK_TEMPLATE_TARGETS[$template_name]}"
    local process_vars="${EMPACK_TEMPLATE_PROCESS_VARS[$template_name]}"
    
    # Resolve absolute paths
    local abs_source="$EMPACK_ROOT/$source_path"
    
    if [ ! -f "$abs_source" ]; then
        log_error "Template source not found: $abs_source"
        return 1
    fi
    
    # Ensure target directory exists
    local target_dir=$(dirname "$target_path")
    ensure_directory "$target_dir"
    
    # Read template content
    local content
    content=$(cat "$abs_source")
    
    # Process variables if required
    if [ "$process_vars" = "true" ]; then
        content=$(process_template_variables "$content")
    fi
    
    # Write to target
    echo "$content" > "$target_path"
    
    log_debug "Installed template: $template_name -> $target_path"
}

# Install multiple templates
install_templates() {
    local templates=("$@")
    
    if [ ${#templates[@]} -eq 0 ]; then
        log_error "No templates specified for installation"
        return 1
    fi
    
    log_info "Installing templates..."
    
    for template in "${templates[@]:-}"; do
        if ! install_template "$template"; then
            log_error "Failed to install template: $template"
            return 1
        fi
    done
    
    log_success "Templates installed successfully"
}

# Install all registered templates
install_all_templates() {
    local all_templates=()
    
    # Collect all template names
    for template_name in "${!EMPACK_TEMPLATES[@]}"; do
        all_templates+=("$template_name")
    done
    
    install_templates "${all_templates[@]:-}"
}

# Install templates by category
install_config_templates() {
    install_templates "gitignore" "actrc"
}

install_client_templates() {
    install_templates "client_instance" "client_mmc"
}

install_server_templates() {
    install_templates "server_install" "server_properties"
}

install_github_templates() {
    install_templates "github_validate" "github_release"
}

# Process templates for build operations (existing functionality)
process_build_templates() {
    local template_dir="$1"
    local target_dir="$2"
    
    if [ ! -d "$template_dir" ]; then
        log_error "Template directory not found: $template_dir"
        return 1
    fi
    
    ensure_directory "$target_dir"
    
    # Load pack info for processing
    if ! load_pack_info; then
        return 1
    fi
    
    for file in "$template_dir"/*; do
        [ -f "$file" ] || continue # Skip if no files match
        
        local filename=$(basename "$file")
        local target_file="$target_dir/$filename"
        
        if [[ $filename == *.template ]]; then
            # Remove .template suffix for output
            local output_name="${filename%.template}"
            target_file="$target_dir/$output_name"
            
            # Process template variables
            sed -e "s/{{VERSION}}/$PACK_VERSION/g" \
                -e "s/{{NAME}}/$PACK_NAME/g" \
                -e "s/{{AUTHOR}}/$PACK_AUTHOR/g" \
                -e "s/{{MC_VERSION}}/$PACK_MC_VERSION/g" \
                -e "s/{{FABRIC_VERSION}}/$PACK_FABRIC_VERSION/g" \
                "$file" > "$target_file"
        else
            # Copy file as-is
            cp "$file" "$target_file"
        fi
    done
}

# Export template functions
export -f register_template register_all_templates load_pack_info get_pack_info
export -f process_template_variables install_template install_templates install_all_templates
export -f install_config_templates install_client_templates install_server_templates install_github_templates
export -f process_build_templates