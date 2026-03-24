use super::*;

#[test]
fn test_normalize_project_type() {
    let resolver = ProjectResolver::new(Client::new(), None);

    assert_eq!(
        resolver.normalize_project_type("texture-pack"),
        "resourcepack"
    );
    assert_eq!(resolver.normalize_project_type("data-pack"), "datapack");
    assert_eq!(resolver.normalize_project_type("mod"), "mod");
}

#[test]
fn test_curseforge_class_id() {
    let resolver = ProjectResolver::new(Client::new(), None);

    assert_eq!(resolver.curseforge_class_id("mod"), 6);
    assert_eq!(resolver.curseforge_class_id("resourcepack"), 12);
    assert_eq!(resolver.curseforge_class_id("datapack"), 17);
    assert_eq!(resolver.curseforge_class_id("unknown"), 6);
}

#[test]
fn test_curseforge_loader_id() {
    let resolver = ProjectResolver::new(Client::new(), None);

    assert_eq!(resolver.curseforge_loader_id("forge"), Some(1));
    assert_eq!(resolver.curseforge_loader_id("fabric"), Some(4));
    assert_eq!(resolver.curseforge_loader_id("quilt"), Some(5));
    assert_eq!(resolver.curseforge_loader_id("neoforge"), Some(6));
    assert_eq!(resolver.curseforge_loader_id("unknown"), None);
}

// ===== PLATFORM PRIORITY TESTS =====

/// Helper: Modrinth JSON response with a single hit
fn modrinth_hit_json(project_id: &str, title: &str, downloads: u64) -> String {
    serde_json::json!({
        "hits": [{
            "project_id": project_id,
            "title": title,
            "downloads": downloads,
            "categories": ["fabric"]
        }]
    })
    .to_string()
}

/// Helper: CurseForge JSON response with a single result
fn curseforge_hit_json(id: u32, name: &str, download_count: u64) -> String {
    serde_json::json!({
        "data": [{
            "id": id,
            "name": name,
            "downloadCount": download_count
        }]
    })
    .to_string()
}

/// Helper: empty Modrinth response
fn modrinth_empty_json() -> String {
    serde_json::json!({ "hits": [] }).to_string()
}

/// Helper: empty CurseForge response
fn curseforge_empty_json() -> String {
    serde_json::json!({ "data": [] }).to_string()
}

#[cfg(feature = "test-utils")]
#[tokio::test]
async fn test_platform_priority_default_modrinth_first() {
    // Default (preferred_platform=None): Modrinth is tried first.
    // Mock Modrinth to succeed → result should be Modrinth.
    let mut mr_server = mockito::Server::new_async().await;
    let mut cf_server = mockito::Server::new_async().await;

    let mr_mock = mr_server
        .mock("GET", mockito::Matcher::Regex(r"/v2/search\?.*".to_string()))
        .with_status(200)
        .with_body(modrinth_hit_json("AANobbMI", "Sodium", 50_000))
        .create_async()
        .await;

    // CurseForge should NOT be called
    let cf_mock = cf_server
        .mock("GET", mockito::Matcher::Regex(r"/v1/mods/search\?.*".to_string()))
        .expect(0)
        .create_async()
        .await;

    let resolver = ProjectResolver::new_with_base_urls(
        Client::new(),
        Some("test-api-key".to_string()),
        Some(mr_server.url()),
        Some(cf_server.url()),
    );

    let result = resolver
        .resolve_project("Sodium", Some("mod"), None, None, None)
        .await
        .expect("should resolve");

    assert_eq!(result.platform, ProjectPlatform::Modrinth);
    assert_eq!(result.project_id, "AANobbMI");
    assert_eq!(result.title, "Sodium");
    assert!(result.confidence >= fuzzy::MODRINTH_CONFIDENCE_THRESHOLD);

    mr_mock.assert_async().await;
    cf_mock.assert_async().await;
}

