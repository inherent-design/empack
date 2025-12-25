# VCR Recording Guide

**Project:** empack - Minecraft Modpack Lifecycle Management
**Purpose:** Guide for recording and managing VCR test cassettes
**Date:** 2025-12-23

---

## Overview

VCR (Video Cassette Recorder) testing captures real HTTP API responses and replays them in tests, enabling:
- **Fast tests** - No network calls during test execution
- **Deterministic tests** - Same responses every time
- **Offline development** - No internet required for testing
- **CI/CD efficiency** - No API rate limits or downtime issues

---

## Prerequisites

### Tools Required

```bash
# macOS (Homebrew)
brew install curl jq

# Ubuntu/Debian
sudo apt-get install curl jq

# Verify installation
curl --version
jq --version
```

### API Keys

CurseForge API requires authentication:

1. **Get API key:**
   - Visit: https://console.curseforge.com
   - Create account → Generate API key
   - Copy key to clipboard

2. **Configure environment:**
   ```bash
   # Copy template
   cp .env.local.template .env.local

   # Edit .env.local
   vim .env.local

   # Add your API key:
   EMPACK_KEY_CURSEFORGE=your_actual_api_key_here
   ```

3. **Verify:**
   ```bash
   # Should show your API key
   grep EMPACK_KEY_CURSEFORGE .env.local
   ```

---

## Recording Cassettes

### Record All Cassettes (Phase 1 - 12 total)

```bash
# From project root
./scripts/record-vcr-cassettes.sh
```

**What it does:**
- Records 4 Modrinth endpoints (search, project, dependencies, versions)
- Records 3 CurseForge endpoints (search, mod, files)
- Records 4 loader endpoints (Fabric, Forge, NeoForge, Quilt)
- Records 1 Minecraft endpoint (version manifest)
- Sanitizes API keys from cassettes
- Validates JSON structure
- Prints summary

**Expected output:**
```
[INFO] Validating prerequisites...
[SUCCESS] Prerequisites validated
[INFO] Creating cassette directory structure...
[SUCCESS] Directory structure created
[INFO] Recording 12 cassette(s)...

[INFO] Recording cassette: modrinth/search_sodium
  URL: https://api.modrinth.com/v2/search
[SUCCESS] Recorded: modrinth/search_sodium
[SUCCESS] Sanitized: search_sodium.json
[SUCCESS] Valid cassette: search_sodium.json

...

[INFO] ════════════════════════════════════════
[INFO] Recording Summary
[INFO] ════════════════════════════════════════
[SUCCESS] Recorded: 12 cassettes
[INFO] Cassette Summary:
  Directory: crates/empack-tests/fixtures/cassettes
  Total cassettes: 12
    modrinth: 4 cassettes
    curseforge: 3 cassettes
    loaders: 4 cassettes
    minecraft: 1 cassettes
```

---

### Dry Run (Preview Without Recording)

```bash
./scripts/record-vcr-cassettes.sh --dry-run
```

**Use case:** Verify URLs and configuration before recording

---

### Record Single Cassette

```bash
# Modrinth example
./scripts/record-vcr-cassettes.sh --only modrinth/search_sodium

# CurseForge example
./scripts/record-vcr-cassettes.sh --only curseforge/search_jei

# Loader example
./scripts/record-vcr-cassettes.sh --only loaders/fabric_versions_1.21.1
```

**Use case:** Update specific cassette after API changes

---

## Cassette Structure

### Directory Layout

```
crates/empack-tests/fixtures/cassettes/
├── modrinth/
│   ├── search_sodium.json
│   ├── project_AANobbMI.json
│   ├── dependencies_AANobbMI.json
│   └── versions_AANobbMI.json
├── curseforge/
│   ├── search_jei.json
│   ├── mod_238222.json
│   └── files_238222.json
├── loaders/
│   ├── fabric_versions_1.21.1.json
│   ├── forge_promotions.json
│   ├── neoforge_versions.json
│   └── quilt_versions.json
└── minecraft/
    └── version_manifest.json
```

### Cassette Format

Each cassette is JSON file with this structure:

