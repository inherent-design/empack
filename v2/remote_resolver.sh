#!/usr/bin/env bash
# Remote Resolver Module - Intelligent project resolution with confidence scoring
# Orchestrates search modules and fuzz matching for optimal project selection

# Only set strict mode if not already sourced
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    set -euo pipefail
fi

# Source required modules
source ./empack_reader.sh
source ./search_modrinth.sh
source ./search_curseforge.sh
source ./fuzz_match.sh

# Configuration constants
MODRINTH_CONFIDENCE_THRESHOLD=90
CURSEFORGE_CONFIDENCE_THRESHOLD=80
MIN_DOWNLOAD_THRESHOLD=1000
EXTRA_WORDS_MAX_RATIO=150

# Resolve a single project using platform priority and confidence scoring
# Returns: platform|project_id|confidence|title or empty string on failure
resolve_remote_project() {
    local title="$1"
    local project_type="${2:-mod}"
    local minecraft_version="${3:-}"
    local mod_loader="${4:-}"
    
    # Validate required input
    if [[ -z "$title" ]]; then
        echo "❌ ERROR: Project title is required" >&2
        return 1
    fi
    
    echo "🔍 Resolving: $title ($project_type)" >&2
    
    # Try Modrinth first (preferred platform)
    local mr_result=$(mr_get_project_data "$title" "$project_type" "$minecraft_version" "$mod_loader")
    
    if [[ -n "$mr_result" ]]; then
        IFS='|' read -r mr_project_id mr_type mr_found_title mr_downloads <<<"$mr_result"
        
        # Calculate confidence using fuzz matching
        local mr_confidence=$(calculate_combined_confidence "$title" "$mr_found_title" "$mr_downloads" "$MIN_DOWNLOAD_THRESHOLD")
        
        echo "   🔧 DEBUG: Modrinth found '$mr_found_title' (ID: $mr_project_id, Downloads: $mr_downloads, Confidence: $mr_confidence%)" >&2
        
        # Check for extra words rule
        if has_extra_words "$title" "$mr_found_title" "$EXTRA_WORDS_MAX_RATIO"; then
            echo "   ⚠️  REJECTED: '$mr_found_title' has extra words compared to '$title'" >&2
        elif [[ $mr_confidence -ge $MODRINTH_CONFIDENCE_THRESHOLD ]]; then
            echo "   ✅ High confidence match on Modrinth" >&2
            echo "modrinth|$mr_project_id|$mr_confidence|$mr_found_title"
            return 0
        else
            echo "   ⚠️  Low confidence on Modrinth ($mr_confidence% < $MODRINTH_CONFIDENCE_THRESHOLD%)" >&2
        fi
    else
        echo "   ❌ No results on Modrinth" >&2
    fi
    
    # Fallback to CurseForge (with lower confidence threshold)
    local cf_result=$(cf_get_project_data "$title" "$project_type" "$minecraft_version" "$mod_loader")
    
    if [[ -n "$cf_result" ]]; then
        IFS='|' read -r cf_project_id cf_type cf_found_title cf_downloads <<<"$cf_result"
        
        # Calculate confidence using fuzz matching
        local cf_confidence=$(calculate_combined_confidence "$title" "$cf_found_title" "$cf_downloads" "$MIN_DOWNLOAD_THRESHOLD")
        
        echo "   🔧 DEBUG: CurseForge found '$cf_found_title' (ID: $cf_project_id, Downloads: $cf_downloads, Confidence: $cf_confidence%)" >&2
        
        # Check for extra words rule
        if has_extra_words "$title" "$cf_found_title" "$EXTRA_WORDS_MAX_RATIO"; then
            echo "   ⚠️  REJECTED: '$cf_found_title' has extra words compared to '$title'" >&2
        elif [[ $cf_confidence -ge $CURSEFORGE_CONFIDENCE_THRESHOLD ]]; then
            echo "   ✅ Acceptable confidence match on CurseForge" >&2
            echo "curseforge|$cf_project_id|$cf_confidence|$cf_found_title"
            return 0
        else
            echo "   ⚠️  Low confidence on CurseForge ($cf_confidence% < $CURSEFORGE_CONFIDENCE_THRESHOLD%)" >&2
        fi
    else
        echo "   ❌ No results on CurseForge" >&2
    fi
    
    # No suitable result found
    echo "   ❌ RESOLUTION FAILED: No high-confidence match found" >&2
    return 1
}