#[cfg(feature = "test-utils")]
#[tokio::test]
async fn test_platform_priority_preferred_curseforge_first() {
    // preferred_platform=CurseForge: CurseForge is tried first.
    let mut mr_server = mockito::Server::new_async().await;
    let mut cf_server = mockito::Server::new_async().await;

    // Modrinth should NOT be called
    let mr_mock = mr_server
        .mock("GET", mockito::Matcher::Regex(r"/v2/search\?.*".to_string()))
        .expect(0)
        .create_async()
        .await;

    let cf_mock = cf_server
        .mock("GET", mockito::Matcher::Regex(r"/v1/mods/search\?.*".to_string()))
        .with_status(200)
        .with_body(curseforge_hit_json(12345, "Sodium", 50_000))
        .create_async()
        .await;

    let resolver = ProjectResolver::new_with_base_urls(
        Client::new(),
        Some("test-api-key".to_string()),
        Some(mr_server.url()),
        Some(cf_server.url()),
    );

    let result = resolver
        .resolve_project(
            "Sodium",
            Some("mod"),
            None,
            None,
            Some(ProjectPlatform::CurseForge),
        )
        .await
        .expect("should resolve from CurseForge");

    assert_eq!(result.platform, ProjectPlatform::CurseForge);
    assert_eq!(result.project_id, "12345");
    assert_eq!(result.title, "Sodium");

    mr_mock.assert_async().await;
    cf_mock.assert_async().await;
}

#[cfg(feature = "test-utils")]
#[tokio::test]
async fn test_platform_priority_first_fails_second_succeeds() {
    // Default order: Modrinth first → returns NoResults → falls back to CurseForge
    let mut mr_server = mockito::Server::new_async().await;
    let mut cf_server = mockito::Server::new_async().await;

    mr_server
        .mock("GET", mockito::Matcher::Regex(r"/v2/search\?.*".to_string()))
        .with_status(200)
        .with_body(modrinth_empty_json())
        .create_async()
        .await;

    cf_server
        .mock("GET", mockito::Matcher::Regex(r"/v1/mods/search\?.*".to_string()))
        .with_status(200)
        .with_body(curseforge_hit_json(99, "Sodium", 50_000))
        .create_async()
        .await;

    let resolver = ProjectResolver::new_with_base_urls(
        Client::new(),
        Some("test-api-key".to_string()),
        Some(mr_server.url()),
        Some(cf_server.url()),
    );

    let result = resolver
        .resolve_project("Sodium", Some("mod"), None, None, None)
        .await
        .expect("should fall back to CurseForge");

    assert_eq!(result.platform, ProjectPlatform::CurseForge);
    assert_eq!(result.project_id, "99");
}

#[cfg(feature = "test-utils")]
#[tokio::test]
async fn test_platform_priority_both_fail_returns_no_results() {
    // Both platforms return empty → final NoResults error
    let mut mr_server = mockito::Server::new_async().await;
    let mut cf_server = mockito::Server::new_async().await;

    mr_server
        .mock("GET", mockito::Matcher::Regex(r"/v2/search\?.*".to_string()))
        .with_status(200)
        .with_body(modrinth_empty_json())
        .create_async()
        .await;

    cf_server
        .mock("GET", mockito::Matcher::Regex(r"/v1/mods/search\?.*".to_string()))
        .with_status(200)
        .with_body(curseforge_empty_json())
        .create_async()
        .await;

    let resolver = ProjectResolver::new_with_base_urls(
        Client::new(),
        Some("test-api-key".to_string()),
        Some(mr_server.url()),
        Some(cf_server.url()),
    );

    let err = resolver
        .resolve_project("NonexistentMod", Some("mod"), None, None, None)
        .await
        .unwrap_err();

    assert!(
        matches!(err, SearchError::NoResults { ref query } if query == "NonexistentMod"),
        "Expected NoResults with query 'NonexistentMod', got: {err:?}"
    );
}

// ===== TIERED SEARCH FALLBACK TESTS =====

#[cfg(feature = "test-utils")]
#[tokio::test]
async fn test_tiered_search_first_tier_mod_succeeds() {
    // project_type=None: tries mod first → succeeds → stops
    let mut mr_server = mockito::Server::new_async().await;

    // Only expect one call (for "mod" tier)
    let mr_mock = mr_server
        .mock("GET", mockito::Matcher::Regex(r"/v2/search\?.*".to_string()))
        .with_status(200)
        .with_body(modrinth_hit_json("MOD123", "Sodium", 50_000))
        .expect(1)
        .create_async()
        .await;

    let resolver = ProjectResolver::new_with_base_urls(
        Client::new(),
        None, // No CF key: CurseForge will return MissingApiKey (swallowed as Ok(None))
        Some(mr_server.url()),
        Some("http://unused-cf:1".to_string()),
    );

    let result = resolver
        .resolve_project("Sodium", None, None, None, None)
        .await
        .expect("mod tier should succeed");

    assert_eq!(result.project_id, "MOD123");
    assert_eq!(result.project_type, "mod");
    mr_mock.assert_async().await;
}

