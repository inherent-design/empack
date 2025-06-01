#!/usr/bin/env bash
# Module: builds
# Description: Build system implementation for modpack targets
# Dependencies: core, logger, utils, templates

# Prevent multiple loading
if [ "${EMPACK_MODULE_BUILDS:-}" = "loaded" ]; then
    return 0
fi
readonly EMPACK_MODULE_BUILDS="loaded"

#=============================================================================
# BUILD TARGET REGISTRY
#=============================================================================

# Build target storage (arrays declared in core.sh)

# Build state tracking
PACK_REFRESHED=false
MRPACK_EXTRACTED=false

# Register a build target
register_build_target() {
    local name="$1"
    local handler="$2"
    local dependencies="$3"
    
    BUILD_TARGETS["$name"]="$handler"
    BUILD_DEPENDENCIES["$name"]="$dependencies"
    
    log_debug "Registered build target: $name (handler: $handler)"
}

# Register all build targets
register_all_build_targets() {
    log_debug "Registering build targets..."
    
    register_build_target "mrpack" "build_mrpack_impl" ""
    register_build_target "client" "build_client_impl" "mrpack"
    register_build_target "server" "build_server_impl" "mrpack"
    register_build_target "client-full" "build_client_full_impl" ""
    register_build_target "server-full" "build_server_full_impl" ""
    
    log_debug "Build target registration complete"
}

#=============================================================================
# BUILD UTILITIES
#=============================================================================

# Refresh pack files using packwiz
refresh_pack() {
    if [ "$PACK_REFRESHED" = true ]; then
        return 0
    fi
    
    local pack_file="pack/pack.toml"
    
    if [ ! -f "$pack_file" ]; then
        log_error "Pack file not found: $pack_file"
        return 1
    fi
    
    log_info "Refreshing pack: $pack_file"
    packwiz --pack-file "$pack_file" refresh
    PACK_REFRESHED=true
}

# Extract mrpack for distribution builds
extract_mrpack() {
    if [ "$MRPACK_EXTRACTED" = true ]; then
        return 0
    fi
    
    local pack_name=$(get_pack_info name)
    local pack_version=$(get_pack_info version)
    local mrpack_file="dist/${pack_name}-v${pack_version}.mrpack"
    
    if [ ! -f "$mrpack_file" ]; then
        log_info "Building mrpack for extraction..."
        build_mrpack_impl
    fi
    
    log_info "Extracting modpack for distribution builds"
    
    local temp_extract_dir="dist/temp-mrpack-extract"
    rm -rf "$temp_extract_dir"
    ensure_directory "$temp_extract_dir"
    
    if ! extract_archive "$mrpack_file" "$temp_extract_dir"; then
        log_error "Failed to extract mrpack file"
        return 1
    fi
    
    MRPACK_EXTRACTED=true
}

# Create distribution zip file
zip_distribution() {
    local target="$1"
    
    if [ "$target" != "client" ] && [ "$target" != "server" ] && [ "$target" != "client-full" ] && [ "$target" != "server-full" ]; then
        log_error "Invalid zip target: $target"
        return 1
    fi
    
    local dist_dir="dist/$target"
    
    if [ -z "$(find "$dist_dir" -mindepth 1 ! -name '.gitkeep' -print -quit 2>/dev/null)" ]; then
        log_error "No files to zip in '$dist_dir'"
        return 1
    fi
    
    local pack_name=$(get_pack_info name)
    local pack_version=$(get_pack_info version)
    local filename="${pack_name}-v${pack_version}-${target}.zip"
    local zip_path="dist/$filename"
    
    # Remove existing zip file
    rm -f "$zip_path"
    
    # Create zip file
    log_build "Creating distribution: $filename"
    
    (cd "$dist_dir" && zip -r0 "../$filename" ./ -x '.gitkeep' >/dev/null)
    
    if [ $? -eq 0 ]; then
        log_success "Export '$target' ready at '$zip_path'"
    else
        log_error "Failed to create zip file: $zip_path"
        return 1
    fi
}

