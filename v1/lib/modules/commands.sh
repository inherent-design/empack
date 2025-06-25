#!/usr/bin/env bash
# Module: commands
# Description: Command registry, argument parsing, and routing system with runtime boundary enforcement
# Dependencies: core, logger, utils, deps, boundaries

# Prevent multiple loading
if [ "${EMPACK_MODULE_COMMANDS:-}" = "loaded" ]; then
    return 0
fi
readonly EMPACK_MODULE_COMMANDS="loaded"

#=============================================================================
# COMMAND STATE VARIABLES
#=============================================================================

# Command execution state (EMPACK_COMMAND_EXEC_* namespace to avoid conflict with registry arrays)
declare -g EMPACK_COMMAND_EXEC_CURRENT_COMMAND=""
declare -g EMPACK_COMMAND_EXEC_EXECUTION_ORDER=""
declare -g EMPACK_COMMAND_EXEC_VALIDATION_STATUS=""
declare -g EMPACK_COMMAND_EXEC_LAST_HANDLER_RESULT=""
declare -g EMPACK_COMMAND_EXEC_RUNTIME_BOUNDARY_VALIDATED="false"
declare -g EMPACK_COMMAND_EXEC_MODPACK_VALIDATION_COMPLETE="false"

# Clear command execution state
clear_command_exec_state() {
    EMPACK_COMMAND_EXEC_CURRENT_COMMAND=""
    EMPACK_COMMAND_EXEC_EXECUTION_ORDER=""
    EMPACK_COMMAND_EXEC_VALIDATION_STATUS=""
    EMPACK_COMMAND_EXEC_LAST_HANDLER_RESULT=""
    EMPACK_COMMAND_EXEC_RUNTIME_BOUNDARY_VALIDATED="false"
    EMPACK_COMMAND_EXEC_MODPACK_VALIDATION_COMPLETE="false"
    log_debug "Command execution state cleared"
}

#=============================================================================
# MODULE INTERFACE CONTRACT
#=============================================================================

# Standard module interface - export commands state variables
export_commands_state() {
    echo "EMPACK_COMMAND_EXEC_CURRENT_COMMAND='$EMPACK_COMMAND_EXEC_CURRENT_COMMAND'"
    echo "EMPACK_COMMAND_EXEC_EXECUTION_ORDER='$EMPACK_COMMAND_EXEC_EXECUTION_ORDER'"
    echo "EMPACK_COMMAND_EXEC_VALIDATION_STATUS='$EMPACK_COMMAND_EXEC_VALIDATION_STATUS'"
    echo "EMPACK_COMMAND_EXEC_LAST_HANDLER_RESULT='$EMPACK_COMMAND_EXEC_LAST_HANDLER_RESULT'"
    echo "EMPACK_COMMAND_EXEC_RUNTIME_BOUNDARY_VALIDATED='$EMPACK_COMMAND_EXEC_RUNTIME_BOUNDARY_VALIDATED'"
    echo "EMPACK_COMMAND_EXEC_MODPACK_VALIDATION_COMPLETE='$EMPACK_COMMAND_EXEC_MODPACK_VALIDATION_COMPLETE'"
}

# Get current module status
get_commands_status() {
    local status="operational"
    local details=""
    
    if [ "$EMPACK_COMMAND_EXEC_VALIDATION_STATUS" = "HANDLER_EXECUTION_FAILED" ]; then
        status="error"
        details="Command handler execution failed"
    elif [ "$EMPACK_COMMAND_EXEC_VALIDATION_STATUS" = "HANDLER_NOT_FOUND" ]; then
        status="error"
        details="Command handler not found"
    elif [ "$EMPACK_COMMAND_EXEC_VALIDATION_STATUS" = "RUNTIME_BOUNDARY_VIOLATION" ]; then
        status="error"
        details="Runtime boundary violation detected"
    elif [ -n "$EMPACK_COMMAND_EXEC_CURRENT_COMMAND" ]; then
        status="active"
        details="Executing command: $EMPACK_COMMAND_EXEC_CURRENT_COMMAND"
    fi
    
    echo "status=$status"
    echo "current_command=$EMPACK_COMMAND_EXEC_CURRENT_COMMAND"
    echo "validation_status=$EMPACK_COMMAND_EXEC_VALIDATION_STATUS"
    echo "details=$details"
}

