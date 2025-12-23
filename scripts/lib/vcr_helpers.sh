#!/usr/bin/env bash
#
# VCR Helpers - Shared functions for VCR cassette recording
# empack - Minecraft Modpack Lifecycle Management
# Date: 2025-12-23

set -euo pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Logging functions
log_info() {
    echo -e "${BLUE}[INFO]${NC} $*"
}

log_success() {
    echo -e "${GREEN}[SUCCESS]${NC} $*"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $*"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $*" >&2
}

# Validate prerequisites
validate_prerequisites() {
    local missing_tools=()

    if ! command -v curl &> /dev/null; then
        missing_tools+=("curl")
    fi

    if ! command -v jq &> /dev/null; then
        missing_tools+=("jq")
    fi

    if [[ ${#missing_tools[@]} -gt 0 ]]; then
        log_error "Missing required tools: ${missing_tools[*]}"
        log_error "Install with: brew install ${missing_tools[*]}"
        return 1
    fi

    return 0
}

# Check if .env.local exists with required keys
validate_env_file() {
    local env_file="${1:-.env.local}"

    if [[ ! -f "$env_file" ]]; then
        log_error "Environment file not found: $env_file"
        log_error "Create from template: cp .env.local.template .env.local"
        return 1
    fi

    # Source the env file
    set -a
    # shellcheck disable=SC1090
    source "$env_file"
    set +a

    if [[ -z "${CURSEFORGE_API_CLIENT_KEY:-}" ]]; then
        log_error "CURSEFORGE_API_CLIENT_KEY not found in $env_file"
        log_error "Add your API key to $env_file"
        return 1
    fi

    return 0
}

# Record endpoint to cassette
# Usage: record_endpoint <cassette_name> <url> <headers_json> <query_params_json> <output_path>
record_endpoint() {
    local cassette_name="$1"
    local url="$2"
    local headers_json="${3:-"{}"}"
    local query_params_json="${4:-"{}"}"
    local output_path="$5"

    log_info "Recording cassette: $cassette_name"
    log_info "  URL: $url"

    # Create output directory if needed
    local output_dir
    output_dir=$(dirname "$output_path")
    mkdir -p "$output_dir"

    # Build curl command with headers
    local curl_cmd=(curl -s -w '\n%{http_code}\n%{content_type}')

    # Add headers from JSON
    while IFS= read -r header; do
        curl_cmd+=(-H "$header")
    done < <(echo "$headers_json" | jq -r 'to_entries[] | "\(.key): \(.value)"')

    # Add query parameters to URL
    local full_url="$url"
    if [[ "$query_params_json" != "{}" ]]; then
        local query_string
        query_string=$(echo "$query_params_json" | jq -r 'to_entries | map("\(.key)=\(.value|@uri)") | join("&")')
        full_url="${url}?${query_string}"
    fi

    # Execute request with retry logic
    local max_retries=3
    local retry_count=0
    local response_file
    response_file=$(mktemp)

    while [[ $retry_count -lt $max_retries ]]; do
        # Execute curl and capture output
        local curl_output
        if curl_output=$("${curl_cmd[@]}" "$full_url" 2>&1); then
            echo "$curl_output" > "$response_file"

            # Extract status code and content type from last two lines
            local response_body
            local http_code
            local content_type
            local total_lines
            total_lines=$(wc -l < "$response_file" | tr -d ' ')
            local body_lines=$((total_lines - 2))
            response_body=$(head -n "$body_lines" "$response_file")
            http_code=$(tail -n 2 "$response_file" | head -n 1)
            content_type=$(tail -n 1 "$response_file")

            # Handle different status codes
            case "$http_code" in
                200)
                    # Success - create cassette
                    create_cassette "$cassette_name" "$url" "$query_params_json" "$headers_json" \
                        "$http_code" "$content_type" "$response_body" "$output_path"
                    log_success "Recorded: $cassette_name"
                    rm -f "$response_file"
                    return 0
                    ;;
                429)
                    # Rate limited - retry with backoff
                    log_warn "Rate limited (429) - retry $((retry_count + 1))/$max_retries in 5s"
                    sleep 5
                    retry_count=$((retry_count + 1))
                    ;;
                5*)
                    # Server error - retry with backoff
                    log_warn "Server error ($http_code) - retry $((retry_count + 1))/$max_retries in 2s"
                    sleep 2
                    retry_count=$((retry_count + 1))
                    ;;
                *)
                    # Other error - record error response
                    log_warn "HTTP $http_code for $cassette_name - recording error response"
                    create_cassette "$cassette_name" "$url" "$query_params_json" "$headers_json" \
                        "$http_code" "$content_type" "$response_body" "$output_path"
                    rm -f "$response_file"
                    return 0
                    ;;
            esac
        else
            log_error "curl failed for $cassette_name: $curl_output"
            retry_count=$((retry_count + 1))
            sleep 2
        fi
    done

    # Max retries exhausted
    log_error "Failed to record $cassette_name after $max_retries attempts"
    rm -f "$response_file"
    return 1
}