# Clean build target
clean_target() {
    local target="$1"
    
    if [ "$target" = "dist" ]; then
        # Clean root dist files but preserve subdirectories
        if [ -n "$(find dist -mindepth 1 -maxdepth 1 ! -name 'client' ! -name 'server' ! -name 'client-full' ! -name 'server-full' -print -quit 2>/dev/null)" ]; then
            log_info "Cleaning 'dist' root builds"
            find dist -mindepth 1 -maxdepth 1 \
                ! -name 'client' \
                ! -name 'server' \
                ! -name 'client-full' \
                ! -name 'server-full' \
                -delete
        fi
        return 0
    fi
    
    # Validate target
    if [ "$target" != "client" ] && [ "$target" != "server" ] && [ "$target" != "client-full" ] && [ "$target" != "server-full" ]; then
        log_error "Invalid clean target: $target"
        return 1
    fi
    
    local dist_dir="dist/$target"
    local pack_name=$(get_pack_info name)
    local pack_version=$(get_pack_info version)
    local zip_file="dist/${pack_name}-v${pack_version}-${target}.zip"
    
    # Clean directory contents
    if [ -n "$(find "$dist_dir" -mindepth 1 ! -name '.gitkeep' -print -quit 2>/dev/null)" ]; then
        log_info "Cleaning $target build in '$dist_dir'"
        find "$dist_dir" -mindepth 1 ! -name '.gitkeep' -delete
    fi
    
    # Clean zip file
    if [ -f "$zip_file" ]; then
        log_info "Cleaning $target build '$zip_file'"
        rm -f "$zip_file"
    fi
}

#=============================================================================
# BUILD COMMAND IMPLEMENTATIONS
#=============================================================================

# Clean command
clean_command() {
    log_info "Cleaning build artifacts..."
    clean_target dist
}

# mrpack build implementation
build_mrpack_impl() {
    log_build "Building modpack (Modrinth)"
    
    refresh_pack
    
    local pack_name=$(get_pack_info name)
    local pack_version=$(get_pack_info version)
    local pack_file="pack/pack.toml"
    local output_file="dist/${pack_name}-v${pack_version}.mrpack"
    
    # Remove existing mrpack file
    rm -f "$output_file"
    
    # Build mrpack using packwiz
    packwiz --pack-file "$pack_file" mr export -o "$output_file"
    
    if [ $? -eq 0 ]; then
        log_success "Modrinth export ready at '$output_file'"
    else
        log_error "Failed to build mrpack"
        return 1
    fi
}

