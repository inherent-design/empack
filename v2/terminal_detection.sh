#!/usr/bin/env bash
# Terminal Detection Module - Clean capability detection with clear naming paradigms
# Side-effect functions: update_*, initialize_*, generate_*, set_*
# Pure functions: get_*, has_*, is_*

# Only set strict mode if not already sourced
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    set -euo pipefail
fi

# Guard against multiple loads
[[ -n "${TERMINAL_DETECTION_LOADED:-}" ]] && return 0
readonly TERMINAL_DETECTION_LOADED=1

# ============================================================================
# GLOBAL STATE VARIABLES (PascalCase for mutable, UPPER_SNAKE for constants)
# ============================================================================

# Terminal identification
declare -g TerminalType="${TERM:-unknown}"
declare -g TerminalProgram="${TERM_PROGRAM:-unknown}"

# Capability detection results (global mutable state)
if ! declare -p TerminalCapabilities &>/dev/null || [[ "${#TerminalCapabilities[@]}" -eq 0 ]] 2>/dev/null; then
    declare -gA TerminalCapabilities=(
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
if ! declare -p EffectiveSupport &>/dev/null || [[ "${#EffectiveSupport[@]}" -eq 0 ]] 2>/dev/null; then
    declare -gA EffectiveSupport=(
        ["ansi"]="none"                # none/basic/256/truecolor
        ["unicode"]="false"            # true/false
        ["graphics"]="false"           # true/false
    )
fi

# Color primitives (populated based on effective support)
declare -g TdRed=""
declare -g TdGreen=""
declare -g TdYellow=""
declare -g TdBlue=""
declare -g TdCyan=""
declare -g TdMagenta=""
declare -g TdWhite=""
declare -g TdReset=""

if ! declare -p TdEnhancedColors &>/dev/null; then
    declare -gA TdEnhancedColors
fi

# ============================================================================
# SIDE-EFFECT FUNCTIONS: ANSI ESCAPE SEQUENCE SUPPORT
# ============================================================================

# Updates global TerminalCapabilities array with ANSI color support detection
# Globals modified: TerminalCapabilities[ansi_*]
# Returns: void (prints detection level to stderr for debugging)
update_ansi_capabilities() {
    local ansi_level="none"
    
    echo "   🔍 Detecting ANSI color support..." >&2
    
    # Check COLORTERM (most reliable)
    case "${COLORTERM:-}" in
        "truecolor"|"24bit")
            ansi_level="truecolor"
            TerminalCapabilities["ansi_truecolor"]="true"
            TerminalCapabilities["ansi_256"]="true"
            TerminalCapabilities["ansi_basic"]="true"
            echo "      ✅ COLORTERM indicates truecolor support" >&2
            ;;
    esac
    
    # Check tput colors if available
    if command -v tput >/dev/null 2>&1; then
        local tput_colors
        tput_colors=$(tput colors 2>/dev/null || echo "0")
        case "$tput_colors" in
            "16777216"|"16M")
                [[ "$ansi_level" == "none" ]] && ansi_level="truecolor"
                TerminalCapabilities["ansi_truecolor"]="true"
                TerminalCapabilities["ansi_256"]="true"
                TerminalCapabilities["ansi_basic"]="true"
                echo "      ✅ tput reports $tput_colors colors (truecolor)" >&2
                ;;
            "256")
                [[ "$ansi_level" == "none" ]] && ansi_level="256"
                TerminalCapabilities["ansi_256"]="true"
                TerminalCapabilities["ansi_basic"]="true"
                echo "      ✅ tput reports 256 colors" >&2
                ;;
            "16"|"8")
                [[ "$ansi_level" == "none" ]] && ansi_level="basic"
                TerminalCapabilities["ansi_basic"]="true"
                echo "      ✅ tput reports $tput_colors colors (basic)" >&2
                ;;
        esac
    fi
    
    # Check TERM patterns
    case "$TerminalType" in
        *"256color"*)
            if [[ "$ansi_level" == "none" ]]; then
                ansi_level="256"
                TerminalCapabilities["ansi_256"]="true"
                TerminalCapabilities["ansi_basic"]="true"
                echo "      ✅ TERM contains '256color'" >&2
            fi
            ;;
        *"truecolor"*)
            if [[ "$ansi_level" == "none" ]]; then
                ansi_level="truecolor"
                TerminalCapabilities["ansi_truecolor"]="true"
                TerminalCapabilities["ansi_256"]="true"
                TerminalCapabilities["ansi_basic"]="true"
                echo "      ✅ TERM contains 'truecolor'" >&2
            fi
            ;;
        "xterm-kitty"|"kitty"|"xterm-ghostty"|"ghostty"|"wezterm"|"xterm-wezterm")
            if [[ "$ansi_level" == "none" ]]; then
                ansi_level="truecolor"
                TerminalCapabilities["ansi_truecolor"]="true"
                TerminalCapabilities["ansi_256"]="true"
                TerminalCapabilities["ansi_basic"]="true"
                echo "      ✅ Modern terminal type: $TerminalType" >&2
            fi
            ;;
        "xterm"*|"screen"*|"tmux"*)
            if [[ "$ansi_level" == "none" ]]; then
                ansi_level="basic"
                TerminalCapabilities["ansi_basic"]="true"
                echo "      ✅ Standard terminal type: $TerminalType" >&2
            fi
            ;;
    esac
    
    echo "      🎯 Final ANSI level: $ansi_level" >&2
}

