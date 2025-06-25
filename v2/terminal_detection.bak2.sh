#!/usr/bin/env bash
# Terminal Detection Module - Clean capability detection for Layer_2
# Answers three core questions: ANSI support, Unicode support, Graphics protocol support

# Only set strict mode if not already sourced
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    set -euo pipefail
fi

# Guard against multiple loads
[[ -n "${TERMINAL_DETECTION_LOADED:-}" ]] && return 0
readonly TERMINAL_DETECTION_LOADED=1

# ============================================================================
# CORE STATE VARIABLES
# ============================================================================

# Terminal identification
declare -g TERMINAL_TYPE="${TERM:-unknown}"
declare -g TERMINAL_PROGRAM="${TERM_PROGRAM:-unknown}"

# Capability detection results  
declare -gA TERMINAL_CAPABILITIES 2>/dev/null || true
if [[ ${#TERMINAL_CAPABILITIES[@]} -eq 0 ]]; then
    TERMINAL_CAPABILITIES=(
        ["ansi_basic"]="false"         # Basic 8/16 color ANSI
        ["ansi_256"]="false"           # 256 color support  
        ["ansi_truecolor"]="false"     # 24-bit RGB support
        ["unicode_basic"]="false"      # Basic UTF-8 support
        ["unicode_emoji"]="false"      # Emoji/complex unicode
        ["graphics_kitty"]="false"     # Kitty Graphics Protocol
        ["interactive"]="false"        # Interactive terminal
    )
fi

# Current effective support levels (respects interactive/force settings)
declare -gA EFFECTIVE_SUPPORT 2>/dev/null || true
if [[ ${#EFFECTIVE_SUPPORT[@]} -eq 0 ]]; then
    EFFECTIVE_SUPPORT=(
        ["ansi"]="none"                # none/basic/256/truecolor
        ["unicode"]="false"            # true/false
        ["graphics"]="false"           # true/false
    )
fi

# Color primitives (populated based on effective support)
declare -g TD_RED=""
declare -g TD_GREEN=""
declare -g TD_YELLOW=""
declare -g TD_BLUE=""
declare -g TD_CYAN=""
declare -g TD_MAGENTA=""
declare -g TD_WHITE=""
declare -g TD_NC=""

declare -gA TD_ENHANCED_COLORS 2>/dev/null || true

# ============================================================================
# QUESTION 1: ANSI ESCAPE SEQUENCE SUPPORT
# ============================================================================

# Detect ANSI color support capabilities
detect_ansi_support() {
    local ansi_level="none"
    
    # Check COLORTERM (most reliable)
    case "${COLORTERM:-}" in
        "truecolor"|"24bit")
            ansi_level="truecolor"
            TERMINAL_CAPABILITIES["ansi_truecolor"]="true"
            TERMINAL_CAPABILITIES["ansi_256"]="true"
            TERMINAL_CAPABILITIES["ansi_basic"]="true"
            ;;
    esac
    
    # Check tput colors if available
    if command -v tput >/dev/null 2>&1; then
        local tput_colors
        tput_colors=$(tput colors 2>/dev/null || echo "0")
        case "$tput_colors" in
            "16777216"|"16M")
                [[ "$ansi_level" == "none" ]] && ansi_level="truecolor"
                TERMINAL_CAPABILITIES["ansi_truecolor"]="true"
                TERMINAL_CAPABILITIES["ansi_256"]="true"
                TERMINAL_CAPABILITIES["ansi_basic"]="true"
                ;;
            "256")
                [[ "$ansi_level" == "none" ]] && ansi_level="256"
                TERMINAL_CAPABILITIES["ansi_256"]="true"
                TERMINAL_CAPABILITIES["ansi_basic"]="true"
                ;;
            "16"|"8")
                [[ "$ansi_level" == "none" ]] && ansi_level="basic"
                TERMINAL_CAPABILITIES["ansi_basic"]="true"
                ;;
        esac
    fi
    
    # Check TERM patterns
    case "$TERMINAL_TYPE" in
        *"256color"*)
            [[ "$ansi_level" == "none" ]] && ansi_level="256"
            TERMINAL_CAPABILITIES["ansi_256"]="true"
            TERMINAL_CAPABILITIES["ansi_basic"]="true"
            ;;
        *"truecolor"*)
            [[ "$ansi_level" == "none" ]] && ansi_level="truecolor"
            TERMINAL_CAPABILITIES["ansi_truecolor"]="true"
            TERMINAL_CAPABILITIES["ansi_256"]="true"
            TERMINAL_CAPABILITIES["ansi_basic"]="true"
            ;;
        "xterm-kitty"|"kitty"|"xterm-ghostty"|"ghostty"|"wezterm"|"xterm-wezterm")
            [[ "$ansi_level" == "none" ]] && ansi_level="truecolor"
            TERMINAL_CAPABILITIES["ansi_truecolor"]="true"
            TERMINAL_CAPABILITIES["ansi_256"]="true"
            TERMINAL_CAPABILITIES["ansi_basic"]="true"
            ;;
        "xterm"*|"screen"*|"tmux"*)
            [[ "$ansi_level" == "none" ]] && ansi_level="basic"
            TERMINAL_CAPABILITIES["ansi_basic"]="true"
            ;;
    esac
    
    echo "$ansi_level"
}