#[cfg(feature = "test-utils")]
#[tokio::test]
async fn test_tiered_search_first_fails_second_succeeds() {
    // project_type=None: mod tier → empty → resourcepack tier → success
    let mut mr_server = mockito::Server::new_async().await;

    // First call (mod): empty. Second call (resourcepack): hit.
    mr_server
        .mock("GET", mockito::Matcher::Regex(r"project%5Ftype%3Amod%22".to_string()))
        .with_status(200)
        .with_body(modrinth_empty_json())
        .create_async()
        .await;

    mr_server
        .mock("GET", mockito::Matcher::Regex(r"project%5Ftype%3Aresourcepack".to_string()))
        .with_status(200)
        .with_body(modrinth_hit_json("RP456", "Faithful", 30_000))
        .create_async()
        .await;

    let resolver = ProjectResolver::new_with_base_urls(
        Client::new(),
        None,
        Some(mr_server.url()),
        Some("http://unused-cf:1".to_string()),
    );

    let result = resolver
        .resolve_project("Faithful", None, None, None, None)
        .await
        .expect("resourcepack tier should succeed");

    assert_eq!(result.project_id, "RP456");
    assert_eq!(result.project_type, "resourcepack");
}

#[cfg(feature = "test-utils")]
#[tokio::test]
async fn test_tiered_search_third_tier_shader_succeeds() {
    // mod → empty, resourcepack → empty, shader → success
    let mut mr_server = mockito::Server::new_async().await;

    mr_server
        .mock("GET", mockito::Matcher::Regex(r"project%5Ftype%3Amod%22".to_string()))
        .with_status(200)
        .with_body(modrinth_empty_json())
        .create_async()
        .await;

    mr_server
        .mock("GET", mockito::Matcher::Regex(r"project%5Ftype%3Aresourcepack".to_string()))
        .with_status(200)
        .with_body(modrinth_empty_json())
        .create_async()
        .await;

    mr_server
        .mock("GET", mockito::Matcher::Regex(r"project%5Ftype%3Ashader".to_string()))
        .with_status(200)
        .with_body(modrinth_hit_json("SH789", "BSL Shaders", 20_000))
        .create_async()
        .await;

    let resolver = ProjectResolver::new_with_base_urls(
        Client::new(),
        None,
        Some(mr_server.url()),
        Some("http://unused-cf:1".to_string()),
    );

    let result = resolver
        .resolve_project("BSL Shaders", None, None, None, None)
        .await
        .expect("shader tier should succeed");

    assert_eq!(result.project_id, "SH789");
    assert_eq!(result.project_type, "shader");
}

#[cfg(feature = "test-utils")]
#[tokio::test]
async fn test_tiered_search_all_tiers_fail() {
    // All four tiers return empty → NoResults error with original query
    let mut mr_server = mockito::Server::new_async().await;

    mr_server
        .mock("GET", mockito::Matcher::Regex(r"/v2/search\?.*".to_string()))
        .with_status(200)
        .with_body(modrinth_empty_json())
        .create_async()
        .await;

    let resolver = ProjectResolver::new_with_base_urls(
        Client::new(),
        None,
        Some(mr_server.url()),
        Some("http://unused-cf:1".to_string()),
    );

    let err = resolver
        .resolve_project("TotallyFakeMod", None, None, None, None)
        .await
        .unwrap_err();

    assert!(
        matches!(err, SearchError::NoResults { ref query } if query == "TotallyFakeMod"),
        "Expected NoResults with query 'TotallyFakeMod', got: {err:?}"
    );
}

// ===== ERROR PROPAGATION TESTS =====