# ============================================================================
# SIDE-EFFECT FUNCTIONS: UNICODE SUPPORT
# ============================================================================

# Updates global TerminalCapabilities array with Unicode support detection
# Globals modified: TerminalCapabilities[unicode_*]
# Returns: void (prints detection results to stderr for debugging)
update_unicode_capabilities() {
    local unicode_basic="false"
    local unicode_emoji="false"
    
    echo "   🔍 Detecting Unicode support..." >&2
    
    # Check locale settings
    local locale_string="${LC_ALL:-${LC_CTYPE:-${LANG:-}}}"
    case "$locale_string" in
        *"UTF-8"*|*"utf8"*)
            unicode_basic="true"
            echo "      ✅ Locale supports UTF-8: $locale_string" >&2
            ;;
    esac
    
    # Check terminal types known to support Unicode well
    case "$TerminalType" in
        "xterm-kitty"|"kitty"|"xterm-ghostty"|"ghostty"|"wezterm"|"xterm-wezterm")
            unicode_basic="true"
            unicode_emoji="true"
            echo "      ✅ Modern terminal with full Unicode: $TerminalType" >&2
            ;;
        *"256color"*|*"truecolor"*|"xterm"*|"screen"*|"tmux"*|"konsole"*|"gnome"*)
            unicode_basic="true"
            echo "      ✅ Terminal supports basic Unicode: $TerminalType" >&2
            # Conservative on emoji - only mark true for known good terminals
            ;;
    esac
    
    # Check terminal program
    case "$TerminalProgram" in
        "ghostty"|"kitty"|"wezterm"|"iTerm.app")
            unicode_basic="true"
            unicode_emoji="true"
            echo "      ✅ Terminal program with full Unicode: $TerminalProgram" >&2
            ;;
    esac
    
    TerminalCapabilities["unicode_basic"]="$unicode_basic"
    TerminalCapabilities["unicode_emoji"]="$unicode_emoji"
    
    echo "      🎯 Unicode basic: $unicode_basic, emoji: $unicode_emoji" >&2
}

# ============================================================================
# SIDE-EFFECT FUNCTIONS: KITTY GRAPHICS PROTOCOL SUPPORT
# ============================================================================