```json
{
  "name": "modrinth/search_sodium",
  "request": {
    "method": "GET",
    "url": "https://api.modrinth.com/v2/search",
    "query": {
      "query": "sodium",
      "limit": "10"
    },
    "headers": {
      "User-Agent": "empack-tests/0.1.0"
    }
  },
  "response": {
    "status": 200,
    "headers": {
      "content-type": "application/json"
    },
    "body": {
      "hits": [
        {
          "slug": "sodium",
          "title": "Sodium",
          "project_id": "AANobbMI",
          ...
        }
      ],
      "offset": 0,
      "limit": 10,
      "total_hits": 150
    }
  },
  "recorded_at": "2025-12-23T12:00:00Z"
}
```

**Key Fields:**
- `name` - Cassette identifier (matches filename without .json)
- `request` - HTTP request details (method, URL, query params, headers)
- `response` - HTTP response (status, headers, body)
- `recorded_at` - ISO 8601 timestamp

---

## Using Cassettes in Tests

### Loading Cassette in Test

```rust
#[tokio::test]
async fn test_modrinth_search_from_cassette() {
    // Load cassette fixture
    let cassette_json = include_str!(
        "../fixtures/cassettes/modrinth/search_sodium.json"
    );

    // Parse cassette
    let cassette: VcrCassette = serde_json::from_str(cassette_json)
        .expect("Failed to parse cassette");

    // Mock NetworkProvider to return cassette response
    let mut mock_network = MockNetworkProvider::new();
    mock_network
        .expect_http_get()
        .with(eq("https://api.modrinth.com/v2/search"))
        .returning(move |_| Ok(cassette.response.body.clone()));

    // Run test with mocked network
    let result = search_modrinth(&mock_network, "sodium").await;

    assert!(result.is_ok());
    assert_eq!(result.unwrap().hits.len(), 10);
}
```

### VCR Test Pattern

1. **Load cassette** - Read JSON fixture
2. **Mock network** - Configure NetworkProvider to return cassette response
3. **Execute code** - Run code under test with mocked network
4. **Assert results** - Verify code behavior matches expectations

---

## Maintenance

### When to Update Cassettes

