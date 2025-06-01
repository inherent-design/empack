#!/usr/bin/env bash
# Module: commands
# Description: Command registry, argument parsing, and routing system
# Dependencies: core, logger

# Prevent multiple loading
if [ "${EMPACK_MODULE_COMMANDS:-}" = "loaded" ]; then
    return 0
fi
readonly EMPACK_MODULE_COMMANDS="loaded"

#=============================================================================
# COMMAND REGISTRY SYSTEM
#=============================================================================

# Command registry storage (arrays declared in core.sh)

# Register a command in the system
register_command() {
    local name="$1"
    local description="$2"
    local handler="$3"
    local order="${4:-999}"
    local requires_modpack="${5:-false}"
    
    EMPACK_COMMANDS["${name}"]="${name}"
    EMPACK_COMMAND_DESCRIPTIONS["${name}"]="${description}"
    EMPACK_COMMAND_HANDLERS["${name}"]="${handler}"
    EMPACK_COMMAND_ORDER["${name}"]="${order}"
    EMPACK_COMMAND_REQUIRES_MODPACK["${name}"]="${requires_modpack}"
    
    log_debug "Registered command: $name (order: $order, handler: $handler)"
}

# Check if a command is valid
is_valid_command() {
    local command="$1"
    [ -n "${EMPACK_COMMANDS[$command]:-}" ]
}

# Get command execution order
get_command_order() {
    local command="$1"
    echo "${EMPACK_COMMAND_ORDER[$command]:-999}"
}

# Get command handler function name
get_command_handler() {
    local command="$1"
    echo "${EMPACK_COMMAND_HANDLERS[$command]:-}"
}

# Check if command requires modpack directory
command_requires_modpack() {
    local command="$1"
    [ "${EMPACK_COMMAND_REQUIRES_MODPACK[$command]:-false}" = "true" ]
}

# Register all available commands
register_all_commands() {
    log_debug "Registering all commands..."
    
    # Meta commands (don't require modpack)
    register_command "requirements" "Check tool dependencies and show setup guidance" "requirements_command" 0 false
    register_command "init" "Initialize modpack development environment" "init_command" 1 false
    register_command "version" "Show version information" "version_command" 0 false
    
    # Maintenance commands (require modpack)
    register_command "clean" "Clean the dist (build) directories" "clean_command" 2 true
    
    # Build commands (require modpack)
    register_command "mrpack" "Build a Modrinth-compatible '*.mrpack' file" "build_mrpack" 10 true
    register_command "client" "Build a bootstrapped client installer" "build_client" 11 true
    register_command "server" "Build a bootstrapped server installer" "build_server" 12 true
    register_command "client-full" "Build a non-redistributable client (⚠️  embeds non-redistributable content)" "build_client_full" 13 true
    register_command "server-full" "Build a non-redistributable server (⚠️  embeds non-redistributable content)" "build_server_full" 14 true
    
    # Meta build commands (require modpack)
    register_command "all" "Equivalent to 'mrpack client server'" "build_all" 50 true
    
    log_debug "Command registration complete"
}

#=============================================================================
# ARGUMENT PARSING AND EXECUTION
#=============================================================================

# Parse and execute commands from command line arguments
parse_and_execute_commands() {
    local -a run_commands
    local has_special_commands=false
    
    # First pass: validate all commands and check for special cases
    for arg in "$@"; do
        if ! is_valid_command "$arg"; then
            log_error "Invalid command: $arg"
            echo
            print_help
            exit 1
        fi
        
        # Handle special commands that change the execution model
        if [ "$arg" = "all" ]; then
            log_info "Running all commands (mrpack client server)"
            run_commands=("mrpack" "client" "server")
            has_special_commands=true
            break
        fi
        
        # Add command if not already in the list
        local duplicate=false
        for existing in "${run_commands[@]:-}"; do
            if [ "$existing" = "$arg" ]; then
                duplicate=true
                break
            fi
        done
        
        if [ "$duplicate" = false ]; then
            run_commands+=("$arg")
        fi
    done
    
    # Sort commands by execution order if we have multiple
    if [ "${#run_commands[@]}" -gt 1 ] && [ "$has_special_commands" = false ]; then
        sort_commands_by_order run_commands
    fi
    
    # Execute commands in order
    execute_command_list run_commands
}