# Updates global TerminalCapabilities array with graphics protocol detection
# Globals modified: TerminalCapabilities[graphics_kitty]
# Returns: void (prints detection results to stderr for debugging)
update_graphics_capabilities() {
    local graphics_support="false"
    
    echo "   🔍 Detecting Kitty Graphics Protocol support..." >&2
    
    # Skip detection in SSH without graphics forwarding
    if [[ -n "${SSH_CLIENT:-}${SSH_TTY:-}" ]] && [[ -z "${DISPLAY:-}${WAYLAND_DISPLAY:-}" ]]; then
        echo "      ❌ SSH session without graphics forwarding detected" >&2
        TerminalCapabilities["graphics_kitty"]="false"
        return 0
    fi
    
    # Check for known graphics-capable terminals by TERM
    case "$TerminalType" in
        "xterm-kitty"|"kitty")
            graphics_support="true"
            echo "      ✅ Kitty terminal detected" >&2
            ;;
        "xterm-ghostty"|"ghostty")
            graphics_support="true"
            echo "      ✅ Ghostty terminal detected" >&2
            ;;
        "wezterm"|"xterm-wezterm")
            graphics_support="true"
            echo "      ✅ WezTerm terminal detected" >&2
            ;;
        "konsole"|"xterm-konsole")
            graphics_support="true"
            echo "      ✅ Konsole terminal detected" >&2
            ;;
        "warp"|"xterm-warp"|"wayst"|"xterm-wayst")
            graphics_support="true"
            echo "      ✅ Modern graphics terminal detected: $TerminalType" >&2
            ;;
        "st-graphics"|"st-256color-graphics")
            graphics_support="true"
            echo "      ✅ Graphics-enabled st terminal detected" >&2
            ;;
    esac
    
    # Check environment variables
    if [[ -n "${KITTY_WINDOW_ID:-}" ]] || [[ -n "${KITTY_PID:-}" ]]; then
        graphics_support="true"
        echo "      ✅ Kitty environment variables detected" >&2
    fi
    
    if [[ -n "${WEZTERM_PANE:-}" ]] || [[ -n "${WEZTERM_UNIX_SOCKET:-}" ]]; then
        graphics_support="true"
        echo "      ✅ WezTerm environment variables detected" >&2
    fi
    
    TerminalCapabilities["graphics_kitty"]="$graphics_support"
    echo "      🎯 Graphics support: $graphics_support" >&2
}

# ============================================================================
# SIDE-EFFECT FUNCTIONS: INTERACTIVE DETECTION
# ============================================================================

# Updates global TerminalCapabilities array with interactive terminal detection
# Globals modified: TerminalCapabilities[interactive]
# Returns: void (prints detection results to stderr for debugging)
update_interactive_capability() {
    local interactive="false"
    
    echo "   🔍 Detecting interactive terminal..." >&2
    
    if [[ -t 1 ]] && [[ -t 0 ]]; then
        interactive="true"
        echo "      ✅ Interactive terminal detected (stdin & stdout are TTYs)" >&2
    else
        echo "      ❌ Non-interactive environment detected" >&2
    fi
    
    TerminalCapabilities["interactive"]="$interactive"
}

# ============================================================================
# SIDE-EFFECT FUNCTIONS: COMPREHENSIVE DETECTION ORCHESTRATION
# ============================================================================

# Orchestrates full terminal capability detection and state initialization
# Globals modified: TerminalCapabilities (all), EffectiveSupport (all)
# Returns: void (prints progress to stderr)
initialize_terminal_state() {
    local force_detection="${1:-false}"
    
    echo "🚀 Initializing terminal detection state..." >&2
    echo "   Terminal: $TerminalType (Program: $TerminalProgram)" >&2
    echo "   Force detection: $force_detection" >&2
    echo "" >&2
    
    # Core detection (always run) - call side-effect functions directly
    update_interactive_capability
    update_unicode_capabilities
    
    # ANSI and Graphics detection (respect interactive mode unless forced)
    if [[ "${TerminalCapabilities[interactive]}" == "true" ]] || [[ "$force_detection" == "true" ]]; then
        echo "   🎯 Running full capability detection..." >&2
        update_ansi_capabilities
        update_graphics_capabilities
    else
        echo "   🎯 Running detection for non-interactive environment..." >&2
        update_ansi_capabilities
        update_graphics_capabilities
        echo "   ⚠️  Note: Effective support will be conservative for non-interactive" >&2
    fi
    
    echo "" >&2
    
    # Set effective support based on environment and policies
    set_effective_support "$force_detection"
    
    # Generate color primitives based on effective support
    generate_color_primitives
    
    echo "✅ Terminal detection complete!" >&2
}

# ============================================================================
# SIDE-EFFECT FUNCTIONS: EFFECTIVE SUPPORT CALCULATION
# ============================================================================