# ============================================================================
# QUESTION 2: UNICODE SUPPORT
# ============================================================================

# Detect Unicode support capabilities
detect_unicode_support() {
    local unicode_basic="false"
    local unicode_emoji="false"
    
    # Check locale settings
    local locale_string="${LC_ALL:-${LC_CTYPE:-${LANG:-}}}"
    case "$locale_string" in
        *"UTF-8"*|*"utf8"*)
            unicode_basic="true"
            ;;
    esac
    
    # Check terminal types known to support Unicode well
    case "$TERMINAL_TYPE" in
        "xterm-kitty"|"kitty"|"xterm-ghostty"|"ghostty"|"wezterm"|"xterm-wezterm")
            unicode_basic="true"
            unicode_emoji="true"
            ;;
        *"256color"*|*"truecolor"*|"xterm"*|"screen"*|"tmux"*|"konsole"*|"gnome"*)
            unicode_basic="true"
            # Conservative on emoji - only mark true for known good terminals
            ;;
    esac
    
    # Check terminal program
    case "$TERMINAL_PROGRAM" in
        "ghostty"|"kitty"|"wezterm"|"iTerm.app")
            unicode_basic="true"
            unicode_emoji="true"
            ;;
    esac
    
    TERMINAL_CAPABILITIES["unicode_basic"]="$unicode_basic"
    TERMINAL_CAPABILITIES["unicode_emoji"]="$unicode_emoji"
    
    echo "$unicode_basic"
}

# ============================================================================
# QUESTION 3: KITTY GRAPHICS PROTOCOL SUPPORT
# ============================================================================

# Detect Kitty Graphics Protocol support
detect_graphics_support() {
    local graphics_support="false"
    
    # Skip detection in SSH without graphics forwarding
    if [[ -n "${SSH_CLIENT:-}${SSH_TTY:-}" ]] && [[ -z "${DISPLAY:-}${WAYLAND_DISPLAY:-}" ]]; then
        echo "false"
        return 1
    fi
    
    # Check for known graphics-capable terminals by TERM
    case "$TERMINAL_TYPE" in
        "xterm-kitty"|"kitty")
            graphics_support="true"
            ;;
        "xterm-ghostty"|"ghostty")
            graphics_support="true"
            ;;
        "wezterm"|"xterm-wezterm")
            graphics_support="true"
            ;;
        "konsole"|"xterm-konsole")
            graphics_support="true"
            ;;
        "warp"|"xterm-warp"|"wayst"|"xterm-wayst")
            graphics_support="true"
            ;;
        "st-graphics"|"st-256color-graphics")
            graphics_support="true"
            ;;
    esac
    
    # Check environment variables
    if [[ -n "${KITTY_WINDOW_ID:-}" ]] || [[ -n "${KITTY_PID:-}" ]]; then
        graphics_support="true"
    fi
    
    if [[ -n "${WEZTERM_PANE:-}" ]] || [[ -n "${WEZTERM_UNIX_SOCKET:-}" ]]; then
        graphics_support="true"
    fi
    
    TERMINAL_CAPABILITIES["graphics_kitty"]="$graphics_support"
    echo "$graphics_support"
}