# Resolve all projects from empack.yml
# Returns: key|platform|project_id|confidence|title for each successful resolution
resolve_all_projects() {
    if ! validate_empack >/dev/null 2>&1; then
        echo "❌ ERROR: empack.yml validation failed" >&2
        return 1
    fi
    
    local success_count=0
    local total_count=0
    
    echo "🚀 Starting bulk project resolution..." >&2
    echo "" >&2
    
    # Process all projects using empack_reader
    while IFS='|' read -r key spec; do
        total_count=$((total_count + 1))
        
        # Parse project specification
        local parsed_spec=$(parse_project_spec "$spec")
        IFS='|' read -r title project_type minecraft_version mod_loader <<<"$parsed_spec"
        
        # Attempt resolution
        local resolution_result=$(resolve_remote_project "$title" "$project_type" "$minecraft_version" "$mod_loader")
        
        if [[ -n "$resolution_result" ]]; then
            echo "$key|$resolution_result"
            success_count=$((success_count + 1))
        fi
        
        echo "" >&2
    done < <(get_all_projects)
    
    echo "📊 Resolution Summary: $success_count/$total_count projects resolved" >&2
}

# Self-test function with dependency checks
run_self_test() {
    echo "==========================================" >&2
    echo "Remote Resolver Module Self-Test" >&2
    echo "==========================================" >&2
    echo "" >&2
    
    # Check dependencies
    echo "🔧 Checking Dependencies:" >&2
    local deps_ok=true
    
    if ! command -v jq &>/dev/null; then
        echo "   ❌ jq not found" >&2
        deps_ok=false
    else
        echo "   ✅ jq available" >&2
    fi
    
    if ! validate_empack >/dev/null 2>&1; then
        echo "   ❌ empack.yml validation failed" >&2
        deps_ok=false
    else
        echo "   ✅ empack.yml valid" >&2
    fi
    
    if [[ "$deps_ok" != "true" ]]; then
        echo "❌ Dependencies not met, skipping resolution tests" >&2
        return 1
    fi
    echo "" >&2
    
    # Test confidence thresholds
    echo "🧪 Testing Confidence Configuration:" >&2
    echo "   Modrinth threshold: $MODRINTH_CONFIDENCE_THRESHOLD%" >&2
    echo "   CurseForge threshold: $CURSEFORGE_CONFIDENCE_THRESHOLD%" >&2
    echo "   Min downloads: $MIN_DOWNLOAD_THRESHOLD" >&2
    echo "   Extra words ratio: $EXTRA_WORDS_MAX_RATIO%" >&2
    echo "" >&2
    
    # Test individual project resolution (use a known simple case)
    echo "🎯 Testing Individual Resolution:" >&2
    echo "   Testing well-known mod 'Citadel'..." >&2
    
    local test_result=$(resolve_remote_project "Citadel" "mod" "" "")
    if [[ -n "$test_result" ]]; then
        IFS='|' read -r platform project_id confidence found_title <<<"$test_result"
        echo "   ✅ Found: $found_title on $platform (ID: $project_id, Confidence: $confidence%)" >&2
    else
        echo "   ⚠️  No high-confidence match found (this may be expected)" >&2
    fi
    echo "" >&2
    
    echo "✅ Self-test complete" >&2
    echo "💡 For full resolution test, run with --resolve-all" >&2
    return 0
}

# Show usage information
show_usage() {
    echo "Remote Resolver Module - Intelligent project resolution with confidence" >&2
    echo "" >&2
    echo "Usage when sourced:" >&2
    echo "  result=\$(resolve_remote_project \"title\" \"type\" \"version\" \"loader\")" >&2
    echo "  resolve_all_projects  # Process entire empack.yml" >&2
    echo "" >&2
    echo "Direct usage:" >&2
    echo "  $0 --self-test      Run dependency checks and basic tests" >&2
    echo "  $0 --resolve-all    Resolve all projects from empack.yml" >&2
    echo "  $0 --help          Show this help message" >&2
}

# Only run if executed directly (not sourced)
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    case "${1:-}" in
    --self-test)
        run_self_test
        ;;
    --resolve-all)
        echo "🚀 Resolving All Projects from empack.yml" >&2
        echo "" >&2
        resolve_all_projects
        ;;
    --help | -h)
        show_usage
        ;;
    *)
        show_usage
        ;;
    esac
fi