#[cfg(feature = "test-utils")]
#[tokio::test]
async fn test_error_propagation_network_error_stops_immediately() {
    // NetworkError on first platform → propagates, second platform NOT tried
    let mut mr_server = mockito::Server::new_async().await;
    let mut cf_server = mockito::Server::new_async().await;

    // Modrinth returns 500 → triggers NetworkError
    mr_server
        .mock("GET", mockito::Matcher::Regex(r"/v2/search\?.*".to_string()))
        .with_status(500)
        .with_body("Internal Server Error")
        .create_async()
        .await;

    // CurseForge should NOT be called
    let cf_mock = cf_server
        .mock("GET", mockito::Matcher::Regex(r"/v1/mods/search\?.*".to_string()))
        .expect(0)
        .create_async()
        .await;

    let resolver = ProjectResolver::new_with_base_urls(
        Client::new(),
        Some("test-api-key".to_string()),
        Some(mr_server.url()),
        Some(cf_server.url()),
    );

    let err = resolver
        .resolve_project("Sodium", Some("mod"), None, None, None)
        .await
        .unwrap_err();

    assert!(
        matches!(err, SearchError::NetworkError { .. }),
        "Expected NetworkError, got: {err:?}"
    );
    cf_mock.assert_async().await;
}

#[cfg(feature = "test-utils")]
#[tokio::test]
async fn test_error_propagation_network_error_in_tiered_search() {
    // During tiered search: mod tier → NetworkError → propagates immediately,
    // next tiers NOT tried
    let mut mr_server = mockito::Server::new_async().await;

    let mr_mock = mr_server
        .mock("GET", mockito::Matcher::Regex(r"/v2/search\?.*".to_string()))
        .with_status(500)
        .with_body("Internal Server Error")
        .expect(1) // Only one call, then error propagates
        .create_async()
        .await;

    let resolver = ProjectResolver::new_with_base_urls(
        Client::new(),
        None,
        Some(mr_server.url()),
        Some("http://unused-cf:1".to_string()),
    );

    let err = resolver
        .resolve_project("Sodium", None, None, None, None)
        .await
        .unwrap_err();

    assert!(
        matches!(err, SearchError::NetworkError { .. }),
        "Expected NetworkError to propagate through tiered search, got: {err:?}"
    );
    mr_mock.assert_async().await;
}

#[cfg(feature = "test-utils")]
#[tokio::test]
async fn test_error_propagation_missing_api_key_swallowed() {
    // MissingApiKey is treated as recoverable in try_platform_search (Ok(None)),
    // so it falls through to the next platform.
    // If CurseForge is tried first (preferred) but has no key → falls to Modrinth.
    // But wait: MissingApiKey only happens inside search_curseforge when key is None.
    // With default order: Modrinth first (succeeds or not), then CurseForge (MissingApiKey → Ok(None)).
    // Let's test: no CF key, Modrinth empty → both fail → NoResults
    let mut mr_server = mockito::Server::new_async().await;

    mr_server
        .mock("GET", mockito::Matcher::Regex(r"/v2/search\?.*".to_string()))
        .with_status(200)
        .with_body(modrinth_empty_json())
        .create_async()
        .await;

    let resolver = ProjectResolver::new_with_base_urls(
        Client::new(),
        None, // No CF API key → MissingApiKey when CF is tried → swallowed
        Some(mr_server.url()),
        Some("http://unused-cf:1".to_string()),
    );

    // MissingApiKey does NOT propagate; we get NoResults at the end
    let err = resolver
        .resolve_project("Sodium", Some("mod"), None, None, None)
        .await
        .unwrap_err();

    assert!(
        matches!(err, SearchError::NoResults { .. }),
        "MissingApiKey should be swallowed, final error should be NoResults; got: {err:?}"
    );
}

#[cfg(feature = "test-utils")]
#[tokio::test]
async fn test_error_propagation_no_results_swallowed_second_platform_succeeds() {
    // Modrinth → NoResults (empty hits) → swallowed, falls to CurseForge → success
    let mut mr_server = mockito::Server::new_async().await;
    let mut cf_server = mockito::Server::new_async().await;

    mr_server
        .mock("GET", mockito::Matcher::Regex(r"/v2/search\?.*".to_string()))
        .with_status(200)
        .with_body(modrinth_empty_json())
        .create_async()
        .await;

    cf_server
        .mock("GET", mockito::Matcher::Regex(r"/v1/mods/search\?.*".to_string()))
        .with_status(200)
        .with_body(curseforge_hit_json(777, "Sodium", 50_000))
        .create_async()
        .await;

    let resolver = ProjectResolver::new_with_base_urls(
        Client::new(),
        Some("test-api-key".to_string()),
        Some(mr_server.url()),
        Some(cf_server.url()),
    );

    let result = resolver
        .resolve_project("Sodium", Some("mod"), None, None, None)
        .await
        .expect("Should fall back to CurseForge after Modrinth NoResults");

    assert_eq!(result.platform, ProjectPlatform::CurseForge);
    assert_eq!(result.project_id, "777");
}