# Sets effective support levels based on detection results and environment policies
# Globals modified: EffectiveSupport (all)
# Returns: void (prints decisions to stderr for debugging)
set_effective_support() {
    local force_detection="${1:-false}"
    local interactive="${TerminalCapabilities[interactive]}"
    
    echo "🎯 Calculating effective support levels..." >&2
    
    # Check for explicit color disabling
    if [[ -n "${NO_COLOR:-}" ]]; then
        EffectiveSupport["ansi"]="none"
        EffectiveSupport["graphics"]="false"
        echo "   ❌ NO_COLOR environment variable set - disabling color/graphics" >&2
    elif [[ "$interactive" == "true" ]] || [[ -n "${FORCE_COLOR:-}" ]] || [[ "$force_detection" == "true" ]]; then
        echo "   ✅ Interactive or forced mode - using full capabilities" >&2
        
        # Interactive or explicitly forced - use full capabilities
        if [[ "${TerminalCapabilities[ansi_truecolor]}" == "true" ]]; then
            EffectiveSupport["ansi"]="truecolor"
            echo "      🎨 Enabling truecolor ANSI support" >&2
        elif [[ "${TerminalCapabilities[ansi_256]}" == "true" ]]; then
            EffectiveSupport["ansi"]="256"
            echo "      🎨 Enabling 256-color ANSI support" >&2
        elif [[ "${TerminalCapabilities[ansi_basic]}" == "true" ]]; then
            EffectiveSupport["ansi"]="basic"
            echo "      🎨 Enabling basic ANSI support" >&2
        else
            EffectiveSupport["ansi"]="none"
            echo "      ❌ No ANSI support detected" >&2
        fi
        
        EffectiveSupport["graphics"]="${TerminalCapabilities[graphics_kitty]}"
        if [[ "${EffectiveSupport[graphics]}" == "true" ]]; then
            echo "      🖼️  Enabling graphics protocol support" >&2
        fi
    else
        echo "   ⚠️  Non-interactive mode - using conservative approach" >&2
        
        # Non-interactive without force - conservative approach
        if [[ "${TerminalCapabilities[ansi_basic]}" == "true" ]] && [[ -n "${COLORTERM:-}" ]]; then
            EffectiveSupport["ansi"]="basic"
            echo "      🎨 Enabling basic colors (COLORTERM detected)" >&2
        else
            EffectiveSupport["ansi"]="none"
            echo "      ❌ Disabling colors for non-interactive" >&2
        fi
        EffectiveSupport["graphics"]="false"
        echo "      ❌ Disabling graphics for non-interactive" >&2
    fi
    
    # Unicode support is less restrictive
    EffectiveSupport["unicode"]="${TerminalCapabilities[unicode_basic]}"
    if [[ "${EffectiveSupport[unicode]}" == "true" ]]; then
        echo "      🌍 Enabling Unicode support" >&2
    else
        echo "      ❌ Unicode support disabled" >&2
    fi
    
    echo "   📋 Final effective support: ANSI=${EffectiveSupport[ansi]}, Unicode=${EffectiveSupport[unicode]}, Graphics=${EffectiveSupport[graphics]}" >&2
}

# ============================================================================
# SIDE-EFFECT FUNCTIONS: COLOR PRIMITIVE GENERATION
# ============================================================================

# Generates color primitive variables based on effective support level
# Globals modified: TdRed, TdGreen, etc., TdEnhancedColors
# Returns: void (prints generation info to stderr for debugging)
generate_color_primitives() {
    echo "🎨 Generating color primitives for level: ${EffectiveSupport[ansi]}" >&2
    
    case "${EffectiveSupport[ansi]}" in
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
    TdRed='\033[38;2;220;50;47m'
    TdGreen='\033[38;2;0;136;0m'
    TdYellow='\033[38;2;181;137;0m'
    TdBlue='\033[38;2;38;139;210m'
    TdCyan='\033[38;2;42;161;152m'
    TdMagenta='\033[38;2;211;54;130m'
    TdWhite='\033[38;2;238;232;213m'
    TdReset='\033[0m'
    
    TdEnhancedColors=(
        ["SUCCESS"]='\033[38;2;0;136;0m'
        ["ERROR"]='\033[38;2;220;50;47m'
        ["WARNING"]='\033[38;2;181;137;0m'
        ["INFO"]='\033[38;2;38;139;210m'
        ["DEBUG"]='\033[38;2;147;161;161m'
        ["ACCENT"]='\033[38;2;42;161;152m'
    )
    
    echo "   ✅ Generated 24-bit truecolor primitives" >&2
}