# Validate commands module state and configuration
validate_commands_state() {
    local validation_passed=true
    local errors=()
    
    # Check if core.sh command registry arrays are available
    if ! declare -p EMPACK_COMMANDS >/dev/null 2>&1; then
        errors+=("Global EMPACK_COMMANDS array not available from core.sh")
        validation_passed=false
    fi
    
    if ! declare -p EMPACK_COMMAND_DESCRIPTIONS >/dev/null 2>&1; then
        errors+=("Global EMPACK_COMMAND_DESCRIPTIONS array not available from core.sh")
        validation_passed=false
    fi
    
    if ! declare -p EMPACK_COMMAND_HANDLERS >/dev/null 2>&1; then
        errors+=("Global EMPACK_COMMAND_HANDLERS array not available from core.sh")
        validation_passed=false
    fi
    
    if ! declare -p EMPACK_COMMAND_ORDER >/dev/null 2>&1; then
        errors+=("Global EMPACK_COMMAND_ORDER array not available from core.sh")
        validation_passed=false
    fi
    
    if ! declare -p EMPACK_COMMAND_REQUIRES_MODPACK >/dev/null 2>&1; then
        errors+=("Global EMPACK_COMMAND_REQUIRES_MODPACK array not available from core.sh")
        validation_passed=false
    fi
    
    # Check boundaries module functions are available
    if ! declare -F require_post_init >/dev/null 2>&1; then
        errors+=("Function require_post_init not available from boundaries module")
        validation_passed=false
    fi
    
    if ! declare -F require_pre_init >/dev/null 2>&1; then
        errors+=("Function require_pre_init not available from boundaries module")
        validation_passed=false
    fi
    
    # Check validation functions are available
    if ! declare -F validate_pack_toml >/dev/null 2>&1; then
        errors+=("Function validate_pack_toml not available from validation module")
        validation_passed=false
    fi
    
    echo "validation_passed=$validation_passed"
    if [ ${#errors[@]} -gt 0 ]; then
        echo "errors=${errors[*]}"
    fi
    
    return $([ "$validation_passed" = true ] && echo 0 || echo 1)
}

#=============================================================================
# COMMAND REGISTRATION SYSTEM
#=============================================================================

# Register a command in the system with runtime boundary enforcement
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
    
    log_debug "Registered command: $name (order: $order, handler: $handler, requires_modpack: $requires_modpack)"
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

# Check if command requires modpack directory (runtime boundary enforcement)
command_requires_modpack() {
    local command="$1"
    [ "${EMPACK_COMMAND_REQUIRES_MODPACK[$command]:-false}" = "true" ]
}

# Register all available commands with runtime boundary compliance
register_all_commands() {
    log_debug "Registering all commands with runtime boundary enforcement..."
    
    # Pre-init commands (no modpack required - available before pack.toml exists)
    register_command "requirements" "Check tool dependencies and show setup guidance" "requirements_command" 0 false
    register_command "init" "Initialize modpack development environment" "init_command" 1 false
    register_command "version" "Show version information" "version_command" 0 false
    
    # Post-init commands (require modpack - available after pack.toml exists)
    register_command "clean" "Clean the dist (build) directories" "clean_command" 2 true
    register_command "mrpack" "Build a Modrinth-compatible '*.mrpack' file" "build_mrpack" 10 true
    register_command "client" "Build a bootstrapped client installer" "build_client" 11 true
    register_command "server" "Build a bootstrapped server installer" "build_server" 12 true
    register_command "client-full" "Build a non-redistributable client (⚠️  embeds non-redistributable content)" "build_client_full" 13 true
    register_command "server-full" "Build a non-redistributable server (⚠️  embeds non-redistributable content)" "build_server_full" 14 true
    
    # Meta build commands (require modpack)
    register_command "all" "Equivalent to 'mrpack client server'" "build_all" 50 true
    
    log_debug "Command registration complete with runtime boundary enforcement"
}

#=============================================================================
# COMMAND EXECUTION SYSTEM (TWO-PASS PIPELINE)
#=============================================================================

# Execute commands via the registry system (called from core.sh)
execute_commands() {
    local -a args=("$@")
    
    # If no commands provided, show help
    if [ ${#args[@]} -eq 0 ]; then
        show_help
        return 0
    fi
    
    # Register all commands
    register_all_commands
    
    # Execute two-pass pipeline
    parse_and_execute_commands "${args[@]}"
}

# Two-pass execution pipeline: validation then execution
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
        
        # Add command if not already in the list (deduplication)
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
    
    # Execute commands in order with runtime boundary enforcement
    execute_command_list run_commands
}

# Execute a list of commands in order with runtime boundary validation
execute_command_list() {
    local -n commands_ref=$1
    
    # Check if we have any commands to execute
    if [ ${#commands_ref[@]} -eq 0 ]; then
        log_warning "No commands to execute"
        EMPACK_COMMAND_EXEC_VALIDATION_STATUS="EMPTY_COMMAND_LIST"
        return 0
    fi
    
    # Store execution order for debugging
    EMPACK_COMMAND_EXEC_EXECUTION_ORDER=$(IFS=' '; echo "${commands_ref[*]}")
    log_debug "Command execution order: $EMPACK_COMMAND_EXEC_EXECUTION_ORDER"
    
    for command in "${commands_ref[@]:-}"; do
        EMPACK_COMMAND_EXEC_CURRENT_COMMAND="$command"
        log_info "Executing command: $command"
        
        # Runtime boundary enforcement: validate modpack directory if required
        if command_requires_modpack "$command"; then
            if ! require_post_init "$command"; then
                log_error "Command '$command' requires valid modpack structure (pack.toml)"
                log_error "Run 'empack init' first to set up the modpack environment"
                EMPACK_COMMAND_EXEC_VALIDATION_STATUS="RUNTIME_BOUNDARY_VIOLATION"
                exit 1
            fi
            
            # Validate only once for efficiency using state management
            if [ "$EMPACK_COMMAND_EXEC_MODPACK_VALIDATION_COMPLETE" = "false" ]; then
                if ! validate_pack_toml; then
                    log_error "Invalid pack.toml structure detected"
                    EMPACK_COMMAND_EXEC_VALIDATION_STATUS="PACK_TOML_INVALID"
                    exit 1
                fi
                EMPACK_COMMAND_EXEC_MODPACK_VALIDATION_COMPLETE="true"
                log_debug "Modpack validation completed successfully"
            fi
            EMPACK_COMMAND_EXEC_RUNTIME_BOUNDARY_VALIDATED="true"
        else
            # Pre-init commands should explicitly check they're in the right phase
            if ! require_pre_init "$command" 2>/dev/null; then
                # Some pre-init commands (like requirements) work in both phases
                log_debug "Command '$command' running in post-init phase"
            fi
        fi
        
        # Get and execute handler with state tracking
        local handler=$(get_command_handler "$command")
        if [ -n "$handler" ] && declare -F "$handler" >/dev/null; then
            log_debug "Executing handler: $handler for command: $command"
            if "$handler"; then
                EMPACK_COMMAND_EXEC_LAST_HANDLER_RESULT="success"
                log_debug "Command '$command' completed successfully"
            else
                EMPACK_COMMAND_EXEC_LAST_HANDLER_RESULT="error"
                EMPACK_COMMAND_EXEC_VALIDATION_STATUS="HANDLER_EXECUTION_FAILED"
                log_error "Command '$command' failed with handler: $handler"
                exit 1
            fi
        else
            log_error "Command handler not found: $handler for command $command"
            EMPACK_COMMAND_EXEC_VALIDATION_STATUS="HANDLER_NOT_FOUND"
            exit 1
        fi
    done
    
    EMPACK_COMMAND_EXEC_VALIDATION_STATUS="ALL_COMMANDS_COMPLETED"
    log_debug "All commands completed successfully"
}

# Sort commands array by execution order (bubble sort for small arrays)
sort_commands_by_order() {
    local -n commands_ref=$1
    local -a orders
    
    # Get orders for all commands
    for command in "${commands_ref[@]:-}"; do
        orders+=($(get_command_order "$command"))
    done
    
    # Simple bubble sort (sufficient for small command arrays)
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

#=============================================================================
# HELP SYSTEM WITH RUNTIME BOUNDARY CATEGORIZATION
#=============================================================================

# Print help message with runtime boundary categorization
print_help() {
    cat << 'EOF'
empack - Professional Minecraft Modpack Development Tool

USAGE:
    empack [OPTIONS] [COMMAND...]

OPTIONS:
    -v, --verbose              Enable verbose output
    -d, --debug                Enable debug output (implies --verbose)
    -q, --quiet                Suppress non-essential output
    --dry-run                  Preview operations without making changes
    -y, --non-interactive      Run in non-interactive mode
    -m, --modpack-directory    Set target directory for modpack operations
    -h, --help                 Show this help message
    -V, --version              Show version information

PRE-INIT COMMANDS (available before modpack setup):
    requirements               Check system dependencies
    init                       Initialize modpack development environment
    version                    Show version information
    help                       Show help information

POST-INIT COMMANDS (require valid modpack structure):
    mrpack                     Build .mrpack distribution
    client                     Build client installer
    server                     Build server installer
    client-full                Build client installer with direct mod downloads
    server-full                Build server installer with direct mod downloads
    clean                      Clean build artifacts
    all                        Build all distributions (mrpack client server)

EXAMPLES:
    empack requirements                    # Check dependencies
    empack init                           # Interactive initialization
    empack --non-interactive --modloader fabric init
    empack --modpack-directory /tmp/test init
    empack mrpack client server          # Build distributions
    empack --verbose all                  # Build all with verbose output

For more information: https://github.com/empack/empack
EOF
}

#=============================================================================
# COMMAND HANDLERS (PLACEHOLDER IMPLEMENTATIONS)
#=============================================================================

# Version command handler
version_command() {
    echo "$EMPACK_NAME $EMPACK_VERSION"
}

# Build all command handler (expands to mrpack client server)
build_all() {
    log_info "Building all distributions (mrpack client server)"
    # This will be handled by the execution pipeline expansion
    # Implementation will be in builds.sh
    log_error "Build system not yet implemented"
    return 1
}

# Placeholder build command handlers (will be implemented in builds.sh)
clean_command() {
    log_error "Clean command not yet implemented"
    return 1
}

build_mrpack() {
    log_error "Mrpack build not yet implemented"
    return 1
}

build_client() {
    log_error "Client build not yet implemented"
    return 1
}

build_server() {
    log_error "Server build not yet implemented"
    return 1
}

build_client_full() {
    log_error "Client-full build not yet implemented"
    return 1
}

build_server_full() {
    log_error "Server-full build not yet implemented"
    return 1
}

# Init command implementation is in init.sh module
# This is just a placeholder - actual implementation loaded from init.sh

# Export command functions
export -f register_command is_valid_command get_command_order get_command_handler
export -f command_requires_modpack register_all_commands execute_commands
export -f parse_and_execute_commands execute_command_list sort_commands_by_order
export -f print_help version_command clear_command_exec_state
export -f export_commands_state get_commands_status validate_commands_state