#[cfg(feature = "test-utils")]
#[tokio::test]
async fn test_error_propagation_low_confidence_swallowed_next_tier_succeeds() {
    // Tiered search: mod tier → low confidence result (swallowed) → resourcepack tier → success.
    // Low confidence: Modrinth returns a result but title is very different from query.
    let mut mr_server = mockito::Server::new_async().await;

    // Mod tier: returns result with wrong title → low confidence → swallowed
    mr_server
        .mock("GET", mockito::Matcher::Regex(r"project%5Ftype%3Amod%22".to_string()))
        .with_status(200)
        .with_body(modrinth_hit_json("WRONG1", "Totally Different Name XYZ", 100))
        .create_async()
        .await;

    // Resourcepack tier: returns exact match → high confidence → accepted
    mr_server
        .mock("GET", mockito::Matcher::Regex(r"project%5Ftype%3Aresourcepack".to_string()))
        .with_status(200)
        .with_body(modrinth_hit_json("RP999", "Faithful", 30_000))
        .create_async()
        .await;

    let resolver = ProjectResolver::new_with_base_urls(
        Client::new(),
        None,
        Some(mr_server.url()),
        Some("http://unused-cf:1".to_string()),
    );

    let result = resolver
        .resolve_project("Faithful", None, None, None, None)
        .await
        .expect("Low confidence in mod tier should be swallowed, resourcepack tier should succeed");

    assert_eq!(result.project_id, "RP999");
    assert_eq!(result.project_type, "resourcepack");
}

// ===== CACHE INTEGRATION TESTS =====

#[cfg(feature = "test-utils")]
#[tokio::test]
async fn test_cache_miss_makes_network_call() {
    use crate::networking::cache::HttpCache;
    use crate::networking::rate_limit::RateLimiterManager;
    use std::sync::Arc;
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let cache = Arc::new(HttpCache::new(temp_dir.path().to_path_buf()));
    let rate_limiter = Arc::new(RateLimiterManager::new(Client::new()));

    let mut mr_server = mockito::Server::new_async().await;

    let mr_mock = mr_server
        .mock(
            "GET",
            mockito::Matcher::Regex(r"/v2/search\?.*".to_string()),
        )
        .with_status(200)
        .with_body(modrinth_hit_json("CACHED1", "Sodium", 50_000))
        .expect(1)
        .create_async()
        .await;

    let resolver = ProjectResolver::new_with_base_urls_and_networking(
        Client::new(),
        None,
        Some(mr_server.url()),
        Some("http://unused-cf:1".to_string()),
        cache.clone(),
        rate_limiter,
    );

    let result = resolver
        .resolve_project("Sodium", Some("mod"), None, None, None)
        .await
        .expect("should resolve");

    assert_eq!(result.project_id, "CACHED1");
    assert_eq!(result.platform, ProjectPlatform::Modrinth);

    // Verify network call was made (cache miss)
    mr_mock.assert_async().await;

    // Verify the response is now cached
    assert!(!cache.is_empty().await);
}

#[cfg(feature = "test-utils")]
#[tokio::test]
async fn test_cache_hit_skips_network_call() {
    use crate::networking::cache::HttpCache;
    use crate::networking::rate_limit::RateLimiterManager;
    use std::sync::Arc;
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let cache = Arc::new(HttpCache::new(temp_dir.path().to_path_buf()));
    let rate_limiter = Arc::new(RateLimiterManager::new(Client::new()));

    let mut mr_server = mockito::Server::new_async().await;

    // Only expect ONE network call — the second should be served from cache
    let mr_mock = mr_server
        .mock(
            "GET",
            mockito::Matcher::Regex(r"/v2/search\?.*".to_string()),
        )
        .with_status(200)
        .with_body(modrinth_hit_json("CACHED2", "Sodium", 50_000))
        .expect(1)
        .create_async()
        .await;

    let resolver = ProjectResolver::new_with_base_urls_and_networking(
        Client::new(),
        None,
        Some(mr_server.url()),
        Some("http://unused-cf:1".to_string()),
        cache,
        rate_limiter,
    );

    // First call — cache miss, hits network
    let result1 = resolver
        .resolve_project("Sodium", Some("mod"), None, None, None)
        .await
        .expect("first resolve should succeed");
    assert_eq!(result1.project_id, "CACHED2");

    // Second call — cache hit, no network call
    let result2 = resolver
        .resolve_project("Sodium", Some("mod"), None, None, None)
        .await
        .expect("second resolve should succeed from cache");
    assert_eq!(result2.project_id, "CACHED2");

    // Mock asserts only 1 call was made
    mr_mock.assert_async().await;
}