# Execute a list of commands in order
execute_command_list() {
    local -n commands_ref=$1
    local modpack_validated=false
    
    # Check if we have any commands to execute
    if [ ${#commands_ref[@]} -eq 0 ]; then
        log_warning "No commands to execute"
        return 0
    fi
    
    for command in "${commands_ref[@]:-}"; do
        log_info "Executing command: $command"
        
        # Validate modpack directory if required (only once)
        if command_requires_modpack "$command" && [ "$modpack_validated" = false ]; then
            empack_validate_modpack_directory
            modpack_validated=true
        fi
        
        # Get and execute handler
        local handler=$(get_command_handler "$command")
        if [ -n "$handler" ] && declare -F "$handler" >/dev/null; then
            "$handler"
        else
            log_error "Command handler not found: $handler for command $command"
            exit 1
        fi
    done
}

# Sort commands array by execution order
sort_commands_by_order() {
    local -n commands_ref=$1
    local -a sorted_commands
    local -a orders
    
    # Get orders for all commands
    for command in "${commands_ref[@]:-}"; do
        orders+=($(get_command_order "$command"))
    done
    
    # Simple bubble sort (sufficient for small arrays)
    local n=${#commands_ref[@]}
    for ((i = 0; i < n; i++)); do
        for ((j = 0; j < n - i - 1; j++)); do
            if [[ ${orders[j]} -gt ${orders[j + 1]} ]]; then
                # Swap commands
                local temp_cmd="${commands_ref[j]}"
                commands_ref[j]="${commands_ref[j + 1]}"
                commands_ref[j + 1]="$temp_cmd"
                
                # Swap orders
                local temp_order=${orders[j]}
                orders[j]=${orders[j + 1]}
                orders[j + 1]=$temp_order
            fi
        done
    done
}

# Check if array contains value
array_contains() {
    local -n array_ref=$1
    local value="$2"
    
    # Handle empty arrays safely
    if [ ${#array_ref[@]} -eq 0 ]; then
        return 1
    fi
    
    for item in "${array_ref[@]:-}"; do
        if [ "$item" = "$value" ]; then
            return 0
        fi
    done
    return 1
}

# Print help message with all available commands
print_help() {
    echo "Usage: empack [flags] [command1] [command2] ..."
    echo
    echo "Standalone Minecraft modpack development tool. Multiple commands can be specified"
    echo "and will be automatically sorted by execution priority and deduplicated."
    echo
    echo "Global flags:"
    echo "  -v, --verbose            Enable verbose output"
    echo "  -d, --debug              Enable debug output (implies --verbose)"
    echo "  -q, --quiet              Reduce output to warnings and errors only"
    echo "      --dry-run            Show what would be done without executing"
    echo "  -m, --modpack-directory  Target a specific directory for operations"
    echo "  -h, --help               Show this help message"
    echo "  -V, --version            Show version information"
    echo
    echo "Examples:"
    echo "  empack --verbose requirements           # Check dependencies with detailed output"
    echo "  empack --debug init                    # Initialize with debug logging"
    echo "  empack --dry-run mrpack                # Simulate mrpack build"
    echo "  empack -m /tmp/test-pack init          # Initialize modpack in test directory"
    echo "  empack --modpack-directory ./my-pack requirements  # Check dependencies for specific modpack"
    echo "  empack requirements                    # Check tool dependencies"
    echo "  empack init                           # Initialize modpack development environment"
    echo "  empack mrpack                         # Build just the .mrpack file"
    echo "  empack clean mrpack                   # Clean then build .mrpack"
    echo "  empack client server                  # Build both client and server"
    echo "  empack all                            # Equal to 'mrpack client server'"
    echo
    echo "Available commands:"
    
    # Sort commands by order for display
    local -a sorted_command_names
    local -a sorted_orders
    
    for cmd in "${!EMPACK_COMMANDS[@]}"; do
        sorted_command_names+=("$cmd")
        sorted_orders+=($(get_command_order "$cmd"))
    done
    
    # Simple sort
    local n=${#sorted_command_names[@]}
    for ((i = 0; i < n; i++)); do
        for ((j = 0; j < n - i - 1; j++)); do
            if [[ ${sorted_orders[j]} -gt ${sorted_orders[j + 1]} ]]; then
                # Swap
                local temp_cmd="${sorted_command_names[j]}"
                sorted_command_names[j]="${sorted_command_names[j + 1]}"
                sorted_command_names[j + 1]="$temp_cmd"
                
                local temp_order=${sorted_orders[j]}
                sorted_orders[j]=${sorted_orders[j + 1]}
                sorted_orders[j + 1]=$temp_order
            fi
        done
    done
    
    for cmd in "${sorted_command_names[@]:-}"; do
        printf "  %-12s - %s\n" "$cmd" "${EMPACK_COMMAND_DESCRIPTIONS[$cmd]}"
    done
    
    echo
    echo "Version: $(empack_version)"
}

# Version command handler
version_command() {
    echo "empack version $(empack_version)"
}

# Export command functions
export -f register_command is_valid_command get_command_order get_command_handler
export -f command_requires_modpack register_all_commands parse_and_execute_commands
export -f print_help version_command