#!/usr/bin/env bash
# Terminal Detection Module - Simplified terminal capability detection for Layer_2
# Based on Abaddon TTY concepts but adapted for standalone use

# Only set strict mode if not already sourced
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    set -euo pipefail
fi

# Guard against multiple loads
[[ -n "${TERMINAL_DETECTION_LOADED:-}" ]] && return 0
readonly TERMINAL_DETECTION_LOADED=1

# Terminal capability state variables
declare -g TERMINAL_TYPE="${TERM:-unknown}"
declare -g TERMINAL_COLOR_DEPTH="8"
declare -g TERMINAL_UNICODE_SUPPORT="false"
declare -g TERMINAL_GRAPHICS_SUPPORT="false"
declare -g TERMINAL_CAPABILITIES=""

# Terminal capability flags
declare -gA TERMINAL_CAPS=(
    ["true_color"]="false"
    ["256_color"]="false"
    ["16_color"]="true"
    ["8_color"]="true"
    ["unicode"]="false"
    ["graphics"]="false"
    ["interactive"]="false"
)

# Color primitives (will be enhanced based on capabilities)
declare -g TD_RED='\033[0;31m'
declare -g TD_GREEN='\033[0;32m'
declare -g TD_YELLOW='\033[0;33m'
declare -g TD_BLUE='\033[0;34m'
declare -g TD_CYAN='\033[0;36m'
declare -g TD_MAGENTA='\033[0;35m'
declare -g TD_WHITE='\033[0;37m'
declare -g TD_NC='\033[0m'

# Enhanced color primitives (populated based on detection)
declare -gA TD_ENHANCED_COLORS=()

# ============================================================================
# CORE DETECTION FUNCTIONS
# ============================================================================

# Detect if we're in an interactive terminal
detect_interactive_terminal() {
    if [[ -t 1 ]]; then
        TERMINAL_CAPS["interactive"]="true"
        echo "true"
    else
        TERMINAL_CAPS["interactive"]="false"
        echo "false"
    fi
}

# Detect color depth capabilities
detect_color_depth() {
    local color_support="8"
    
    # Check COLORTERM first (most reliable)
    case "${COLORTERM:-}" in
        "truecolor"|"24bit")
            color_support="truecolor"
            TERMINAL_CAPS["true_color"]="true"
            TERMINAL_CAPS["256_color"]="true"
            TERMINAL_CAPS["16_color"]="true"
            ;;
    esac
    
    # Check tput if available
    if command -v tput >/dev/null 2>&1; then
        local tput_colors
        tput_colors=$(tput colors 2>/dev/null || echo "8")
        case "$tput_colors" in
            "16777216"|"16M")
                [[ "$color_support" == "8" ]] && color_support="truecolor"
                TERMINAL_CAPS["true_color"]="true"
                TERMINAL_CAPS["256_color"]="true"
                TERMINAL_CAPS["16_color"]="true"
                ;;
            "256")
                [[ "$color_support" == "8" ]] && color_support="256"
                TERMINAL_CAPS["256_color"]="true"
                TERMINAL_CAPS["16_color"]="true"
                ;;
            "16")
                [[ "$color_support" == "8" ]] && color_support="16"
                TERMINAL_CAPS["16_color"]="true"
                ;;
        esac
    fi
    
    # Check TERM patterns
    case "$TERMINAL_TYPE" in
        *"256color"*|*"truecolor"*)
            [[ "$color_support" == "8" ]] && color_support="256"
            TERMINAL_CAPS["256_color"]="true"
            ;;
        "xterm-kitty"|"kitty"|"xterm-ghostty"|"ghostty"|"wezterm"|"xterm-wezterm")
            [[ "$color_support" == "8" ]] && color_support="truecolor"
            TERMINAL_CAPS["true_color"]="true"
            TERMINAL_CAPS["256_color"]="true"
            TERMINAL_CAPS["16_color"]="true"
            ;;
    esac
    
    TERMINAL_COLOR_DEPTH="$color_support"
    echo "$color_support"
}