#[cfg(feature = "test-utils")]
#[tokio::test]
async fn test_rate_limiter_retries_on_429() {
    use crate::networking::cache::HttpCache;
    use crate::networking::rate_limit::{BackoffConfig, RateLimiterManager};
    use std::sync::Arc;
    use std::time::Duration;
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let cache = Arc::new(HttpCache::new(temp_dir.path().to_path_buf()));

    let mut mr_server = mockito::Server::new_async().await;

    // First request returns 429, second returns 200
    let mr_mock_429 = mr_server
        .mock(
            "GET",
            mockito::Matcher::Regex(r"/v2/search\?.*".to_string()),
        )
        .with_status(429)
        .with_header("retry-after", "1")
        .expect(1)
        .create_async()
        .await;

    let mr_mock_200 = mr_server
        .mock(
            "GET",
            mockito::Matcher::Regex(r"/v2/search\?.*".to_string()),
        )
        .with_status(200)
        .with_body(modrinth_hit_json("RETRY1", "Sodium", 50_000))
        .expect(1)
        .create_async()
        .await;

    // Use a fast backoff config so test doesn't take long
    let backoff = BackoffConfig {
        initial: Duration::from_millis(50),
        multiplier: 2.0,
        max: Duration::from_millis(200),
    };
    let rate_limiter = Arc::new(RateLimiterManager::with_backoff(Client::new(), backoff));

    let resolver = ProjectResolver::new_with_base_urls_and_networking(
        Client::new(),
        None,
        Some(mr_server.url()),
        Some("http://unused-cf:1".to_string()),
        cache,
        rate_limiter,
    );

    let result = resolver
        .resolve_project("Sodium", Some("mod"), None, None, None)
        .await
        .expect("should succeed after 429 retry");

    assert_eq!(result.project_id, "RETRY1");

    mr_mock_429.assert_async().await;
    mr_mock_200.assert_async().await;
}

// ===== MULTI-RESULT / PAGINATION TESTS =====

/// Helper: Modrinth JSON response with multiple hits
fn modrinth_multi_hit_json(hits: &[(&str, &str, u64)]) -> String {
    let hit_objects: Vec<serde_json::Value> = hits
        .iter()
        .map(|(project_id, title, downloads)| {
            serde_json::json!({
                "project_id": project_id,
                "title": title,
                "downloads": downloads,
                "categories": ["fabric"]
            })
        })
        .collect();
    serde_json::json!({ "hits": hit_objects }).to_string()
}

/// Helper: CurseForge JSON response with multiple results
fn curseforge_multi_hit_json(results: &[(u32, &str, u64)]) -> String {
    let data_objects: Vec<serde_json::Value> = results
        .iter()
        .map(|(id, name, download_count)| {
            serde_json::json!({
                "id": id,
                "name": name,
                "downloadCount": download_count
            })
        })
        .collect();
    serde_json::json!({ "data": data_objects }).to_string()
}

#[test]
fn test_score_results_ranks_by_confidence_descending() {
    let projects = vec![
        ProjectInfo {
            platform: ProjectPlatform::Modrinth,
            project_id: "low".to_string(),
            title: "Totally Wrong Name".to_string(),
            downloads: 500,
            confidence: 0,
            project_type: "mod".to_string(),
        },
        ProjectInfo {
            platform: ProjectPlatform::Modrinth,
            project_id: "exact".to_string(),
            title: "Sodium".to_string(),
            downloads: 50_000,
            confidence: 0,
            project_type: "mod".to_string(),
        },
        ProjectInfo {
            platform: ProjectPlatform::Modrinth,
            project_id: "partial".to_string(),
            title: "Sodium Extra".to_string(),
            downloads: 10_000,
            confidence: 0,
            project_type: "mod".to_string(),
        },
    ];

    let ranked = ProjectResolver::score_results("Sodium", projects);

    assert_eq!(ranked[0].project_id, "exact");
    assert_eq!(ranked[0].confidence, 100);
    assert!(ranked[0].confidence >= ranked[1].confidence);
    assert!(ranked[1].confidence >= ranked[2].confidence);
}