# Client build implementation
build_client_impl() {
    log_build "Building client distribution"
    
    # Clean first
    clean_target client
    
    # Ensure dependencies
    refresh_pack
    
    local dist_dir="dist/client"
    
    # Process templates
    process_build_templates "templates/client" "$dist_dir"
    
    # Set up client structure
    ensure_directory "$dist_dir/.minecraft"
    
    # Copy packwiz installer
    if [ -f "installer/packwiz-installer-bootstrap.jar" ]; then
        cp "installer/packwiz-installer-bootstrap.jar" "$dist_dir/.minecraft/"
    else
        log_error "packwiz-installer-bootstrap.jar not found in installer/"
        return 1
    fi
    
    # Copy pack files
    cp -r pack "$dist_dir/.minecraft/"
    
    # Extract mrpack overrides
    extract_mrpack
    local temp_extract_dir="dist/temp-mrpack-extract"
    
    if [ -d "$temp_extract_dir/overrides" ]; then
        cp -r "$temp_extract_dir/overrides"/* "$dist_dir/.minecraft/"
    fi
    
    # Create distribution
    zip_distribution client
}

# Server build implementation  
build_server_impl() {
    log_build "Building server distribution"
    
    # Clean first
    clean_target server
    
    # Ensure dependencies
    refresh_pack
    
    local dist_dir="dist/server"
    local pack_fabric_version=$(get_pack_info fabric_version)
    local pack_mc_version=$(get_pack_info mc_version)
    
    # Process templates
    process_build_templates "templates/server" "$dist_dir"
    
    # Copy pack files
    cp -r pack "$dist_dir/"
    
    # Copy packwiz installer
    if [ -f "installer/packwiz-installer-bootstrap.jar" ]; then
        cp "installer/packwiz-installer-bootstrap.jar" "$dist_dir/"
    else
        log_error "packwiz-installer-bootstrap.jar not found in installer/"
        return 1
    fi
    
    # Install server using mrpack-install
    log_info "Installing Minecraft server..."
    mrpack-install server \
        fabric --flavor-version "$pack_fabric_version" \
        --minecraft-version "$pack_mc_version" \
        --server-dir "$dist_dir" \
        --server-file srv.jar
    
    if [ $? -ne 0 ]; then
        log_error "Failed to install Minecraft server"
        return 1
    fi
    
    # Extract mrpack overrides
    extract_mrpack
    local temp_extract_dir="dist/temp-mrpack-extract"
    
    if [ -d "$temp_extract_dir/overrides" ]; then
        cp -r "$temp_extract_dir/overrides"/* "$dist_dir/"
    fi
    
    # Create distribution
    zip_distribution server
}

# Client-full build implementation
build_client_full_impl() {
    log_build "Building client-full distribution (non-redistributable)"
    
    # Clean first
    clean_target client-full
    
    # Ensure dependencies
    refresh_pack
    
    local dist_dir="dist/client-full"
    local pack_file="pack/pack.toml"
    
    # Use packwiz installer to download everything
    log_info "Downloading all mods and resources..."
    
    (cd "$dist_dir" && \
        java -jar "../../installer/packwiz-installer-bootstrap.jar" \
        --bootstrap-main-jar "../../installer/packwiz-installer.jar" \
        -g -s both \
        "../../$pack_file")
    
    if [ $? -ne 0 ]; then
        log_error "Failed to build client-full"
        return 1
    fi
    
    # Create distribution
    zip_distribution client-full
}

# Server-full build implementation
build_server_full_impl() {
    log_build "Building server-full distribution (non-redistributable)"
    
    # Clean first
    clean_target server-full
    
    # Ensure dependencies
    refresh_pack
    
    local dist_dir="dist/server-full"
    local pack_fabric_version=$(get_pack_info fabric_version)
    local pack_mc_version=$(get_pack_info mc_version)
    local pack_file="pack/pack.toml"
    
    # Process templates
    process_build_templates "templates/server" "$dist_dir"
    
    # Install server using mrpack-install
    log_info "Installing Minecraft server..."
    mrpack-install server \
        fabric --flavor-version "$pack_fabric_version" \
        --minecraft-version "$pack_mc_version" \
        --server-dir "$dist_dir" \
        --server-file srv.jar
    
    if [ $? -ne 0 ]; then
        log_error "Failed to install Minecraft server"
        return 1
    fi
    
    # Use packwiz installer to download all server mods
    log_info "Downloading all server mods and resources..."
    
    (cd "$dist_dir" && \
        java -jar "../../installer/packwiz-installer-bootstrap.jar" \
        --bootstrap-main-jar "../../installer/packwiz-installer.jar" \
        -g -s server \
        "../../$pack_file")
    
    if [ $? -ne 0 ]; then
        log_error "Failed to build server-full"
        return 1
    fi
    
    # Create distribution
    zip_distribution server-full
}

# Build all targets
build_all() {
    log_info "Building all distribution targets"
    
    build_mrpack_impl && \
    build_client_impl && \
    build_server_impl
}

# Cleanup temporary files
cleanup_build_temp() {
    if [ "$MRPACK_EXTRACTED" = true ]; then
        rm -rf dist/temp-mrpack-extract
        log_debug "Cleaned up temporary extraction directory"
    fi
}

# Export build functions
export -f register_build_target register_all_build_targets
export -f refresh_pack extract_mrpack zip_distribution clean_target
export -f clean_command build_mrpack_impl build_client_impl build_server_impl
export -f build_client_full_impl build_server_full_impl build_all
export -f cleanup_build_temp

# Command aliases for registration
build_mrpack() { build_mrpack_impl; }
build_client() { build_client_impl; }
build_server() { build_server_impl; }
build_client_full() { build_client_full_impl; }
build_server_full() { build_server_full_impl; }

export -f build_mrpack build_client build_server build_client_full build_server_full