# Detect Unicode support
detect_unicode_support() {
    local unicode_support="false"
    
    # Check TERM patterns that typically support Unicode
    case "$TERMINAL_TYPE" in
        *"256color"*|*"truecolor"*|"xterm-kitty"|"kitty"|"screen"*|"tmux"*|"xterm"*|"konsole"*|"gnome"*|"wezterm"*|"ghostty"*)
            unicode_support="true"
            TERMINAL_CAPS["unicode"]="true"
            ;;
    esac
    
    # Check locale
    case "${LC_ALL:-${LC_CTYPE:-${LANG:-}}}" in
        *"UTF-8"*|*"utf8"*)
            unicode_support="true"
            TERMINAL_CAPS["unicode"]="true"
            ;;
    esac
    
    TERMINAL_UNICODE_SUPPORT="$unicode_support"
    echo "$unicode_support"
}

# Detect graphics protocol support (simplified from Abaddon)
detect_graphics_support() {
    local graphics_support="false"
    
    # Check even if not interactive (since we might be in a capable terminal)
    # Skip detection in SSH without graphics forwarding
    if [[ -n "${SSH_CLIENT:-}${SSH_TTY:-}" ]] && [[ -z "${DISPLAY:-}${WAYLAND_DISPLAY:-}" ]]; then
        echo "false"
        return 1
    fi
    
    # Check for known graphics-capable terminals
    case "$TERMINAL_TYPE" in
        "xterm-kitty"|"kitty")
            graphics_support="true"
            TERMINAL_CAPS["graphics"]="true"
            ;;
        "xterm-ghostty"|"ghostty")
            graphics_support="true"
            TERMINAL_CAPS["graphics"]="true"
            ;;
        "wezterm"|"xterm-wezterm")
            graphics_support="true"
            TERMINAL_CAPS["graphics"]="true"
            ;;
        "konsole"|"xterm-konsole")
            graphics_support="true"
            TERMINAL_CAPS["graphics"]="true"
            ;;
        "warp"|"xterm-warp"|"wayst"|"xterm-wayst")
            graphics_support="true"
            TERMINAL_CAPS["graphics"]="true"
            ;;
        "st-graphics"|"st-256color-graphics")
            graphics_support="true"
            TERMINAL_CAPS["graphics"]="true"
            ;;
    esac
    
    # Check environment variables
    if [[ -n "${KITTY_WINDOW_ID:-}" ]] || [[ -n "${KITTY_PID:-}" ]] || [[ -n "${WEZTERM_PANE:-}" ]] || [[ -n "${WEZTERM_UNIX_SOCKET:-}" ]]; then
        graphics_support="true"
        TERMINAL_CAPS["graphics"]="true"
    fi
    
    TERMINAL_GRAPHICS_SUPPORT="$graphics_support"
    echo "$graphics_support"
}

# ============================================================================
# COLOR ENHANCEMENT SYSTEM
# ============================================================================

# Generate enhanced color primitives based on capabilities
enhance_color_primitives() {
    case "$TERMINAL_COLOR_DEPTH" in
        "truecolor")
            generate_truecolor_primitives
            ;;
        "256")
            generate_256color_primitives
            ;;
        "16")
            generate_16color_primitives
            ;;
        *)
            generate_8color_primitives
            ;;
    esac
}

# True color (24-bit) primitives
generate_truecolor_primitives() {
    TD_RED='\033[38;2;220;50;47m'
    TD_GREEN='\033[38;2;0;136;0m'
    TD_YELLOW='\033[38;2;181;137;0m'
    TD_BLUE='\033[38;2;38;139;210m'
    TD_CYAN='\033[38;2;42;161;152m'
    TD_MAGENTA='\033[38;2;211;54;130m'
    TD_WHITE='\033[38;2;238;232;213m'
    
    TD_ENHANCED_COLORS=(
        ["SUCCESS"]='\033[38;2;0;136;0m'
        ["ERROR"]='\033[38;2;220;50;47m'
        ["WARNING"]='\033[38;2;181;137;0m'
        ["INFO"]='\033[38;2;38;139;210m'
        ["DEBUG"]='\033[38;2;147;161;161m'
        ["ACCENT"]='\033[38;2;42;161;152m'
    )
}

