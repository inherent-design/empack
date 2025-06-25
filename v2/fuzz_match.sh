#!/usr/bin/env bash
# Fuzz Match Module - Pure string similarity algorithms for project name matching
# Provides confidence scoring for search result validation

# Only set strict mode if not already sourced
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    set -euo pipefail
fi

# Calculate string similarity between search term and found result
# Returns: percentage (0-100) indicating confidence level
calculate_similarity() {
    local search="$1"
    local found="$2"
    
    # Validate inputs
    if [[ -z "$search" || -z "$found" ]]; then
        echo "0"
        return 1
    fi
    
    # Normalize: lowercase, no spaces, no special chars
    local norm_search=$(echo "$search" | tr '[:upper:]' '[:lower:]' | tr -d ' -_.')
    local norm_found=$(echo "$found" | tr '[:upper:]' '[:lower:]' | tr -d ' -_.')
    
    # Exact match after normalization
    if [[ "$norm_search" == "$norm_found" ]]; then
        echo "100"
        return 0
    fi
    
    # Containment check (search term contained in result)
    if [[ "$norm_found" == *"$norm_search"* ]]; then
        local len_search=${#norm_search}
        local len_found=${#norm_found}
        local ratio=$((len_search * 100 / len_found))
        echo "$ratio"
        return 0
    fi
    
    # Reverse containment (result contained in search - less confident)
    if [[ "$norm_search" == *"$norm_found"* ]]; then
        echo "75"
        return 0
    fi
    
    # No similarity found
    echo "0"
    return 1
}

# Calculate download confidence based on threshold
# Returns: confidence percentage based on download count
calculate_download_confidence() {
    local downloads="$1"
    local min_threshold="${2:-1000}"
    local high_threshold="${3:-10000}"
    
    # Validate downloads is a number
    if ! [[ "$downloads" =~ ^[0-9]+$ ]]; then
        echo "0"
        return 1
    fi
    
    # Below minimum threshold = 0% confidence
    if [[ $downloads -lt $min_threshold ]]; then
        echo "0"
        return 0
    fi
    
    # Above high threshold = 100% confidence
    if [[ $downloads -ge $high_threshold ]]; then
        echo "100"
        return 0
    fi
    
    # Linear scaling between thresholds
    local range=$((high_threshold - min_threshold))
    local above_min=$((downloads - min_threshold))
    local confidence=$((above_min * 100 / range))
    
    echo "$confidence"
    return 0
}

# Calculate combined confidence from multiple factors
# Returns: weighted average confidence (0-100)
calculate_combined_confidence() {
    local search_term="$1"
    local found_title="$2"
    local downloads="$3"
    local min_downloads="${4:-1000}"
    
    # Get individual confidence scores
    local name_confidence=$(calculate_similarity "$search_term" "$found_title")
    local download_confidence=$(calculate_download_confidence "$downloads" "$min_downloads")
    
    # Weighted average: 70% name similarity, 30% download confidence
    local combined_confidence=$(( (name_confidence * 70 + download_confidence * 30) / 100 ))
    
    echo "$combined_confidence"
    return 0
}

# Check if confidence meets threshold for acceptance
# Returns: 0 (true) if confidence is acceptable, 1 (false) otherwise
is_high_confidence() {
    local confidence="$1"
    local threshold="${2:-80}"
    
    if [[ $confidence -ge $threshold ]]; then
        return 0  # High confidence
    else
        return 1  # Low confidence
    fi
}

# Check for "extra words" rule - fail if result has significantly more content
# This catches cases like "Apotheosis" vs "Apotheosis Ascended"
has_extra_words() {
    local search="$1"
    local found="$2"
    local max_extra_ratio="${3:-150}"  # Allow up to 50% extra content
    
    local norm_search=$(echo "$search" | tr '[:upper:]' '[:lower:]' | tr -d ' -_.')
    local norm_found=$(echo "$found" | tr '[:upper:]' '[:lower:]' | tr -d ' -_.')
    
    local len_search=${#norm_search}
    local len_found=${#norm_found}
    
    # If found is significantly longer than search term, it might have extra words
    if [[ $len_found -gt 0 && $len_search -gt 0 ]]; then
        local ratio=$((len_found * 100 / len_search))
        if [[ $ratio -gt $max_extra_ratio ]]; then
            return 0  # Has extra words
        fi
    fi
    
    return 1  # No extra words detected
}

# Self-test function to validate all algorithms
run_self_test() {
    echo "=========================================="
    echo "Fuzz Match Module Self-Test"
    echo "=========================================="
    echo ""
    
    # Test similarity calculations
    echo "🧪 Testing Similarity Calculations:"
    
    test_cases=(
        "Citadel|Citadel|100"
        "AppleSkin|AppleSkin|100"
        "Apotheosis|Apotheosis Ascended|55"
        "JEI|Just Enough Items|37"
        "Create|Create: Steam 'n' Rails|30"
        "Botarium|Totally Different Mod|0"
        "Iron Chests|Iron Chests: Restocked|73"
    )
    
    for test_case in "${test_cases[@]}"; do
        IFS='|' read -r search found expected <<< "$test_case"
        result=$(calculate_similarity "$search" "$found")
        if [[ $result -eq $expected ]]; then
            echo "   ✅ '$search' vs '$found' = $result% (expected $expected%)"
        else
            echo "   ❌ '$search' vs '$found' = $result% (expected $expected%)"
        fi
    done
    echo ""
    
    # Test download confidence
    echo "🧪 Testing Download Confidence:"
    download_tests=(
        "500|0"      # Below threshold
        "1000|0"     # At minimum
        "5500|45"    # Middle range
        "10000|100"  # At high threshold
        "50000|100"  # Above high threshold
    )
    
    for test_case in "${download_tests[@]}"; do
        IFS='|' read -r downloads expected <<< "$test_case"
        result=$(calculate_download_confidence "$downloads")
        echo "   📊 $downloads downloads = $result% confidence (expected ~$expected%)"
    done
    echo ""
    
    # Test extra words detection
    echo "🧪 Testing Extra Words Detection:"
    extra_word_tests=(
        "Apotheosis|Apotheosis Ascended|true"
        "JEI|Just Enough Items|true"
        "Create|Create Mod|false"
        "Iron Chests|Iron Chests|false"
    )
    
    for test_case in "${extra_word_tests[@]}"; do
        IFS='|' read -r search found expected <<< "$test_case"
        if has_extra_words "$search" "$found"; then
            result="true"
        else
            result="false"
        fi
        
        if [[ "$result" == "$expected" ]]; then
            echo "   ✅ '$search' vs '$found' = $result (expected $expected)"
        else
            echo "   ❌ '$search' vs '$found' = $result (expected $expected)"
        fi
    done
    echo ""
    
    # Test combined confidence
    echo "🧪 Testing Combined Confidence:"
    echo "   Apotheosis vs 'Apotheosis Ascended' (50k downloads):"
    combined=$(calculate_combined_confidence "Apotheosis" "Apotheosis Ascended" "50000")
    echo "   Combined confidence: $combined%"
    
    if is_high_confidence "$combined" 80; then
        echo "   Result: ✅ HIGH CONFIDENCE (≥80%)"
    else
        echo "   Result: ⚠️  LOW CONFIDENCE (<80%)"
    fi
    echo ""
    
    echo "✅ Self-test complete"
    return 0
}

# Default action: show usage information
show_usage() {
    echo "Fuzz Match Module - String similarity for project name matching"
    echo ""
    echo "Usage when sourced:"
    echo "  similarity=\$(calculate_similarity \"search\" \"found\")"
    echo "  confidence=\$(calculate_combined_confidence \"search\" \"found\" downloads)"
    echo "  if is_high_confidence \$confidence 80; then ..."
    echo ""
    echo "Direct usage:"
    echo "  $0 --self-test    Run comprehensive self-tests"
    echo "  $0 --help        Show this help message"
}

# Only run if executed directly (not sourced)
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    case "${1:-}" in
    --self-test)
        run_self_test
        ;;
    --help | -h)
        show_usage
        ;;
    *)
        show_usage
        ;;
    esac
fi