# 256 color primitives
generate_256color_primitives() {
    TdRed='\033[38;5;160m'
    TdGreen='\033[38;5;28m'
    TdYellow='\033[38;5;136m'
    TdBlue='\033[38;5;33m'
    TdCyan='\033[38;5;37m'
    TdMagenta='\033[38;5;125m'
    TdWhite='\033[38;5;230m'
    TdReset='\033[0m'
    
    TdEnhancedColors=(
        ["SUCCESS"]='\033[38;5;28m'
        ["ERROR"]='\033[38;5;160m'
        ["WARNING"]='\033[38;5;136m'
        ["INFO"]='\033[38;5;33m'
        ["DEBUG"]='\033[38;5;243m'
        ["ACCENT"]='\033[38;5;37m'
    )
    
    echo "   ✅ Generated 256-color primitives" >&2
}

# Basic 8/16 color primitives
generate_basic_primitives() {
    TdRed='\033[0;31m'
    TdGreen='\033[0;32m'
    TdYellow='\033[0;33m'
    TdBlue='\033[0;34m'
    TdCyan='\033[0;36m'
    TdMagenta='\033[0;35m'
    TdWhite='\033[0;37m'
    TdReset='\033[0m'
    
    TdEnhancedColors=(
        ["SUCCESS"]='\033[0;32m'
        ["ERROR"]='\033[0;31m'
        ["WARNING"]='\033[0;33m'
        ["INFO"]='\033[0;34m'
        ["DEBUG"]='\033[2m'
        ["ACCENT"]='\033[0;36m'
    )
    
    echo "   ✅ Generated basic 8/16-color primitives" >&2
}

# No color primitives
generate_no_color_primitives() {
    TdRed=""
    TdGreen=""
    TdYellow=""
    TdBlue=""
    TdCyan=""
    TdMagenta=""
    TdWhite=""
    TdReset=""
    
    for key in SUCCESS ERROR WARNING INFO DEBUG ACCENT; do
        TdEnhancedColors["$key"]=""
    done
    
    echo "   ✅ Generated no-color primitives (all empty)" >&2
}

# ============================================================================
# PURE FUNCTIONS: STATE QUERIES (safe for command substitution)
# ============================================================================

# Checks if specific capability is supported (detected, not effective)
# Globals read: TerminalCapabilities
# Returns: "true" or "false" via stdout
has_capability() {
    local capability="$1"
    if [[ "${TerminalCapabilities[$capability]:-false}" == "true" ]]; then
        echo "true"
    else
        echo "false"
    fi
}

# Gets effective support level for specified type
# Globals read: EffectiveSupport
# Returns: support level string via stdout
get_effective_support() {
    local support_type="$1"
    echo "${EffectiveSupport[$support_type]:-false}"
}

# Gets color escape sequence for semantic meaning
# Globals read: TdEnhancedColors
# Returns: ANSI escape sequence via stdout (may be empty)
get_color() {
    local color_name="$1"
    echo "${TdEnhancedColors[$color_name]:-}"
}

# Formats terminal information as structured data
# Globals read: TerminalType, TerminalProgram, TerminalCapabilities, EffectiveSupport
# Returns: formatted info string via stdout
format_terminal_info() {
    cat <<EOF
Terminal Information:
  Type: $TerminalType
  Program: $TerminalProgram
  Interactive: ${TerminalCapabilities[interactive]}

Detected Capabilities:
  ANSI Basic: ${TerminalCapabilities[ansi_basic]}
  ANSI 256: ${TerminalCapabilities[ansi_256]}
  ANSI Truecolor: ${TerminalCapabilities[ansi_truecolor]}
  Unicode Basic: ${TerminalCapabilities[unicode_basic]}
  Unicode Emoji: ${TerminalCapabilities[unicode_emoji]}
  Graphics (Kitty): ${TerminalCapabilities[graphics_kitty]}

Effective Support:
  ANSI: ${EffectiveSupport[ansi]}
  Unicode: ${EffectiveSupport[unicode]}
  Graphics: ${EffectiveSupport[graphics]}
EOF
}