# 256 color primitives
generate_256color_primitives() {
    TD_RED='\033[38;5;160m'
    TD_GREEN='\033[38;5;28m'
    TD_YELLOW='\033[38;5;136m'
    TD_BLUE='\033[38;5;33m'
    TD_CYAN='\033[38;5;37m'
    TD_MAGENTA='\033[38;5;125m'
    TD_WHITE='\033[38;5;230m'
    
    TD_ENHANCED_COLORS=(
        ["SUCCESS"]='\033[38;5;28m'
        ["ERROR"]='\033[38;5;160m'
        ["WARNING"]='\033[38;5;136m'
        ["INFO"]='\033[38;5;33m'
        ["DEBUG"]='\033[38;5;243m'
        ["ACCENT"]='\033[38;5;37m'
    )
}

# 16 color primitives
generate_16color_primitives() {
    TD_RED='\033[0;91m'
    TD_GREEN='\033[0;92m'
    TD_YELLOW='\033[0;93m'
    TD_BLUE='\033[0;94m'
    TD_CYAN='\033[0;96m'
    TD_MAGENTA='\033[0;95m'
    TD_WHITE='\033[0;97m'
    
    TD_ENHANCED_COLORS=(
        ["SUCCESS"]='\033[0;92m'
        ["ERROR"]='\033[0;91m'
        ["WARNING"]='\033[0;93m'
        ["INFO"]='\033[0;94m'
        ["DEBUG"]='\033[2;37m'
        ["ACCENT"]='\033[0;96m'
    )
}

# 8 color primitives (fallback)
generate_8color_primitives() {
    TD_RED='\033[0;31m'
    TD_GREEN='\033[0;32m'
    TD_YELLOW='\033[0;33m'
    TD_BLUE='\033[0;34m'
    TD_CYAN='\033[0;36m'
    TD_MAGENTA='\033[0;35m'
    TD_WHITE='\033[0;37m'
    
    TD_ENHANCED_COLORS=(
        ["SUCCESS"]='\033[0;32m'
        ["ERROR"]='\033[0;31m'
        ["WARNING"]='\033[0;33m'
        ["INFO"]='\033[0;34m'
        ["DEBUG"]='\033[2m'
        ["ACCENT"]='\033[0;36m'
    )
}

# Apply no-color mode (for CI/non-interactive environments)
apply_no_color() {
    # Only disable color output, don't change capability detection
    TD_RED=""
    TD_GREEN=""
    TD_YELLOW=""
    TD_BLUE=""
    TD_CYAN=""
    TD_MAGENTA=""
    TD_WHITE=""
    TD_NC=""
    
    for key in "${!TD_ENHANCED_COLORS[@]}"; do
        TD_ENHANCED_COLORS["$key"]=""
    done
}

# ============================================================================
# COMPREHENSIVE DETECTION
# ============================================================================

# Run comprehensive terminal detection
detect_terminal_capabilities() {
    # Core detection
    local interactive=$(detect_interactive_terminal)
    local color_depth=$(detect_color_depth)
    local unicode_support=$(detect_unicode_support)
    local graphics_support=$(detect_graphics_support)
    
    # Build capabilities string
    local capabilities=()
    for capability in "${!TERMINAL_CAPS[@]}"; do
        if [[ "${TERMINAL_CAPS[$capability]}" == "true" ]]; then
            capabilities+=("$capability")
        fi
    done
    TERMINAL_CAPABILITIES="${capabilities[*]}"
    
    # Update global variables to reflect current detection
    TERMINAL_COLOR_DEPTH="$color_depth"
    TERMINAL_UNICODE_SUPPORT="$unicode_support"
    TERMINAL_GRAPHICS_SUPPORT="$graphics_support"
    
    # Check for NO_COLOR environment variable
    if [[ -n "${NO_COLOR:-}" ]]; then
        apply_no_color
    elif [[ "${interactive}" == "false" && -z "${FORCE_COLOR:-}" && -z "${COLORTERM:-}" && "${TERMINAL_COLOR_DEPTH}" == "8" ]]; then
        # Only disable colors for truly basic terminals when non-interactive
        apply_no_color
    else
        enhance_color_primitives
    fi
    
    return 0
}