**Update cassettes when:**
- ✅ API schema changes (new fields added/removed)
- ✅ Test scenarios expand (need new edge cases)
- ✅ Dependencies update (mod versions change)
- ✅ Quarterly refresh (ensure real API hasn't drifted)

**Do NOT update for:**
- ❌ Timestamp fields (recorded_at is informational)
- ❌ Volatile data (download counts, follower counts)
- ❌ Minor formatting changes

### Update Process

```bash
# 1. Update specific cassette
./scripts/record-vcr-cassettes.sh --only modrinth/search_sodium

# 2. Verify cassette is valid
jq empty crates/empack-tests/fixtures/cassettes/modrinth/search_sodium.json

# 3. Run tests with new cassette
cargo nextest run --lib -- modrinth_search

# 4. Commit if tests pass
git add crates/empack-tests/fixtures/cassettes/modrinth/search_sodium.json
git commit -m "test: update Modrinth search cassette"
```

### Quarterly Refresh

```bash
# Record all cassettes fresh
./scripts/record-vcr-cassettes.sh

# Run all tests to verify
cargo nextest run --lib

# Review changes before committing
git diff crates/empack-tests/fixtures/cassettes/

# Commit if tests pass
git add crates/empack-tests/fixtures/cassettes/
git commit -m "test: quarterly VCR cassette refresh"
```

---

## Troubleshooting

### "Missing required tools: curl jq"

**Solution:**
```bash
brew install curl jq
```

### "Environment file not found: .env.local"

**Solution:**
```bash
cp .env.local.template .env.local
vim .env.local  # Add your EMPACK_KEY_CURSEFORGE
```

### "EMPACK_KEY_CURSEFORGE not found in .env.local"

**Solution:**
1. Get API key from https://console.curseforge.com
2. Edit `.env.local`
3. Set `EMPACK_KEY_CURSEFORGE=your_key_here`

### "Rate limited (429) - retry 1/3 in 5s"

**Cause:** CurseForge rate limit hit

**Solution:**
- Script will automatically retry with backoff
- Wait for retries to complete
- If persists, wait 5 minutes and retry

### "Server error (500) - retry 1/3 in 2s"

**Cause:** API server temporary error

**Solution:**
- Script will automatically retry
- If persists after 3 retries, check API status pages:
  - Modrinth: https://status.modrinth.com
  - CurseForge: https://twitter.com/CurseForge

### "Invalid JSON in cassette: ..."

**Cause:** Network error during recording or API returned malformed JSON

**Solution:**
```bash
# Delete corrupted cassette
rm crates/empack-tests/fixtures/cassettes/path/to/cassette.json

# Re-record
./scripts/record-vcr-cassettes.sh --only path/to/cassette

# Verify
jq . crates/empack-tests/fixtures/cassettes/path/to/cassette.json
```

---

## Security

### API Key Safety

**DO:**
- ✅ Store API keys in `.env.local` (gitignored)
- ✅ Use `.env.local.template` for documentation
- ✅ Sanitize API keys from cassettes (automatic)
- ✅ Rotate API keys periodically

**DO NOT:**
- ❌ Commit `.env.local` to version control
- ❌ Share API keys in chat/email
- ❌ Use production API keys for testing
- ❌ Commit cassettes with unsanitized API keys

### Cassette Sanitization

Script automatically sanitizes:
- `x-api-key` header → `REDACTED`
- No other sensitive data in Phase 1 endpoints

**Verify sanitization:**
```bash
# Should show "REDACTED"
jq '.request.headers."x-api-key"' \
  crates/empack-tests/fixtures/cassettes/curseforge/search_jei.json
```

---

## Phase 1 Cassettes Reference

### Modrinth (4 cassettes)

| Cassette | Endpoint | Purpose |
|----------|----------|---------|
| `search_sodium` | `GET /v2/search?query=sodium` | Search results for "sodium" |
| `project_AANobbMI` | `GET /v2/project/AANobbMI` | Sodium project details |
| `dependencies_AANobbMI` | `GET /v2/project/AANobbMI/dependencies` | Sodium dependencies |
| `versions_AANobbMI` | `GET /v2/project/AANobbMI/version?game_versions=["1.21.1"]&loaders=["fabric"]` | Sodium versions for MC 1.21.1 + Fabric |

### CurseForge (3 cassettes)

| Cassette | Endpoint | Purpose |
|----------|----------|---------|
| `search_jei` | `GET /v1/mods/search?gameId=432&searchFilter=jei` | Search results for "jei" |
| `mod_238222` | `GET /v1/mods/238222` | JEI mod details |
| `files_238222` | `GET /v1/mods/238222/files` | JEI file listing |

### Loaders (4 cassettes)

| Cassette | Endpoint | Purpose |
|----------|----------|---------|
| `fabric_versions_1.21.1` | `GET /v2/versions/loader/1.21.1` | Fabric loaders for MC 1.21.1 |
| `forge_promotions` | `GET /promotions_slim.json` | Forge recommended versions |
| `neoforge_versions` | `GET /api/maven/versions/releases/...` | NeoForge versions |
| `quilt_versions` | `GET /v3/versions/loader` | Quilt loader versions |

### Minecraft (1 cassette)

| Cassette | Endpoint | Purpose |
|----------|----------|---------|
| `version_manifest` | `GET /mc/game/version_manifest.json` | Minecraft version list |

---

## Next Steps

After recording cassettes:

1. **Verify cassettes:**
   ```bash
   ls -lh crates/empack-tests/fixtures/cassettes/*/*.json
   ```

2. **Inspect cassette:**
   ```bash
   jq . crates/empack-tests/fixtures/cassettes/modrinth/search_sodium.json | less
   ```

3. **Integrate with tests:**
   - Create VCR test helpers in `crates/empack-tests/src/vcr.rs`
   - Add cassette-based tests in `crates/empack-lib/src/api/*.test.rs`
   - Mock NetworkProvider to return cassette responses

4. **Commit fixtures:**
   ```bash
   git add crates/empack-tests/fixtures/cassettes/
   git commit -m "test: add Phase 1 VCR cassettes (12 endpoints)"
   ```

5. **Document usage:**
   - Update test documentation
   - Add VCR examples to test files
   - Document cassette maintenance schedule

---

## References

- **API Documentation:**
  - Modrinth: `docs/reference/MODRINTH.md`
  - CurseForge: `docs/reference/CURSEFORGE.md`

- **Endpoint Catalog:**
  - `~/.atlas/observer/analysis/empack/api-endpoints-catalog-2025-12-23.md`
  - `~/.atlas/observer/analysis/empack/loader-version-endpoints-2025-12-23.md`

- **Testing Framework:**
  - Cargo nextest: https://nexte.st
  - wiremock (Rust HTTP mocking): https://github.com/LukeMathWalker/wiremock-rs

---

**Last Updated:** 2025-12-23
**Version:** 1.0 (Phase 1 - 12 cassettes)
**Next Phase:** Phase 2 - Additional endpoints (bulk queries, hash lookups)