# ============================================================================
# SIDE-EFFECT FUNCTIONS: OUTPUT AND TESTING
# ============================================================================

# Renders colored text safely (respects NO_COLOR and effective support)
# Globals read: TdReset
# Returns: void (prints colored text to stdout)
render_colored_text() {
    local color="$1"
    local text="$2"
    local reset="${3:-true}"
    
    if [[ -n "$color" ]]; then
        echo -ne "$color$text"
        [[ "$reset" == "true" ]] && echo -ne "$TdReset"
    else
        echo -n "$text"
    fi
}

# Displays comprehensive terminal information to user
# Globals read: all terminal state
# Returns: void (prints formatted output to stdout)
show_terminal_info() {
    format_terminal_info
}

# Runs comprehensive self-test with visual demonstrations
# Globals modified: all (via initialize_terminal_state)
# Returns: void (prints test results to stdout, progress to stderr)
run_self_test() {
    echo "==========================================" >&2
    echo "Terminal Detection Module Self-Test" >&2
    echo "==========================================" >&2
    echo "" >&2
    
    # Test with force detection to see full capabilities
    echo "🔍 Running full detection (forced)..." >&2
    initialize_terminal_state "true"
    echo "" >&2
    
    show_terminal_info
    echo ""
    
    echo "Color Test (effective support: $(get_effective_support ansi)):"
    render_colored_text "$(get_color ERROR)" "  ❌ Error" && echo ""
    render_colored_text "$(get_color WARNING)" "  ⚠️  Warning" && echo ""
    render_colored_text "$(get_color SUCCESS)" "  ✅ Success" && echo ""
    render_colored_text "$(get_color INFO)" "  ℹ️  Info" && echo ""
    render_colored_text "$(get_color DEBUG)" "  🔧 Debug" && echo ""
    render_colored_text "$(get_color ACCENT)" "  🔍 Accent" && echo ""
    echo ""
    
    echo "Unicode Test (effective support: $(get_effective_support unicode)):"
    if [[ "$(get_effective_support unicode)" == "true" ]]; then
        echo "  ✓ Basic Unicode: àáâãäå αβγδε 中文"
        if [[ "$(has_capability unicode_emoji)" == "true" ]]; then
            echo "  ✓ Emoji Support: 🎉 🚀 📊 ⚡ 🎯"
        fi
    else
        echo "  ✗ Unicode support disabled"
    fi
    echo ""
    
    echo "Graphics Test (effective support: $(get_effective_support graphics)):"
    if [[ "$(get_effective_support graphics)" == "true" ]]; then
        echo "  ✓ Kitty Graphics Protocol: SUPPORTED"
    else
        echo "  ✗ Kitty Graphics Protocol: NOT SUPPORTED"
    fi
    echo ""
    
    echo "Self-test complete!"
}

# Shows usage information
# Returns: void (prints usage to stdout)
show_usage() {
    cat <<EOF
Terminal Detection Module - Clean capability detection with clear paradigms

Usage when sourced:
  initialize_terminal_state [force]     # Side-effect: Initialize all state
  has_capability "ansi_truecolor"       # Pure: Check specific capability
  get_effective_support "ansi"          # Pure: Get effective support level
  get_color "SUCCESS"                   # Pure: Get semantic color
  render_colored_text "\$color" "text"   # Side-effect: Output colored text
  
Color primitives (available after initialize_terminal_state):
  TdRed TdGreen TdYellow TdBlue TdCyan TdMagenta TdWhite TdReset

Direct usage:
  $0 --self-test   Run comprehensive self-test
  $0 --info       Show terminal information
  $0 --help       Show this help message

Environment variables:
  NO_COLOR=1      Disable all color output
  FORCE_COLOR=1   Enable colors even in non-interactive mode

Function Naming Paradigms:
  Side-effect functions (modify global state): update_*, initialize_*, generate_*, set_*, show_*, run_*
  Pure functions (return data via stdout): get_*, has_*, is_*, format_*
EOF
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
        initialize_terminal_state "true"
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