# ============================================================================
# UTILITY FUNCTIONS
# ============================================================================

# Check if specific capability is supported
has_capability() {
    local capability="$1"
    [[ "${TERMINAL_CAPS[$capability]:-false}" == "true" ]]
}

# Get color for semantic meaning
get_color() {
    local color_name="$1"
    echo "${TD_ENHANCED_COLORS[$color_name]:-}"
}

# Safe color output (respects NO_COLOR)
color_text() {
    local color="$1"
    local text="$2"
    local reset="${3:-true}"
    
    if [[ -n "$color" ]]; then
        echo -ne "$color$text"
        [[ "$reset" == "true" ]] && echo -ne "$TD_NC"
    else
        echo -n "$text"
    fi
}

# Terminal information summary
terminal_info() {
    echo "Terminal Type: $TERMINAL_TYPE"
    echo "Color Depth: $TERMINAL_COLOR_DEPTH"
    echo "Unicode Support: $TERMINAL_UNICODE_SUPPORT"
    echo "Graphics Support: $TERMINAL_GRAPHICS_SUPPORT"
    echo "Interactive: ${TERMINAL_CAPS[interactive]}"
    echo "Capabilities: $TERMINAL_CAPABILITIES"
}

# Self-test function
run_terminal_test() {
    echo "=========================================="
    echo "Terminal Detection Module Self-Test"
    echo "=========================================="
    echo ""
    
    detect_terminal_capabilities
    terminal_info
    echo ""
    
    echo "Color Test:"
    color_text "$(get_color ERROR)" "  ❌ Error" && echo ""
    color_text "$(get_color WARNING)" "  ⚠️  Warning" && echo ""
    color_text "$(get_color SUCCESS)" "  ✅ Success" && echo ""
    color_text "$(get_color INFO)" "  ℹ️  Info" && echo ""
    color_text "$(get_color DEBUG)" "  🔧 Debug" && echo ""
    color_text "$(get_color ACCENT)" "  🔍 Accent" && echo ""
    echo ""
    
    if has_capability "graphics"; then
        echo "Graphics Protocol: SUPPORTED"
    else
        echo "Graphics Protocol: NOT SUPPORTED"
    fi
    
    echo ""
    echo "Self-test complete!"
}

# Show usage information
show_usage() {
    echo "Terminal Detection Module - Simplified terminal capability detection"
    echo ""
    echo "Usage when sourced:"
    echo "  detect_terminal_capabilities    # Run detection"
    echo "  has_capability \"true_color\"     # Check specific capability"
    echo "  get_color \"SUCCESS\"             # Get semantic color"
    echo "  color_text \"\$color\" \"text\"      # Output colored text"
    echo ""
    echo "Direct usage:"
    echo "  $0 --self-test Run self-test with color examples"
    echo "  $0 --info     Show terminal information"
    echo "  $0 --help     Show this help message"
}

# ============================================================================
# INITIALIZATION
# ============================================================================

# Auto-detect capabilities when sourced
detect_terminal_capabilities

# Only run if executed directly (not sourced)
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    case "${1:-}" in
    --self-test)
        run_terminal_test
        ;;
    --info)
        terminal_info
        ;;
    --help | -h)
        show_usage
        ;;
    *)
        show_usage
        ;;
    esac
fi