# ============================================================================
# INTERACTIVE DETECTION
# ============================================================================

# Detect if we're in an interactive terminal
detect_interactive() {
    local interactive="false"
    if [[ -t 1 ]] && [[ -t 0 ]]; then
        interactive="true"
    fi
    
    TERMINAL_CAPABILITIES["interactive"]="$interactive"
    echo "$interactive"
}

# ============================================================================
# COMPREHENSIVE DETECTION
# ============================================================================

# Run full terminal capability detection
detect_terminal_capabilities() {
    local force_detection="${1:-false}"
    
    # Core detection (always run)
    local interactive=$(detect_interactive)
    local unicode_support=$(detect_unicode_support)
    
    # ANSI and Graphics detection (respect interactive mode unless forced)
    local ansi_support="none"
    local graphics_support="false"
    
    if [[ "$interactive" == "true" ]] || [[ "$force_detection" == "true" ]]; then
        ansi_support=$(detect_ansi_support)
        graphics_support=$(detect_graphics_support)
    else
        # Non-interactive: still detect but mark as disabled
        ansi_support=$(detect_ansi_support)
        graphics_support=$(detect_graphics_support)
        # Note: detection runs but effective support will be limited
    fi
    
    # Set effective support based on environment and policies
    set_effective_support "$interactive" "$force_detection"
    
    # Generate color primitives based on effective support
    generate_color_primitives
    
    return 0
}

# Set effective support levels based on detection and environment
set_effective_support() {
    local interactive="$1"
    local force_detection="${2:-false}"
    
    # Check for explicit color disabling
    if [[ -n "${NO_COLOR:-}" ]]; then
        EFFECTIVE_SUPPORT["ansi"]="none"
        EFFECTIVE_SUPPORT["graphics"]="false"
    elif [[ "$interactive" == "true" ]] || [[ -n "${FORCE_COLOR:-}" ]] || [[ "$force_detection" == "true" ]]; then
        # Interactive or explicitly forced - use full capabilities
        if [[ "${TERMINAL_CAPABILITIES[ansi_truecolor]}" == "true" ]]; then
            EFFECTIVE_SUPPORT["ansi"]="truecolor"
        elif [[ "${TERMINAL_CAPABILITIES[ansi_256]}" == "true" ]]; then
            EFFECTIVE_SUPPORT["ansi"]="256"
        elif [[ "${TERMINAL_CAPABILITIES[ansi_basic]}" == "true" ]]; then
            EFFECTIVE_SUPPORT["ansi"]="basic"
        else
            EFFECTIVE_SUPPORT["ansi"]="none"
        fi
        
        EFFECTIVE_SUPPORT["graphics"]="${TERMINAL_CAPABILITIES[graphics_kitty]}"
    else
        # Non-interactive without force - conservative approach
        if [[ "${TERMINAL_CAPABILITIES[ansi_basic]}" == "true" ]] && [[ -n "${COLORTERM:-}" ]]; then
            # Only enable basic colors for non-interactive if COLORTERM suggests support
            EFFECTIVE_SUPPORT["ansi"]="basic"
        else
            EFFECTIVE_SUPPORT["ansi"]="none"
        fi
        EFFECTIVE_SUPPORT["graphics"]="false"
    fi
    
    # Unicode support is less restrictive
    EFFECTIVE_SUPPORT["unicode"]="${TERMINAL_CAPABILITIES[unicode_basic]}"
}