#[cfg(feature = "test-utils")]
#[tokio::test]
async fn test_resolve_picks_best_from_multiple_results() {
    // Modrinth returns 3 results; the second is an exact match.
    // resolve_project should pick the highest-confidence result.
    let mut mr_server = mockito::Server::new_async().await;

    mr_server
        .mock("GET", mockito::Matcher::Regex(r"/v2/search\?.*".to_string()))
        .with_status(200)
        .with_body(modrinth_multi_hit_json(&[
            ("WRONG1", "Sodium Reforged Extra", 5_000),
            ("EXACT1", "Sodium", 80_000),
            ("WRONG2", "Totally Different", 200),
        ]))
        .create_async()
        .await;

    let resolver = ProjectResolver::new_with_base_urls(
        Client::new(),
        None,
        Some(mr_server.url()),
        Some("http://unused-cf:1".to_string()),
    );

    let result = resolver
        .resolve_project("Sodium", Some("mod"), None, None, None)
        .await
        .expect("should pick the exact match");

    assert_eq!(result.project_id, "EXACT1");
    assert_eq!(result.confidence, 100);
}

#[cfg(feature = "test-utils")]
#[tokio::test]
async fn test_resolve_auto_selects_high_confidence() {
    // Single result at >=90% confidence → auto-selected without needing candidates
    let mut mr_server = mockito::Server::new_async().await;

    mr_server
        .mock("GET", mockito::Matcher::Regex(r"/v2/search\?.*".to_string()))
        .with_status(200)
        .with_body(modrinth_hit_json("AUTO1", "Sodium", 50_000))
        .create_async()
        .await;

    let resolver = ProjectResolver::new_with_base_urls(
        Client::new(),
        None,
        Some(mr_server.url()),
        Some("http://unused-cf:1".to_string()),
    );

    let result = resolver
        .resolve_project("Sodium", Some("mod"), None, None, None)
        .await
        .expect("high confidence should auto-select");

    assert!(result.confidence >= 90);
    assert_eq!(result.project_id, "AUTO1");
}

#[cfg(feature = "test-utils")]
#[tokio::test]
async fn test_search_candidates_returns_ranked_list() {
    // Both platforms return results; search_candidates merges and ranks them
    let mut mr_server = mockito::Server::new_async().await;
    let mut cf_server = mockito::Server::new_async().await;

    mr_server
        .mock("GET", mockito::Matcher::Regex(r"/v2/search\?.*".to_string()))
        .with_status(200)
        .with_body(modrinth_multi_hit_json(&[
            ("MR1", "Sodium", 80_000),
            ("MR2", "Sodium Extra", 5_000),
        ]))
        .create_async()
        .await;

    cf_server
        .mock("GET", mockito::Matcher::Regex(r"/v1/mods/search\?.*".to_string()))
        .with_status(200)
        .with_body(curseforge_multi_hit_json(&[
            (100, "Sodium", 60_000),
            (101, "Sodium Reforged", 3_000),
        ]))
        .create_async()
        .await;

    let resolver = ProjectResolver::new_with_base_urls(
        Client::new(),
        Some("test-api-key".to_string()),
        Some(mr_server.url()),
        Some(cf_server.url()),
    );

    let candidates = resolver
        .search_candidates("Sodium", "mod", None, None, 70, None)
        .await
        .expect("should return candidates");

    // Should have results from both platforms
    assert!(candidates.len() >= 2);
    // First result should be highest confidence (exact match)
    assert_eq!(candidates[0].confidence, 100);
    // Results should be sorted by confidence descending
    for window in candidates.windows(2) {
        assert!(window[0].confidence >= window[1].confidence);
    }
}