# Create cassette JSON file
create_cassette() {
    local name="$1"
    local url="$2"
    local query_params_json="$3"
    local headers_json="$4"
    local status_code="$5"
    local content_type="$6"
    local response_body="$7"
    local output_path="$8"

    # Parse response body as JSON (if valid)
    local parsed_body
    if parsed_body=$(echo "$response_body" | jq . 2>/dev/null); then
        response_body="$parsed_body"
    else
        # Not JSON - escape for JSON string
        response_body=$(echo "$response_body" | jq -Rs .)
    fi

    # Build cassette structure
    local cassette
    cassette=$(jq -n \
        --arg name "$name" \
        --arg url "$url" \
        --argjson query "$query_params_json" \
        --argjson req_headers "$headers_json" \
        --arg status "$status_code" \
        --arg content_type "$content_type" \
        --argjson body "$response_body" \
        --arg recorded_at "$(date -u +"%Y-%m-%dT%H:%M:%SZ")" \
        '{
            name: $name,
            request: {
                method: "GET",
                url: $url,
                query: $query,
                headers: $req_headers
            },
            response: {
                status: ($status | tonumber),
                headers: {
                    "content-type": $content_type
                },
                body: $body
            },
            recorded_at: $recorded_at
        }')

    # Write cassette to file (pretty-printed)
    echo "$cassette" | jq . > "$output_path"
}

# Sanitize cassette (remove API keys)
sanitize_cassette() {
    local cassette_path="$1"

    if [[ ! -f "$cassette_path" ]]; then
        log_error "Cassette not found: $cassette_path"
        return 1
    fi

    log_info "Sanitizing: $(basename "$cassette_path")"

    # Remove API keys from headers
    local sanitized
    sanitized=$(jq '
        .request.headers |= with_entries(
            if .key == "x-api-key" then
                .value = "REDACTED"
            else
                .
            end
        )
    ' "$cassette_path")

    echo "$sanitized" > "$cassette_path"
    log_success "Sanitized: $(basename "$cassette_path")"
}

# Verify cassette is valid JSON
verify_cassette() {
    local cassette_path="$1"

    if ! jq empty "$cassette_path" 2>/dev/null; then
        log_error "Invalid JSON in cassette: $cassette_path"
        return 1
    fi

    # Verify required fields
    local required_fields=(".request.url" ".response.status" ".response.body")
    for field in "${required_fields[@]}"; do
        if ! jq -e "$field" "$cassette_path" &>/dev/null; then
            log_error "Missing required field $field in $cassette_path"
            return 1
        fi
    done

    log_success "Valid cassette: $(basename "$cassette_path")"
    return 0
}

# Print cassette summary
print_cassette_summary() {
    local cassette_dir="$1"

    if [[ ! -d "$cassette_dir" ]]; then
        log_warn "Cassette directory not found: $cassette_dir"
        return 1
    fi

    log_info "Cassette Summary:"
    log_info "  Directory: $cassette_dir"

    local total_cassettes
    total_cassettes=$(find "$cassette_dir" -name "*.json" | wc -l | xargs)
    log_info "  Total cassettes: $total_cassettes"

    # Count by platform
    for platform in modrinth curseforge loaders minecraft; do
        local platform_dir="$cassette_dir/$platform"
        if [[ -d "$platform_dir" ]]; then
            local count
            count=$(find "$platform_dir" -name "*.json" | wc -l | xargs)
            log_info "    $platform: $count cassettes"
        fi
    done
}