# ============================================================================
# COLOR PRIMITIVE GENERATION
# ============================================================================

# Generate color primitives based on effective support level
generate_color_primitives() {
    case "${EFFECTIVE_SUPPORT[ansi]}" in
        "truecolor")
            generate_truecolor_primitives
            ;;
        "256")
            generate_256color_primitives
            ;;
        "basic")
            generate_basic_primitives
            ;;
        *)
            generate_no_color_primitives
            ;;
    esac
}

# True color (24-bit RGB) primitives
generate_truecolor_primitives() {
    TD_RED='\033[38;2;220;50;47m'
    TD_GREEN='\033[38;2;0;136;0m'
    TD_YELLOW='\033[38;2;181;137;0m'
    TD_BLUE='\033[38;2;38;139;210m'
    TD_CYAN='\033[38;2;42;161;152m'
    TD_MAGENTA='\033[38;2;211;54;130m'
    TD_WHITE='\033[38;2;238;232;213m'
    TD_NC='\033[0m'
    
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
    TD_NC='\033[0m'
    
    TD_ENHANCED_COLORS=(
        ["SUCCESS"]='\033[38;5;28m'
        ["ERROR"]='\033[38;5;160m'
        ["WARNING"]='\033[38;5;136m'
        ["INFO"]='\033[38;5;33m'
        ["DEBUG"]='\033[38;5;243m'
        ["ACCENT"]='\033[38;5;37m'
    )
}

# Basic 8/16 color primitives
generate_basic_primitives() {
    TD_RED='\033[0;31m'
    TD_GREEN='\033[0;32m'
    TD_YELLOW='\033[0;33m'
    TD_BLUE='\033[0;34m'
    TD_CYAN='\033[0;36m'
    TD_MAGENTA='\033[0;35m'
    TD_WHITE='\033[0;37m'
    TD_NC='\033[0m'
    
    TD_ENHANCED_COLORS=(
        ["SUCCESS"]='\033[0;32m'
        ["ERROR"]='\033[0;31m'
        ["WARNING"]='\033[0;33m'
        ["INFO"]='\033[0;34m'
        ["DEBUG"]='\033[2m'
        ["ACCENT"]='\033[0;36m'
    )
}

# No color primitives
generate_no_color_primitives() {
    TD_RED=""
    TD_GREEN=""
    TD_YELLOW=""
    TD_BLUE=""
    TD_CYAN=""
    TD_MAGENTA=""
    TD_WHITE=""
    TD_NC=""
    
    for key in SUCCESS ERROR WARNING INFO DEBUG ACCENT; do
        TD_ENHANCED_COLORS["$key"]=""
    done
}

# ============================================================================
# UTILITY FUNCTIONS
# ============================================================================

# Check if specific capability is supported (detected, not effective)
has_capability() {
    local capability="$1"
    [[ "${TERMINAL_CAPABILITIES[$capability]:-false}" == "true" ]]
}

# Check effective support level
get_effective_support() {
    local type="$1"
    echo "${EFFECTIVE_SUPPORT[$type]:-false}"
}

# Get color for semantic meaning
get_color() {
    local color_name="$1"
    echo "${TD_ENHANCED_COLORS[$color_name]:-}"
}

# Safe color output
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

# ============================================================================
# INFORMATION AND TESTING
# ============================================================================