#[cfg(feature = "test-utils")]
#[tokio::test]
async fn test_search_candidates_filters_below_min_confidence() {
    // Only results above min_confidence threshold should be returned
    let mut mr_server = mockito::Server::new_async().await;

    mr_server
        .mock("GET", mockito::Matcher::Regex(r"/v2/search\?.*".to_string()))
        .with_status(200)
        .with_body(modrinth_multi_hit_json(&[
            ("GOOD1", "Sodium", 80_000),
            ("BAD1", "Completely Unrelated Mod Name Here", 200),
        ]))
        .create_async()
        .await;

    let resolver = ProjectResolver::new_with_base_urls(
        Client::new(),
        None,
        Some(mr_server.url()),
        Some("http://unused-cf:1".to_string()),
    );

    let candidates = resolver
        .search_candidates("Sodium", "mod", None, None, 80, None)
        .await
        .expect("should return filtered candidates");

    // All returned candidates must be above threshold
    for c in &candidates {
        assert!(c.confidence >= 80, "confidence {} < 80", c.confidence);
    }
    // The low-confidence result should be filtered out
    assert!(!candidates.iter().any(|c| c.project_id == "BAD1"));
}

#[cfg(feature = "test-utils")]
#[tokio::test]
async fn test_search_candidates_all_below_threshold_returns_error() {
    // When all results are below min_confidence, return LowConfidence error
    let mut mr_server = mockito::Server::new_async().await;

    mr_server
        .mock("GET", mockito::Matcher::Regex(r"/v2/search\?.*".to_string()))
        .with_status(200)
        .with_body(modrinth_multi_hit_json(&[
            ("BAD1", "Totally Wrong Mod ABCXYZ", 100),
            ("BAD2", "Another Wrong Mod DEFGHI", 50),
        ]))
        .create_async()
        .await;

    let resolver = ProjectResolver::new_with_base_urls(
        Client::new(),
        None,
        Some(mr_server.url()),
        Some("http://unused-cf:1".to_string()),
    );

    let err = resolver
        .search_candidates("Sodium", "mod", None, None, 70, None)
        .await
        .unwrap_err();

    assert!(
        matches!(err, SearchError::LowConfidence { .. }),
        "Expected LowConfidence when all results are below threshold, got: {err:?}"
    );
}

#[cfg(feature = "test-utils")]
#[tokio::test]
async fn test_search_candidates_respects_preferred_platform() {
    // With preferred_platform=CurseForge, CurseForge results should be searched first
    let mut mr_server = mockito::Server::new_async().await;
    let mut cf_server = mockito::Server::new_async().await;

    mr_server
        .mock("GET", mockito::Matcher::Regex(r"/v2/search\?.*".to_string()))
        .with_status(200)
        .with_body(modrinth_hit_json("MR1", "Sodium", 80_000))
        .create_async()
        .await;

    cf_server
        .mock("GET", mockito::Matcher::Regex(r"/v1/mods/search\?.*".to_string()))
        .with_status(200)
        .with_body(curseforge_hit_json(200, "Sodium", 60_000))
        .create_async()
        .await;

    let resolver = ProjectResolver::new_with_base_urls(
        Client::new(),
        Some("test-api-key".to_string()),
        Some(mr_server.url()),
        Some(cf_server.url()),
    );

    let candidates = resolver
        .search_candidates(
            "Sodium", "mod", None, None, 70,
            Some(ProjectPlatform::CurseForge),
        )
        .await
        .expect("should return candidates from both platforms");

    // Both platforms should be represented
    assert!(candidates.iter().any(|c| c.platform == ProjectPlatform::Modrinth));
    assert!(candidates.iter().any(|c| c.platform == ProjectPlatform::CurseForge));
}

#[cfg(feature = "test-utils")]
#[tokio::test]
async fn test_search_candidates_network_error_propagates() {
    // Network errors should propagate even in search_candidates
    let mut mr_server = mockito::Server::new_async().await;

    mr_server
        .mock("GET", mockito::Matcher::Regex(r"/v2/search\?.*".to_string()))
        .with_status(500)
        .with_body("Internal Server Error")
        .create_async()
        .await;

    let resolver = ProjectResolver::new_with_base_urls(
        Client::new(),
        None,
        Some(mr_server.url()),
        Some("http://unused-cf:1".to_string()),
    );

    let err = resolver
        .search_candidates("Sodium", "mod", None, None, 70, None)
        .await
        .unwrap_err();

    assert!(
        matches!(err, SearchError::NetworkError { .. }),
        "Expected NetworkError to propagate, got: {err:?}"
    );
}