# Show comprehensive terminal information
show_terminal_info() {
    echo "Terminal Information:"
    echo "  Type: $TERMINAL_TYPE"
    echo "  Program: $TERMINAL_PROGRAM"
    echo "  Interactive: ${TERMINAL_CAPABILITIES[interactive]}"
    echo ""
    echo "Detected Capabilities:"
    echo "  ANSI Basic: ${TERMINAL_CAPABILITIES[ansi_basic]}"
    echo "  ANSI 256: ${TERMINAL_CAPABILITIES[ansi_256]}"
    echo "  ANSI Truecolor: ${TERMINAL_CAPABILITIES[ansi_truecolor]}"
    echo "  Unicode Basic: ${TERMINAL_CAPABILITIES[unicode_basic]}"
    echo "  Unicode Emoji: ${TERMINAL_CAPABILITIES[unicode_emoji]}"
    echo "  Graphics (Kitty): ${TERMINAL_CAPABILITIES[graphics_kitty]}"
    echo ""
    echo "Effective Support:"
    echo "  ANSI: ${EFFECTIVE_SUPPORT[ansi]}"
    echo "  Unicode: ${EFFECTIVE_SUPPORT[unicode]}"
    echo "  Graphics: ${EFFECTIVE_SUPPORT[graphics]}"
}

# Run comprehensive self-test
run_self_test() {
    echo "=========================================="
    echo "Terminal Detection Module Self-Test"
    echo "=========================================="
    echo ""
    
    # Test with force detection to see full capabilities
    echo "🔍 Running full detection (forced)..."
    detect_terminal_capabilities "true"
    echo ""
    
    show_terminal_info
    echo ""
    
    echo "Color Test (effective support: ${EFFECTIVE_SUPPORT[ansi]}):"
    color_text "$(get_color ERROR)" "  ❌ Error" && echo ""
    color_text "$(get_color WARNING)" "  ⚠️  Warning" && echo ""
    color_text "$(get_color SUCCESS)" "  ✅ Success" && echo ""
    color_text "$(get_color INFO)" "  ℹ️  Info" && echo ""
    color_text "$(get_color DEBUG)" "  🔧 Debug" && echo ""
    color_text "$(get_color ACCENT)" "  🔍 Accent" && echo ""
    echo ""
    
    echo "Unicode Test (effective support: ${EFFECTIVE_SUPPORT[unicode]}):"
    if [[ "${EFFECTIVE_SUPPORT[unicode]}" == "true" ]]; then
        echo "  ✓ Basic Unicode: àáâãäå αβγδε 中文"
        if [[ "${TERMINAL_CAPABILITIES[unicode_emoji]}" == "true" ]]; then
            echo "  ✓ Emoji Support: 🎉 🚀 📊 ⚡ 🎯"
        fi
    else
        echo "  ✗ Unicode support disabled"
    fi
    echo ""
    
    echo "Graphics Test (effective support: ${EFFECTIVE_SUPPORT[graphics]}):"
    if [[ "${EFFECTIVE_SUPPORT[graphics]}" == "true" ]]; then
        echo "  ✓ Kitty Graphics Protocol: SUPPORTED"
    else
        echo "  ✗ Kitty Graphics Protocol: NOT SUPPORTED"
    fi
    echo ""
    
    echo "Self-test complete!"
}

# Show usage information
show_usage() {
    echo "Terminal Detection Module - Clean capability detection"
    echo ""
    echo "Usage when sourced:"
    echo "  detect_terminal_capabilities [force]  # Run detection"
    echo "  has_capability \"ansi_truecolor\"        # Check specific capability" 
    echo "  get_effective_support \"ansi\"           # Get effective support level"
    echo "  get_color \"SUCCESS\"                    # Get semantic color"
    echo "  color_text \"\$color\" \"text\"            # Output colored text"
    echo ""
    echo "Direct usage:"
    echo "  $0 --self-test   Run comprehensive self-test"
    echo "  $0 --info       Show terminal information"
    echo "  $0 --help       Show this help message"
    echo ""
    echo "Environment variables:"
    echo "  NO_COLOR=1      Disable all color output"
    echo "  FORCE_COLOR=1   Enable colors even in non-interactive mode"
}

# ============================================================================
# INITIALIZATION AND MAIN
# ============================================================================

# Only run if executed directly (not sourced)
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    case "${1:-}" in
    --self-test)
        run_self_test
        ;;
    --info)
        detect_terminal_capabilities "true"
        show_terminal_info
        ;;
    --help | -h)
        show_usage
        ;;
    *)
        show_usage
        ;;
    esac
fi